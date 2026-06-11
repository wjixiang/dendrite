use std::fs;

use agentik_sdk::model::model_pool::ModelPool;
use agentik_sdk::provider::LlmProvider;
use agentik_sdk::provider::mimo::{MimoEndpoint, TokenPlanRegion};

use crate::state::SettingsProvider;

pub const SETTINGS_FILE: &str = "data/settings.json";

/// A provider entry as it is persisted to disk. There are no built-in,
/// env-discovered providers any more: every provider the TUI uses lives
/// in `settings.json` and is configured entirely through the in-TUI
/// provider form. Multiple entries with the same `provider_type` are
/// allowed (so a user can fan out TPM with several API keys for the
/// same provider type).
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct ProviderConfig {
    /// Stable unique id; used by `PoolEntry` to reference this provider.
    pub id: String,
    /// User-chosen display name (e.g. "mimo-2").
    pub display_name: String,
    /// Which built-in provider type this instantiates ("mimo", "minimax", ...).
    pub provider_type: String,
    /// API key for this provider.
    pub api_key: String,
    /// Base URL (some providers need it; empty string if not used).
    #[serde(default)]
    pub base_url: String,
}

/// A single model entry in the pool. References a `SettingsProvider` by
/// its stable `id`.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct PoolEntry {
    pub provider_id: String,
    pub model: String,
}

/// Top-level settings file shape. The TUI reads/writes only this file;
/// no environment variables are consulted at all.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Debug)]
pub struct Settings {
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub pool: Vec<PoolEntry>,
}

/// Legacy v1: separate "custom_providers" + "pool" fields. Kept so users
/// upgrading from a previous build don't lose their config.
#[derive(serde::Deserialize, Default)]
struct LegacyV1Settings {
    #[serde(default)]
    custom_providers: Vec<ProviderConfig>,
    #[serde(default)]
    pool: Vec<PoolEntry>,
}

/// Legacy v0: simple `{provider, model}` single-model format. We don't
/// try to recover the API key (it was in an env var anyway), so we
/// just drop the pool entry and keep the rest empty.
#[derive(serde::Deserialize, Default)]
struct LegacyV0Settings {
    provider: String,
    model: String,
}

/// Load settings from disk, transparently migrating legacy formats.
pub fn load_settings() -> Settings {
    let data = match fs::read_to_string(SETTINGS_FILE) {
        Ok(d) => d,
        Err(_) => return Settings::default(),
    };
    // Current format.
    if let Ok(s) = serde_json::from_str::<Settings>(&data) {
        return s;
    }
    // v1: {custom_providers, pool}
    if let Ok(v1) = serde_json::from_str::<LegacyV1Settings>(&data) {
        return Settings {
            providers: v1.custom_providers,
            pool: v1.pool,
        };
    }
    // v0: {provider, model} — single model, no API key, drop it.
    let _ = serde_json::from_str::<LegacyV0Settings>(&data);
    Settings::default()
}

/// Persist providers and pool to disk. Called eagerly after every
/// mutation (toggle, add, remove) so `data/settings.json` is the
/// source of truth at all times and a sudden quit can't lose the
/// user's most recent edits. Serialization and I/O errors are logged
/// at WARN so they show up in the rotating `tui.log` but don't
/// crash the TUI.
pub fn save_settings(providers: &[ProviderConfig], pool: &[PoolEntry]) {
    let settings = Settings {
        providers: providers.to_vec(),
        pool: pool.to_vec(),
    };
    let json = match serde_json::to_string_pretty(&settings) {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!("save_settings: serialize failed: {}", e);
            return;
        }
    };
    if let Err(e) = fs::write(SETTINGS_FILE, json) {
        tracing::warn!("save_settings: write {:?} failed: {}", SETTINGS_FILE, e);
    }
}

/// Build a `ModelPool` from a list of pool entries against a known set of
/// providers. Each `PoolEntry::provider_id` is matched to a `SettingsProvider`.
pub fn build_pool_from_entries(
    entries: &[PoolEntry],
    providers: &[SettingsProvider],
) -> Option<ModelPool> {
    let mut pool = ModelPool::new();
    for entry in entries {
        let provider = providers.iter().find(|p| p.id == entry.provider_id)?;
        let model = get_model_for_provider_with_creds(
            &provider.provider_type,
            &provider.api_key,
            &provider.base_url,
            &provider.models,
            &entry.model,
        )?;
        pool.add_model(model);
    }
    if pool.model_names().is_empty() {
        None
    } else {
        Some(pool)
    }
}

fn get_model_for_provider_with_creds(
    provider_type: &str,
    api_key: &str,
    base_url: &str,
    model_list: &[String],
    model: &str,
) -> Option<agentik_sdk::model::Model> {
    match provider_type {
        "mimo" => {
            let p = agentik_sdk::provider::mimo::MimoProvider::new(
                mimo_endpoint_from_url(base_url),
                api_key.to_string(),
            );
            p.get_model(model).ok()
        }
        "minimax" => {
            let mut p = agentik_sdk::provider::minimax::MinimaxProvider::new(
                base_url.to_string(),
                api_key.to_string(),
            );
            // Register the user-chosen model list so `get_model` can
            // resolve names that aren't part of the SDK's preset set.
            if !model_list.is_empty() {
                let infos: Vec<agentik_sdk::model::ModelInfo> = model_list
                    .iter()
                    .map(|m| agentik_sdk::model::ModelInfo {
                        model_name: m.clone(),
                        provider: "minimax".to_string(),
                        ..Default::default()
                    })
                    .collect();
                p.add_models(infos);
            }
            p.get_model(model).ok()
        }
        "sensenova" => {
            // sensenova's base_url is optional — the SDK defaults to
            // https://token.sensenova.cn. Pass `None` when the user
            // left the field blank so we don't accidentally pin a
            // stale or mistyped value.
            let mut p = agentik_sdk::provider::sensenova::SensenovaProvider::new(
                if base_url.is_empty() {
                    None
                } else {
                    Some(base_url.to_string())
                },
                api_key.to_string(),
            );
            // Register the user-chosen model list so `get_model` can
            // resolve names that aren't part of the SDK's preset set.
            if !model_list.is_empty() {
                let infos: Vec<agentik_sdk::model::ModelInfo> = model_list
                    .iter()
                    .map(|m| agentik_sdk::model::ModelInfo {
                        model_name: m.clone(),
                        provider: "sensenova".to_string(),
                        ..Default::default()
                    })
                    .collect();
                p.add_models(infos);
            }
            p.get_model(model).ok()
        }
        _ => None,
    }
}

/// Translate a persisted mimo base URL into a `MimoEndpoint` enum value
/// the SDK constructor accepts. The SDK dropped the `Option<String>`
/// endpoint argument in favor of a typed enum, so a custom URL string
/// can no longer be threaded through directly. We map the four known
/// endpoints and let anything else fall through to `None` (which makes
/// the SDK use `MimoEndpoint::default()` = `TokenPlan(China)`).
fn mimo_endpoint_from_url(url: &str) -> Option<MimoEndpoint> {
    match url {
        "" => None,
        "https://api.xiaomimimo.com/anthropic" => Some(MimoEndpoint::Api),
        "https://token-plan-cn.xiaomimimo.com/anthropic" => {
            Some(MimoEndpoint::TokenPlan(TokenPlanRegion::China))
        }
        "https://token-plan-eur.xiaomimimo.com/anthropic" => {
            Some(MimoEndpoint::TokenPlan(TokenPlanRegion::Eur))
        }
        "https://token-plan-sgp.xiaomimimo.com/anthropic" => {
            Some(MimoEndpoint::TokenPlan(TokenPlanRegion::Sgp))
        }
        // Custom or unknown URL — the SDK no longer accepts a free-form
        // base URL, so we fall back to the SDK's default endpoint. The
        // persisted `base_url` is still preserved on disk; the user can
        // re-select one of the known presets to actually use it.
        _ => None,
    }
}

/// Generate a unique provider id (timestamp + random).
pub fn new_provider_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("prov-{:x}", nanos)
}

/// Known built-in provider types the user can pick from when creating a
/// new provider.
pub const BUILTIN_PROVIDER_TYPES: &[&str] = &["mimo", "minimax", "sensenova"];

/// Default model list for a built-in provider type. Used as a starting
/// point for newly-created providers; the live list is fetched at
/// startup (and the TUI re-uses whatever the SDK reports).
pub fn default_models_for_type(provider_type: &str) -> Vec<String> {
    match provider_type {
        "mimo" => vec![
            "mimo-v2.5-pro".into(),
            "mimo-v2-pro".into(),
            "mimo-v2.5".into(),
            "mimo-v2-omni".into(),
            "mimo-v2-flash".into(),
        ],
        "minimax" => vec!["MiniMax-M2.7".into()],
        "sensenova" => vec![
            "sensenova-6.7-flash-lite".into(),
            "deepseek-v4-flash".into(),
        ],
        _ => Vec::new(),
    }
}

/// Default base URL for a built-in provider type (empty if not needed).
/// Used as a fallback for legacy / unknown provider types.
pub fn default_base_url_for_type(provider_type: &str) -> &'static str {
    match provider_type {
        "minimax" => "https://api.minimaxi.com/anthropic",
        "sensenova" => "https://token.sensenova.cn",
        _ => "",
    }
}

/// A built-in base-URL preset the user can pick from the new-provider
/// form. The form renders one preset per row of a "cycle through with
/// \u{2191}/\u{2193}" selector, mirroring opencode's provider dropdown
/// design. The "Custom..." preset (one per provider type) flips the
/// field into a free-text input so users can point at self-hosted or
/// unreleased endpoints.
#[derive(Debug, Clone, Copy)]
pub struct BaseUrlPreset {
    /// Display label shown in the form selector.
    pub label: &'static str,
    /// The actual URL to persist. Empty string means "use the SDK's
    /// built-in default for this provider type" — i.e. don't override
    /// anything. This is the default for the mimo endpoints because
    /// the SDK already knows them.
    pub url: &'static str,
    /// If true, picking this preset switches the URL field to a
    /// text-input mode where the user types the full URL themselves.
    pub is_custom: bool,
}

/// List of base-URL presets for a given provider type. The first
/// non-custom entry is what the form selects by default.
pub fn base_url_presets_for_type(provider_type: &str) -> Vec<BaseUrlPreset> {
    match provider_type {
        "mimo" => vec![
            BaseUrlPreset {
                label: "Mimo API (default)",
                url: "",
                is_custom: false,
            },
            BaseUrlPreset {
                label: "Mimo Token Plan — China",
                url: "https://token-plan-cn.xiaomimimo.com/anthropic",
                is_custom: false,
            },
            BaseUrlPreset {
                label: "Mimo Token Plan — Europe",
                url: "https://token-plan-eur.xiaomimimo.com/anthropic",
                is_custom: false,
            },
            BaseUrlPreset {
                label: "Mimo Token Plan — Singapore",
                url: "https://token-plan-sgp.xiaomimimo.com/anthropic",
                is_custom: false,
            },
            BaseUrlPreset {
                label: "Custom URL…",
                url: "",
                is_custom: true,
            },
        ],
        "minimax" => vec![
            BaseUrlPreset {
                label: "MiniMax — Global",
                url: "https://api.minimaxi.com/anthropic",
                is_custom: false,
            },
            BaseUrlPreset {
                label: "Custom URL…",
                url: "",
                is_custom: true,
            },
        ],
        "sensenova" => vec![
            BaseUrlPreset {
                label: "SenseTime (default)",
                url: "",
                is_custom: false,
            },
            BaseUrlPreset {
                label: "Custom URL…",
                url: "",
                is_custom: true,
            },
        ],
        _ => vec![BaseUrlPreset {
            label: "Default",
            url: "",
            is_custom: false,
        }],
    }
}

/// Find the index of the preset that matches a stored URL, or None if
/// the URL is a custom one. Used to pre-select the right preset when
/// the user opens the form against an existing provider.
pub fn find_preset_index(provider_type: &str, url: &str) -> Option<usize> {
    let presets = base_url_presets_for_type(provider_type);
    if url.is_empty() {
        // Empty = "use SDK default" = first non-custom preset.
        return presets.iter().position(|p| !p.is_custom);
    }
    presets.iter().position(|p| p.url == url)
}

/// Refresh the model list for a single provider configuration by calling
/// the SDK's `list_models()`. This is called at startup for every
/// persisted provider so the in-memory `SettingsProvider` carries an
/// up-to-date model list even after the upstream API has added or
/// renamed models. Falls back to `default_models_for_type` on error.
pub async fn refresh_models(config: &ProviderConfig) -> Vec<String> {
    let ptype = config.provider_type.as_str();
    let fallback = default_models_for_type(ptype);
    match ptype {
        "mimo" => {
            let _p = agentik_sdk::provider::mimo::MimoProvider::new(
                mimo_endpoint_from_url(&config.base_url),
                config.api_key.clone(),
            );
            // mimo doesn't have an async list_models; fall back.
            fallback
        }
        "minimax" => {
            let p = agentik_sdk::provider::minimax::MinimaxProvider::new(
                config.base_url.clone(),
                config.api_key.clone(),
            );
            p.list_models()
                .await
                .map(|ms| ms.into_iter().map(|m| m.model_info.model_name).collect())
                .unwrap_or(fallback)
        }
        "sensenova" => {
            let p = agentik_sdk::provider::sensenova::SensenovaProvider::new(
                if config.base_url.is_empty() {
                    None
                } else {
                    Some(config.base_url.clone())
                },
                config.api_key.clone(),
            );
            p.list_models()
                .await
                .map(|ms| ms.into_iter().map(|m| m.model_info.model_name).collect())
                .unwrap_or(fallback)
        }
        _ => fallback,
    }
}
