use std::sync::Arc;

use kms::Index;
use ratatui::text::Line;
use ratatui::widgets::{ListItem, ListState};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Tree,
    KnowledgeEntity,
    Agent,
    Diagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeTab {
    Knowledge,
    Entity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPane {
    Provider,
    Model,
}

#[derive(Debug, Clone)]
pub struct SettingsProvider {
    pub name: String,
    pub models: Vec<String>,
}

pub enum Action {
    None,
    Quit,
    TreeChanged,
    SubmitAgent(String),
    OpenSettings,
    SettingsNav(SettingsPane, isize),
    SettingsSwitchPane(SettingsPane),
    SettingsConfirm,
}

pub struct App {
    pub should_quit: bool,
    pub tree_items: Vec<ListItem<'static>>,
    pub tree_nodes: Vec<Index>,
    pub tree_state: ListState,
    pub diagnostic_lines: Vec<Line<'static>>,
    pub scroll_diag: u16,
    pub knowledge_lines: Vec<Line<'static>>,
    pub entity_lines: Vec<Line<'static>>,
    pub agent_lines: Vec<Line<'static>>,
    pub agent_scroll: usize,
    pub agent_visible_height: usize,
    pub focused: Panel,
    pub svc: kms::KmsService,
    pub ke_tab: KeTab,
    pub ke_scroll: u16,
    pub agent: Arc<tokio::sync::Mutex<agent::Agent>>,
    pub agent_event_rx: Option<mpsc::UnboundedReceiver<types::AgentUiEvent>>,
    pub agent_running: bool,
    pub agent_following: bool,
    pub agent_requesting: bool,
    pub spinner_tick: usize,
    pub agent_input: String,
    pub agent_input_active: bool,
    pub settings_modal_open: bool,
    pub settings_pane: SettingsPane,
    pub settings_selected_provider: usize,
    pub settings_selected_model: usize,
    pub providers: Vec<SettingsProvider>,
    pub current_provider: String,
    pub current_model: String,
}

impl Default for App {
    fn default() -> Self {
        unreachable!("use App::new(svc, agent) instead")
    }
}

impl App {
    pub fn new(
        svc: kms::KmsService,
        agent: Arc<tokio::sync::Mutex<agent::Agent>>,
        providers: Vec<SettingsProvider>,
        current_provider: String,
        current_model: String,
    ) -> Self {
        let mut tree_state = ListState::default();
        tree_state.select(Some(0));

        let settings_selected_provider = providers
            .iter()
            .position(|p| p.name == current_provider)
            .unwrap_or(0);
        let settings_selected_model = providers
            .get(settings_selected_provider)
            .map(|p| {
                p.models
                    .iter()
                    .position(|m| m == &current_model)
                    .unwrap_or(0)
            })
            .unwrap_or(0);

        Self {
            should_quit: false,
            tree_items: vec![],
            tree_nodes: vec![],
            tree_state,
            diagnostic_lines: vec![Line::from("Loading...")],
            scroll_diag: 0,
            knowledge_lines: vec![Line::from("Select a Knowledge node")],
            entity_lines: vec![Line::from("Entity view")],
            agent_lines: vec![Line::from("Agent — press Enter to start typing")],
            agent_scroll: 0,
            agent_visible_height: 10,
            focused: Panel::Tree,
            svc,
            ke_tab: KeTab::Knowledge,
            ke_scroll: 0,
            agent,
            agent_event_rx: None,
            agent_running: false,
            agent_following: true,
            agent_requesting: false,
            spinner_tick: 0,
            agent_input: String::new(),
            agent_input_active: false,
            settings_modal_open: false,
            settings_pane: SettingsPane::Provider,
            settings_selected_provider,
            settings_selected_model,
            providers,
            current_provider,
            current_model,
        }
    }

    pub async fn on_tree_select(&mut self) {
        if let Some(sel) = self.tree_state.selected() {
            if let Some(node) = self.tree_nodes.get(sel) {
                if node.target_type == kms::TargetType::Knowledge {
                    if let Some(target_id) = node.target {
                        match self.svc.get_knowledge(target_id).await {
                            Ok(k) => {
                                self.knowledge_lines = vec![
                                    Line::from(format!("Title: {}", k.title)),
                                    Line::from(format!("Type: {:?}", k.knowledge_type)),
                                    Line::from(format!("Entities: {}", k.entities.len())),
                                    Line::from(""),
                                ];
                                if let Some(content) = &k.content {
                                    for line in content.lines() {
                                        self.knowledge_lines.push(Line::from(line.to_owned()));
                                    }
                                } else {
                                    self.knowledge_lines.push(Line::from("(no content)"));
                                }
                                self.load_entity_lines(&k.entities).await;
                            }
                            Err(e) => {
                                self.knowledge_lines = vec![Line::from(format!("Error: {}", e))];
                                self.entity_lines = vec![Line::from("")];
                            }
                        }
                    }
                } else {
                    self.knowledge_lines = vec![Line::from("Select a Knowledge node")];
                    self.entity_lines = vec![Line::from("")];
                }
            }
        }
    }

    async fn load_entity_lines(&mut self, entity_ids: &[uuid::Uuid]) {
        if entity_ids.is_empty() {
            self.entity_lines = vec![Line::from("(no entities)")];
            return;
        }
        let mut lines = vec![
            Line::from(format!("Entities ({})", entity_ids.len())),
            Line::from(""),
        ];
        for id in entity_ids {
            match self.svc.get_entity(*id).await {
                Ok(entity) => {
                    let primary_name = entity
                        .name
                        .first()
                        .map(|n| n.full.as_str())
                        .unwrap_or("(unnamed)");
                    lines.push(Line::from(format!("  {}", primary_name)));
                    if !entity.definition.is_empty() {
                        for def_line in entity.definition.lines().take(3) {
                            lines.push(Line::from(format!("    {}", def_line)));
                        }
                    }
                }
                Err(_) => {
                    lines.push(Line::from(format!("  [error loading entity]")));
                }
            }
        }
        self.entity_lines = lines;
    }

    pub async fn refresh_tree(&mut self) {
        let prev_selected = self
            .tree_state
            .selected()
            .and_then(|i| self.tree_nodes.get(i).map(|n| n.id));

        self.tree_items.clear();
        self.tree_nodes.clear();

        let root_children = match self.svc.get_children(None).await {
            Ok(c) => c,
            Err(_) => return,
        };
        let mut stack: Vec<(kms::Index, usize)> =
            root_children.into_iter().map(|c| (c, 0)).collect();

        while let Some((node, depth)) = stack.pop() {
            let title = node.title.as_deref().unwrap_or("(unnamed)");
            let indent = "  ".repeat(depth);
            let icon = match node.target_type {
                kms::TargetType::Group => "▸ ",
                kms::TargetType::Knowledge => "● ",
            };
            self.tree_items
                .push(ListItem::new(format!("{}{}{}", indent, icon, title)));
            self.tree_nodes.push(node.clone());

            if let Ok(children) = self.svc.get_children(Some(node.id)).await {
                for child in children.into_iter().rev() {
                    stack.push((child, depth + 1));
                }
            }
        }

        if let Some(prev_id) = prev_selected {
            if let Some(new_idx) = self.tree_nodes.iter().position(|n| n.id == prev_id) {
                self.tree_state.select(Some(new_idx));
            } else {
                self.tree_state.select(Some(0));
            }
        } else {
            self.tree_state.select(Some(0));
        }

        if let Ok(diagnostics) = self.svc.diagnose().await {
            if diagnostics.is_empty() {
                self.diagnostic_lines = vec![Line::from(ratatui::text::Span::styled(
                    "No issues found.".to_owned(),
                    ratatui::style::Style::default().fg(ratatui::style::Color::Green),
                ))];
            } else {
                let mut lines = vec![Line::from(format!("{} issues found:", diagnostics.len()))];
                for d in &diagnostics {
                    lines.push(crate::styles::style_diagnostic_line(&format!(
                        "[{}] {} — {}",
                        d.severity.label(),
                        d.code,
                        d.message
                    )));
                    if !d.location.is_empty() {
                        lines.push(crate::styles::style_diagnostic_line(&d.location));
                    }
                    for action in &d.suggested_actions {
                        lines.push(crate::styles::style_diagnostic_line(&format!(
                            "  → {}",
                            action
                        )));
                    }
                    lines.push(Line::from(""));
                }
                self.diagnostic_lines = lines;
            }
        }

        self.on_tree_select().await;
    }
}
