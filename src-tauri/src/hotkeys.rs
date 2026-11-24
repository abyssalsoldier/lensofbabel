//! Global hotkeys through `RegisterHotKey`, the normal Windows API for app shortcuts.
//!
//! EDR note: this deliberately avoids low-level keyboard hooks (`SetWindowsHookEx`)
//! and key-state polling (`GetAsyncKeyState`), which are classic keylogger signals.
//! The OS delivers WM_HOTKEY only for the exact chords registered here.

use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetMessageW, PeekMessageW, PostThreadMessageW, MSG, PM_NOREMOVE, WM_APP, WM_HOTKEY, WM_USER,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Popup,
    Replace,
    /// Translate text read from a screen region (OCR).
    Screen,
}

impl Action {
    const ALL: [Action; 3] = [Action::Popup, Action::Replace, Action::Screen];

    fn id(self) -> i32 {
        match self {
            Action::Popup => 1,
            Action::Replace => 2,
            Action::Screen => 3,
        }
    }

    fn from_id(id: i32) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.id() == id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    /// MOD_* flags
    pub mods: u32,
    pub vk: u32,
}

const WM_REREGISTER: u32 = WM_APP + 1;

pub const MODS_CTRL: u32 = MOD_CONTROL.0;
pub const MODS_ALT: u32 = MOD_ALT.0;
pub const MODS_SHIFT: u32 = MOD_SHIFT.0;
pub const MODS_WIN: u32 = MOD_WIN.0;

/// Parses strings like "Ctrl+Alt+T" or "Ctrl+Shift+F2".
pub fn parse(s: &str) -> Result<Hotkey, String> {
    let mut mods = 0u32;
    let mut vk = None;
    for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods |= MODS_CTRL,
            "alt" => mods |= MODS_ALT,
            "shift" => mods |= MODS_SHIFT,
            "win" | "super" | "meta" => mods |= MODS_WIN,
            key => {
                if vk.is_some() {
                    return Err(format!("'{s}' has more than one non-modifier key."));
                }
                vk = Some(key_code(key).ok_or_else(|| format!("Unknown key '{part}'."))?);
            }
        }
    }
    let vk = vk.ok_or_else(|| format!("'{s}' needs a key besides the modifiers."))?;
    if mods == 0 {
        return Err(format!("'{s}' needs at least one modifier (Ctrl, Alt, Shift or Win)."));
    }
    Ok(Hotkey { mods, vk })
}

fn key_code(k: &str) -> Option<u32> {
    let b = k.as_bytes();
    if b.len() == 1 && (b[0].is_ascii_alphanumeric()) {
        return Some(b[0].to_ascii_uppercase() as u32);
    }
    if let Some(n) = k.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) {
            return Some(0x70 + n - 1);
        }
    }
    Some(match k {
        "space" => 0x20,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "end" => 0x23,
        "home" => 0x24,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        "insert" => 0x2D,
        "delete" => 0x2E,
        ";" => 0xBA,
        "=" => 0xBB,
        "," => 0xBC,
        "-" => 0xBD,
        "." => 0xBE,
        "/" => 0xBF,
        "`" => 0xC0,
        "[" => 0xDB,
        "\\" => 0xDC,
        "]" => 0xDD,
        "'" => 0xDE,
        _ => return None,
    })
}

/// One hotkey to register: the action, the parsed chord and its display name.
pub type Binding = (Action, Hotkey, String);

struct Request {
    bindings: Vec<Binding>,
    reply: mpsc::Sender<Result<(), String>>,
}

/// Owns a thread that runs a message loop and holds the hotkey registrations
/// (`RegisterHotKey` ties them to the thread that calls it).
pub struct HotkeyManager {
    thread_id: u32,
    tx: Mutex<mpsc::Sender<Request>>,
}

impl HotkeyManager {
    pub fn start(on_hotkey: impl Fn(Action) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel::<Request>();
        let (tid_tx, tid_rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("hotkeys".into())
            .spawn(move || unsafe {
                let mut msg = MSG::default();
                // Make sure this thread has a message queue before anyone posts to it.
                let _ = PeekMessageW(&mut msg, HWND::default(), WM_USER, WM_USER, PM_NOREMOVE);
                let _ = tid_tx.send(GetCurrentThreadId());
                loop {
                    let r = GetMessageW(&mut msg, HWND::default(), 0, 0);
                    if r.0 <= 0 {
                        break;
                    }
                    match msg.message {
                        WM_HOTKEY => {
                            if let Some(action) = Action::from_id(msg.wParam.0 as i32) {
                                on_hotkey(action);
                            }
                        }
                        WM_REREGISTER => {
                            while let Ok(req) = rx.try_recv() {
                                let _ = req.reply.send(apply(&req));
                            }
                        }
                        _ => {}
                    }
                }
            })
            .expect("failed to start hotkey thread");
        let thread_id = tid_rx.recv().expect("hotkey thread did not start");
        Self { thread_id, tx: Mutex::new(tx) }
    }

    /// Replaces all registrations with `bindings` (an empty list unregisters everything).
    pub fn set(&self, bindings: Vec<Binding>) -> Result<(), String> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .lock()
            .unwrap()
            .send(Request { bindings, reply })
            .map_err(|_| "The hotkey thread has stopped.".to_string())?;
        unsafe { PostThreadMessageW(self.thread_id, WM_REREGISTER, WPARAM(0), LPARAM(0)) }
            .map_err(|e| format!("Could not update hotkeys: {e}"))?;
        rx.recv_timeout(Duration::from_secs(3))
            .map_err(|_| "Timed out while updating hotkeys.".to_string())?
    }
}

fn apply(req: &Request) -> Result<(), String> {
    for action in Action::ALL {
        unsafe {
            let _ = UnregisterHotKey(HWND::default(), action.id());
        }
    }
    let mut errors = Vec::new();
    for (action, hk, name) in &req.bindings {
        let mods = HOT_KEY_MODIFIERS(hk.mods | MOD_NOREPEAT.0);
        if unsafe { RegisterHotKey(HWND::default(), action.id(), mods, hk.vk) }.is_err() {
            errors.push(format!("{name} is already in use by another app."));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_hotkeys() {
        assert_eq!(parse("Ctrl+Alt+T").unwrap(), Hotkey { mods: MODS_CTRL | MODS_ALT, vk: 'T' as u32 });
        assert_eq!(parse("shift + win + f2").unwrap(), Hotkey { mods: MODS_SHIFT | MODS_WIN, vk: 0x71 });
        assert_eq!(parse("Ctrl+Space").unwrap().vk, 0x20);
    }

    #[test]
    fn rejects_bad_hotkeys() {
        assert!(parse("T").is_err());
        assert!(parse("Ctrl+Alt").is_err());
        assert!(parse("Ctrl+A+B").is_err());
        assert!(parse("Ctrl+Banana").is_err());
    }
}
