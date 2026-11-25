//! Start with Windows: one value under HKCU\...\Run, written only when the user turns it on.
//!
//! EDR: HKCU only (never HKLM, scheduled tasks or services), and the exe path is quoted so
//! a path with spaces can't be read as a shorter, different program ("unquoted path").
//! The installer (installer/hooks.nsh) writes and removes the same value, under the same
//! name (the product name), so the installer's checkbox and Settings always agree.
//!
//! This replaces tauri-plugin-autostart, which wrote the path unquoted and tried HKLM first.

use std::ffi::c_void;
use windows::core::HSTRING;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_BINARY, REG_ROUTINE_FLAGS, REG_SZ,
    RRF_RT_REG_BINARY, RRF_RT_REG_SZ,
};

/// Passed to the app when Windows starts it, so it starts quietly in the tray.
pub const ARG: &str = "--autostart";

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Where Task Manager → Startup records "Disabled" without removing the Run value.
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
/// Task Manager's "Enabled" record: 0x02 followed by zeros.
const APPROVED_ENABLED: [u8; 12] = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

/// On only if the Run value exists and Task Manager hasn't disabled it.
pub fn is_enabled(name: &str) -> Result<bool, String> {
    if read(RUN, name, RRF_RT_REG_SZ)?.is_none() {
        return Ok(false);
    }
    Ok(is_approved(read(APPROVED, name, RRF_RT_REG_BINARY)?.as_deref()))
}

pub fn enable(name: &str) -> Result<(), String> {
    write_string(RUN, name, &command()?)?;
    // A "Disabled" mark from Task Manager would win over the Run value, so clear it.
    if !is_approved(read(APPROVED, name, RRF_RT_REG_BINARY)?.as_deref()) {
        write(APPROVED, name, REG_BINARY.0, &APPROVED_ENABLED)?;
    }
    Ok(())
}

pub fn disable(name: &str) -> Result<(), String> {
    delete(RUN, name)
}

/// Rewrites an entry left by tauri-plugin-autostart (version 0.1.0) for this same exe, which
/// has the path unquoted. Anything else is left alone: an entry only changes when the user
/// asks, and this one is just the same choice written safely.
pub fn repair_legacy(name: &str) -> Result<(), String> {
    let Some(data) = read(RUN, name, RRF_RT_REG_SZ)? else { return Ok(()) };
    let exe = exe_path()?;
    if from_wide(&data) == format!("{exe} {ARG}") {
        write_string(RUN, name, &quoted_command(&exe))?;
    }
    Ok(())
}

fn exe_path() -> Result<String, String> {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .map_err(|e| format!("Could not find the app's path: {e}"))
}

fn command() -> Result<String, String> {
    Ok(quoted_command(&exe_path()?))
}

fn quoted_command(exe: &str) -> String {
    format!("\"{exe}\" {ARG}")
}

/// Task Manager stores an odd first byte (0x03, 0x07) for "Disabled".
fn is_approved(record: Option<&[u8]>) -> bool {
    record.and_then(|r| r.first()).map_or(true, |b| b & 1 == 0)
}

fn check(err: WIN32_ERROR, what: &str) -> Result<(), String> {
    if err == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("Could not {what} the startup entry: {}", windows::core::Error::from(err.to_hresult()).message()))
    }
}

/// Reads a value's raw bytes; `None` if the key or value doesn't exist.
fn read(subkey: &str, name: &str, flags: REG_ROUTINE_FLAGS) -> Result<Option<Vec<u8>>, String> {
    let (subkey, name) = (HSTRING::from(subkey), HSTRING::from(name));
    loop {
        let mut size = 0u32;
        // SAFETY: size query with no buffer; the strings outlive the call.
        let err = unsafe { RegGetValueW(HKEY_CURRENT_USER, &subkey, &name, flags, None, None, Some(&mut size)) };
        if err == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        check(err, "read")?;
        let mut buf = vec![0u8; size as usize];
        // SAFETY: `buf` is `size` bytes long and RegGetValueW writes at most `size` bytes.
        let err = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                &subkey,
                &name,
                flags,
                None,
                Some(buf.as_mut_ptr() as *mut c_void),
                Some(&mut size),
            )
        };
        match err {
            ERROR_FILE_NOT_FOUND => return Ok(None),
            // The value grew between the two calls: try again.
            e if e == windows::Win32::Foundation::ERROR_MORE_DATA => continue,
            e => check(e, "read")?,
        }
        buf.truncate(size as usize);
        return Ok(Some(buf));
    }
}

fn write(subkey: &str, name: &str, kind: u32, data: &[u8]) -> Result<(), String> {
    // SAFETY: `data` is valid for `data.len()` bytes; the strings outlive the call.
    let err = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(subkey),
            &HSTRING::from(name),
            kind,
            Some(data.as_ptr() as *const c_void),
            data.len() as u32,
        )
    };
    check(err, "write")
}

fn write_string(subkey: &str, name: &str, value: &str) -> Result<(), String> {
    let bytes: Vec<u8> = value.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
    write(subkey, name, REG_SZ.0, &bytes)
}

fn delete(subkey: &str, name: &str) -> Result<(), String> {
    // SAFETY: the strings outlive the call.
    let err = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(subkey), &HSTRING::from(name)) };
    if err == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    check(err, "remove")
}

/// REG_SZ bytes (UTF-16LE, null-terminated) to a String.
fn from_wide(data: &[u8]) -> String {
    let wide: Vec<u16> = data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_quotes_the_path() {
        assert_eq!(
            quoted_command(r"C:\Users\A B\AppData\Local\Lens of Babel Translator\lens-of-babel.exe"),
            r#""C:\Users\A B\AppData\Local\Lens of Babel Translator\lens-of-babel.exe" --autostart"#
        );
    }

    #[test]
    fn task_manager_records() {
        assert!(is_approved(None));
        assert!(is_approved(Some(&APPROVED_ENABLED)));
        assert!(is_approved(Some(&[6, 0, 0, 0])));
        assert!(!is_approved(Some(&[3, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8])));
        assert!(!is_approved(Some(&[7, 0, 0, 0])));
    }

    #[test]
    fn wide_round_trip() {
        let bytes: Vec<u8> = "\"C:\\x y\\a.exe\" --autostart".encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
        assert_eq!(from_wide(&bytes), "\"C:\\x y\\a.exe\" --autostart");
    }

    /// Writes, reads and removes a throwaway value under HKCU\...\Run.
    #[test]
    #[ignore]
    fn registry_round_trip() {
        let name = "Lens of Babel autostart test";
        enable(name).unwrap();
        assert!(is_enabled(name).unwrap());
        let data = read(RUN, name, RRF_RT_REG_SZ).unwrap().unwrap();
        assert!(from_wide(&data).starts_with('"'));
        disable(name).unwrap();
        assert!(!is_enabled(name).unwrap());
        disable(name).unwrap(); // removing twice is fine
    }
}
