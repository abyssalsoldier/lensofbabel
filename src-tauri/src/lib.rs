mod actions;
mod autostart;
mod config;
mod hotkeys;
mod ocr;
mod popup;
mod selection;
mod theme;
mod translate;
mod tray;
mod update;

use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

pub struct AppState {
    pub settings: Mutex<config::Settings>,
    pub hotkeys: hotkeys::HotkeyManager,
    pub hotkey_error: Mutex<Option<String>>,
    pub paused: AtomicBool,
    pub busy: AtomicBool,
    /// The window that had focus when a hotkey was pressed, used by "Replace".
    pub last_foreground: AtomicIsize,
    pub popup_anchor: Mutex<Option<popup::Anchor>>,
    pub popup_shown_at: Mutex<Option<Instant>>,
    /// Set by the popup while its language dropdown is open.
    pub popup_hold: AtomicBool,
    /// Monitor covered by the screen-text overlay.
    pub snip_monitor: Mutex<Option<ocr::Region>>,
    pub snip_shown_at: Mutex<Option<Instant>>,
    pub client: translate::Client,
}

pub fn show_window(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Parses and checks the enabled hotkeys in `s`.
fn bindings(s: &config::Settings) -> Result<Vec<hotkeys::Binding>, String> {
    use hotkeys::Action;
    let mut wanted = vec![("Popup", Action::Popup, &s.popup_hotkey)];
    if s.replace_enabled {
        wanted.push(("Replace", Action::Replace, &s.replace_hotkey));
    }
    if s.screen_enabled {
        wanted.push(("Screen text", Action::Screen, &s.screen_hotkey));
    }
    let mut out: Vec<hotkeys::Binding> = Vec::new();
    for (label, action, text) in wanted {
        let hk = hotkeys::parse(text).map_err(|e| format!("{label} hotkey: {e}"))?;
        if out.iter().any(|(_, other, _)| *other == hk) {
            return Err(format!("{label} hotkey ({text}) is already used by another action."));
        }
        out.push((action, hk, text.clone()));
    }
    Ok(out)
}

/// Registers hotkeys from the current settings, or none while paused.
pub fn apply_hotkeys(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let result = (|| {
        if state.paused.load(Ordering::SeqCst) {
            return state.hotkeys.set(Vec::new());
        }
        let s = state.settings.lock().unwrap().clone();
        state.hotkeys.set(bindings(&s)?)
    })();
    *state.hotkey_error.lock().unwrap() = result.clone().err();
    result
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    hotkey_error: Option<String>,
    paused: bool,
}

#[tauri::command]
async fn translate(
    state: State<'_, AppState>,
    text: String,
    source: String,
    target: String,
) -> Result<translate::Translation, String> {
    let client = state.client.clone();
    client.translate(&text, &source, &target).await
}

#[tauri::command]
fn get_config(app: AppHandle, state: State<AppState>) -> config::Settings {
    let mut s = state.settings.lock().unwrap().clone();
    // Show the real autostart state (the user may have removed it in Task Manager).
    if let Ok(enabled) = autostart::is_enabled(&app.package_info().name) {
        s.autostart = enabled;
    }
    s
}

#[tauri::command]
fn app_version() -> &'static str {
    update::CURRENT
}

#[tauri::command]
async fn check_for_update() -> Result<update::UpdateInfo, String> {
    update::check().await
}

#[tauri::command]
fn open_releases_page() -> Result<(), String> {
    update::open_releases_page()
}

#[tauri::command]
fn get_status(state: State<AppState>) -> Status {
    Status { hotkey_error: state.hotkey_error.lock().unwrap().clone(), paused: state.paused.load(Ordering::SeqCst) }
}

#[tauri::command]
fn save_config(app: AppHandle, state: State<AppState>, mut settings: config::Settings) -> Result<(), String> {
    // The popup size is managed by resizing the popup, so the settings page's
    // copy may be out of date. Keep the current value.
    {
        let current = state.settings.lock().unwrap();
        settings.popup_width = current.popup_width;
        settings.popup_height = current.popup_height;
    }
    bindings(&settings)?;

    // EDR note: autostart writes an HKCU Run key, and only when the user asks for it.
    let name = &app.package_info().name;
    if settings.autostart != autostart::is_enabled(name).unwrap_or(false) {
        let r = if settings.autostart { autostart::enable(name) } else { autostart::disable(name) };
        r.map_err(|e| format!("Could not change Start with Windows: {e}"))?;
    }

    config::save(&app, &settings)?;
    theme::apply_window_theme(&app, &settings.theme);
    *state.settings.lock().unwrap() = settings.clone();
    let _ = app.emit("config:changed", &settings);
    apply_hotkeys(&app)
}

#[tauri::command]
fn replace_with(app: AppHandle, state: State<AppState>, text: String) -> Result<(), String> {
    if !state.settings.lock().unwrap().replace_enabled {
        return Err("Replace is turned off in settings.".into());
    }
    popup::hide(&app, false);
    let target = state.last_foreground.load(Ordering::SeqCst);
    std::thread::spawn(move || {
        selection::activate(target);
        std::thread::sleep(Duration::from_millis(150));
        let _ = selection::paste_text(&text, 0);
    });
    Ok(())
}

#[tauri::command]
fn open_main_with(app: AppHandle, text: String) {
    popup::hide(&app, false);
    show_window(&app, "main");
    let _ = app.emit_to("main", "main:set-text", text);
}

#[tauri::command]
fn open_settings(app: AppHandle) {
    popup::hide(&app, false);
    show_window(&app, "settings");
}

#[tauri::command]
fn hide_popup(app: AppHandle) {
    popup::hide(&app, true);
}

#[tauri::command]
fn popup_resize(app: AppHandle, height: f64) {
    popup::resize(&app, height);
}

#[tauri::command]
fn popup_remember_size(app: AppHandle, width: f64, height: f64) -> Result<(), String> {
    popup::remember_size(&app, width, height)
}

#[tauri::command]
fn reset_popup_size(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let settings = {
        let mut s = state.settings.lock().unwrap();
        s.popup_width = 0;
        s.popup_height = 0;
        s.clone()
    };
    config::save(&app, &settings)
}

#[tauri::command]
fn get_accent() -> Option<theme::Accent> {
    theme::accent()
}

#[tauri::command]
fn snip_done(app: AppHandle, x: f64, y: f64, width: f64, height: f64) {
    actions::finish_screen(&app, x, y, width, height);
}

#[tauri::command]
fn snip_cancel(app: AppHandle) {
    actions::cancel_screen(&app);
}

#[tauri::command]
fn popup_hold(state: State<AppState>, hold: bool) {
    state.popup_hold.store(hold, Ordering::SeqCst);
}

pub fn run() {
    tauri::Builder::default()
        // EDR: by default tao registers keyboard/mouse raw input (while focused).
        // We don't need device events, so unregister them entirely (RIDEV_REMOVE).
        .device_event_filter(tauri::DeviceEventFilter::Always)
        // Must be first: a second launch just focuses the running instance.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_window(app, "main")))
        .setup(|app| {
            let handle = app.handle().clone();
            // Quote the path in a startup entry an older version wrote unquoted.
            let _ = autostart::repair_legacy(&handle.package_info().name);
            let settings = config::load(&handle);
            theme::apply_window_theme(&handle, &settings.theme);
            theme::watch(&handle);
            let cb_handle = handle.clone();
            let hotkeys = hotkeys::HotkeyManager::start(move |action| actions::on_hotkey(&cb_handle, action));
            app.manage(AppState {
                settings: Mutex::new(settings),
                hotkeys,
                hotkey_error: Mutex::new(None),
                paused: AtomicBool::new(false),
                busy: AtomicBool::new(false),
                last_foreground: AtomicIsize::new(0),
                popup_anchor: Mutex::new(None),
                popup_shown_at: Mutex::new(None),
                popup_hold: AtomicBool::new(false),
                snip_monitor: Mutex::new(None),
                snip_shown_at: Mutex::new(None),
                client: translate::Client::new(),
            });
            tray::create(&handle)?;

            let hotkeys_ok = apply_hotkeys(&handle).is_ok();
            let from_autostart = std::env::args().any(|a| a == autostart::ARG);
            if !hotkeys_ok {
                show_window(&handle, "settings");
            } else if !from_autostart {
                show_window(&handle, "main");
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Closing a window only hides it; the app keeps running in the tray.
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            // Switching away from the selection overlay (for example with Alt+Tab) cancels it.
            WindowEvent::Focused(false) if window.label() == "snip" => {
                let shown_at = *window.state::<AppState>().snip_shown_at.lock().unwrap();
                if shown_at.map_or(true, |t| t.elapsed() > Duration::from_millis(400)) {
                    let _ = window.hide();
                }
            }
            WindowEvent::Focused(false) if window.label() == "popup" => {
                let shown_at = *window.state::<AppState>().popup_shown_at.lock().unwrap();
                if shown_at.map_or(false, |t| t.elapsed() < Duration::from_millis(400)) {
                    return;
                }
                // Check again after a moment: an open <select> dropdown can briefly
                // take focus without the user having clicked away.
                let window = window.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(200));
                    let held = window.state::<AppState>().popup_hold.load(Ordering::SeqCst);
                    if !held && !window.is_focused().unwrap_or(false) {
                        let _ = window.hide();
                    }
                });
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            translate,
            get_config,
            get_status,
            save_config,
            replace_with,
            open_main_with,
            open_settings,
            hide_popup,
            popup_resize,
            popup_remember_size,
            reset_popup_size,
            get_accent,
            snip_done,
            snip_cancel,
            popup_hold,
            app_version,
            check_for_update,
            open_releases_page
        ])
        .run(tauri::generate_context!())
        .expect("error while running Lens of Babel Translator");
}
