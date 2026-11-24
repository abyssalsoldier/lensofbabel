//! Follows the Windows theme: accent colour (live) and window title-bar theme.
//! Light/dark page colours follow Windows through CSS `prefers-color-scheme`.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Theme};
use windows::core::IInspectable;
use windows::Foundation::TypedEventHandler;
use windows::UI::Color;
use windows::UI::ViewManagement::{UIColorType, UISettings};

/// The Windows accent colour and the two shades Windows itself uses for text
/// and buttons on light and dark backgrounds.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Accent {
    accent: String,
    /// For dark backgrounds.
    accent_light: String,
    /// For light backgrounds.
    accent_dark: String,
}

fn hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.R, c.G, c.B)
}

pub fn accent() -> Option<Accent> {
    let s = UISettings::new().ok()?;
    let get = |t: UIColorType| s.GetColorValue(t).ok().map(hex);
    Some(Accent {
        accent: get(UIColorType::Accent)?,
        accent_light: get(UIColorType::AccentLight2)?,
        accent_dark: get(UIColorType::AccentDark1)?,
    })
}

/// Sends `theme:accent` to every window when the Windows colours change.
pub fn watch(app: &AppHandle) {
    let Ok(settings) = UISettings::new() else { return };
    let app = app.clone();
    let handler = TypedEventHandler::<UISettings, IInspectable>::new(move |_, _| {
        if let Some(a) = accent() {
            let _ = app.emit("theme:accent", a);
        }
        Ok(())
    });
    if settings.ColorValuesChanged(&handler).is_ok() {
        // The subscription lives as long as the UISettings object: keep it for the app's lifetime.
        Box::leak(Box::new(settings));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_windows_accent() {
        let a = super::accent().expect("accent colour");
        println!("{a:?}");
        assert!(a.accent.starts_with('#') && a.accent.len() == 7);
    }
}

/// Title bars: follow Windows ("system") or force the theme chosen in Settings.
pub fn apply_window_theme(app: &AppHandle, theme: &str) {
    let t = match theme {
        "light" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        _ => None,
    };
    for (_, w) in app.webview_windows() {
        let _ = w.set_theme(t);
    }
}
