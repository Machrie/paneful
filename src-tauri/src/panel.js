// Injected into every x.com panel (main frame only) before x.com's own
// scripts. Two jobs, nothing else:
// 1. tell the app when this panel gets keyboard focus (active-panel marker)
// 2. catch the app's own shortcuts before x.com sees them
// The panel capability only allows `panel_focused` and `shortcut`.
// Ctrl/Cmd+/ is free on x.com (its own help is a bare `?`).
// Keep the key mapping in sync with ui/app.js.
(() => {
  if (window.top !== window) return;
  const mac = /Mac/.test(navigator.platform || navigator.userAgent);
  const call = (cmd, args) => {
    const t = window.__TAURI_INTERNALS__;
    if (t) t.invoke(cmd, args || {}).catch(() => {});
  };

  // e.code, not e.key: stays the same under the Korean IME.
  const shortcutOf = (e) => {
    const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
    if (e.altKey) return null;
    if (mod && !e.shiftKey && /^Digit[1-9]$/.test(e.code)) {
      return { action: "focus", index: Number(e.code.slice(5)) - 1 };
    }
    if (e.ctrlKey && !e.metaKey && e.code === "Tab") {
      return { action: e.shiftKey ? "prev" : "next" };
    }
    if (mod && e.shiftKey && e.code === "KeyH") return { action: "bar" };
    if (mod && !e.shiftKey && e.code === "Slash") return { action: "help" };
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
      if (!e.repeat || !["bar", "help"].includes(a.action)) call("shortcut", a);
    },
    true,
  );
  window.addEventListener("focus", () => call("panel_focused"));
  const ready = () => {
    if (document.hasFocus()) call("panel_focused");
  };
  document.addEventListener("DOMContentLoaded", ready);
})();
