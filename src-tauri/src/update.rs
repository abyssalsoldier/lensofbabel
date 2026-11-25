//! "Check for updates": compares this build's version with the latest GitHub release.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::AppState;

const LATEST_API: &str = "https://api.github.com/repos/abyssalsoldier/lensofbabel/releases/latest";
pub const RELEASES_PAGE: &str = "https://github.com/abyssalsoldier/lensofbabel/releases/latest";

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub available: bool,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

pub async fn check() -> Result<UpdateInfo, String> {
    let http = reqwest::Client::builder()
        .user_agent(concat!("LensOfBabel/", env!("CARGO_PKG_VERSION"), " (Windows desktop app)"))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = http
        .get(LATEST_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("Couldn't reach GitHub: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub answered {}", resp.status()));
    }
    let release: Release = resp.json().await.map_err(|e| format!("Unexpected answer from GitHub: {e}"))?;
    let latest = release.tag_name.trim_start_matches('v').to_string();
    Ok(UpdateInfo { available: is_newer(&latest, CURRENT), current: CURRENT.to_string(), latest })
}

/// True when `latest` is a higher `x.y.z` version than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        v.split(['-', '+']).next().unwrap_or("").split('.').map(|p| p.parse().unwrap_or(0)).collect()
    }
    let (a, b) = (parts(latest), parts(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

/// Checks shortly after startup and then once a day, while "check automatically" is on.
pub fn start_background(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Let startup finish (and the network come up after login) first.
        tokio_sleep(Duration::from_secs(20)).await;
        loop {
            let enabled = app.state::<AppState>().settings.lock().unwrap().auto_update_check;
            if enabled {
                if let Ok(info) = check().await {
                    if info.available {
                        found(&app, &info.latest, true);
                    }
                }
            }
            tokio_sleep(Duration::from_secs(24 * 60 * 60)).await;
        }
    });
}

async fn tokio_sleep(d: Duration) {
    tokio::time::sleep(d).await;
}

/// Records a newer version: adds a download item to the tray menu and, with
/// `notify`, shows a Windows notification once per version.
pub fn found(app: &AppHandle, latest: &str, notify: bool) {
    let state = app.state::<AppState>();
    *state.update_available.lock().unwrap() = Some(latest.to_string());
    crate::tray::refresh(app);
    if !notify {
        return;
    }
    let settings = {
        let mut s = state.settings.lock().unwrap();
        if s.update_notified == latest {
            return;
        }
        s.update_notified = latest.to_string();
        s.clone()
    };
    let _ = crate::config::save(app, &settings);
    let _ = app
        .notification()
        .builder()
        .title("Lens of Babel update available")
        .body(format!("Version {latest} is out (you have {CURRENT}). Right-click the tray icon and choose Download update."))
        .show();
}

/// Opens the latest release page in the default browser.
pub fn open_releases_page() -> Result<(), String> {
    std::process::Command::new("explorer.exe")
        .arg(RELEASES_PAGE)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.1.3", "0.1.2"));
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.10.0"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(!is_newer("0.1.2", "0.1.2"));
        assert!(!is_newer("0.1.1", "0.1.2"));
        assert!(!is_newer("0.1", "0.1.0"));
    }
}
