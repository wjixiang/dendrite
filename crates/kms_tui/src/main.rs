mod input;
mod layout;
mod state;
mod styles;
mod tree;
mod widgets;

use std::io;

use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use kms::KmsService;
use ratatui::{Terminal, style::Style};

use crate::state::App;
use crate::styles::style_diagnostic_line;
use crate::input::run_app;

type CrosstermBackend = ratatui::backend::CrosstermBackend<std::io::Stdout>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv_override().unwrap();
    let db_path = std::env::var("KMS_DB_PATH").unwrap_or_else(|_| "data/deepmem.db".to_string());

    let svc = KmsService::new(&db_path).await.map_err(|e| e.to_string())?;

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;

    let mut app = App::new(svc);

    // Initial load: knowledge tree — 栈式 DFS 遍历整棵索引树
    let root_children = app.svc.get_children(None).await?;
    let mut stack: Vec<(kms::Index, usize)> = root_children
        .into_iter()
        .map(|c| (c, 0))
        .collect();

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
