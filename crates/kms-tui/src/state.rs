use kms::Index;
use ratatui::text::Line;
use ratatui::widgets::{ListItem, ListState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Tree,
    Knowledge,
    Entity,
    Diagnostics,
    Agent,
}

/// Application state shared across the event loop.
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
    pub agent_scroll: u16,
    pub focused: Panel,
    pub svc: kms::KmsService,
}

impl Default for App {
    fn default() -> Self {
        unreachable!("use App::new(svc) instead")
    }
}

impl App {
    pub fn new(svc: kms::KmsService) -> Self {
        let mut tree_state = ListState::default();
        tree_state.select(Some(0));
        Self {
            should_quit: false,
            tree_items: vec![],
            tree_nodes: vec![],
            tree_state,
            diagnostic_lines: vec![Line::from("Loading...")],
            scroll_diag: 0,
            knowledge_lines: vec![Line::from("Select a Knowledge node")],
            entity_lines: vec![Line::from("Entity view")],
            agent_lines: vec![Line::from("Agent view")],
            agent_scroll: 0,
            focused: Panel::Tree,
            svc,
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
                                    Line::from(format!(
                                        "Entities: {}",
                                        k.entities.len()
                                    )),
                                    Line::from(""),
                                ];
                                if let Some(content) = &k.content {
                                    for line in content.lines() {
                                        self.knowledge_lines.push(Line::from(line.to_owned()));
                                    }
                                } else {
                                    self.knowledge_lines
                                        .push(Line::from("(no content)"));
                                }
                                self.load_entity_lines(&k.entities).await;
                            }
                            Err(e) => {
                                self.knowledge_lines =
                                    vec![Line::from(format!("Error: {}", e))];
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
        let mut lines = vec![Line::from(format!("Entities ({})", entity_ids.len())), Line::from("")];
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
}
