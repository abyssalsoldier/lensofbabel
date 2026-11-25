# Lens of Babel

A small Windows tray app, written in Rust with Tauri v2, that translates selected text in **any** window using Google Translate's free endpoint.

## Download

**[Download the latest release](https://github.com/abyssalsoldier/lensofbabel/releases/latest)**

- `LensOfBabel-x.y.z-setup.exe`: recommended. An install wizard that adds a Start menu shortcut and an uninstaller. It can also add a desktop shortcut and turn on Start with Windows.
- `LensOfBabel-x.y.z.msi`: installs for all users on the machine, for IT deployment. Needs admin rights.
- `LensOfBabel-x.y.z-portable.zip`: nothing to install. Extract it and run `LensOfBabel.exe`.

Windows 10 or 11. The downloads aren't code-signed yet, so Windows SmartScreen may warn the first time you run one: choose **More info → Run anyway**.

## Usage

| Action | Default hotkey |
|---|---|
| Show a translation popup for the selected text | `Ctrl+Alt+T` |
| Replace the selected text with its translation | `Ctrl+Alt+R` |
| Translate text on screen (OCR): drag a box around any text, even text you can't select | `Ctrl+Alt+S` |

All three hotkeys can be changed in Settings, and replace and screen text can each be turned off.

**Screen text** uses the OCR engine built into Windows. It can read the languages whose OCR packs are installed (Windows Settings → Time & Language → Language & region); your display language is always included. If "Translate from" is set to a specific language with an installed OCR pack, that language is used for reading the text. In the popup you can click the recognised text to correct mistakes; the translation updates as you edit.

- **Popup:** Copy, Replace selection, Open (in the main window), and source- and target-language pickers that re-translate. The source picker shows the detected language; if detection got it wrong, pick the right one. Close it with Esc or by clicking away; there is also an optional auto-close timer.
- **Main window:** opened by left-clicking the tray icon. It works like Google Translate: type or paste text and it translates as you type.
- **Settings:** languages, both hotkeys (click the field and press the keys), replace mode on/off, clipboard fallback on/off, Start with Windows, auto-close timer and theme. The About section shows the installed version and has a **Check for updates** button, which asks GitHub for the latest release and opens its download page if there is a newer one. It also checks automatically at startup and once a day (turn this off in Settings): a new version gets one Windows notification and a **Download update** item in the tray menu.
- **Tray menu:** Open, Settings, Pause hotkeys, Quit. Closing a window only hides it.

The installer shows the MIT licence in `LICENSE`.

Settings are stored in `%APPDATA%\io.github.abyssalsoldier.lensofbabel\config.json`.

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
- `src-tauri\target\release\lens-of-babel.exe`: a standalone exe.
- `bundle\nsis\*-setup.exe`: install wizard (recommended). It offers a per-user install with no admin rights or an all-users install, adds a Start menu shortcut and an uninstaller, and on the last page offers a desktop shortcut and Start with Windows. Its pages, artwork and hooks are in `src-tauri\installer\`.
- `bundle\msi\*.msi`: per-machine installer for IT deployment; needs admin rights. Adds a Start menu shortcut.

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
| Start with Windows | Moderate | Writes an HKCU `Run` value. It is **off by default** and only written when the user ticks the box. HKCU only: no HKLM, scheduled tasks or services. The exe path is quoted (no "unquoted path" entry). It shows up in Task Manager → Startup. The setup.exe wizard offers the same setting (also off by default) and writes the same value; the uninstaller removes it. |
| Network | Low | HTTPS to `translate.googleapis.com` only, and only when the user asks for a translation. Uses Windows Schannel and the system proxy, with an honest `User-Agent`. No telemetry or update checks. |
| Raw input | Low | Tauri's windowing layer (tao) registers keyboard/mouse raw input by default. It is turned off with `DeviceEventFilter::Always` (`RIDEV_REMOVE`). |
| Imported key-state APIs | Low | `GetAsyncKeyState`, `GetKeyState` and `GetKeyboardState` appear in the import table because tao uses them to process keyboard messages for **its own windows**. Our code never calls them. |
| Unsigned exe | Moderate–high | Version info, product name and publisher are embedded; there is no packer or obfuscation. **Sign the exe and installer before distributing** (for example with Azure Trusted Signing). |
| Installer | Low | The WebView2 bootstrapper is **not** bundled or downloaded (`webviewInstallMode: skip`), so the installer never downloads and runs code. The NSIS installer asks whether to install for the current user (no elevation) or all users (`installMode: both`). Because of that choice, Windows asks an **administrator** account for UAC approval when the installer starts. Standard users get no prompt and the per-user install. |
| Elevation | n/a | The app never requests it. The installer may (see above), but it starts the app as the normal user. Windows blocks synthetic input to elevated (admin) windows, so replace and the clipboard fallback don't work in those windows. That is expected. |

Never used: process injection, reading other processes' memory, `CreateRemoteThread`, downloading or running code, self-update, hidden processes, packers, obfuscation, or saving typed or translated text to disk.

## Notes and limitations

- **Unofficial endpoint.** Google's `translate_a/single` endpoint has no SLA and can rate-limit or change without notice. All HTTP code lives in `translate.rs` so another backend can be swapped in. Google currently blocks some HTTP clients when they use `client=gtx`, so the app also tries `client=dict-chrome-ex` and remembers whichever works.
- **Clipboard restore** keeps text or image content. Other formats (rich text, files) on the clipboard are not restored after a fallback copy or a paste.
- **Admin windows:** see Elevation above.
