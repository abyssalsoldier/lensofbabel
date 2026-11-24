const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const box = document.getElementById("box");
let start = null;

function reset() {
  start = null;
  document.body.classList.remove("dragging");
}

function rect(e) {
  const x = Math.min(start.x, e.clientX), y = Math.min(start.y, e.clientY);
  return { x, y, w: Math.abs(e.clientX - start.x), h: Math.abs(e.clientY - start.y) };
}

document.addEventListener("pointerdown", (e) => {
  if (e.button === 2) { reset(); invoke("snip_cancel"); return; }
  if (e.button !== 0) return;
  document.body.setPointerCapture(e.pointerId);
  start = { x: e.clientX, y: e.clientY };
  Object.assign(box.style, { left: `${start.x}px`, top: `${start.y}px`, width: "0px", height: "0px" });
  document.body.classList.add("dragging");
});

document.addEventListener("pointermove", (e) => {
  if (!start) return;
  const r = rect(e);
  Object.assign(box.style, { left: `${r.x}px`, top: `${r.y}px`, width: `${r.w}px`, height: `${r.h}px` });
});

document.addEventListener("pointerup", (e) => {
  if (!start) return;
  const r = rect(e);
  reset();
  if (r.w < 4 || r.h < 4) return; // a click, not a drag: keep the overlay open
  // Convert CSS pixels to physical pixels; Rust adds the monitor's origin.
  const s = window.devicePixelRatio || 1;
  invoke("snip_done", { x: r.x * s, y: r.y * s, width: r.w * s, height: r.h * s });
});

document.addEventListener("contextmenu", (e) => e.preventDefault());
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") { reset(); invoke("snip_cancel"); }
});

listen("snip:start", reset);
