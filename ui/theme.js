// Applies the Windows accent colour and updates it live when it changes.
(function () {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;

  function apply(a) {
    if (!a) return;
    const s = document.documentElement.style;
    s.setProperty("--win-accent", a.accent);
    s.setProperty("--win-accent-light", a.accentLight);
    s.setProperty("--win-accent-dark", a.accentDark);
  }

  invoke("get_accent").then(apply).catch(() => {});
  listen("theme:accent", (e) => apply(e.payload));
})();
