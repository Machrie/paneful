// Injected into every x.com panel (main frame only) before x.com's own
// scripts. Six jobs, nothing else:
// 1. tell the app when this panel gets keyboard focus (active-panel marker)
// 2. catch the app's own shortcuts before x.com sees them (and Ctrl/Cmd +
//    wheel for zoom)
// 3. Ctrl/Cmd+V outside a text field: ask Rust for the X link on the
//    clipboard (Rust reads the clipboard, not the page), ask the user, then
//    open it in this panel
// 4. "다른 패널에서 열기": send the post under the pointer (or the one X's
//    j/k selected, or this page) to another panel, asking which one when
//    there are several
// 5. window hidden to the tray: pause every video/audio, and keep new ones
//    from starting until it is back (sound must not leak while hidden)
// 6. read the unread counts X already shows (badges on the 알림 / 쪽지
//    links, or "(3)" in the title) and tell the app when they change.
//    Nothing is requested from X for this.
// The panel capability only allows `panel_focused`, `panel_unread`, `shortcut`,
// `panel_keys` (read the configured bindings), `paste_link`, `open_link`,
// `send_link` and, for capture.js,
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

  const capturing = () => !!window[Symbol.for("paneful.capturing")];
  // Ctrl/Cmd + wheel (and a trackpad pinch, which arrives as Ctrl + wheel):
  // the same zoom steps as the keys. One step per notch, at most one per
  // 80 ms, so a fast trackpad doesn't run through every level at once.
  let wheelSum = 0;
  let wheelAt = 0;
  window.addEventListener(
    "wheel",
    (e) => {
      if (!(e.ctrlKey || (mac && e.metaKey))) return;
      e.preventDefault();
      if (capturing()) return;
      wheelSum += e.deltaMode === 0 ? e.deltaY : e.deltaY * 40;
      const now = Date.now();
      if (Math.abs(wheelSum) < 40 || now - wheelAt < 80) return;
      call("shortcut", { action: wheelSum < 0 ? "zoom_in" : "zoom_out" }).catch(() => {});
      wheelSum = 0;
      wheelAt = now;
    },
    { capture: true, passive: false },
  );

  window.addEventListener(
    "keydown",
    (e) => {
      const a = shortcutOf(e);
      if (!a) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      // Zoom while picking posts for a capture would change the crop sizes.
      if (a.action.startsWith("zoom_") && capturing()) return;
      // Holding a toggle key would flip it on and off; only the first press counts.
      if (e.repeat && ["bar", "help", "settings", "capture", "zoom", "send"].includes(a.action)) return;
      // This page knows which post is meant; Rust only opens it.
      if (a.action === "send") send();
      else call("shortcut", a).catch(() => {});
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
    // "다른 패널에서 열기" with several panels: one button per panel.
    const pick = document.createElement("div");
    pick.style.cssText = "display:flex;gap:8px;flex-wrap:wrap;justify-content:flex-end;";
    box.append(text, addr, row, pick);
    host.attachShadow({ mode: "closed" }).append(box);
    ui = { host, text, addr, row, pick, btn };
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
    choosing = null;
    clearTimeout(msgTimer);
    if (ui) ui.host.remove();
  };
  const say = (msg, ms) => {
    ensureUi();
    pending = null;
    ui.text.textContent = msg;
    ui.addr.style.display = "none";
    ui.row.style.display = "none";
    ui.pick.style.display = "none";
    choosing = null;
    place(false);
    show();
    clearTimeout(msgTimer);
    msgTimer = setTimeout(hide, ms || 3000);
  };
  // The app changed this panel's zoom: show the new value for a moment
  // (not over an open "go there?" question).
  window.addEventListener("paneful-zoom", (e) => {
    const p = Number(e.detail);
    if (!pending && p > 0) say(`배율 ${p}%`, 1200);
  });
  const ask = (url) => {
    ensureUi();
    pending = url;
    clearTimeout(msgTimer);
    ui.text.textContent = "복사한 링크로 이동할까?";
    ui.addr.textContent = url.replace(/^https:\/\//, "");
    ui.addr.title = url;
    ui.addr.style.display = "block";
    ui.row.style.display = "flex";
    ui.pick.style.display = "none";
    choosing = null;
    place(true);
    show();
  };
  // ---- open in another panel
  // The post meant: the one under the pointer, else the one X's j/k
  // selected (it has focus), else this page. Post = <article>, its address
  // = the link around its time stamp (same rule as media.js).
  let hovered = null;
  document.addEventListener(
    "mouseover",
    (e) => {
      const t = e.target;
      if (t instanceof Element && !(ui && t === ui.host)) hovered = t.closest("article");
    },
    true,
  );
  // Pointer left the panel (no element it went to).
  document.addEventListener("mouseout", (e) => {
    if (!e.relatedTarget) hovered = null;
  });
  const postUrl = (art) => {
    if (!art || !art.isConnected) return null;
    const t = art.querySelector('a[href*="/status/"] time');
    const a = t && t.closest("a");
    return a && a.href ? a.href : null;
  };
  const target = () => {
    const sel = document.activeElement && document.activeElement.closest ? document.activeElement.closest("article") : null;
    return postUrl(hovered) || postUrl(sel) || location.href;
  };
  let choosing = null; // { url, targets } while asking which panel
  const sendTo = (url, to) =>
    call("send_link", { url, to: to || null }).then((r) => {
      if (r && r.to) say(`${r.to}에서 엶`);
      else if (r && r.targets && r.targets.length) choose(url, r.targets);
      else if (r && r.msg) say(r.msg);
    }, (e) => say(String(e || "열지 못함")));
  const send = () => {
    if (window[Symbol.for("paneful.capturing")]) return;
    sendTo(target());
  };
  window.addEventListener("paneful-send", send);
  const choose = (url, targets) => {
    ensureUi();
    pending = null;
    clearTimeout(msgTimer);
    ui.text.textContent = "어느 패널에서 열까? (숫자 키로 골라도 됨)";
    ui.addr.textContent = url.replace(/^https:\/\//, "");
    ui.addr.title = url;
    ui.addr.style.display = "block";
    ui.row.style.display = "none";
    ui.pick.replaceChildren();
    for (const t of targets) {
      const b = ui.btn(`${t.index + 1} · ${t.label}`, false);
      b.style.borderColor = t.color;
      b.onclick = () => {
        hide();
        sendTo(url, t.id);
      };
      ui.pick.append(b);
    }
    const no = ui.btn("취소 (Esc)", false);
    no.onclick = hide;
    ui.pick.append(no);
    ui.pick.style.display = "flex";
    choosing = { url, targets };
    place(true);
    show();
  };
  // While choosing: 1-9 (the panel's place, like Ctrl+1..9) or Esc.
  window.addEventListener(
    "keydown",
    (e) => {
      if (!choosing || e.ctrlKey || e.metaKey || e.altKey) return;
      const m = /^(?:Digit|Numpad)([1-9])$/.exec(e.code);
      if (!m && e.code !== "Escape") return;
      e.preventDefault();
      e.stopImmediatePropagation();
      if (e.repeat) return;
      const c = choosing;
      if (!m) return hide();
      const t = c.targets.find((x) => x.index === Number(m[1]) - 1);
      if (!t) return;
      hide();
      sendTo(c.url, t.id);
    },
    true,
  );
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
      if ((pending || choosing) && ui && !e.composedPath().includes(ui.host)) hide();
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

  // ---- unread counts
  // Badge = a leaf element on the nav link whose text is just a number
  // ("3", "20+"). The nav link is the left sidebar on wide panels and the
  // bottom bar on narrow ones. No link (e.g. a page without the nav): the
  // "(3)" X puts in front of the title, for 알림 only.
  const BADGE = /^\d{1,4}\+?$/;
  const badgeIn = (a) => {
    if (!a) return "";
    for (const el of a.querySelectorAll("div, span")) {
      if (el.childElementCount) continue;
      const t = (el.textContent || "").trim();
      if (BADGE.test(t)) return t;
    }
    return "";
  };
  const readUnread = () => {
    const nl = document.querySelector('a[data-testid="AppTabBar_Notifications_Link"], a[href="/notifications"][role="link"]');
    const ml = document.querySelector(
      'a[data-testid="AppTabBar_DirectMessage_Link"], a[href="/messages"][role="link"], a[href^="/i/chat"][role="link"]',
    );
    let n = badgeIn(nl);
    if (!nl) {
      const m = /^\((\d{1,4}\+?)\)\s/.exec(document.title || "");
      n = m ? m[1] : "";
    }
    return { n, dm: badgeIn(ml) };
  };
  let sent = "";
  const checkUnread = () => {
    const u = readUnread();
    const key = u.n + "|" + u.dm;
    if (key === sent) return;
    sent = key;
    call("panel_unread", u).catch(() => (sent = "")); // retried on the next tick
  };
  // Cheap (two selectors) and only sends on change.
  // Driven by DOM changes, not only a timer: in a hidden page (window in
  // the tray, panels hidden) Chromium slows repeating timers down to once a
  // minute after a few minutes, while X still gets its updates over the
  // network and still renders the badge, and MutationObserver callbacks
  // still run. One check per burst of changes; a one-shot timeout started
  // from the observer isn't a chained timer, so it isn't held back that far.
  let queued = false;
  const soon = () => {
    if (queued) return;
    queued = true;
    setTimeout(() => {
      queued = false;
      checkUnread();
    }, 500);
  };
  const watch = () => {
    new MutationObserver(soon).observe(document.documentElement, {
      subtree: true,
      childList: true,
      characterData: true,
    });
  };
  if (document.documentElement) watch();
  else document.addEventListener("DOMContentLoaded", watch);
  setInterval(checkUnread, 5000); // fallback, e.g. a badge the observer missed
  document.addEventListener("DOMContentLoaded", checkUnread);
  // ---- window hidden to the tray: no sound
  // Rust sends this before the window hides and after it is back. Paused
  // media stay paused when it is back (X's play button resumes them).
  let quiet = false;
  const hush = () => {
    for (const m of document.querySelectorAll("video, audio")) if (!m.paused) m.pause();
  };
  window.addEventListener("paneful-hidden", (e) => {
    quiet = e.detail === true;
    if (quiet) hush();
  });
  // X starts videos on its own (autoplay on scroll, next video): stop
  // them while hidden. "play" doesn't bubble, so listen while capturing.
  document.addEventListener(
    "play",
    (e) => {
      if (quiet && e.target instanceof HTMLMediaElement) e.target.pause();
    },
    true,
  );

  window.addEventListener("focus", () => call("panel_focused").catch(() => {}));
  const ready = () => {
    if (document.hasFocus()) call("panel_focused").catch(() => {});
  };
  document.addEventListener("DOMContentLoaded", ready);
})();
