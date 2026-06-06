use std::fs;

pub const SETTINGS_FILE: &str = "data/settings.json";

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct Settings {
    pub provider: String,
    pub model: String,
}

#[allow(dead_code)]
pub fn load_settings() -> Settings {
    match fs::read_to_string(SETTINGS_FILE) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

pub fn save_settings(provider: &str, model: &str) {
    let settings = Settings {
        provider: provider.to_string(),
        model: model.to_string(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&settings) {
        let _ = fs::write(SETTINGS_FILE, json);
    }
}
