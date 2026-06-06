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

use std::fs;
use std::io;
use std::sync::Arc;

use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    event::{DisableBracketedPaste, EnableBracketedPaste},
};
use agent_compose::KmsContext;
use agent_knowledge::KnowledgeContext;
use kms::KmsService;
use agentik_sdk::model::model_pool::ModelPool;
use agentik_sdk::provider::LlmProvider;
use agentik_sdk::provider::mimo::MimoProvider;
use ratatui::Terminal;

use crate::input::run_app;
use crate::state::{AgentKind, App, SettingsProvider};
use crate::theme::Theme;

use std::collections::HashMap;

type CrosstermBackend = ratatui::backend::CrosstermBackend<std::io::Stdout>;

fn init_logging() {
    use std::fs::{create_dir_all, File};
    let _ = create_dir_all("data");
    let log_file = File::create("data/tui.log").expect("failed to create log file");
    tracing_subscriber::fmt()
        .with_writer(log_file)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::DEBUG.into()),
        )
        .init();
}

const SETTINGS_FILE: &str = "data/settings.json";

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Settings {
    provider: String,
    model: String,
}

fn load_settings() -> Settings {
    match fs::read_to_string(SETTINGS_FILE) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

fn save_settings(provider: &str, model: &str) {
    let settings = Settings {
        provider: provider.to_string(),
        model: model.to_string(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&settings) {
        let _ = fs::write(SETTINGS_FILE, json);
    }
}

async fn discover_providers() -> Vec<SettingsProvider> {
    let mut providers = Vec::new();

    // Mimo - always available (panics if MIMO_API_KEY not set)
    let _mimo_provider = MimoProvider::new(None, None, None);
    let mimo_models = vec![
        "mimo-v2.5-pro".to_string(),
        "mimo-v2-pro".to_string(),
        "mimo-v2.5".to_string(),
        "mimo-v2-omni".to_string(),
        "mimo-v2-flash".to_string(),
    ];
    providers.push(SettingsProvider {
        name: "mimo".to_string(),
        models: mimo_models,
    });

    // MiniMax
    if std::env::var("MINIMAX_API_KEY").is_ok() && std::env::var("MINIMAX_BASE_URL").is_ok() {
        let minimax_provider = agentik_sdk::provider::minimax::MinimaxProvider::new(None, None, None);
        let minimax_models = minimax_provider
            .list_models()
            .await
            .map(|ms| ms.into_iter().map(|m| m.model_info.model_name).collect())
            .unwrap_or_else(|_| vec!["MiniMax-M2.7".to_string()]);
        providers.push(SettingsProvider {
            name: "minimax".to_string(),
            models: minimax_models,
        });
    }

    providers
}

fn build_pool(provider: &str, model: &str) -> Option<ModelPool> {
    use agentik_sdk::provider::LlmProvider;
    match provider {
        "mimo" => {
            let mimo_provider = MimoProvider::new(None, None, None);
            let m = mimo_provider.get_model(model).ok()?;
            let mut pool = ModelPool::new();
            pool.add_model(m);
            Some(pool)
        }
        "minimax" => {
            let minimax_provider = agentik_sdk::provider::minimax::MinimaxProvider::new(None, None, None);
            let m = minimax_provider.get_model(model).ok()?;
            let mut pool = ModelPool::new();
            pool.add_model(m);
            Some(pool)
        }
        _ => None,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    dotenvy::dotenv_override().unwrap();

    let db_path = std::env::var("KMS_DB_PATH").unwrap_or_else(|_| "data/kms_sqlite.db".to_string());
    let svc = KmsService::new(&db_path).await.map_err(|e| e.to_string())?;

    let providers = discover_providers().await;
    if providers.is_empty() {
        eprintln!("Error: No LLM providers available. Set MIMO_API_KEY or MINIMAX_API_KEY.");
        std::process::exit(1);
    }

    let settings = load_settings();

    let current_provider = if providers.iter().any(|p| p.name == settings.provider) {
        settings.provider.clone()
    } else {
        providers[0].name.clone()
    };

    let provider_idx = providers.iter().position(|p| p.name == current_provider).unwrap_or(0);
    let current_model = if providers[provider_idx].models.contains(&settings.model) {
        settings.model.clone()
    } else {
        providers[provider_idx].models[0].clone()
    };

    let pool = build_pool(&current_provider, &current_model)
        .expect("Failed to build model pool for selected provider/model");
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

    let mut agents: HashMap<AgentKind, Arc<tokio::sync::Mutex<agentik_core::Agent>>> =
        HashMap::new();
    agents.insert(AgentKind::Compose, Arc::new(tokio::sync::Mutex::new(compose_agent)));
    agents.insert(
        AgentKind::Knowledge,
        Arc::new(tokio::sync::Mutex::new(knowledge_agent)),
    );

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;

    let mut app = App::new(
        svc,
        agents,
        providers,
        current_provider.clone(),
        current_model.clone(),
    );

    save_settings(&current_provider, &current_model);

    // Initial load: knowledge tree
    let root_children = app.svc.get_children(None).await?;
    let mut stack: Vec<(kms::Index, usize)> = root_children.into_iter().map(|c| (c, 0)).collect();

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
