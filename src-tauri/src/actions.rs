//! What happens when a hotkey is pressed.

use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};

use crate::hotkeys::{self, Action};
use crate::ocr;
use crate::popup::{self, PopupPayload};
use crate::selection;
use crate::AppState;

const NO_SELECTION: &str = "No selected text found. Select some text, then press the hotkey again.";

struct BusyGuard(AppHandle);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.state::<AppState>().busy.store(false, Ordering::SeqCst);
    }
}

/// Runs on the hotkey thread, so the real work moves to a worker thread.
pub fn on_hotkey(app: &AppHandle, action: Action) {
    if app.state::<AppState>().busy.swap(true, Ordering::SeqCst) {
        return; // the previous hotkey press is still being handled
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let _busy = BusyGuard(app.clone());
        let _com = selection::ComGuard::init();
        match action {
            Action::Popup => popup_flow(&app),
            Action::Replace => replace_flow(&app),
            Action::Screen => screen_flow(&app),
        }
    });
}

/// Records which window to read from. If one of our own windows (usually the
/// popup) has focus, hand focus back to the previously remembered app first,
/// so the selection is read from that app and not from the popup.
fn remember_foreground(app: &AppHandle) {
    let fg = selection::foreground();
    if !selection::is_own_window(fg) {
        app.state::<AppState>().last_foreground.store(fg, Ordering::SeqCst);
        return;
    }
    popup::hide(app, true);
    std::thread::sleep(std::time::Duration::from_millis(150));
}

fn popup_flow(app: &AppHandle) {
    let state = app.state::<AppState>();
    let s = state.settings.lock().unwrap().clone();
    remember_foreground(app);
    let mods = hotkeys::parse(&s.popup_hotkey).map(|h| h.mods).unwrap_or(0);
    let payload = match selection::capture(s.clipboard_fallback, mods) {
        Some(c) => PopupPayload::text(c.text, &s),
        None => PopupPayload::error(NO_SELECTION, &s),
    };
    popup::show(app, payload);
}

/// Shows the region-selection overlay on the monitor under the cursor. The
/// capture itself happens in `finish_screen` after the user drags a box.
fn screen_flow(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !state.settings.lock().unwrap().screen_enabled {
        return;
    }
    remember_foreground(app);
    popup::hide(app, false);
    let Some(win) = app.get_webview_window("snip") else { return };
    let m = ocr::cursor_monitor();
    *state.snip_monitor.lock().unwrap() = Some(m);
    let _ = win.set_position(PhysicalPosition::new(m.x, m.y));
    let _ = win.set_size(PhysicalSize::new(m.w as u32, m.h as u32));
    let _ = win.set_position(PhysicalPosition::new(m.x, m.y));
    let _ = app.emit_to("snip", "snip:start", ());
    *state.snip_shown_at.lock().unwrap() = Some(std::time::Instant::now());
    let _ = win.show();
    let _ = win.set_focus();
}

/// Called with the selected box in physical pixels relative to the overlay.
pub fn finish_screen(app: &AppHandle, x: f64, y: f64, w: f64, h: f64) {
    let state = app.state::<AppState>();
    if let Some(win) = app.get_webview_window("snip") {
        let _ = win.hide();
    }
    let Some(m) = *state.snip_monitor.lock().unwrap() else { return };
    let region = ocr::Region {
        x: m.x + x.round() as i32,
        y: m.y + y.round() as i32,
        w: (w.round() as i32).min(m.w),
        h: (h.round() as i32).min(m.h),
    };
    if region.w < 4 || region.h < 4 {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let _com = selection::ComGuard::init();
        // Let the overlay disappear from the screen before capturing.
        std::thread::sleep(std::time::Duration::from_millis(150));
        let s = app.state::<AppState>().settings.lock().unwrap().clone();
        let result = ocr::capture(region).and_then(|px| ocr::recognize(px, region.w, region.h, &s.source_lang));
        let payload = match result {
            Ok(text) if !text.trim().is_empty() => PopupPayload::screen_text(text, &s),
            Ok(_) => PopupPayload::error("No text was recognised in the selected area.", &s),
            Err(e) => PopupPayload::error(&e, &s),
        };
        popup::show(&app, payload);
    });
}

pub fn cancel_screen(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("snip") {
        let had_focus = win.is_focused().unwrap_or(false);
        let _ = win.hide();
        if had_focus {
            selection::activate(app.state::<AppState>().last_foreground.load(Ordering::SeqCst));
        }
    }
}

fn replace_flow(app: &AppHandle) {
    let state = app.state::<AppState>();
    let s = state.settings.lock().unwrap().clone();
    if !s.replace_enabled {
        return;
    }
    remember_foreground(app);
    let mods = hotkeys::parse(&s.replace_hotkey).map(|h| h.mods).unwrap_or(0);
    let Some(captured) = selection::capture(s.clipboard_fallback, mods) else {
        popup::show(app, PopupPayload::error(NO_SELECTION, &s));
        return;
    };
    let client = state.client.clone();
    match tauri::async_runtime::block_on(client.translate(&captured.text, &s.source_lang, &s.target_lang)) {
        Ok(t) if !t.text.is_empty() => {
            if let Some(target) = &captured.value_target {
                if selection::set_value(target, &t.text) {
                    return;
                }
            }
            if let Err(e) = selection::paste_text(&t.text, mods) {
                popup::show(app, PopupPayload::error(&e, &s));
            }
        }
        Ok(_) => {}
        Err(e) => popup::show(app, PopupPayload::error(&e, &s)),
    }
}
