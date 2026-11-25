const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;

const $ = (id) => document.getElementById(id);
const msg = $("msg"), banner = $("banner");

fillLanguages($("sourceLang"), true);
fillLanguages($("targetLang"), false);

let config = null;

function setMsg(text, cls = "muted") {
  msg.textContent = text;
  msg.className = "msg " + cls;
}

let render = function (cfg) {
  config = cfg;
  applyTheme(cfg.theme);
  for (const key of ["sourceLang", "targetLang", "popupHotkey", "replaceHotkey", "screenHotkey", "theme"]) $(key).value = cfg[key];
  for (const key of ["replaceEnabled", "screenEnabled", "clipboardFallback", "autostart", "autoUpdateCheck"]) $(key).checked = cfg[key];
  $("popupDismissSecs").value = cfg.popupDismissSecs;
  $("replaceHotkey").disabled = !cfg.replaceEnabled;
  $("screenHotkey").disabled = !cfg.screenEnabled;
};

async function load() {
  render(await invoke("get_config"));
  const status = await invoke("get_status");
  banner.hidden = !status.hotkeyError;
  banner.textContent = status.hotkeyError ? `Hotkey problem: ${status.hotkeyError}` : "";
  // An update found by the automatic check.
  if (status.updateAvailable && $("getUpdate").hidden) {
    $("updateStatus").className = "hint";
    $("updateStatus").textContent = `Version ${status.updateAvailable} is available.`;
    $("getUpdate").hidden = false;
  }
}

$("replaceEnabled").addEventListener("change", (e) => { $("replaceHotkey").disabled = !e.target.checked; });
$("screenEnabled").addEventListener("change", (e) => { $("screenHotkey").disabled = !e.target.checked; });
$("theme").addEventListener("change", (e) => applyTheme(e.target.value));

// Hotkey recorder: focus the field and press the combination.
const KEY_NAMES = { " ": "Space", ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right" };
function keyName(e) {
  if (/^Key[A-Z]$/.test(e.code)) return e.code.slice(3);
  if (/^Digit[0-9]$/.test(e.code)) return e.code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(e.key)) return e.key;
  if (KEY_NAMES[e.key]) return KEY_NAMES[e.key];
  if (["PageUp", "PageDown", "Home", "End", "Insert", "Delete"].includes(e.key)) return e.key;
  if (e.key.length === 1 && ";=,-./`[]\\'".includes(e.key)) return e.key;
  return null;
}

for (const field of document.querySelectorAll(".hotkey")) {
  let previous = "";
  field.addEventListener("focus", () => {
    previous = field.value;
    field.value = "Press keys…";
    field.classList.add("recording");
  });
  field.addEventListener("blur", () => {
    if (field.value === "Press keys…") field.value = previous;
    field.classList.remove("recording");
  });
  field.addEventListener("keydown", (e) => {
    e.preventDefault();
    if (e.key === "Escape") { field.blur(); return; }
    const key = keyName(e);
    if (!key) return; // modifier on its own; wait for the main key
    const parts = [];
    if (e.ctrlKey) parts.push("Ctrl");
    if (e.altKey) parts.push("Alt");
    if (e.shiftKey) parts.push("Shift");
    if (e.metaKey) parts.push("Win");
    if (!parts.length) { setMsg("Hotkeys need at least one modifier (Ctrl, Alt, Shift or Win).", "error"); return; }
    field.value = [...parts, key].join("+");
    previous = field.value;
    setMsg("");
    field.blur();
  });
}

$("save").addEventListener("click", async () => {
  const next = {
    ...config,
    sourceLang: $("sourceLang").value,
    targetLang: $("targetLang").value,
    popupHotkey: $("popupHotkey").value,
    replaceHotkey: $("replaceHotkey").value,
    replaceEnabled: $("replaceEnabled").checked,
    screenHotkey: $("screenHotkey").value,
    screenEnabled: $("screenEnabled").checked,
    clipboardFallback: $("clipboardFallback").checked,
    autostart: $("autostart").checked,
    autoUpdateCheck: $("autoUpdateCheck").checked,
    popupDismissSecs: Math.max(0, Math.min(120, parseInt($("popupDismissSecs").value, 10) || 0)),
    theme: $("theme").value,
  };
  try {
    await invoke("save_config", { settings: next });
    config = next;
    dirty = false;
    banner.hidden = true;
    setMsg("Saved", "ok");
  } catch (e) {
    setMsg(String(e), "error");
  }
});

$("close").addEventListener("click", () => getCurrentWindow().hide());

$("resetPopupSize").addEventListener("click", async () => {
  try {
    await invoke("reset_popup_size");
    setMsg("The popup will size itself to its content again.", "ok");
  } catch (e) {
    setMsg(String(e), "error");
  }
});

// --- About: version and update check ---------------------------------------
invoke("app_version").then((v) => { $("version").textContent = v; });

$("checkUpdate").addEventListener("click", async () => {
  const btn = $("checkUpdate"), status = $("updateStatus");
  btn.disabled = true;
  status.className = "hint";
  status.textContent = "Checking…";
  try {
    const info = await invoke("check_for_update");
    $("getUpdate").hidden = !info.available;
    status.textContent = info.available
      ? `Version ${info.latest} is available.`
      : `You have the latest version (${info.current}).`;
  } catch (e) {
    status.className = "hint error";
    status.textContent = String(e);
  }
  btn.disabled = false;
});

$("getUpdate").addEventListener("click", () => {
  invoke("open_releases_page").catch((e) => setMsg(String(e), "error"));
});

// Track unsaved edits so a refresh never discards them.
let dirty = false;
document.addEventListener("change", () => { dirty = true; });
document.addEventListener("input", () => { dirty = true; });
const origRender = render;
render = (cfg) => { origRender(cfg); dirty = false; };

listen("config:changed", (e) => { if (!dirty) render(e.payload); });
// Refresh when the window is shown again (for example, autostart removed in Task Manager).
window.addEventListener("focus", () => { if (!dirty && !document.querySelector(".recording")) load(); });
load();
