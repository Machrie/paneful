// Injected into every x.com panel (main frame only) before x.com's own
// scripts. Two jobs, nothing else:
// 1. tell the app when this panel gets keyboard focus (active-panel marker)
// 2. catch the app's own shortcuts before x.com sees them
// The panel capability only allows `panel_focused`, `shortcut`,
// `panel_keys` (read the configured bindings) and, for capture.js,
// `capture_posts`.
// Keep the key mapping in sync with ui/app.js.
(() => {
  if (window.top !== window) return;
  const mac = /Mac/.test(navigator.platform || navigator.userAgent);
  const call = (cmd, args) => {
    const t = window.__TAURI_INTERNALS__;
    return t ? t.invoke(cmd, args || {}) : Promise.reject();
  };

  // Configurable shortcuts: { action: {code, ctrl, alt, shift, meta} }.
  // Filled in by Rust when the panel is created; refreshed on every page
  // load (the panel may be older than the last change) and on change.
  let keys = /*KEYS*/null || {};
  call("panel_keys").then((k) => (keys = k || keys), () => {});
  window.addEventListener("paneful-keys", (e) => {
    if (e.detail && typeof e.detail === "object") keys = e.detail;
  });
  const hit = (b, e) =>
    b && e.code === b.code && e.ctrlKey === b.ctrl && e.altKey === b.alt &&
    e.shiftKey === b.shift && e.metaKey === b.meta;

  // e.code, not e.key: stays the same under the Korean IME.
  const shortcutOf = (e) => {
    for (const action in keys) {
      if (hit(keys[action], e)) return { action };
    }
    // Panel focus is fixed: Ctrl+1..9 (Windows) / Cmd+1..9 (macOS).
    const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
    if (mod && !e.shiftKey && !e.altKey && /^Digit[1-9]$/.test(e.code)) {
      return { action: "focus", index: Number(e.code.slice(5)) - 1 };
    }
    return null;
  };

  window.addEventListener(
    "keydown",
    (e) => {
      const a = shortcutOf(e);
      if (!a) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      // Holding a toggle key would flip it on and off; only the first press counts.
      if (!e.repeat || !["bar", "help", "settings", "capture"].includes(a.action)) call("shortcut", a).catch(() => {});
    },
    true,
  );
  window.addEventListener("focus", () => call("panel_focused").catch(() => {}));
  const ready = () => {
    if (document.hasFocus()) call("panel_focused").catch(() => {});
  };
  document.addEventListener("DOMContentLoaded", ready);
})();
