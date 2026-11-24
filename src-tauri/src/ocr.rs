//! Screen-text mode: capture a region the user selected and read it with the OCR
//! engine built into Windows (`Windows.Media.Ocr`).
//!
//! EDR notes (⚠ screen capture, moderate risk):
//! - Runs only after the user presses the screen-text hotkey and drags a box.
//! - Captures only that box, once, with standard GDI `BitBlt` (as screenshot tools do).
//! - Pixels stay in memory: never written to disk, never sent over the network.
//!   OCR runs locally; only the recognised text is translated.

use windows::core::HSTRING;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, GetMonitorInfoW,
    MonitorFromPoint, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

/// A rectangle in physical screen pixels.
#[derive(Debug, Clone, Copy)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// The full rectangle of the monitor under the cursor, in physical pixels.
pub fn cursor_monitor() -> Region {
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let r = mi.rcMonitor;
        Region { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
    }
}

/// Copies the pixels of `r` from the screen as top-down BGRA.
pub fn capture(r: Region) -> Result<Vec<u8>, String> {
    if r.w <= 0 || r.h <= 0 {
        return Err("The selected area is empty.".into());
    }
    unsafe {
        let screen = GetDC(HWND::default());
        if screen.is_invalid() {
            return Err("Could not access the screen.".into());
        }
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, r.w, r.h);
        let old = SelectObject(mem, bmp);
        let blt = BitBlt(mem, 0, 0, r.w, r.h, screen, r.x, r.y, SRCCOPY);
        SelectObject(mem, old);

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: r.w,
                biHeight: -r.h, // negative height = top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (r.w as usize) * (r.h as usize) * 4];
        let lines = GetDIBits(mem, bmp, 0, r.h as u32, Some(pixels.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);

        let _ = DeleteObject(bmp);
        let _ = DeleteDC(mem);
        ReleaseDC(HWND::default(), screen);

        blt.map_err(|e| format!("Screen capture failed: {e}"))?;
        if lines == 0 {
            return Err("Screen capture failed.".into());
        }
        // GDI leaves alpha at 0; make the image opaque.
        for px in pixels.chunks_exact_mut(4) {
            px[3] = 255;
        }
        Ok(pixels)
    }
}

/// Doubles the image size (nearest neighbour). Small UI text is recognised much
/// more reliably when it is larger.
fn upscale2(src: &[u8], w: i32, h: i32) -> Vec<u8> {
    let (w2, h2) = (w * 2, h * 2);
    let mut out = vec![0u8; (w2 as usize) * (h2 as usize) * 4];
    for y in 0..h2 {
        for x in 0..w2 {
            let s = (((y / 2) * w + x / 2) * 4) as usize;
            let d = ((y * w2 + x) * 4) as usize;
            out[d..d + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
    out
}

/// Google language codes that differ from the BCP-47 tags Windows uses.
fn windows_language_tag(code: &str) -> &str {
    match code {
        "zh-CN" => "zh-Hans",
        "zh-TW" => "zh-Hant",
        "iw" => "he",
        "jw" => "jv",
        other => other,
    }
}

fn engine_for(source_lang: &str) -> Result<OcrEngine, String> {
    if source_lang != "auto" {
        let tag = HSTRING::from(windows_language_tag(source_lang));
        if let Ok(lang) = Language::CreateLanguage(&tag) {
            if OcrEngine::IsLanguageSupported(&lang).unwrap_or(false) {
                if let Ok(engine) = OcrEngine::TryCreateFromLanguage(&lang) {
                    return Ok(engine);
                }
            }
        }
    }
    OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| {
        "No Windows OCR language is installed. Add a language in Windows Settings → Time & Language → \
         Language & region."
            .to_string()
    })
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xAC00..=0xD7AF | 0xFF00..=0xFFEF | 0x3000..=0x303F)
}

/// Windows OCR separates CJK characters with spaces; remove those.
fn tidy_cjk_spacing(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == ' ' && i > 0 && i + 1 < chars.len() && is_cjk(chars[i - 1]) && is_cjk(chars[i + 1]) {
            continue;
        }
        out.push(c);
    }
    out
}

/// Recognises the text in a top-down BGRA image. Lines are joined with newlines.
pub fn recognize(pixels: Vec<u8>, w: i32, h: i32, source_lang: &str) -> Result<String, String> {
    let max = OcrEngine::MaxImageDimension().map(|m| m as i32).unwrap_or(4000);
    let (pixels, w, h) = if h < 600 && w * 2 <= max && h * 2 <= max {
        (upscale2(&pixels, w, h), w * 2, h * 2)
    } else {
        (pixels, w, h)
    };
    if w > max || h > max {
        return Err(format!("The selected area is too large for OCR (max {max} pixels per side)."));
    }

    let engine = engine_for(source_lang)?;
    let err = |e: windows::core::Error| format!("OCR failed: {}", e.message());
    let writer = DataWriter::new().map_err(err)?;
    writer.WriteBytes(&pixels).map_err(err)?;
    let buffer = writer.DetachBuffer().map_err(err)?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, w, h).map_err(err)?;
    let result = engine.RecognizeAsync(&bitmap).map_err(err)?.get().map_err(err)?;

    let lines = result.Lines().map_err(err)?;
    let mut out = Vec::new();
    for i in 0..lines.Size().map_err(err)? {
        let text = lines.GetAt(i).and_then(|l| l.Text()).map_err(err)?;
        out.push(tidy_cjk_spacing(&text.to_string()));
    }
    Ok(out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_spaces_between_cjk_only() {
        assert_eq!(tidy_cjk_spacing("日 本 語 の テ キ ス ト"), "日本語のテキスト");
        assert_eq!(tidy_cjk_spacing("Hello world"), "Hello world");
        assert_eq!(tidy_cjk_spacing("Rust で 書く"), "Rust で書く");
    }

    /// Set OCR_SAMPLE to a raw top-down BGRA file named like `name_520x60.bgra`.
    #[test]
    #[ignore = "needs a sample image and Windows OCR"]
    fn recognizes_sample_image() {
        let path = std::env::var("OCR_SAMPLE").expect("set OCR_SAMPLE");
        let dims = path.rsplit('_').next().unwrap().trim_end_matches(".bgra");
        let (w, h) = dims.split_once('x').unwrap();
        let (w, h): (i32, i32) = (w.parse().unwrap(), h.parse().unwrap());
        let _com = crate::selection::ComGuard::init();
        let text = recognize(std::fs::read(&path).unwrap(), w, h, "auto").unwrap();
        println!("OCR: {text:?}");
        assert!(text.to_lowercase().contains("quick brown fox"));
    }

    #[test]
    fn upscale_doubles_pixels() {
        let src = vec![1, 2, 3, 255, 4, 5, 6, 255]; // 2x1
        let out = upscale2(&src, 2, 1);
        assert_eq!(out.len(), 4 * 2 * 4);
        assert_eq!(&out[0..8], &[1, 2, 3, 255, 1, 2, 3, 255]);
        assert_eq!(&out[16..24], &[1, 2, 3, 255, 1, 2, 3, 255]);
    }
}
