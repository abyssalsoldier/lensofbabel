const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const source = $("source"), target = $("target"), input = $("input"), output = $("output");
const statusEl = $("status"), detectedEl = $("detected");

fillLanguages(source, true);
fillLanguages(target, false);

let timer = null;
let requestId = 0;
let lastDetected = null;

async function run() {
  const text = input.value;
  const id = ++requestId;
  if (!text.trim()) {
    output.textContent = "";
    statusEl.textContent = "";
    detectedEl.textContent = "";
    return;
  }
  statusEl.textContent = "Translating…";
  statusEl.className = "info muted";
  try {
    const res = await invoke("translate", { text, source: source.value, target: target.value });
    if (id !== requestId) return; // a newer request superseded this one
    output.textContent = res.text;
    statusEl.textContent = "";
    lastDetected = res.detectedSource;
    detectedEl.textContent =
      source.value === "auto" && res.detectedSource ? `Detected: ${languageName(res.detectedSource)}` : "";
  } catch (e) {
    if (id !== requestId) return;
    statusEl.textContent = String(e);
    statusEl.className = "info error";
  }
}

function schedule(delay = 400) {
  clearTimeout(timer);
  timer = setTimeout(run, delay);
}

input.addEventListener("input", () => schedule());
source.addEventListener("change", () => schedule(0));
target.addEventListener("change", () => schedule(0));

$("swap").addEventListener("click", () => {
  const from = source.value === "auto" ? lastDetected || "en" : source.value;
  if (![...target.options].some((o) => o.value === from)) return;
  source.value = target.value;
  target.value = from;
  if (output.textContent) input.value = output.textContent;
  schedule(0);
});

$("clear").addEventListener("click", () => {
  input.value = "";
  input.focus();
  schedule(0);
});

$("copy").addEventListener("click", async () => {
  if (!output.textContent) return;
  await navigator.clipboard.writeText(output.textContent);
  statusEl.textContent = "Copied";
  statusEl.className = "info ok";
});

$("settings").addEventListener("click", () => invoke("open_settings"));

function applyConfig(cfg) {
  applyTheme(cfg.theme);
  source.value = cfg.sourceLang;
  target.value = cfg.targetLang;
}

invoke("get_config").then(applyConfig);
listen("config:changed", (e) => { applyConfig(e.payload); schedule(0); });
listen("main:set-text", (e) => {
  input.value = e.payload;
  input.focus();
  schedule(0);
});
