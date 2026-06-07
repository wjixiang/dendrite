use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::state::{App, SettingsPane};
use crate::theme::Theme;

pub fn render_settings_modal(f: &mut Frame, app: &App, theme: &Theme) {
    let area = f.area();

    // Compute dynamic height based on content.
    let providers_row_count = app.providers.len() as u16 + 3; // +border+title
    let selected_models = app
        .providers
        .get(app.settings_selected_provider)
        .map(|p| p.models.len() as u16)
        .unwrap_or(0);
    let models_row_count = selected_models + 3;
    let pool_row_count = app.pool_entries.len().max(1) as u16 + 3;
    let top_rows = providers_row_count.max(models_row_count);
    let total_height = top_rows + pool_row_count + 3; // +outer border + footer

    let width = 70.min(area.width.saturating_sub(4));
    let height = (total_height as u16).min(area.height.saturating_sub(2));
    let left = (area.width.saturating_sub(width)) / 2;
    let top = (area.height.saturating_sub(height)) / 2;
    let modal_area = Rect::new(left, top, width, height);

    // Dim the background behind the modal.
    f.render_widget(
        Paragraph::new("").style(Style::default().bg(ratatui::style::Color::Black)),
        area,
    );

    // Outer block.
    let block = Block::default()
        .title(" \u{2699} Settings ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.modal_border))
        .style(Style::default().bg(theme.modal_bg));
    f.render_widget(block, modal_area);

    // Inner area (inside borders).
    let inner = Rect::new(
        modal_area.x + 1,
        modal_area.y + 1,
        modal_area.width.saturating_sub(2),
        modal_area.height.saturating_sub(2),
    );

    // Vertical split: top (providers+models) | pool | footer.
    let top_height = top_rows.min(inner.height.saturating_sub(pool_row_count + 1));
    let vertical_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_height),
            Constraint::Min(pool_row_count),
            Constraint::Length(1), // footer
        ])
        .split(inner);

    // Top horizontal split: providers | models.
    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(vertical_chunks[0]);

    // ── Providers pane ──
    let provider_lines: Vec<Line> = app
        .providers
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let is_selected =
                i == app.settings_selected_provider && app.settings_pane == SettingsPane::Provider;
            let has_pool_models = app
                .pool_entries
                .iter()
                .any(|e| e.provider_id == p.id);
            let prefix = if is_selected { " > " } else { "   " };
            let indicator = if has_pool_models { " \u{25cf}" } else { "" };
            let style = if is_selected {
                theme.modal_highlight_style()
            } else {
                Style::default().fg(theme.text_primary)
            };
            Line::from(Span::styled(
                format!("{}{}{}", prefix, p.display_name, indicator),
                style,
            ))
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
        top_chunks[0],
    );

    // ── Models pane (shows models for the SELECTED provider, not active) ──
    let selected_provider = app.providers.get(app.settings_selected_provider);
    let model_lines: Vec<Line> = selected_provider
        .map(|p| {
            p.models
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let is_selected = i == app.settings_selected_model
                        && app.settings_pane == SettingsPane::Model;
                    let in_pool = app.is_in_pool(&p.id, m);
                    let checkbox = if in_pool { "[x]" } else { "[ ]" };
                    let style = if is_selected {
                        theme.modal_highlight_style()
                    } else if in_pool {
                        Style::default().fg(theme.success)
                    } else {
                        Style::default().fg(theme.text_primary)
                    };
                    Line::from(Span::styled(format!(" {} {}", checkbox, m), style))
                })
                .collect()
        })
        .unwrap_or_default();

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
        top_chunks[1],
    );

    // ── Pool pane ──
    let pool_title = format!(" Pool ({} model{}) ", app.pool_entries.len(),
        if app.pool_entries.len() == 1 { "" } else { "s" });
    let pool_lines: Vec<Line> = if app.pool_entries.is_empty() {
        vec![Line::from(Span::styled(
            "  (empty \u{2014} select models above)",
            Style::default().fg(theme.text_muted),
        ))]
    } else {
        app.pool_entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let is_selected =
                    i == app.settings_selected_pool && app.settings_pane == SettingsPane::Pool;
                let prefix = if is_selected { " > " } else { "   " };
                let style = if is_selected {
                    theme.modal_highlight_style()
                } else {
                    Style::default().fg(theme.success)
                };
                Line::from(Span::styled(
                    format!(
                        "{}[x] {} / {}",
                        prefix,
                        app.providers
                            .iter()
                            .find(|p| p.id == e.provider_id)
                            .map(|p| p.short_label())
                            .unwrap_or_else(|| e.provider_id.clone()),
                        e.model
                    ),
                    style,
                ))
            })
            .collect()
    };
    let pool_block = Block::default()
        .title(pool_title)
        .borders(Borders::ALL)
        .border_style(if app.settings_pane == SettingsPane::Pool {
            Style::default().fg(theme.modal_border)
        } else {
            Style::default().fg(theme.text_muted)
        });
    f.render_widget(
        Paragraph::new(pool_lines)
            .block(pool_block)
            .wrap(Wrap { trim: false }),
        vertical_chunks[1],
    );

    // ── Footer ──
    let footer = match app.settings_pane {
        SettingsPane::Provider => {
            "[Tab] pane   [j/k] nav   [n] new   [r] del   [Esc] close"
        }
        SettingsPane::Model => {
            "[Tab] pane   [j/k] nav   [Space] toggle   [Enter] apply   [Esc] close"
        }
        SettingsPane::Pool => {
            "[Tab] pane   [j/k] nav   [d] remove   [Enter] apply   [Esc] close"
        }
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            footer.to_string(),
            Style::default().fg(theme.text_muted),
        ))),
        vertical_chunks[2],
    );
}

/// Render the "add new provider" form modal on top of the regular
/// settings modal.
pub fn render_new_provider_form(f: &mut Frame, app: &App, theme: &Theme) {
    let area = f.area();
    let form = match app.new_provider_form.as_ref() {
        Some(f) => f,
        None => return,
    };
    let ptype = crate::settings::BUILTIN_PROVIDER_TYPES[form.type_idx];

    let width = 70.min(area.width.saturating_sub(4));
    let height = 15.min(area.height.saturating_sub(2));
    let left = (area.width.saturating_sub(width)) / 2;
    let top = (area.height.saturating_sub(height)) / 2;
    let modal_area = Rect::new(left, top, width, height);

    // The form is rendered on top of the settings modal which already
    // has its own border, so we deliberately do NOT add another
    // Bordered block here — the form is just a floating title +
    // text panel inside the existing modal. The dark background is
    // enough to make it feel like its own layer.
    f.render_widget(
        Paragraph::new("").style(Style::default().bg(theme.modal_bg)),
        modal_area,
    );

    let inner = modal_area;

    let label_style = |active: bool, t: &Theme| {
        if active {
            Style::default()
                .fg(t.modal_selected_fg)
                .bg(t.modal_selected_bg)
                .add_modifier(ratatui::style::Modifier::BOLD)
        } else {
            Style::default().fg(t.text_secondary)
        }
    };
    let value_style = |active: bool, t: &Theme| {
        if active {
            Style::default().fg(t.accent)
        } else {
            Style::default().fg(t.text_primary)
        }
    };

    // Field 0: provider type
    let type_line = Line::from(vec![
        Span::styled(" Type:    ", label_style(form.active_field == 0, theme)),
        Span::styled(
            format!("< {} >", ptype),
            value_style(form.active_field == 0, theme),
        ),
    ]);
    // Field 1: display name
    let cursor = if form.active_field == 1 { "|" } else { "" };
    let name_line = Line::from(vec![
        Span::styled(" Name:    ", label_style(form.active_field == 1, theme)),
        Span::styled(
            format!("{}{}", form.display_name, cursor),
            value_style(form.active_field == 1, theme),
        ),
    ]);
    // Field 2: API key (masked)
    let masked: String = "\u{2022}".repeat(form.api_key.chars().count());
    let key_line = Line::from(vec![
        Span::styled(" API key: ", label_style(form.active_field == 2, theme)),
        Span::styled(
            format!("{}{}", masked, if form.active_field == 2 { "|" } else { "" }),
            value_style(form.active_field == 2, theme),
        ),
    ]);
    // Field 3: base URL — rendered as a preset selector. By default
    // it shows `< preset label >` and the user cycles with ↑/↓. The
    // last entry in each provider's preset list is a "Custom..." mode
    // that flips the field into a text input.
    let url_label = form.url_label();
    let url_active = form.active_field == 3;
    let url_value = if form.url_is_custom() {
        // Custom mode: show what the user is typing, plus a cursor.
        let cur = if url_active { "|" } else { "" };
        format!("{}{}", url_label, cur)
    } else {
        // Preset mode: show the label bracketed with chevrons to
        // suggest that ↑/↓ cycles through the list.
        format!("\u{25c0} {} \u{25b6}", url_label)
    };
    let url_line = Line::from(vec![
        Span::styled(" URL:     ", label_style(url_active, theme)),
        Span::styled(url_value, value_style(url_active, theme)),
    ]);

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            " \u{2795}  Add Provider",
            Style::default()
                .fg(theme.accent)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )),
        Line::from(""),
        type_line,
        name_line,
        key_line,
        url_line,
        Line::from(""),
    ];
    if let Some(err) = &form.error {
        lines.push(Line::from(Span::styled(
            format!(" Error: {}", err),
            Style::default().fg(theme.error),
        )));
    }
    lines.push(Line::from(""));

    // ── Bottom action hints ──
    // Each hint is `[key] label`, with the key in the accent color
    // (so it stands out) and the label in muted text. The hint
    // block is grouped into two rows so the form stays compact.
    let key_style = Style::default()
        .fg(theme.accent)
        .add_modifier(ratatui::style::Modifier::BOLD);
    let dim = Style::default().fg(theme.text_muted);
    let sep = "\u{2502}";

    // Pick the URL row's hint based on the active field + mode.
    let url_hint = if form.active_field == 3 {
        if form.url_is_custom() {
            ("type", "URL")
        } else {
            ("\u{2191}/\u{2193}", "pick URL")
        }
    } else {
        ("\u{2191}/\u{2193}", "type")
    };

    let row1 = Line::from(vec![
        Span::styled(" [Tab]", key_style),
        Span::styled(" next field  ", dim),
        Span::styled(sep, dim),
        Span::styled(format!(" [{}] ", url_hint.0), key_style),
        Span::styled(format!("{}  ", url_hint.1), dim),
        Span::styled(sep, dim),
        Span::styled(" [Backspace]", key_style),
        Span::styled(" delete  ", dim),
    ]);
    let row2 = Line::from(vec![
        Span::styled(" [Enter]", key_style),
        Span::styled(" save  ", dim),
        Span::styled(sep, dim),
        Span::styled(" [Esc]", key_style),
        Span::styled(" cancel  ", dim),
        Span::styled(sep, dim),
        Span::styled(" credentials live in ", dim),
        Span::styled("data/settings.json", key_style),
    ]);

    lines.push(row1);
    lines.push(row2);

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}
