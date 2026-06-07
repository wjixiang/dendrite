mod chat;
mod components;
mod input;
mod layout;
mod parallel_panel;
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
    // Append mode so a previous run's logs survive a restart — invaluable
    // for "the app crashed last time, where?" postmortem. The file is
    // created on first run.
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
    // First entry, so the user can `grep tui.log | head` and immediately
    // see where the log went on this run.
    tracing::info!("logging to {:?}", log_path);
}

/// Resolve the log file path. `KMS_LOG_PATH` wins, then the XDG data
/// directory (`$XDG_DATA_HOME/kms/tui.log` or
/// `$HOME/.local/share/kms/tui.log`). Falls back to the legacy
/// CWD-relative `data/tui.log` only when neither is available, so the
/// TUI never crashes on startup just because $HOME is unset (e.g.
/// inside an unprivileged systemd unit).
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

    // Note: we deliberately do NOT load .env files and do NOT read
    // MIMO_API_KEY / MINIMAX_* / SENSENOVA_* from the environment. All
    // provider configuration lives in `data/settings.json` and is
    // managed through the in-TUI settings form.
    let db_path = std::env::var("KMS_DB_PATH").unwrap_or_else(|_| "data/kms_sqlite.db".to_string());
    let svc = KmsService::new(&db_path).await.map_err(|e| e.to_string())?;

    // Load all providers from settings.json. No env-var discovery.
    let raw_settings = load_settings();
    let provider_configs: Vec<ProviderConfig> = raw_settings.providers;

    // Refresh each provider's model list from the SDK (e.g. minimax
    // exposes `/v1/models`). The fresh list is what the in-memory
    // SettingsProvider carries; on-disk config keeps the credentials
    // but the model list is treated as ephemeral.
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

    // Validate persisted pool entries against the live provider list.
    // If a provider or model disappeared, drop the stale entry.
    let pool_entries = raw_settings
        .pool
        .into_iter()
        .filter(|e| {
            providers
                .iter()
                .any(|p| p.id == e.provider_id && p.models.contains(&e.model))
        })
        .collect::<Vec<_>>();

    // Build agents only when we have at least one working model.
    // Otherwise the TUI starts in "needs configuration" mode.
    let mut agents: HashMap<AgentKind, Arc<tokio::sync::Mutex<agentik_core::Agent>>> =
        HashMap::new();
    let (parallel_progress_tx, parallel_progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<dendrite_tools::parallel_progress::ParallelProgress>(
        );

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
                parallel_progress_tx.clone(),
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
        parallel_progress_tx,
        parallel_progress_rx,
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
