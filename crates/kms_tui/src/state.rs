use std::collections::HashMap;
use std::sync::Arc;

use kms::Index;
use ratatui::text::Line;
use ratatui::widgets::{ListItem, ListState};
use tokio::sync::mpsc;

use crate::agent_panel::AgentPanelState;
use crate::chat::ChatMessage;
use crate::components::toast::ToastManager;
use crate::settings::{PoolEntry, ProviderConfig};
use uuid::Uuid;

use crate::theme::Theme;

/// A paste entry awaiting submission. Either the full text is still
/// held in memory (`content = Some`), or it has already been
/// uploaded as a document (`content = None`, `doc_id = Some`).
#[derive(Debug, Clone)]
pub struct PasteEntry {
    /// Short text shown in the input area and chat history.
    pub placeholder: String,
    /// Text shown in chat history (always compact, never full text).
    pub display: String,
    /// Full text of the paste, if not yet ingested as a document.
    pub content: Option<String>,
    /// Document UUID, if the content has been uploaded.
    pub doc_id: Option<Uuid>,
}

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
///
/// `Panel::Agent` is a single top-level panel, but it has two
/// internal sub-sections (the chat history at the top and the
/// sub-agent status list at the bottom). `ChatFocus` selects
/// which sub-section receives key events when the Agent panel
/// is focused. The renderer uses it only to pick borders and
/// help-bar text — the sub-agent list is always visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatFocus {
    /// Chat history (the upper region). `j`/`k` scroll messages.
    Messages,
    /// Sub-agent status list (the lower region). `j`/`k`/`Enter`/`e`/`c`
    /// navigate/expand the agent rows. Only reachable when the
    /// sub-agent list has at least one entry.
    AgentsPanel,
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

    /// The URL string that should be persisted when the form
    /// is submitted, based on the current preset selection.
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
    pub corpus: std::sync::Arc<corpus::CorpusService>,

    pub ke_tab: KeTab,
    pub ke_scroll: u16,

    pub agents: HashMap<AgentKind, Arc<tokio::sync::Mutex<agentik_core::Agent>>>,
    pub agent_kind: AgentKind,
    pub agent_messages_map: HashMap<AgentKind, Vec<ChatMessage>>,
    pub agent_event_rx: Option<mpsc::UnboundedReceiver<agentik_sdk::types::AgentEvent>>,
    pub agent_running: bool,
    pub agent_requesting: bool,
    /// True while the model is actively streaming token deltas
    /// (TextDelta / ThinkingDelta events). Drives the status-bar
    /// label so the user can tell at a glance whether the spinner
    /// is "thinking" (requesting) or "talking back" (streaming).
    pub agent_streaming: bool,
    pub spinner_tick: usize,
    /// Latest output-token count reported by `UsageUpdate` during
    /// streaming. Displayed in the status bar so the user sees how
    /// many tokens the LLM has generated so far.
    pub agent_usage_tokens: Option<u64>,
    /// Vertical scroll offset (in **post-wrap visual rows**) for the
    /// Agent chat panel. 0 = top. Used together with
    /// `agent_auto_scroll`: when auto-scroll is on, the renderer
    /// pins the scroll to the bottom on every frame so newly-streamed
    /// events stay visible without the user having to scroll.
    /// Manual `j`/`k`/`PageUp`/`PageDown` flips auto-scroll off and
    /// lets the user browse history. The unit is post-wrap visual
    /// rows (not pre-wrap source `Line`s) so it matches the unit
    /// `Paragraph::scroll.y` consumes.
    pub agent_scroll: u16,
    /// True when the chat panel should follow the bottom of the
    /// stream. Disabled the moment the user scrolls up, re-enabled
    /// when they hit `End` to jump to the bottom or start a new
    /// submission.
    pub agent_auto_scroll: bool,

    pub agent_input: String,
    pub agent_input_active: bool,

    /// Side-channel for paste entries. Each entry either holds the
    /// full text (to be ingested as a document at submit time) or has
    /// already been uploaded (content=None, doc_id=Some).
    pub agent_pastes: Vec<PasteEntry>,

    /// Singleton ProcessManager that manages all sub-agents spawned
    /// by parallel dispatch. Owned by the TUI, shared via Arc to the
    /// tool layer.
    pub process_manager: Arc<agentik_core::process::ProcessManager>,

    /// Broadcast receiver for ProcessManager events (all managed agents).
    pub process_event_rx: Option<tokio::sync::broadcast::Receiver<agentik_core::process::ProcessEvent>>,

    /// Shared map from agent UUID to human-readable title. Written by
    /// the dispatch tool after spawn(), read by the TUI to label agents.
    pub agent_titles: Arc<std::sync::RwLock<HashMap<uuid::Uuid, String>>>,

    /// State for the dedicated Agent Status panel.
    pub agent_panel: AgentPanelState,

    /// Sub-focus within the Agent panel. Drives which sub-section
    /// (`Messages` vs the embedded `AgentsPanel`) gets key events
    /// when the Agent panel is the focused top-level panel.
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

    /// Whether the terminal needs to be redrawn. Set `true` when state
    /// changes (events, key presses, resize); reset after each
    /// `terminal.draw()`.
    pub needs_render: bool,

    /// Monotonic version counter for the current agent kind's message
    /// history. Incremented whenever a message is added, its text is
    /// appended to during streaming, or the user switches agent kinds.
    pub message_version: u64,

    /// Cached rendered lines for the agent chat panel.
    /// `(message_version, lines)`. Avoids re-running `to_lines()` on
    /// every frame when messages haven't changed. The renderer now
    /// always flattens the full history (no message-window culling),
    /// so the start/end indices are no longer needed in the key.
    pub cached_agent_lines: Option<(u64, Vec<Line<'static>>)>,

    /// Cached total post-wrap visual row count for the agent chat
    /// panel: `(message_version, inner_width_u16, total_visual_rows)`.
    ///
    /// This is the exact value `Paragraph::line_count(width)` would
    /// return when called on the full history. Caching it avoids
    /// walking `WordWrapper` twice per frame (once for counting, once
    /// for rendering) — the count result is identical in both passes.
    ///
    /// Invalidates on a new `message_version` (history changed) or
    /// a different `inner_width` (panel resize), since wrap layout
    /// depends on viewport width. The renderer uses this to compute
    /// `max_scroll = total_visual_rows.saturating_sub(inner_height)`
    /// so the auto-scroll pin and the user-driven `j`/`k` scroll
    /// both bottom out at the actual last visible row.
    pub cached_estimates: Option<(u64, u16, usize)>,

    /// Pending key for two-key vim motions (e.g. `gg` = first `g`
    /// sets this, second `g` consumes it and jumps to top).
    pub pending_key: Option<char>,
}

impl Default for App {
    fn default() -> Self {
        unreachable!("use App::new(svc, agents, ...) instead")
    }
}

impl App {
    pub fn new(
        svc: kms::KmsService,
        corpus: std::sync::Arc<corpus::CorpusService>,
        agents: HashMap<AgentKind, Arc<tokio::sync::Mutex<agentik_core::Agent>>>,
        providers: Vec<SettingsProvider>,
        provider_configs: Vec<ProviderConfig>,
        pool_entries: Vec<PoolEntry>,
        process_manager: Arc<agentik_core::process::ProcessManager>,
        agent_titles: Arc<std::sync::RwLock<HashMap<uuid::Uuid, String>>>,
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

        let process_event_rx = process_manager.events();

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
            corpus,
            ke_tab: KeTab::Knowledge,
            ke_scroll: 0,
            agents,
            agent_kind: AgentKind::Compose,
            agent_messages_map,
            agent_event_rx: None,
            agent_running: false,
            agent_requesting: false,
            agent_streaming: false,
            spinner_tick: 0,
            agent_usage_tokens: None,
            agent_scroll: 0,
            // Start with auto-scroll on so the first agent run streams
            // smoothly; the renderer will pin the scroll to the bottom.
            agent_auto_scroll: true,
            agent_input: String::new(),
            agent_input_active: false,
            agent_pastes: Vec::new(), // PasteEntry
            process_manager,
            process_event_rx: Some(process_event_rx),
            agent_titles,
            agent_panel: AgentPanelState::default(),
            chat_focus: ChatFocus::default(),
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
            needs_render: true,
            message_version: 0,
            cached_agent_lines: None,
            cached_estimates: None,
            pending_key: None,
        }
    }

    pub fn agent_messages(&self) -> &[ChatMessage] {
        &self.agent_messages_map[&self.agent_kind]
    }

    pub fn agent_messages_mut(&mut self) -> &mut Vec<ChatMessage> {
        self.agent_messages_map.get_mut(&self.agent_kind).unwrap()
    }

    /// Increment the message version counter, invalidating all
    /// message-related caches.
    pub fn bump_message_version(&mut self) {
        self.message_version = self.message_version.wrapping_add(1);
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
    #[allow(dead_code)]
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
