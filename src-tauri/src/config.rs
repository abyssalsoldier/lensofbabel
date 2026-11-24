use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub source_lang: String,
    pub target_lang: String,
    pub popup_hotkey: String,
    pub replace_hotkey: String,
    /// Replace mode sends synthetic paste input to other apps (EDR: moderate risk).
    pub replace_enabled: bool,
    /// Screen-text (OCR) mode: captures only the region the user drags out
    /// (EDR: moderate risk, screen capture).
    pub screen_enabled: bool,
    pub screen_hotkey: String,
    /// Fall back to a synthetic Ctrl+C when UI Automation finds no selection.
    pub clipboard_fallback: bool,
    /// Writes an HKCU Run key (EDR: moderate risk). Off by default.
    pub autostart: bool,
    /// Seconds before the popup hides itself; 0 keeps it open.
    pub popup_dismiss_secs: u32,
    /// "system" | "light" | "dark"
    pub theme: String,
    /// Popup size chosen by the user, in logical pixels; 0 = size to content.
    pub popup_width: u32,
    pub popup_height: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            source_lang: "auto".into(),
            target_lang: "en".into(),
            popup_hotkey: "Ctrl+Alt+T".into(),
            replace_hotkey: "Ctrl+Alt+R".into(),
            replace_enabled: true,
            screen_enabled: true,
            screen_hotkey: "Ctrl+Alt+S".into(),
            clipboard_fallback: true,
            autostart: false,
            popup_dismiss_secs: 0,
            theme: "system".into(),
            popup_width: 0,
            popup_height: 0,
        }
    }
}

fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("config.json"))
}

pub fn load(app: &AppHandle) -> Settings {
    path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let p = path(app).ok_or("Could not find the config directory.")?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Could not create the config directory: {e}"))?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(&p, json).map_err(|e| format!("Could not save settings: {e}"))
}
