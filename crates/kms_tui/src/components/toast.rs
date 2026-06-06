use std::time::Instant;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::theme::Theme;

#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub variant: ToastVariant,
    pub created_at: Instant,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastVariant {
    Info,
    Success,
    #[allow(dead_code)]
    Warning,
    #[allow(dead_code)]
    Error,
}

#[derive(Debug, Default)]
pub struct ToastManager {
    toasts: Vec<Toast>,
}

impl ToastManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, message: impl Into<String>, variant: ToastVariant) {
        self.toasts.push(Toast {
            message: message.into(),
            variant,
            created_at: Instant::now(),
            duration_ms: 3000,
        });
        if self.toasts.len() > 5 {
            self.toasts.remove(0);
        }
    }

    pub fn info(&mut self, message: impl Into<String>) {
        self.add(message, ToastVariant::Info);
    }

    pub fn success(&mut self, message: impl Into<String>) {
        self.add(message, ToastVariant::Success);
    }

    #[allow(dead_code)]
    pub fn warning(&mut self, message: impl Into<String>) {
        self.add(message, ToastVariant::Warning);
    }

    #[allow(dead_code)]
    pub fn error(&mut self, message: impl Into<String>) {
        self.add(message, ToastVariant::Error);
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        self.toasts
            .retain(|t| (now.duration_since(t.created_at).as_millis() as u64) < t.duration_ms);
    }

    pub fn render(&self, f: &mut Frame, theme: &Theme) {
        if self.toasts.is_empty() {
            return;
        }
        let area = f.area();
        let max_width = 40.min(area.width);
        for (i, toast) in self.toasts.iter().enumerate() {
            let y = area.y + 1 + i as u16;
            let x = area.x + area.width.saturating_sub(max_width + 2);
            let toast_area = Rect::new(x, y, max_width + 2, 1);
            let (icon, color) = match toast.variant {
                ToastVariant::Info => ("\u{2139}", theme.info),
                ToastVariant::Success => ("\u{2713}", theme.success),
                ToastVariant::Warning => ("\u{26a0}", theme.warning),
                ToastVariant::Error => ("\u{2717}", theme.error),
            };
            let text = format!(" {} {} ", icon, toast.message);
            let style = Style::default()
                .fg(color)
                .add_modifier(Modifier::BOLD);
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(text, style))),
                toast_area,
            );
        }
    }
}
