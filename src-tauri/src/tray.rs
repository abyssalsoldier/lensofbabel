use std::sync::atomic::Ordering;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::AppState;

/// Builds the tray menu from the current state (paused, update available).
fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let state = app.state::<AppState>();
    let paused = state.paused.load(Ordering::SeqCst);
    let update = state.update_available.lock().unwrap().clone();

    let open = MenuItem::with_id(app, "open", "Open Lens of Babel", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", "Pause hotkeys", true, paused, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;
    let update_item = match &update {
        Some(v) => Some(MenuItem::with_id(app, "update", format!("Download update ({v})…"), true, None::<&str>)?),
        None => None,
    };

    let mut items: Vec<&dyn IsMenuItem<Wry>> = Vec::new();
    if let Some(item) = &update_item {
        items.push(item);
        items.push(&sep3);
    }
    items.extend([&open as &dyn IsMenuItem<Wry>, &settings, &sep1, &pause, &sep2, &quit]);
    Menu::with_items(app, &items)
}

fn tooltip(app: &AppHandle) -> String {
    let state = app.state::<AppState>();
    let mut t = String::from("Lens of Babel Translator");
    if state.paused.load(Ordering::SeqCst) {
        t.push_str(" (hotkeys paused)");
    }
    if let Some(v) = state.update_available.lock().unwrap().as_ref() {
        t.push_str(&format!("\nUpdate available: {v}"));
    }
    t
}

/// Rebuilds the tray menu and tooltip after the state changed.
pub fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id("tray") else { return };
    if let Ok(menu) = build_menu(app) {
        let _ = tray.set_menu(Some(menu));
    }
    let _ = tray.set_tooltip(Some(tooltip(app)));
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id("tray")
        .tooltip(tooltip(app))
        .menu(&build_menu(app)?)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => crate::show_window(app, "main"),
            "settings" => crate::show_window(app, "settings"),
            "update" => {
                let _ = crate::update::open_releases_page();
            }
            "pause" => {
                let state = app.state::<AppState>();
                let paused = !state.paused.load(Ordering::SeqCst);
                state.paused.store(paused, Ordering::SeqCst);
                let _ = crate::apply_hotkeys(app);
                refresh(app);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                crate::show_window(tray.app_handle(), "main");
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
