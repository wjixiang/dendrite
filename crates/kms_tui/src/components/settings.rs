use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::state::{App, SettingsPane};
use crate::theme::Theme;

pub fn render_settings_modal(f: &mut Frame, app: &App, theme: &Theme) {
    let area = f.area();
    let total_providers = app.providers.len().max(1);
    let current_provider_idx = app
        .providers
        .iter()
        .position(|p| p.name == app.current_provider)
        .unwrap_or(0);
    let current_models: &[String] = app
        .providers
        .get(current_provider_idx)
        .map(|p| p.models.as_slice())
        .unwrap_or(&[]);
    let total_rows = total_providers.max(current_models.len()) + 4;

    let width = 60.min(area.width.saturating_sub(4));
    let height = (total_rows as u16).min(area.height.saturating_sub(2));
    let left = (area.width.saturating_sub(width)) / 2;
    let top = (area.height.saturating_sub(height)) / 2;
    let modal_area = Rect::new(left, top, width, height);

    f.render_widget(
        Paragraph::new("").style(Style::default().bg(ratatui::style::Color::Black)),
        area,
    );

    let block = Block::default()
        .title(" \u{2699} Settings ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.modal_border))
        .style(Style::default().bg(theme.modal_bg));
    f.render_widget(block, modal_area);

    let half_width = width / 2;
    let provider_area = Rect::new(
        modal_area.x + 1,
        modal_area.y + 1,
        half_width.saturating_sub(1),
        height.saturating_sub(2),
    );
    let model_area = Rect::new(
        modal_area.x + half_width,
        modal_area.y + 1,
        half_width.saturating_sub(2),
        height.saturating_sub(2),
    );

    let provider_lines: Vec<Line> = app
        .providers
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let is_selected =
                i == app.settings_selected_provider && app.settings_pane == SettingsPane::Provider;
            let is_active = p.name == app.current_provider;
            let prefix = if is_active {
                " \u{25cf} "
            } else if is_selected {
                " > "
            } else {
                "   "
            };
            let style = if is_selected {
                theme.modal_highlight_style()
            } else {
                Style::default().fg(theme.text_primary)
            };
            Line::from(Span::styled(format!("{}{}", prefix, p.name), style))
        })
        .collect();
    let provider_pane_label = if app.settings_pane == SettingsPane::Provider {
        " Providers *"
    } else {
        " Providers"
    };
    let provider_block = Block::default()
        .title(provider_pane_label)
        .borders(Borders::ALL)
        .border_style(if app.settings_pane == SettingsPane::Provider {
            Style::default().fg(theme.modal_border)
        } else {
            Style::default().fg(theme.text_muted)
        });
    f.render_widget(
        Paragraph::new(provider_lines)
            .block(provider_block)
            .wrap(Wrap { trim: false }),
        provider_area,
    );

    let model_lines: Vec<Line> = current_models
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let is_selected =
                i == app.settings_selected_model && app.settings_pane == SettingsPane::Model;
            let is_current = m == &app.current_model;
            let prefix = if is_current {
                " \u{25cf} "
            } else if is_selected {
                " > "
            } else {
                "   "
            };
            let style = if is_selected {
                theme.modal_highlight_style()
            } else {
                Style::default().fg(theme.text_primary)
            };
            Line::from(Span::styled(format!("{}{}", prefix, m), style))
        })
        .collect();
    let model_pane_label = if app.settings_pane == SettingsPane::Model {
        " Models *"
    } else {
        " Models"
    };
    let model_block = Block::default()
        .title(model_pane_label)
        .borders(Borders::ALL)
        .border_style(if app.settings_pane == SettingsPane::Model {
            Style::default().fg(theme.modal_border)
        } else {
            Style::default().fg(theme.text_muted)
        });
    f.render_widget(
        Paragraph::new(model_lines)
            .block(model_block)
            .wrap(Wrap { trim: false }),
        model_area,
    );

    let footer_y = modal_area.y + height - 1;
    let footer_area = Rect::new(modal_area.x + 1, footer_y, width - 2, 1);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            " [Tab] switch pane  \u{2191}/\u{2193} navigate  \u{00b7}  [Enter] select  \u{00b7}  [Esc] close",
            Style::default().fg(theme.text_muted),
        ))),
        footer_area,
    );
}
