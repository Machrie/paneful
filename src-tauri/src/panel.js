// Injected into every x.com panel (main frame only) before x.com's own
// scripts. Three jobs, nothing else:
// 1. tell the app when this panel gets keyboard focus (active-panel marker)
// 2. catch the app's own shortcuts before x.com sees them
// 3. Ctrl/Cmd+V outside a text field: ask the app to open the X link on
//    the clipboard in this panel (Rust reads the clipboard, not the page)
// The panel capability only allows `panel_focused`, `shortcut`,
// `panel_keys` (read the configured bindings), `paste_link` and, for capture.js,
// `capture_begin` / `capture_shot` / `capture_drop` / `capture_finish`.
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
  // ---- paste a link to open it
  // Only when the key goes to no text field: pasting into a post, the
  // search box or a DM stays X's. The key itself is left alone (no
  // preventDefault), so X's own paste handling is unchanged.
  const NOT_TEXT = /^(checkbox|radio|button|submit|reset|range|color|file|image)$/i;
  const editable = (el) => {
    while (el && el.shadowRoot && el.shadowRoot.activeElement) el = el.shadowRoot.activeElement;
    if (!(el instanceof Element)) return false;
    if (el.isContentEditable) return true;
    if (el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) return true;
    if (el instanceof HTMLInputElement) return !NOT_TEXT.test(el.type || "");
    return false;
  };
  let toast = null;
  let toastTimer = 0;
  const say = (text) => {
    if (!toast) {
      const host = document.createElement("div");
      host.style.cssText = "all:initial;position:fixed;inset:auto 0 24px 0;z-index:2147483647;pointer-events:none;display:flex;justify-content:center;";
      toast = document.createElement("div");
      toast.style.cssText =
        "max-width:90vw;font:600 13px/1.4 system-ui,-apple-system,'Segoe UI','Malgun Gothic',sans-serif;color:#fff;" +
        "background:rgba(32,35,39,.95);border-radius:999px;padding:8px 16px;box-shadow:0 2px 12px rgba(0,0,0,.4);";
      host.attachShadow({ mode: "closed" }).append(toast);
      toast.host_ = host;
    }
    toast.textContent = text;
    if (!toast.host_.isConnected) document.documentElement.appendChild(toast.host_);
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => toast.host_.remove(), 3000);
  };
  window.addEventListener(
    "keydown",
    (e) => {
      if (e.code !== "KeyV" || e.repeat || e.shiftKey || e.altKey) return;
      if (!(mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey)) return;
      if (editable(e.target) || editable(document.activeElement)) return;
      // Picking posts for a capture: a page change would drop the picks.
      if (window[Symbol.for("paneful.capturing")]) return;
      call("paste_link").then((msg) => msg && say(msg), () => {});
    },
    true,
  );

  window.addEventListener("focus", () => call("panel_focused").catch(() => {}));
  const ready = () => {
    if (document.hasFocus()) call("panel_focused").catch(() => {});
  };
  document.addEventListener("DOMContentLoaded", ready);
})();
