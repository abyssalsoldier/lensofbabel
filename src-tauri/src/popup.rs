//! The floating result popup that appears next to the cursor.

use serde::Serialize;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use crate::config::Settings;
use crate::AppState;

const WIDTH: f64 = 400.0;
const INITIAL_HEIGHT: f64 = 150.0;
const MIN_WIDTH: f64 = 260.0;
const MIN_HEIGHT: f64 = 90.0;
/// Largest height when sizing automatically to the content.
const MAX_HEIGHT: f64 = 380.0;
/// Largest size the user can save by resizing.
const MAX_USER_SIZE: f64 = 2000.0;
const OFFSET: f64 = 14.0;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PopupPayload {
    pub text: String,
    pub error: Option<String>,
    pub source: String,
    pub target: String,
    pub dismiss_secs: u32,
    pub replace_enabled: bool,
    pub theme: String,
    /// The user has chosen a size, so the page must not auto-fit its height.
    pub fixed_size: bool,
    /// The text came from screen OCR: it can be edited, and there is no selection to replace.
    pub ocr: bool,
}

impl PopupPayload {
    pub fn text(text: String, s: &Settings) -> Self {
        Self::build(text, None, s)
    }

    pub fn error(msg: &str, s: &Settings) -> Self {
        Self::build(String::new(), Some(msg.to_string()), s)
    }

    pub fn screen_text(text: String, s: &Settings) -> Self {
        let mut p = Self::build(text, None, s);
        p.ocr = true;
        p.replace_enabled = false;
        p
    }

    fn build(text: String, error: Option<String>, s: &Settings) -> Self {
        Self {
            text,
            error,
            source: s.source_lang.clone(),
            target: s.target_lang.clone(),
            dismiss_secs: s.popup_dismiss_secs,
            replace_enabled: s.replace_enabled,
            theme: s.theme.clone(),
            fixed_size: s.popup_width > 0 && s.popup_height > 0,
            ocr: false,
        }
    }
}

/// Cursor position and monitor work area, in physical pixels.
#[derive(Debug, Clone, Copy)]
pub struct Anchor {
    x: i32,
    y: i32,
    work: (i32, i32, i32, i32),
    scale: f64,
}

fn cursor_anchor() -> Anchor {
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let r = mi.rcWork;
        Anchor { x: pt.x, y: pt.y, work: (r.left, r.top, r.right, r.bottom), scale: dx as f64 / 96.0 }
    }
}

/// Places the popup below and to the right of the cursor. It flips to the other
/// side when there isn't room and always stays inside the monitor's work area.
fn place(win: &WebviewWindow, a: Anchor, width: f64, height: f64) {
    let w = (width * a.scale).round() as i32;
    let h = (height * a.scale).round() as i32;
    let off = (OFFSET * a.scale).round() as i32;
    let (l, t, r, b) = a.work;
    let mut x = a.x + off;
    if x + w > r {
        x = a.x - off - w;
    }
    x = x.clamp(l, (r - w).max(l));
    let mut y = a.y + off;
    if y + h > b {
        y = a.y - off - h;
    }
    y = y.clamp(t, (b - h).max(t));
    // Position first so that any DPI change between monitors happens before sizing.
    let _ = win.set_position(PhysicalPosition::new(x, y));
    let _ = win.set_size(PhysicalSize::new(w as u32, h as u32));
    let _ = win.set_position(PhysicalPosition::new(x, y));
}

pub fn show(app: &AppHandle, payload: PopupPayload) {
    let Some(win) = app.get_webview_window("popup") else { return };
    let state = app.state::<AppState>();
    let anchor = cursor_anchor();
    *state.popup_anchor.lock().unwrap() = Some(anchor);
    state.popup_hold.store(false, std::sync::atomic::Ordering::SeqCst);
    let (w, h) = {
        let s = state.settings.lock().unwrap();
        if s.popup_width > 0 && s.popup_height > 0 {
            (
                (s.popup_width as f64).clamp(MIN_WIDTH, MAX_USER_SIZE),
                (s.popup_height as f64).clamp(MIN_HEIGHT, MAX_USER_SIZE),
            )
        } else {
            (WIDTH, INITIAL_HEIGHT)
        }
    };
    place(&win, anchor, w, h);
    let _ = app.emit_to("popup", "popup:show", payload);
    *state.popup_shown_at.lock().unwrap() = Some(Instant::now());
    let _ = win.show();
    let _ = win.set_focus();
}

/// Called by the popup page after it has laid out its content.
pub fn resize(app: &AppHandle, height: f64) {
    let Some(win) = app.get_webview_window("popup") else { return };
    if let Some(anchor) = *app.state::<AppState>().popup_anchor.lock().unwrap() {
        place(&win, anchor, WIDTH, height.clamp(MIN_HEIGHT, MAX_HEIGHT));
    }
}

/// Remembers a size the user picked by dragging the popup's edges.
pub fn remember_size(app: &AppHandle, width: f64, height: f64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let settings = {
        let mut s = state.settings.lock().unwrap();
        s.popup_width = width.clamp(MIN_WIDTH, MAX_USER_SIZE).round() as u32;
        s.popup_height = height.clamp(MIN_HEIGHT, MAX_USER_SIZE).round() as u32;
        s.clone()
    };
    crate::config::save(app, &settings)
}

/// Hides the popup. With `restore_focus`, if the popup had keyboard focus, focus
/// goes back to the window that was active when the hotkey was pressed, so its
/// selection is still usable (for example by the replace hotkey). Otherwise
/// Windows may leave no window focused.
pub fn hide(app: &AppHandle, restore_focus: bool) {
    let Some(win) = app.get_webview_window("popup") else { return };
    let had_focus = win.is_focused().unwrap_or(false);
    let _ = win.hide();
    if restore_focus && had_focus {
        let target = app.state::<AppState>().last_foreground.load(std::sync::atomic::Ordering::SeqCst);
        crate::selection::activate(target);
    }
}
