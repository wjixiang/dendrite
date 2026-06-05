mod input;
mod layout;
mod state;
mod styles;
mod tree;
mod widgets;

use std::io;
use std::sync::Arc;

use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use kms::KmsService;
use llm_api::model::model_pool::ModelPool;
use llm_api::provider::LlmProvider;
use llm_api::provider::mimo::{MODEL_MIMO_V2_5, MimoProvider};
use ratatui::{Terminal, style::Style};

use crate::input::run_app;
use crate::state::App;
use crate::styles::style_diagnostic_line;

type CrosstermBackend = ratatui::backend::CrosstermBackend<std::io::Stdout>;

/// Initialize tracing to write all logs (sqlx, agent, etc.) to a file.
/// Must be called before anything else to prevent log output from corrupting the TUI.
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // IMPORTANT: init logging FIRST, before any other crate initialization,
    // so that all tracing events go to the log file instead of the terminal.
    init_logging();

    dotenvy::dotenv_override().unwrap();

    let db_path = std::env::var("KMS_DB_PATH").unwrap_or_else(|_| "data/kms_sqlite.db".to_string());

    let svc = KmsService::new(&db_path).await.map_err(|e| e.to_string())?;

    // Build ModelPool
    let mimo_provider = MimoProvider::new(None, None, None);
    let model = mimo_provider.get_model(MODEL_MIMO_V2_5)?;
    let mut pool = ModelPool::new();
    pool.add_model(model);

    // Build Agent
    let agent = agent::Agent::builder()
        .with_model_pool(Arc::new(pool))
        .with_kms(Arc::new(svc.clone()))
        .build()
        .await
        .map_err(|e| e.to_string())?;

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;

    let mut app = App::new(svc, Arc::new(tokio::sync::Mutex::new(agent)));

    // Initial load: knowledge tree — 栈式 DFS 遍历整棵索引树
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

    if diagnostics.is_empty() {
        app.diagnostic_lines = vec![ratatui::text::Line::from(ratatui::text::Span::styled(
            "No issues found.".to_owned(),
            Style::default().fg(ratatui::style::Color::Green),
        ))];
    } else {
        let mut lines = vec![ratatui::text::Line::from(format!(
            "{} issues found:",
            diagnostics.len()
        ))];
        for d in &diagnostics {
            lines.push(style_diagnostic_line(&format!(
                "[{}] {} — {}",
                d.severity.label(),
                d.code,
                d.message
            )));
            lines.push(style_diagnostic_line(&d.location));
            for action in &d.suggested_actions {
                lines.push(style_diagnostic_line(&format!("  → {}", action)));
            }
            lines.push(ratatui::text::Line::from(""));
        }
        app.diagnostic_lines = lines;
    }

    app.on_tree_select().await;

    let result = run_app(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}
