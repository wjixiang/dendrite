use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::state::{App, SettingsPane};
use crate::theme::Theme;

const ELLIPSIS: &str = "\u{2026}";
const DOT: &str = "\u{25cf}";
const CHECK_ON: &str = "[x]";
const CHECK_OFF: &str = "[ ]";

/// Truncate `s` to at most `max_chars` characters, replacing the tail
/// with `\u{2026}` (`…`) when it doesn't fit. Counts characters (not
/// display cells) so it matches how ratatui's `Paragraph` lays out
/// `Line`s. All prefixes used in this file are 1-cell symbols, so
/// char-counting is accurate for them.
fn fit_chars(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let n = s.chars().count();
    if n <= max_chars {
        return s.to_string();
    }
    if max_chars == 1 {
        return ELLIPSIS.to_string();
    }
    let mut out: String = s.chars().take(max_chars - 1).collect();
    out.push('\u{2026}');
    out
}

/// A row inside a pane: a left-padding prefix (`  ` or `▶ `), the
/// variable text, and (optionally) a right-side indicator. The whole
/// row is truncated to `width` so it can never wrap and break the
/// pane's border.
fn pane_row(prefix: &str, text: &str, suffix: &str, width: usize) -> String {
    let prefix_len = prefix.chars().count();
    let suffix_len = suffix.chars().count();
    let text_w = width.saturating_sub(prefix_len + suffix_len);
    let text = fit_chars(text, text_w);
    format!("{}{}{}", prefix, text, suffix)
}

/// Build the per-pane border style: brighter when the pane is the
/// active one, muted otherwise.
fn pane_border_style(focused: bool, theme: &Theme) -> Style {
    if focused {
        Style::default().fg(theme.modal_border)
    } else {
        Style::default().fg(theme.text_muted)
    }
}

/// Build the focused/unfocused pane title (e.g. ` Providers (3) `).
/// Appends a small `(active)` tag on the focused pane so the user
/// can always tell which sub-section receives key events.
fn pane_title(label: &str, count: usize, focused: bool) -> String {
    if focused {
        format!(" {} ({}) \u{25c0} ", label, count)
    } else {
        format!(" {} ({}) ", label, count)
    }
}

/// Compute the modal rect, plus the inner height of the providers/
/// models top row and the pool row. Border math is explicit and
/// additive: every row is accounted for, nothing is hidden in a
/// magic `+3`.
struct LayoutInfo {
    /// The full outer modal rect (including its own border).
    modal: Rect,
    /// Inside the outer border. The vertical split happens here.
    inner: Rect,
    /// Height (in rows) reserved for the top providers/models row.
    /// Each sub-pane has its own border, so this is `max(items) + 2`.
    top_h: u16,
    /// Height reserved for the pool row (border + content).
    pool_h: u16,
}

fn compute_layout(area: Rect, providers_len: usize, models_len: usize, pool_len: usize) -> LayoutInfo {
    // `pool_len` is at least 1 so the empty-state row ("(empty —
    // select models above)") is always visible. The other two are
    // raw list sizes.
    let pool_len = pool_len.max(1);

    // Each sub-pane has a 1-row top + 1-row bottom border. The
    // content inside is the number of items in the list.
    let top_h = providers_len.max(models_len) as u16 + 2;
    let pool_h = pool_len as u16 + 2;
    let footer_h: u16 = 1;
    // One blank row between the top section and the pool section,
    // so they read as two separate groups rather than one wall.
    let gap_h: u16 = 1;
    let inner_h = top_h + gap_h + pool_h + footer_h;
    let modal_h = (inner_h + 2).min(area.height.saturating_sub(2));
    // 80 cols is comfortable; 50 is the absolute minimum so the
    // longest model name we expect to render doesn't get squashed
    // to nothing.
    let modal_w = 80u16.min(area.width.saturating_sub(4)).max(50);
    let modal_x = (area.width.saturating_sub(modal_w)) / 2;
    let modal_y = (area.height.saturating_sub(modal_h)) / 2;
    let modal = Rect::new(modal_x, modal_y, modal_w, modal_h);
    let inner = Rect::new(
        modal.x + 1,
        modal.y + 1,
        modal.width.saturating_sub(2),
        modal.height.saturating_sub(2),
    );
    LayoutInfo {
        modal,
        inner,
        top_h,
        pool_h,
    }
}

pub fn render_settings_modal(f: &mut Frame, app: &App, theme: &Theme) {
    let area = f.area();
    let providers_len = app.providers.len();
    let models_len = app
        .providers
        .get(app.settings_selected_provider)
        .map(|p| p.models.len())
        .unwrap_or(0);
    let pool_len = app.pool_entries.len();
    let layout = compute_layout(area, providers_len, models_len, pool_len);

    // Dim the background behind the modal so the focused pane reads
    // as the foreground.
    f.render_widget(
        Paragraph::new("").style(Style::default().bg(Color::Black)),
        area,
    );

    // Outer block.
    let outer = Block::default()
        .title(" \u{2699} Settings ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.modal_border))
        .style(Style::default().bg(theme.modal_bg));
    f.render_widget(outer, layout.modal);

    // Inner area split into top section, gap, pool, footer.
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(layout.top_h), // providers + models
            Constraint::Length(1),           // gap row
            Constraint::Length(layout.pool_h),
            Constraint::Length(1), // footer
        ])
        .split(layout.inner);

    // Top: providers (40%) | models (60%).
    let top_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(sections[0]);

    render_providers_pane(f, app, theme, top_cols[0]);
    render_models_pane(f, app, theme, top_cols[1]);
    render_pool_pane(f, app, theme, sections[2]);
    render_footer(f, app, theme, sections[3]);
}

fn render_providers_pane(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let focused = app.settings_pane == SettingsPane::Provider;
    let title = pane_title("Providers", app.providers.len(), focused);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(pane_border_style(focused, theme))
        .style(Style::default().bg(theme.modal_bg));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // 1 char left padding, then the row prefix, then the name, then
    // the "in pool" indicator, then 1 char right padding.
    let content_w = inner.width.saturating_sub(2) as usize;
    let visible = inner.height as usize;

    if app.providers.is_empty() {
        let placeholder = Line::from(Span::styled(
            " (no providers \u{2014} press [n])",
            Style::default().fg(theme.text_muted),
        ));
        f.render_widget(Paragraph::new(placeholder), inner);
        return;
    }

    let mut lines: Vec<Line> = app
        .providers
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let is_selected = i == app.settings_selected_provider && focused;
            let in_pool = app.pool_entries.iter().any(|e| e.provider_id == p.id);
            let prefix = if is_selected { "▶ " } else { "  " };
            // Use the short label so the type tag is visible right
            // inside the providers list (matches the Pool pane).
            let suffix = if in_pool { format!(" {}", DOT) } else { String::new() };
            let row = pane_row(prefix, &p.short_label(), &suffix, content_w);
            let style = if is_selected {
                theme.modal_highlight_style()
            } else if in_pool {
                Style::default().fg(theme.success)
            } else {
                Style::default().fg(theme.text_primary)
            };
            Line::from(Span::styled(row, style))
        })
        .collect();

    push_overflow_hint(&mut lines, app.providers.len(), visible, theme);
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme.modal_bg)),
        inner,
    );
}

fn render_models_pane(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let focused = app.settings_pane == SettingsPane::Model;
    let provider = app.providers.get(app.settings_selected_provider);
    let model_count = provider.map(|p| p.models.len()).unwrap_or(0);
    let title = pane_title("Models", model_count, focused);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(pane_border_style(focused, theme))
        .style(Style::default().bg(theme.modal_bg));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let content_w = inner.width.saturating_sub(2) as usize;
    let visible = inner.height as usize;

    let Some(p) = provider else {
        let placeholder = Line::from(Span::styled(
            " (no provider selected)",
            Style::default().fg(theme.text_muted),
        ));
        f.render_widget(Paragraph::new(placeholder), inner);
        return;
    };

    if p.models.is_empty() {
        let placeholder = Line::from(Span::styled(
            " (no models for this provider)",
            Style::default().fg(theme.text_muted),
        ));
        f.render_widget(Paragraph::new(placeholder), inner);
        return;
    }

    let mut lines: Vec<Line> = p
        .models
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let is_selected = i == app.settings_selected_model && focused;
            let in_pool = app.is_in_pool(&p.id, m);
            let prefix = if is_selected { "▶ " } else { "  " };
            let check = if in_pool { CHECK_ON } else { CHECK_OFF };
            let suffix = format!(" {}", check);
            let row = pane_row(prefix, m, &suffix, content_w);
            let style = if is_selected {
                theme.modal_highlight_style()
            } else if in_pool {
                Style::default().fg(theme.success)
            } else {
                Style::default().fg(theme.text_primary)
            };
            Line::from(Span::styled(row, style))
        })
        .collect();

    push_overflow_hint(&mut lines, p.models.len(), visible, theme);
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme.modal_bg)),
        inner,
    );
}

fn render_pool_pane(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let focused = app.settings_pane == SettingsPane::Pool;
    let n = app.pool_entries.len();
    let title = format!(
        " Pool ({n} model{plural}) ",
        n = n,
        plural = if n == 1 { "" } else { "s" }
    );
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(pane_border_style(focused, theme))
        .style(Style::default().bg(theme.modal_bg));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let content_w = inner.width.saturating_sub(2) as usize;
    let visible = inner.height as usize;

    if app.pool_entries.is_empty() {
        let placeholder = Line::from(Span::styled(
            " (empty \u{2014} select models above)",
            Style::default().fg(theme.text_muted),
        ));
        f.render_widget(Paragraph::new(placeholder), inner);
        return;
    }

    // The pool row layout is `[prefix] [x] prov / model`. Compute a
    // fixed width for the provider and the model so the `/` lines
    // up across rows. Split is 40/60 by default, but the model gets
    // at least 8 chars (most model names are longer than provider
    // names anyway) and the provider gets at least 4.
    let fixed = 2 + 3 + 1 + 3; // prefix + "[x]"/"[ ]" + " " + " / "
    let avail = content_w.saturating_sub(fixed);
    let prov_w = (avail * 2 / 5).max(4);
    let model_w = avail.saturating_sub(prov_w).max(8);

    let mut lines: Vec<Line> = app
        .pool_entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let is_selected = i == app.settings_selected_pool && focused;
            let prefix = if is_selected { "▶ " } else { "  " };
            let prov_name = app
                .providers
                .iter()
                .find(|p| p.id == e.provider_id)
                .map(|p| p.short_label())
                .unwrap_or_else(|| e.provider_id.clone());
            let prov = fit_chars(&prov_name, prov_w);
            let model = fit_chars(&e.model, model_w);
            let row = format!("{prefix}{check} {prov} / {model}", check = CHECK_ON);
            let style = if is_selected {
                theme.modal_highlight_style()
            } else {
                Style::default().fg(theme.success)
            };
            Line::from(Span::styled(row, style))
        })
        .collect();

    push_overflow_hint(&mut lines, app.pool_entries.len(), visible, theme);
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme.modal_bg)),
        inner,
    );
}

/// If the list has more items than fit, replace the last row with
/// a `(+N more)` line so the user knows there is content they're
/// not seeing. The hint uses a muted italic style and is
/// left-padded to match the list rows. When the hint is shown, the
/// caller must be ready for `lines` to be shorter than `total`
/// after this call returns.
fn push_overflow_hint(lines: &mut Vec<Line>, total: usize, visible: usize, theme: &Theme) {
    if visible == 0 || total <= visible {
        return;
    }
    let hint = Line::from(Span::styled(
        format!(" (+{} more)", total - visible + 1),
        Style::default()
            .fg(theme.text_muted)
            .add_modifier(Modifier::ITALIC),
    ));
    // Truncate the existing rows so they fit alongside the hint,
    // then append the hint. The visible pane will render at most
    // `visible` rows, so anything past `visible - 1` is shadowed
    // by the hint anyway.
    lines.truncate(visible.saturating_sub(1));
    lines.push(hint);
}

fn render_footer(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let hint = match app.settings_pane {
        SettingsPane::Provider => {
            "[Tab] pane   [\u{2191}/\u{2193}] or [j/k] nav   [n] new   [r] del   [Enter] apply   [Esc] close"
        }
        SettingsPane::Model => {
            "[Tab] pane   [\u{2191}/\u{2193}] or [j/k] nav   [Space] toggle   [Enter] apply   [Esc] close"
        }
        SettingsPane::Pool => {
            "[Tab] pane   [\u{2191}/\u{2193}] or [j/k] nav   [d] remove   [Enter] apply   [Esc] close"
        }
    };
    let key_style = Style::default().fg(theme.accent);
    let dim = Style::default().fg(theme.text_muted);
    let spans = footer_spans(hint, key_style, dim);
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Parse a footer hint of the form `[key] label   [key] label …`
/// into styled `Span`s. The bracketed keys are highlighted in
/// the accent color, everything else is dimmed. Unknown segments
/// fall through as plain dim text.
fn footer_spans(
    hint: &str,
    key_style: Style,
    dim: Style,
) -> Vec<Span<'static>> {
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut rest = hint;
    while let Some(open) = rest.find('[') {
        if open > 0 {
            out.push(Span::styled(rest[..open].to_string(), dim));
        }
        let after_open = &rest[open + 1..];
        if let Some(close) = after_open.find(']') {
            let key = &after_open[..close];
            out.push(Span::styled(format!("[{}]", key), key_style));
            rest = &after_open[close + 1..];
        } else {
            out.push(Span::styled(rest[open..].to_string(), dim));
            rest = "";
            break;
        }
    }
    if !rest.is_empty() {
        out.push(Span::styled(rest.to_string(), dim));
    }
    out
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

    let modal_w = 64u16.min(area.width.saturating_sub(4)).max(50);
    let modal_h = 16u16.min(area.height.saturating_sub(2)).max(12);
    let modal_x = (area.width.saturating_sub(modal_w)) / 2;
    let modal_y = (area.height.saturating_sub(modal_h)) / 2;
    let modal_area = Rect::new(modal_x, modal_y, modal_w, modal_h);

    // The form is rendered on top of the settings modal which
    // already has its own border, so we deliberately do NOT add
    // another bordered block here — the form is just a floating
    // title + text panel inside the existing modal. The dark
    // background is enough to make it feel like its own layer.
    f.render_widget(
        Paragraph::new("").style(Style::default().bg(theme.modal_bg)),
        modal_area,
    );

    let inner = modal_area;
    // Field labels are 10 chars wide (` URL:     `) so the values
    // line up vertically. This is the one piece of layout in the
    // form that previously drifted depending on label length.
    let label_w = 10;

    let label_style = |active: bool, t: &Theme| {
        if active {
            Style::default()
                .fg(t.modal_selected_fg)
                .bg(t.modal_selected_bg)
                .add_modifier(Modifier::BOLD)
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

    // Available width for values inside the form (after the label
    // and one separating space). Truncate to this so the value
    // can never wrap and break the panel.
    let value_w = (inner.width as usize).saturating_sub(label_w + 1);

    // Field 0: provider type
    let type_label = format!(" Type:    ");
    let type_value = format!("< {} >", ptype);
    let type_line = Line::from(vec![
        Span::styled(type_label, label_style(form.active_field == 0, theme)),
        Span::styled(
            fit_chars(&type_value, value_w),
            value_style(form.active_field == 0, theme),
        ),
    ]);
    // Field 1: display name
    let cursor = if form.active_field == 1 { "|" } else { "" };
    let name_label = format!(" Name:    ");
    let name_value = format!("{}{}", form.display_name, cursor);
    let name_line = Line::from(vec![
        Span::styled(name_label, label_style(form.active_field == 1, theme)),
        Span::styled(
            fit_chars(&name_value, value_w),
            value_style(form.active_field == 1, theme),
        ),
    ]);
    // Field 2: API key (masked)
    let masked: String = "\u{2022}".repeat(form.api_key.chars().count());
    let key_label = format!(" API key: ");
    let key_value = format!(
        "{}{}",
        masked,
        if form.active_field == 2 { "|" } else { "" }
    );
    let key_line = Line::from(vec![
        Span::styled(key_label, label_style(form.active_field == 2, theme)),
        Span::styled(
            fit_chars(&key_value, value_w),
            value_style(form.active_field == 2, theme),
        ),
    ]);
    // Field 3: base URL — rendered as a preset selector. By default
    // it shows `< preset label >` and the user cycles with ↑/↓.
    // The last entry in each provider's preset list is a "Custom…"
    // mode that flips the field into a text input.
    let url_label = form.url_label();
    let url_active = form.active_field == 3;
    let url_value = if form.url_is_custom() {
        let cur = if url_active { "|" } else { "" };
        format!("{}{}", url_label, cur)
    } else {
        format!("\u{25c0} {} \u{25b6}", url_label)
    };
    let url_field_label = format!(" URL:     ");
    let url_line = Line::from(vec![
        Span::styled(url_field_label, label_style(url_active, theme)),
        Span::styled(
            fit_chars(&url_value, value_w),
            value_style(url_active, theme),
        ),
    ]);

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            " \u{2795}  Add Provider",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
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
        lines.push(Line::from(""));
    }

    // ── Bottom action hints ──
    // Each hint is `[key] label`, with the key in the accent color
    // (so it stands out) and the label in muted text. The hint
    // block is grouped into two rows so the form stays compact.
    let dim = Style::default().fg(theme.text_muted);
    let sep = Span::styled("  \u{2502}  ", dim);

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

    let row1 = Line::from(footer_spans(
        &format!(
            " [Tab] next field   [{url_key}] {url_label}   [Backspace] delete",
            url_key = url_hint.0,
            url_label = url_hint.1
        ),
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
        dim,
    ));
    let row2 = Line::from(footer_spans(
        " [Enter] save   [Esc] cancel   credentials live in data/settings.json",
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
        dim,
    ));

    lines.push(row1);
    lines.push(Line::from(sep));
    lines.push(row2);

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── fit_chars ──

    #[test]
    fn fit_returns_input_when_short() {
        assert_eq!(fit_chars("hello", 10), "hello");
    }

    #[test]
    fn fit_truncates_with_ellipsis_when_long() {
        let out = fit_chars("hello world", 6);
        assert_eq!(out.chars().count(), 6);
        assert!(out.ends_with('\u{2026}'));
    }

    #[test]
    fn fit_handles_zero_max() {
        assert_eq!(fit_chars("hello", 0), "");
    }

    #[test]
    fn fit_handles_max_one() {
        assert_eq!(fit_chars("hello", 1), "\u{2026}");
    }

    #[test]
    fn fit_preserves_exact_length() {
        assert_eq!(fit_chars("abcde", 5), "abcde");
    }

    // ── pane_row ──

    #[test]
    fn pane_row_fits_short_text() {
        let r = pane_row("▶ ", "mimo", "", 10);
        assert_eq!(r, "▶ mimo");
    }

    #[test]
    fn pane_row_truncates_long_text() {
        let r = pane_row("▶ ", "a-very-long-provider-name", " ●", 15);
        assert!(r.chars().count() <= 15);
        assert!(r.contains('\u{2026}'));
    }

    #[test]
    fn pane_row_handles_zero_width() {
        // Should never panic, even at degenerate widths.
        let r = pane_row("▶ ", "mimo", " ●", 0);
        assert!(r.chars().count() <= 4);
    }

    // ── footer_spans ──

    #[test]
    fn footer_spans_splits_keys_and_labels() {
        let spans = footer_spans(
            "[Tab] pane   [Esc] close",
            Style::default(),
            Style::default(),
        );
        // 4 segments: "[Tab]" key, " pane   " dim, "[Esc]" key, " close" dim
        assert_eq!(spans.len(), 4);
    }

    #[test]
    fn footer_spans_passes_through_plain_text() {
        let spans = footer_spans("no keys here", Style::default(), Style::default());
        assert_eq!(spans.len(), 1);
    }

    // ── push_overflow_hint ──

    #[test]
    fn overflow_hint_appears_when_list_too_long() {
        let theme = Theme::default_theme();
        let mut lines: Vec<Line> = (0..10).map(|i| Line::from(format!("item {i}"))).collect();
        push_overflow_hint(&mut lines, 10, 5, &theme);
        let last = &lines[lines.len() - 1];
        let text: String = last
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(text.contains("+"), "expected hint to mention hidden count: {text:?}");
    }

    #[test]
    fn overflow_hint_absent_when_list_fits() {
        let theme = Theme::default_theme();
        let mut lines: Vec<Line> = (0..3).map(|i| Line::from(format!("item {i}"))).collect();
        let before = lines.len();
        push_overflow_hint(&mut lines, 3, 5, &theme);
        assert_eq!(lines.len(), before);
    }

    // ── compute_layout ──

    #[test]
    fn layout_inner_h_matches_split() {
        // Inner height = top_h + gap + pool_h + footer
        // Outer height  = inner_h + 2 (modal border)
        // All slices must sum to inner height without remainder.
        let area = Rect::new(0, 0, 100, 40);
        let info = compute_layout(area, 3, 4, 1);
        let total = info.top_h + 1 + info.pool_h + 1;
        assert_eq!(info.inner.height, total);
        assert!(info.modal.width >= 50);
        assert!(info.modal.height <= area.height);
    }

    #[test]
    fn layout_picks_taller_of_top_panes() {
        // The top row should always fit the larger of the two
        // sub-panes; otherwise the smaller pane is wasted space.
        let info = compute_layout(Rect::new(0, 0, 100, 40), 3, 10, 1);
        assert!(info.top_h >= 12); // 10 items + 2 border
        let info2 = compute_layout(Rect::new(0, 0, 100, 40), 10, 3, 1);
        assert!(info2.top_h >= 12);
    }

    #[test]
    fn layout_pool_never_collapses() {
        // An empty pool still needs at least 1 row so the
        // "empty — select models above" hint is visible.
        let info = compute_layout(Rect::new(0, 0, 100, 40), 3, 3, 0);
        assert!(info.pool_h >= 3); // 1 row + 2 border
    }
}
