mod agent_panel;
mod chat;
mod components;
mod input;
mod layout;
mod settings;
mod state;
mod styles;
mod theme;
mod tree;
mod widgets;

use std::io;
use std::sync::Arc;

use crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use agent_compose::KmsContext;
use agent_compose::ParallelComposeContext;
use agent_knowledge::KnowledgeContext;
use kms::KmsService;
use ratatui::Terminal;

use crate::input::run_app;
use crate::settings::{build_pool_from_entries, load_settings, save_settings, ProviderConfig};
use crate::state::{AgentKind, App, SettingsProvider};
use crate::theme::Theme;

use std::collections::HashMap;

type CrosstermBackend = ratatui::backend::CrosstermBackend<std::io::Stdout>;

fn init_logging() {
    use std::fs::{OpenOptions, create_dir_all};
    let log_path = log_path();
    if let Some(parent) = log_path.parent() {
        let _ = create_dir_all(parent);
    }
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .unwrap_or_else(|e| panic!("failed to open log file {:?}: {}", log_path, e));
    tracing_subscriber::fmt()
        .with_writer(log_file)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::DEBUG.into()),
        )
        .init();
    tracing::info!("logging to {:?}", log_path);
}

fn log_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("KMS_LOG_PATH") {
        return std::path::PathBuf::from(p);
    }
    if let Some(data_dir) = std::env::var_os("XDG_DATA_HOME") {
        let mut p = std::path::PathBuf::from(data_dir);
        p.push("kms");
        return p.join("tui.log");
    }
    if let Some(home) = std::env::var_os("HOME") {
        let mut p = std::path::PathBuf::from(home);
        p.push(".local");
        p.push("share");
        p.push("kms");
        return p.join("tui.log");
    }
    std::path::PathBuf::from("data/tui.log")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    let db_path = std::env::var("KMS_DB_PATH").unwrap_or_else(|_| "data/kms_sqlite.db".to_string());
    let svc = KmsService::new(&db_path).await.map_err(|e| e.to_string())?;

    let raw_settings = load_settings();
    let provider_configs: Vec<ProviderConfig> = raw_settings.providers;

    let mut providers: Vec<SettingsProvider> = Vec::with_capacity(provider_configs.len());
    for cp in &provider_configs {
        let models = crate::settings::refresh_models(cp).await;
        providers.push(SettingsProvider {
            id: cp.id.clone(),
            display_name: cp.display_name.clone(),
            provider_type: cp.provider_type.clone(),
            api_key: cp.api_key.clone(),
            base_url: cp.base_url.clone(),
            models,
            is_custom: true,
        });
    }

    let pool_entries = raw_settings
        .pool
        .into_iter()
        .filter(|e| {
            providers
                .iter()
                .any(|p| p.id == e.provider_id && p.models.contains(&e.model))
        })
        .collect::<Vec<_>>();

    // Singleton ProcessManager — owned by the TUI, shared via Arc to
    // the tool layer so parallel dispatch can spawn sub-agents.
    let process_manager = Arc::new(agentik_core::process::ProcessManager::new());

    // Shared title map: agent_id → human-readable title. Written by
    // the dispatch tool after spawn(), read by the TUI panel.
    let agent_titles = Arc::new(std::sync::RwLock::new(HashMap::new()));

    // Build agents only when we have at least one working model.
    let mut agents: HashMap<AgentKind, Arc<tokio::sync::Mutex<agentik_core::Agent>>> =
        HashMap::new();

    if !providers.is_empty()
        && !pool_entries.is_empty()
        && let Some(pool) = build_pool_from_entries(&pool_entries, &providers)
    {
        let pool_arc = Arc::new(pool);

        let compose_agent = agentik_core::Agent::builder()
            .with_model_pool(pool_arc.clone())
            .with_context(Arc::new(KmsContext::new(Arc::new(svc.clone()))))
            .build()
            .await
            .map_err(|e| e.to_string())?;

        let knowledge_agent = agentik_core::Agent::builder()
            .with_model_pool(pool_arc.clone())
            .with_context(Arc::new(KnowledgeContext::new(Arc::new(svc.clone()))))
            .build()
            .await
            .map_err(|e| e.to_string())?;

        let parallel_agent = agentik_core::Agent::builder()
            .with_model_pool(pool_arc.clone())
            .with_context(Arc::new(ParallelComposeContext::new(
                Arc::new(svc.clone()),
                pool_arc.clone(),
                process_manager.clone(),
                agent_titles.clone(),
            )))
            .build()
            .await
            .map_err(|e| e.to_string())?;

        agents.insert(AgentKind::Compose, Arc::new(tokio::sync::Mutex::new(compose_agent)));
        agents.insert(
            AgentKind::Knowledge,
            Arc::new(tokio::sync::Mutex::new(knowledge_agent)),
        );
        agents.insert(
            AgentKind::Parallel,
            Arc::new(tokio::sync::Mutex::new(parallel_agent)),
        );
    }

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;

    let mut app = App::new(
        svc,
        agents,
        providers,
        provider_configs,
        pool_entries.clone(),
        process_manager,
        agent_titles,
    );

    save_settings(&app.provider_configs, &app.pool_entries);

    // Initial load: knowledge tree
    let root_children = app.svc.get_children(None).await?;
    let mut stack: Vec<(kms::Index, usize)> =
        root_children.into_iter().map(|c| (c, 0)).collect();

    while let Some((node, depth)) = stack.pop() {
        let title = node.title.as_deref().unwrap_or("(unnamed)");
        let indent = "  ".repeat(depth);
        let icon = match node.target_type {
            kms::TargetType::Group => "▸ ",
            kms::TargetType::Knowledge => "● ",
        };
        app.tree_items.push(ratatui::widgets::ListItem::new(format!(
            "{}{}{}",
            indent, icon, title
        )));
        app.tree_nodes.push(node.clone());

        if let Ok(children) = app.svc.get_children(Some(node.id)).await {
            for child in children.into_iter().rev() {
                stack.push((child, depth + 1));
            }
        }
    }

    // Initial load: diagnostics
    let diagnostics = app.svc.diagnose().await.unwrap_or_else(|e| {
        vec![kms::Diagnostic {
            code: "tui.error".to_string(),
            code_description: None,
            location: String::new(),
            severity: kms::Severity::Error,
            message: format!("diagnose error: {}", e),
            suggested_actions: vec![],
        }]
    });

    let theme = Theme::default_theme();
    if diagnostics.is_empty() {
        app.diagnostic_lines = vec![ratatui::text::Line::from(ratatui::text::Span::styled(
            "No issues found.".to_owned(),
            theme.success_style(),
        ))];
    } else {
        let mut lines = vec![ratatui::text::Line::from(format!(
            "{} issues found:",
            diagnostics.len()
        ))];
        for d in &diagnostics {
            lines.push(crate::styles::style_diagnostic_line(
                &format!("[{}] {} — {}", d.severity.label(), d.code, d.message),
                &theme,
            ));
            lines.push(crate::styles::style_diagnostic_line(&d.location, &theme));
            for action in &d.suggested_actions {
                lines.push(crate::styles::style_diagnostic_line(
                    &format!("  → {}", action),
                    &theme,
                ));
            }
            lines.push(ratatui::text::Line::from(""));
        }
        app.diagnostic_lines = lines;
    }

    app.on_tree_select().await;

    let result = run_app(&mut terminal, &mut app).await;

    execute!(io::stdout(), DisableBracketedPaste, LeaveAlternateScreen)?;
    disable_raw_mode()?;
    terminal.show_cursor()?;

    result
}
