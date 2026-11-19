// Injected into every x.com panel (main frame only) before x.com's own
// scripts. Three jobs, nothing else:
// 1. tell the app when this panel gets keyboard focus (active-panel marker)
// 2. catch the app's own shortcuts before x.com sees them
// 3. Ctrl/Cmd+V outside a text field: ask Rust for the X link on the
//    clipboard (Rust reads the clipboard, not the page), ask the user, then
//    open it in this panel
// The panel capability only allows `panel_focused`, `shortcut`,
// `panel_keys` (read the configured bindings), `paste_link`, `open_link` and, for capture.js,
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
  // One small box in the panel, in a closed shadow root so
  // X's styles and scripts don't reach it. Either a short message or the
  // question "open this address?" (Enter / Esc, or the buttons). No
  // confirm(): wry's macOS UI delegate always answers false.
  let ui = null;
  let pending = null; // address waiting for a yes
  let msgTimer = 0;
  const FONT = "font:600 13px/1.4 system-ui,-apple-system,'Segoe UI','Malgun Gothic',sans-serif;";
  const ensureUi = () => {
    if (ui) return ui;
    const host = document.createElement("div");
    host.style.cssText =
      "all:initial;position:fixed;z-index:2147483647;pointer-events:none;display:flex;justify-content:center;";
    const box = document.createElement("div");
    box.style.cssText =
      FONT + "max-width:min(560px,92vw);box-sizing:border-box;color:#fff;background:rgba(32,35,39,.97);" +
      "border:1px solid rgba(255,255,255,.12);border-radius:14px;padding:10px 14px;box-shadow:0 4px 16px rgba(0,0,0,.45);" +
      "pointer-events:auto;display:flex;flex-direction:column;gap:8px;";
    const text = document.createElement("div");
    const addr = document.createElement("div");
    addr.style.cssText =
      "font-weight:400;color:#8b98a5;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;direction:ltr;";
    const row = document.createElement("div");
    row.style.cssText = "display:flex;gap:8px;justify-content:flex-end;";
    const btn = (label, primary) => {
      const b = document.createElement("button");
      b.type = "button";
      b.textContent = label;
      b.style.cssText =
        FONT + "cursor:pointer;border-radius:999px;padding:5px 14px;" +
        (primary ? "background:#1d9bf0;color:#fff;border:1px solid #1d9bf0;" : "background:transparent;color:#eff3f4;border:1px solid #536471;");
      return b;
    };
    const go = btn("이동 (Enter)", true);
    const no = btn("취소 (Esc)", false);
    go.onclick = () => answer(true);
    no.onclick = () => answer(false);
    row.append(no, go);
    box.append(text, addr, row);
    host.attachShadow({ mode: "closed" }).append(box);
    ui = { host, text, addr, row };
    return ui;
  };
  // Messages sit at the bottom; the question sits in the middle of the
  // panel, where the eye already is.
  const place = (center) => {
    Object.assign(ui.host.style, center
      ? { inset: "0", alignItems: "center", padding: "0" }
      : { inset: "auto 0 24px 0", alignItems: "flex-end", padding: "0" });
  };
  const show = () => {
    if (!ui.host.isConnected) document.documentElement.appendChild(ui.host);
  };
  const hide = () => {
    pending = null;
    clearTimeout(msgTimer);
    if (ui) ui.host.remove();
  };
  const say = (msg) => {
    ensureUi();
    pending = null;
    ui.text.textContent = msg;
    ui.addr.style.display = "none";
    ui.row.style.display = "none";
    place(false);
    show();
    clearTimeout(msgTimer);
    msgTimer = setTimeout(hide, 3000);
  };
  const ask = (url) => {
    ensureUi();
    pending = url;
    clearTimeout(msgTimer);
    ui.text.textContent = "복사한 링크로 이동할까?";
    ui.addr.textContent = url.replace(/^https:\/\//, "");
    ui.addr.title = url;
    ui.addr.style.display = "block";
    ui.row.style.display = "flex";
    place(true);
    show();
  };
  function answer(yes) {
    const url = pending;
    hide();
    if (yes && url) call("open_link", { url }).catch(() => {});
  }
  // While asking: Enter = go, Esc = cancel, and X doesn't see either key.
  // Other keys pass through and leave the question up.
  window.addEventListener(
    "keydown",
    (e) => {
      if (!pending) return;
      const yes = e.code === "Enter" || e.code === "NumpadEnter";
      if (!yes && e.code !== "Escape") return;
      e.preventDefault();
      e.stopImmediatePropagation();
      if (!e.repeat) answer(yes);
    },
    true,
  );
  // A click anywhere else in the page cancels.
  window.addEventListener(
    "pointerdown",
    (e) => {
      if (pending && ui && !e.composedPath().includes(ui.host)) hide();
    },
    true,
  );
  window.addEventListener(
    "keydown",
    (e) => {
      if (e.code !== "KeyV" || e.repeat || e.shiftKey || e.altKey) return;
      if (!(mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey)) return;
      if (editable(e.target) || editable(document.activeElement)) return;
      // Picking posts for a capture: a page change would drop the picks.
      if (window[Symbol.for("paneful.capturing")]) return;
      call("paste_link").then((r) => {
        if (r && r.url) ask(r.url);
        else if (r && r.msg) say(r.msg);
      }, () => {});
    },
    true,
  );

  window.addEventListener("focus", () => call("panel_focused").catch(() => {}));
  const ready = () => {
    if (document.hasFocus()) call("panel_focused").catch(() => {});
  };
  document.addEventListener("DOMContentLoaded", ready);
})();
