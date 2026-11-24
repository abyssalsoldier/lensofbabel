const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const result = $("result"), original = $("original"), langs = $("langs"), target = $("target");
const footer = $("footer"), replaceBtn = $("replace"), copyBtn = $("copy");

fillLanguages(target, false);

const appWindow = window.__TAURI__.window.getCurrentWindow();

let current = null; // last popup:show payload
let translated = "";
let dismissTimer = null;
let requestId = 0;

// Once the user drags or resizes the popup, stop sizing it to its content and
// moving it back to the cursor.
let userMoved = false;
let userSized = false;
let ignoreResizeUntil = 0; // resize events before this time come from our own code
let rememberTimer = null;

function setFixed(fixed) {
  document.body.classList.toggle("fixed", fixed);
}

function fit() {
  if (userMoved || userSized || (current && current.fixedSize)) return;
  requestAnimationFrame(() => {
    const height = $("card").scrollHeight + 12;
    ignoreResizeUntil = Date.now() + 600;
    invoke("popup_resize", { height });
  });
}

document.querySelector("header").addEventListener("mousedown", (e) => {
  if (e.button === 0 && e.target.hasAttribute("data-tauri-drag-region")) userMoved = true;
});

// --- Splitter: drag up to show more of the original text -------------------
const splitter = $("splitter");
const COLLAPSED = 44; // px, about three lines of the small text
const MIN_RESULT = 60; // px the translation always keeps

function readStoredHeight() {
  try { return parseInt(localStorage.getItem("originalMax"), 10) || COLLAPSED; } catch { return COLLAPSED; }
}
function setOriginalMax(px, persist) {
  original.style.setProperty("--original-max", `${Math.round(px)}px`);
  if (persist) { try { localStorage.setItem("originalMax", String(Math.round(px))); } catch {} }
}
function maxOriginal() {
  // Space the original can take while leaving room for the translation.
  return Math.max(COLLAPSED, original.offsetHeight + $("result").clientHeight - MIN_RESULT);
}
setOriginalMax(readStoredHeight(), false);

let drag = null;
splitter.addEventListener("pointerdown", (e) => {
  if (e.button !== 0) return;
  e.preventDefault();
  splitter.setPointerCapture(e.pointerId);
  splitter.classList.add("dragging");
  drag = { y: e.clientY, h: original.offsetHeight, limit: maxOriginal() };
});
splitter.addEventListener("pointermove", (e) => {
  if (!drag) return;
  const h = Math.min(drag.limit, Math.max(24, drag.h + (drag.y - e.clientY)));
  setOriginalMax(h, false);
});
function endDrag() {
  if (!drag) return;
  drag = null;
  splitter.classList.remove("dragging");
  setOriginalMax(original.offsetHeight, true);
  fit(); // in automatic-size mode, let the window grow to fit
}
splitter.addEventListener("pointerup", endDrag);
splitter.addEventListener("pointercancel", endDrag);
splitter.addEventListener("dblclick", () => {
  const expanded = original.offsetHeight > COLLAPSED + 4;
  setOriginalMax(expanded ? COLLAPSED : Math.min(original.scrollHeight + 6, maxOriginal()), true);
  fit();
});

$("grip").addEventListener("mousedown", (e) => {
  if (e.button !== 0) return;
  e.preventDefault();
  userSized = true;
  setFixed(true);
  appWindow.startResizeDragging("SouthEast");
});

appWindow.onResized(() => {
  if (Date.now() < ignoreResizeUntil || !current) return;
  userSized = true;
  setFixed(true);
  clearTimeout(rememberTimer);
  rememberTimer = setTimeout(() => {
    invoke("popup_remember_size", { width: window.innerWidth, height: window.innerHeight });
  }, 500);
});

function hide() {
  clearTimeout(dismissTimer);
  invoke("hide_popup");
}

function armDismiss() {
  clearTimeout(dismissTimer);
  if (current && current.dismissSecs > 0) {
    dismissTimer = setTimeout(hide, current.dismissSecs * 1000);
  }
}

async function translate() {
  const id = ++requestId;
  result.textContent = "Translating…";
  result.className = "result muted";
  copyBtn.textContent = "Copy";
  fit();
  try {
    const res = await invoke("translate", { text: current.text, source: current.source, target: target.value });
    if (id !== requestId) return;
    translated = res.text;
    result.textContent = res.text;
    result.className = "result";
    const from = current.source === "auto" ? res.detectedSource : current.source;
    langs.textContent = from ? `${languageName(from)} →` : "";
    footer.hidden = false;
  } catch (e) {
    if (id !== requestId) return;
    result.textContent = String(e);
    result.className = "result error";
  }
  fit();
  armDismiss();
}

listen("popup:show", (e) => {
  current = e.payload;
  userMoved = false;
  userSized = false;
  ignoreResizeUntil = Date.now() + 600; // Rust has just placed and sized the window
  setFixed(current.fixedSize);
  applyTheme(current.theme);
  translated = "";
  replaceBtn.hidden = !current.replaceEnabled;
  if (current.error) {
    langs.textContent = "";
    result.textContent = current.error;
    result.className = "result muted";
    original.textContent = "";
    original.hidden = true;
    splitter.hidden = true;
    footer.hidden = true;
    target.hidden = true;
    fit();
    armDismiss();
    return;
  }
  target.hidden = false;
  original.hidden = false;
  splitter.hidden = false;
  // OCR text can contain mistakes, so let the user correct it.
  original.contentEditable = current.ocr ? "plaintext-only" : "false";
  original.title = current.ocr ? "Text read from the screen. Click to correct it; the translation updates." : "";
  target.value = current.target;
  original.textContent = current.text;
  original.scrollTop = 0;
  footer.hidden = true;
  translate();
});

target.addEventListener("change", () => current && !current.error && translate());

let editTimer = null;
original.addEventListener("input", () => {
  if (!current || !current.ocr) return;
  clearTimeout(dismissTimer);
  clearTimeout(editTimer);
  editTimer = setTimeout(() => {
    current.text = original.innerText;
    if (current.text.trim()) translate();
  }, 600);
});
// Keep the popup open while the dropdown list is showing.
target.addEventListener("focus", () => invoke("popup_hold", { hold: true }));
target.addEventListener("blur", () => invoke("popup_hold", { hold: false }));

copyBtn.addEventListener("click", async () => {
  if (!translated) return;
  await navigator.clipboard.writeText(translated);
  copyBtn.textContent = "Copied";
});

replaceBtn.addEventListener("click", () => {
  if (translated) invoke("replace_with", { text: translated });
});

$("open").addEventListener("click", () => current && invoke("open_main_with", { text: current.text }));
$("close").addEventListener("click", hide);

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") hide();
  if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && translated) copyBtn.click();
});

// Pause auto-dismiss while the pointer is over the popup.
document.addEventListener("mouseenter", () => clearTimeout(dismissTimer));
document.addEventListener("mouseleave", armDismiss);
