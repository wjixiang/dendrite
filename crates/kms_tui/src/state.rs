use std::collections::HashMap;
use std::sync::Arc;

use kms::Index;
use ratatui::text::Line;
use ratatui::widgets::{ListItem, ListState};
use tokio::sync::mpsc;

use crate::chat::ChatMessage;
use crate::components::toast::ToastManager;
use crate::settings::{PoolEntry, ProviderConfig};
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentKind {
    Compose,
    Knowledge,
    Parallel,
}

impl AgentKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Compose => "Compose",
            Self::Knowledge => "Retrieval",
            Self::Parallel => "Parallel",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Compose => Self::Knowledge,
            Self::Knowledge => Self::Parallel,
            Self::Parallel => Self::Compose,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Tree,
    KnowledgeEntity,
    Agent,
    Diagnostics,
}

/// Sub-focus within the `Agent` panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatFocus {
    Messages,
    ParallelPanel,
}

impl Default for ChatFocus {
    fn default() -> Self {
        Self::Messages
    }
}

impl std::fmt::Display for Panel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Panel::Tree => write!(f, "Tree"),
            Panel::KnowledgeEntity => write!(f, "Knowledge"),
            Panel::Agent => write!(f, "Agent"),
            Panel::Diagnostics => write!(f, "Diag"),
        }
    }
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
    Pool,
}

/// A provider as the TUI sees it: a single instance, identified by a
/// stable `id`. Both env-discovered providers and user-created ones are
/// represented uniformly. Multiple entries with the same `provider_type`
/// are allowed (e.g. two mimo providers with different API keys).
#[derive(Debug, Clone)]
pub struct SettingsProvider {
    /// Stable unique id (UUID-style for env providers, custom for user ones).
    pub id: String,
    /// Display name shown in the UI (e.g. "mimo", "mimo-2").
    pub display_name: String,
    /// Underlying provider type ("mimo", "minimax", ...).
    pub provider_type: String,
    /// API key in use.
    pub api_key: String,
    /// Base URL (some providers need it; empty string if not used).
    pub base_url: String,
    /// Models exposed by this provider instance.
    pub models: Vec<String>,
    /// True if user-created; false if auto-discovered from env vars.
    pub is_custom: bool,
}

impl SettingsProvider {
    /// Short label for the provider: display name + tag.
    pub fn short_label(&self) -> String {
        if self.is_custom {
            format!("{} ({})", self.display_name, self.provider_type)
        } else {
            self.display_name.clone()
        }
    }
}

/// State for the "Add Provider" form modal. When `Some`, the form is
/// visible and the user is editing fields; key events are routed here.
#[derive(Debug, Clone)]
pub struct NewProviderForm {
    /// Currently selected provider type (index into BUILTIN_PROVIDER_TYPES).
    pub type_idx: usize,
    /// User-chosen display name (e.g. "mimo-2").
    pub display_name: String,
    /// API key.
    pub api_key: String,
    /// Index into `base_url_presets_for_type(provider_type)`. When the
    /// selected preset is a "Custom..." preset, `url_custom` is what
    /// gets persisted; otherwise the preset's `url` is used.
    pub url_preset_idx: usize,
    /// Free-text URL used when the selected preset is "Custom...".
    pub url_custom: String,
    /// Which field is currently focused.
    /// 0 = type (cycle with up/down), 1 = name, 2 = key, 3 = url.
    pub active_field: usize,
    /// Error message to show in the form, if any.
    pub error: Option<String>,
}

impl NewProviderForm {
    pub fn new() -> Self {
        // Default to the first non-custom preset for the initial type.
        let initial_preset = crate::settings::base_url_presets_for_type(
            crate::settings::BUILTIN_PROVIDER_TYPES[0],
        )
        .iter()
        .position(|p| !p.is_custom)
        .unwrap_or(0);
        Self {
            type_idx: 0,
            display_name: String::new(),
            api_key: String::new(),
            url_preset_idx: initial_preset,
            url_custom: String::new(),
            active_field: 0,
            error: None,
        }
    }

    /// The preset list for the currently-selected provider type.
    pub fn presets(&self) -> Vec<crate::settings::BaseUrlPreset> {
        crate::settings::base_url_presets_for_type(
            crate::settings::BUILTIN_PROVIDER_TYPES[self.type_idx],
        )
    }

    /// The URL string that should be persisted when the form is
    /// submitted, based on the current preset selection.
    pub fn resolved_url(&self) -> String {
        let presets = self.presets();
        match presets.get(self.url_preset_idx) {
            Some(p) if p.is_custom => self.url_custom.trim().to_string(),
            Some(p) => p.url.to_string(),
            None => String::new(),
        }
    }

    /// True when the URL field is in free-text mode (Custom preset
    /// selected) and the user is currently typing a URL.
    pub fn url_is_custom(&self) -> bool {
        self.presets()
            .get(self.url_preset_idx)
            .map(|p| p.is_custom)
            .unwrap_or(false)
    }

    /// Label to show in the URL field selector.
    pub fn url_label(&self) -> String {
        let presets = self.presets();
        match presets.get(self.url_preset_idx) {
            Some(p) if p.is_custom => {
                if self.url_custom.is_empty() {
                    "<type a URL>".to_string()
                } else {
                    self.url_custom.clone()
                }
            }
            Some(p) => p.label.to_string(),
            None => String::new(),
        }
    }
}

pub enum Action {
    None,
    Quit,
    TreeChanged,
    SubmitAgent(String),
    OpenSettings,
    SettingsNav(SettingsPane, isize),
    #[allow(dead_code)]
    SettingsSwitchPane(SettingsPane),
    SettingsConfirm,
    SettingsTogglePool,
    SettingsRemovePool,
    /// Open the "add new provider" form.
    SettingsNewProvider,
    /// Delete the currently selected custom provider.
    SettingsDeleteProvider,
    /// Cycle to the next/previous field in the new-provider form.
    SettingsFormCycleField(isize),
    /// Append a character to the current field in the new-provider form.
    SettingsFormType(char),
    /// Backspace in the new-provider form.
    SettingsFormBackspace,
    /// Cycle the provider type selector (up/down arrows in the form).
    SettingsFormCycleType(isize),
    /// Submit the new-provider form.
    SettingsFormSubmit,
    /// Cancel the new-provider form.
    SettingsFormCancel,
    SwitchAgent,
}

pub struct App {
    pub should_quit: bool,
    pub theme: Theme,
    pub toast: ToastManager,

    pub tree_items: Vec<ListItem<'static>>,
    pub tree_nodes: Vec<Index>,
    pub tree_state: ListState,

    pub diagnostic_lines: Vec<Line<'static>>,
    pub scroll_diag: u16,

    pub knowledge_lines: Vec<Line<'static>>,
    pub entity_lines: Vec<Line<'static>>,

    pub focused: Panel,
    pub svc: kms::KmsService,

    pub ke_tab: KeTab,
    pub ke_scroll: u16,

    pub agents: HashMap<AgentKind, Arc<tokio::sync::Mutex<agentik_core::Agent>>>,
    pub agent_kind: AgentKind,
    pub agent_messages_map: HashMap<AgentKind, Vec<ChatMessage>>,
    pub agent_event_rx: Option<mpsc::UnboundedReceiver<agentik_types::AgentEvent>>,
    pub agent_running: bool,
    pub agent_requesting: bool,
    pub spinner_tick: usize,
    /// Latest output-token count reported by `UsageUpdate` during
    /// streaming. Displayed in the status bar so the user sees how
    /// many tokens the LLM has generated so far.
    pub agent_usage_tokens: Option<u64>,
    /// Vertical scroll offset (in lines) for the Agent chat panel.
    /// 0 = top. Used together with `agent_auto_scroll`: when auto-scroll
    /// is on, the renderer pins the scroll to the bottom on every frame
    /// so newly-streamed events stay visible without the user having
    /// to scroll. Manual `j`/`k`/`PageUp`/`PageDown` flips auto-scroll
    /// off and lets the user browse history.
    pub agent_scroll: u16,
    /// True when the chat panel should follow the bottom of the
    /// stream. Disabled the moment the user scrolls up, re-enabled
    /// when they hit `End` to jump to the bottom or start a new
    /// submission.
    pub agent_auto_scroll: bool,

    pub agent_input: String,
    pub agent_input_active: bool,

    /// Side-channel that maps a paste placeholder to the full text the
    /// user pasted.
    pub agent_pastes: Vec<(String, String)>,

    /// Receiver for the parallel-dispatch side-channel.
    pub parallel_progress_rx: Option<
        mpsc::UnboundedReceiver<dendrite_tools::parallel_progress::ParallelProgress>,
    >,

    /// Sender side of the parallel-dispatch channel.
    pub parallel_progress_tx: dendrite_tools::parallel_progress::ParallelProgressTx,

    /// State for the collapsible parallel-dispatch panel.
    pub parallel_panel: Option<crate::parallel_panel::ParallelPanelState>,

    /// Sub-focus within the Agent panel.
    pub chat_focus: ChatFocus,

    /// Wall-clock instant of the last tree refresh. Used to debounce
    /// the per-sub-agent tree refresh trigger so that 5 sub-agents
    /// finishing within 100ms only cause one refresh, not five.
    pub last_tree_refresh_at: Option<std::time::Instant>,

    pub settings_modal_open: bool,
    pub settings_pane: SettingsPane,
    pub settings_selected_provider: usize,
    pub settings_selected_model: usize,
    pub settings_selected_pool: usize,
    pub providers: Vec<SettingsProvider>,
    /// Persisted provider configs (the on-disk view; we re-fetch model
    /// lists at startup so this is the source of truth for credentials).
    pub provider_configs: Vec<ProviderConfig>,
    pub pool_entries: Vec<PoolEntry>,
    /// When `Some`, the new-provider form is open.
    pub new_provider_form: Option<NewProviderForm>,
}

impl Default for App {
    fn default() -> Self {
        unreachable!("use App::new(svc, agents, ...) instead")
    }
}

impl App {
    pub fn new(
        svc: kms::KmsService,
        agents: HashMap<AgentKind, Arc<tokio::sync::Mutex<agentik_core::Agent>>>,
        providers: Vec<SettingsProvider>,
        provider_configs: Vec<ProviderConfig>,
        pool_entries: Vec<PoolEntry>,
        parallel_progress_tx: dendrite_tools::parallel_progress::ParallelProgressTx,
        parallel_progress_rx: mpsc::UnboundedReceiver<
            dendrite_tools::parallel_progress::ParallelProgress,
        >,
    ) -> Self {
        let mut tree_state = ListState::default();
        tree_state.select(Some(0));

        let settings_selected_provider = pool_entries
            .first()
            .and_then(|e| providers.iter().position(|p| p.id == e.provider_id))
            .unwrap_or(0);
        let settings_selected_model = pool_entries
            .first()
            .and_then(|e| {
                providers
                    .get(settings_selected_provider)
                    .and_then(|p| p.models.iter().position(|m| m == &e.model))
            })
            .unwrap_or(0);

        let agent_messages_map: HashMap<AgentKind, Vec<ChatMessage>> = {
            let mut m = HashMap::new();
            m.insert(AgentKind::Compose, vec![ChatMessage::Divider]);
            m.insert(AgentKind::Knowledge, vec![ChatMessage::Divider]);
            m.insert(AgentKind::Parallel, vec![ChatMessage::Divider]);
            m
        };

        Self {
            should_quit: false,
            theme: Theme::default_theme(),
            toast: ToastManager::new(),
            tree_items: vec![],
            tree_nodes: vec![],
            tree_state,
            diagnostic_lines: vec![Line::from("Loading...")],
            scroll_diag: 0,
            knowledge_lines: vec![Line::from("Select a Knowledge node")],
            entity_lines: vec![Line::from("Entity view")],
            focused: Panel::Tree,
            svc,
            ke_tab: KeTab::Knowledge,
            ke_scroll: 0,
            agents,
            agent_kind: AgentKind::Compose,
            agent_messages_map,
            agent_event_rx: None,
            agent_running: false,
            agent_requesting: false,
            spinner_tick: 0,
            agent_usage_tokens: None,
            agent_scroll: 0,
            // Start with auto-scroll on so the first agent run streams
            // smoothly; the renderer will pin the scroll to the bottom.
            agent_auto_scroll: true,
            agent_input: String::new(),
            agent_input_active: false,
            agent_pastes: Vec::new(),
            parallel_progress_rx: Some(parallel_progress_rx),
            parallel_progress_tx,
            parallel_panel: None,
            chat_focus: ChatFocus::Messages,
            last_tree_refresh_at: None,
            settings_modal_open: false,
            settings_pane: SettingsPane::Provider,
            settings_selected_provider,
            settings_selected_model,
            settings_selected_pool: 0,
            providers,
            provider_configs,
            pool_entries,
            new_provider_form: None,
        }
    }

    pub fn agent_messages(&self) -> &[ChatMessage] {
        &self.agent_messages_map[&self.agent_kind]
    }

    pub fn agent_messages_mut(&mut self) -> &mut Vec<ChatMessage> {
        self.agent_messages_map.get_mut(&self.agent_kind).unwrap()
    }

    /// Check whether the (provider_id, model) pair is in the current pool.
    pub fn is_in_pool(&self, provider_id: &str, model: &str) -> bool {
        self.pool_entries
            .iter()
            .any(|e| e.provider_id == provider_id && e.model == model)
    }

    /// Toggle a (provider_id, model) pair in/out of the pool.
    pub fn toggle_pool_entry(&mut self, provider_id: &str, model: &str) {
        if let Some(pos) = self
            .pool_entries
            .iter()
            .position(|e| e.provider_id == provider_id && e.model == model)
        {
            self.pool_entries.remove(pos);
        } else {
            self.pool_entries.push(PoolEntry {
                provider_id: provider_id.to_string(),
                model: model.to_string(),
            });
        }
    }

    /// Remove a pool entry by index.
    pub fn remove_pool_entry(&mut self, index: usize) {
        if index < self.pool_entries.len() {
            self.pool_entries.remove(index);
        }
    }

    /// Find a provider by id.
    pub fn provider_by_id(&self, id: &str) -> Option<&SettingsProvider> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// Add a new provider to the providers list and to provider_configs.
    /// Returns the new id.
    pub fn add_custom_provider(
        &mut self,
        display_name: String,
        provider_type: String,
        api_key: String,
        base_url: String,
    ) -> String {
        let id = crate::settings::new_provider_id();
        let models = crate::settings::default_models_for_type(&provider_type);
        let provider = SettingsProvider {
            id: id.clone(),
            display_name: display_name.clone(),
            provider_type: provider_type.clone(),
            api_key: api_key.clone(),
            base_url: base_url.clone(),
            models,
            is_custom: true,
        };
        self.providers.push(provider);
        self.provider_configs.push(ProviderConfig {
            id: id.clone(),
            display_name,
            provider_type,
            api_key,
            base_url,
        });
        id
    }

    /// Remove a custom provider by id. Built-in (env-discovered) providers
    /// cannot be removed this way. Returns true if removed.
    pub fn remove_custom_provider(&mut self, id: &str) -> bool {
        let pos = match self.providers.iter().position(|p| p.id == id && p.is_custom) {
            Some(p) => p,
            None => return false,
        };
        self.providers.remove(pos);
        self.provider_configs.retain(|c| c.id != id);
        // Also remove any pool entries pointing at this provider.
        self.pool_entries.retain(|e| e.provider_id != id);
        true
    }

    pub async fn on_tree_select(&mut self) {
        if let Some(sel) = self.tree_state.selected()
            && let Some(node) = self.tree_nodes.get(sel)
            && node.target_type == kms::TargetType::Knowledge
            && let Some(target_id) = node.target
        {
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
        } else {
            self.knowledge_lines = vec![Line::from("Select a Knowledge node")];
            self.entity_lines = vec![Line::from("")];
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
                    lines.push(Line::from("  [error loading entity]"));
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
                kms::TargetType::Group => self.theme.tree_group_icon,
                kms::TargetType::Knowledge => self.theme.tree_item_icon,
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
                    self.theme.success_style(),
                ))];
            } else {
                let mut lines = vec![Line::from(format!("{} issues found:", diagnostics.len()))];
                for d in &diagnostics {
                    lines.push(crate::styles::style_diagnostic_line(
                        &format!("[{}] {} — {}", d.severity.label(), d.code, d.message),
                        &self.theme,
                    ));
                    if !d.location.is_empty() {
                        lines.push(crate::styles::style_diagnostic_line(&d.location, &self.theme));
                    }
                    for action in &d.suggested_actions {
                        lines.push(crate::styles::style_diagnostic_line(
                            &format!("  → {}", action),
                            &self.theme,
                        ));
                    }
                    lines.push(Line::from(""));
                }
                self.diagnostic_lines = lines;
            }
        }

        self.on_tree_select().await;
    }
}
