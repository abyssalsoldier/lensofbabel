# Lantern of Babel

A small Windows tray app, written in Rust with Tauri v2, that translates selected text in **any** window using Google Translate's free endpoint.

## Usage

| Action | Default hotkey |
|---|---|
| Show a translation popup for the selected text | `Ctrl+Alt+T` |
| Replace the selected text with its translation | `Ctrl+Alt+R` |
| Translate text on screen (OCR): drag a box around any text, even text you can't select | `Ctrl+Alt+S` |

All three hotkeys can be changed in Settings, and replace and screen text can each be turned off.

**Screen text** uses the OCR engine built into Windows. It can read the languages whose OCR packs are installed (Windows Settings → Time & Language → Language & region); your display language is always included. If "Translate from" is set to a specific language with an installed OCR pack, that language is used for reading the text. In the popup you can click the recognised text to correct mistakes; the translation updates as you edit.

- **Popup:** Copy, Replace selection, Open (in the main window), and a target-language picker that re-translates. Close it with Esc or by clicking away; there is also an optional auto-close timer.
- **Main window:** opened by left-clicking the tray icon. It works like Google Translate: type or paste text and it translates as you type.
- **Settings:** languages, both hotkeys (click the field and press the keys), replace mode on/off, clipboard fallback on/off, Start with Windows, auto-close timer and theme.
- **Tray menu:** Open, Settings, Pause hotkeys, Quit. Closing a window only hides it.

Settings are stored in `%APPDATA%\dev.local.translator\config.json`.

## Build

Requirements: Rust (MSVC toolchain), Visual Studio Build Tools (C++), WebView2 (built into Windows 10/11), and the Tauri CLI (`cargo install tauri-cli --version "^2" --locked`). Node is not needed: the UI is plain HTML/CSS/JS in `ui/`.

```powershell
cd src-tauri
cargo test                      # unit tests
cargo test -- --ignored         # live test against Google's endpoint
cargo tauri dev                 # run in development mode
cargo tauri build               # release exe + installers in target\release\bundle\
```

Release output:
- `src-tauri\target\release\translator.exe`: a standalone exe.
- `bundle\nsis\*-setup.exe`: per-user installer that needs no admin rights (recommended).
- `bundle\msi\*.msi`: per-machine installer for IT deployment; needs admin rights.

## Layout

```
ui/                 index.html (main), popup.html, settings.html, shared css/js
src-tauri/src/
  lib.rs            app setup, commands, window behaviour
  translate.rs      Google endpoint client, response parsing, chunking
  selection.rs      reads the selection (UI Automation, clipboard fallback) and pastes
  hotkeys.rs        RegisterHotKey on a dedicated message-loop thread
  popup.rs          places the popup at the cursor
  ocr.rs            captures a screen region and reads it with Windows OCR
  actions.rs        hotkey → capture → translate → popup/replace
  tray.rs, config.rs
```

## EDR risk register

The app is designed to be EDR-friendly. Every behaviour that EDR could plausibly flag is listed here with its mitigation. **Keep this table up to date when adding features.**

| Feature | Risk | Mitigation |
|---|---|---|
| Global hotkeys | Low | Uses `RegisterHotKey`. No keyboard hooks (`SetWindowsHookEx`) and no key-state polling in our code. The `global-shortcut` plugin isn't used because, as far as I know, it polls `GetAsyncKeyState` to detect key release. |
| Reading the selection | Low | UI Automation (`TextPattern.GetSelection`), the read-only accessibility API that screen readers use. Runs only when the hotkey is pressed. |
| Clipboard fallback (synthetic Ctrl+C) | Low–moderate | Used only when UIA finds nothing, only on hotkey press, and the previous clipboard is restored. It can be turned off in Settings. There is no clipboard monitoring at all. |
| Replace (synthetic Ctrl+V) | Moderate | Only on hotkey press or the popup's button. It first tries UIA `ValuePattern.SetValue` when a field's whole value is selected. It can be turned off in Settings. |
| Screen text (OCR) | Moderate | Screen capture is a spyware trait. It runs only after the hotkey **and** a box drag. Only the dragged box is captured, once, with GDI `BitBlt` (as screenshot tools do). The pixels stay in memory: never saved to disk, never uploaded. OCR runs locally with `Windows.Media.Ocr`, and only the recognised text is translated. It can be turned off in Settings. |
| Start with Windows | Moderate | Writes an HKCU `Run` value. It is **off by default** and only written when the user ticks the box. HKCU only: no HKLM, scheduled tasks or services. It shows up in Task Manager → Startup. |
| Network | Low | HTTPS to `translate.googleapis.com` only, and only when the user asks for a translation. Uses Windows Schannel and the system proxy, with an honest `User-Agent`. No telemetry or update checks. |
| Raw input | Low | Tauri's windowing layer (tao) registers keyboard/mouse raw input by default. It is turned off with `DeviceEventFilter::Always` (`RIDEV_REMOVE`). |
| Imported key-state APIs | Low | `GetAsyncKeyState`, `GetKeyState` and `GetKeyboardState` appear in the import table because tao uses them to process keyboard messages for **its own windows**. Our code never calls them. |
| Unsigned exe | Moderate–high | Version info, product name and publisher are embedded; there is no packer or obfuscation. **Sign the exe and installer before distributing** (for example with Azure Trusted Signing). |
| Installer | Low | The WebView2 bootstrapper is **not** bundled or downloaded (`webviewInstallMode: skip`), so the installer never downloads and runs code. The NSIS installer is per-user with no elevation. |
| Elevation | n/a | Never requested. Windows blocks synthetic input to elevated (admin) windows, so replace and the clipboard fallback don't work in those windows. That is expected. |

Never used: process injection, reading other processes' memory, `CreateRemoteThread`, downloading or running code, self-update, hidden processes, packers, obfuscation, or saving typed or translated text to disk.

## Notes and limitations

- **Unofficial endpoint.** Google's `translate_a/single` endpoint has no SLA and can rate-limit or change without notice. All HTTP code lives in `translate.rs` so another backend can be swapped in. Google currently blocks some HTTP clients when they use `client=gtx`, so the app also tries `client=dict-chrome-ex` and remembers whichever works.
- **Clipboard restore** keeps text or image content. Other formats (rich text, files) on the clipboard are not restored after a fallback copy or a paste.
- **Admin windows:** see Elevation above.
