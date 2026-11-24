use std::sync::atomic::Ordering;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::AppState;

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Lens of Babel", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", "Pause hotkeys", true, false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &settings, &sep1, &pause, &sep2, &quit])?;

    let pause_item = pause.clone();
    let mut builder = TrayIconBuilder::with_id("tray")
        .tooltip("Lens of Babel Translator")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => crate::show_window(app, "main"),
            "settings" => crate::show_window(app, "settings"),
            "pause" => {
                let paused = pause_item.is_checked().unwrap_or(false);
                app.state::<AppState>().paused.store(paused, Ordering::SeqCst);
                let _ = crate::apply_hotkeys(app);
                if let Some(tray) = app.tray_by_id("tray") {
                    let _ = tray.set_tooltip(Some(if paused { "Lens of Babel Translator (hotkeys paused)" } else { "Lens of Babel Translator" }));
                }
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
