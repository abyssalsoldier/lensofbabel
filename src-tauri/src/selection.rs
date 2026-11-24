//! Reads the selected text in the foreground app and writes translations back.
//!
//! EDR notes:
//! - First choice is UI Automation, the read-only accessibility API that screen
//!   readers use. It sends no synthetic input and does not touch the clipboard.
//! - The clipboard fallback (synthetic Ctrl+C) and paste (synthetic Ctrl+V) run only
//!   right after the user presses a hotkey, put the clipboard back afterwards, and
//!   can be turned off in settings. Nothing here monitors the clipboard or keyboard.

use std::thread;
use std::time::{Duration, Instant};
use windows::core::{Interface, BSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomation2, IUIAutomationTextPattern, IUIAutomationValuePattern,
    UIA_TextPatternId, UIA_ValuePatternId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_C,
    VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow};

use crate::hotkeys::{MODS_ALT, MODS_CTRL, MODS_SHIFT, MODS_WIN};

/// Initializes COM on the current thread for as long as it is alive.
pub struct ComGuard(bool);

impl ComGuard {
    pub fn init() -> Self {
        ComGuard(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

pub struct Captured {
    pub text: String,
    /// Set when the whole value of an editable field is selected, so a
    /// replacement can go through UIA instead of a synthetic paste.
    pub value_target: Option<IUIAutomationValuePattern>,
}

pub fn foreground() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

pub fn is_own_window(hwnd: isize) -> bool {
    if hwnd == 0 {
        return false;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(HWND(hwnd as *mut _), Some(&mut pid)) };
    pid == unsafe { GetCurrentProcessId() }
}

pub fn activate(hwnd: isize) {
    if hwnd != 0 {
        unsafe {
            let _ = SetForegroundWindow(HWND(hwnd as *mut _));
        }
    }
}

/// `hotkey_mods` are the MOD_* flags of the hotkey that triggered this call.
/// The caller must have initialized COM on this thread.
pub fn capture(allow_clipboard: bool, hotkey_mods: u32) -> Option<Captured> {
    if let Some(c) = uia_capture() {
        return Some(c);
    }
    if allow_clipboard {
        return clipboard_copy(hotkey_mods).map(|text| Captured { text, value_target: None });
    }
    None
}

fn uia_capture() -> Option<Captured> {
    unsafe {
        let auto: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        if let Ok(a2) = auto.cast::<IUIAutomation2>() {
            // Don't hang if the target app is unresponsive.
            let _ = a2.SetConnectionTimeout(1500);
            let _ = a2.SetTransactionTimeout(1500);
        }
        let element = auto.GetFocusedElement().ok()?;
        let pattern: IUIAutomationTextPattern = element.GetCurrentPatternAs(UIA_TextPatternId).ok()?;
        let ranges = pattern.GetSelection().ok()?;
        let mut text = String::new();
        for i in 0..ranges.Length().ok()? {
            if let Ok(range) = ranges.GetElement(i) {
                if let Ok(s) = range.GetText(-1) {
                    text.push_str(&s.to_string());
                }
            }
        }
        if text.trim().is_empty() {
            return None;
        }
        let value_target = element
            .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            .ok()
            .filter(|vp| {
                let writable = vp.CurrentIsReadOnly().map(|b| !b.as_bool()).unwrap_or(false);
                let whole = vp.CurrentValue().map(|v| v.to_string() == text).unwrap_or(false);
                writable && whole
            });
        Some(Captured { text, value_target })
    }
}

/// Replaces an editable field's whole value through UIA. Returns false if that failed.
pub fn set_value(target: &IUIAutomationValuePattern, text: &str) -> bool {
    unsafe { target.SetValue(&BSTR::from(text)) }.is_ok()
}

enum Backup {
    Text(String),
    Image(arboard::ImageData<'static>),
    Empty,
}

impl Backup {
    fn take(cb: &mut arboard::Clipboard) -> Self {
        if let Ok(t) = cb.get_text() {
            Backup::Text(t)
        } else if let Ok(img) = cb.get_image() {
            Backup::Image(img.to_owned_img())
        } else {
            Backup::Empty
        }
    }

    fn restore(self, cb: &mut arboard::Clipboard) {
        let _ = match self {
            Backup::Text(t) => cb.set_text(t),
            Backup::Image(i) => cb.set_image(i),
            Backup::Empty => cb.clear(),
        };
    }
}

fn clipboard_copy(hotkey_mods: u32) -> Option<String> {
    let mut cb = arboard::Clipboard::new().ok()?;
    let backup = Backup::take(&mut cb);
    let before = unsafe { GetClipboardSequenceNumber() };

    release_modifiers(hotkey_mods);
    send_ctrl_combo(VK_C);

    let start = Instant::now();
    let mut changed = false;
    while start.elapsed() < Duration::from_millis(500) {
        if unsafe { GetClipboardSequenceNumber() } != before {
            changed = true;
            break;
        }
        thread::sleep(Duration::from_millis(15));
    }
    if !changed {
        return None; // nothing selected, or the app ignored Ctrl+C
    }
    thread::sleep(Duration::from_millis(30));
    let text = cb.get_text().ok();
    backup.restore(&mut cb);
    text.filter(|t| !t.trim().is_empty())
}

/// Pastes `text` into the focused app, then puts the old clipboard back.
pub fn paste_text(text: &str, hotkey_mods: u32) -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| format!("Clipboard unavailable: {e}"))?;
    let backup = Backup::take(&mut cb);
    cb.set_text(text.to_string()).map_err(|e| format!("Clipboard unavailable: {e}"))?;
    release_modifiers(hotkey_mods);
    send_ctrl_combo(VK_V);
    // Give the target app time to read the clipboard before restoring it.
    thread::sleep(Duration::from_millis(400));
    backup.restore(&mut cb);
    Ok(())
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) {
    unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
}

/// The user may still be holding the hotkey's modifiers. Release them logically
/// so the synthetic Ctrl+C/V isn't read as Ctrl+Alt+C. This is done without
/// polling key state. An unassigned "mask" key (0xE8) goes first so that
/// releasing Alt or Win doesn't open a menu or the Start menu.
fn release_modifiers(mods: u32) {
    if mods == 0 {
        return;
    }
    let mut inputs = Vec::new();
    if mods & (MODS_ALT | MODS_WIN) != 0 {
        inputs.push(key(VIRTUAL_KEY(0xE8), false));
        inputs.push(key(VIRTUAL_KEY(0xE8), true));
    }
    if mods & MODS_CTRL != 0 {
        inputs.push(key(VK_CONTROL, true));
    }
    if mods & MODS_ALT != 0 {
        inputs.push(key(VK_MENU, true));
    }
    if mods & MODS_SHIFT != 0 {
        inputs.push(key(VK_SHIFT, true));
    }
    if mods & MODS_WIN != 0 {
        inputs.push(key(VK_LWIN, true));
        inputs.push(key(VK_RWIN, true));
    }
    send(&inputs);
    thread::sleep(Duration::from_millis(30));
}

fn send_ctrl_combo(vk: VIRTUAL_KEY) {
    send(&[key(VK_CONTROL, false), key(vk, false), key(vk, true), key(VK_CONTROL, true)]);
}
