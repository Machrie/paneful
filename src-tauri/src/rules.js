// Injected into every x.com panel (main frame only), before media.js and
// capture.js. Holds the X screen rules (rules.rs, rules/x.v1.json): the
// CSS selectors and URL fragments those scripts use to find posts, names,
// avatars, photos and videos. The rules are data only: they are passed to
// querySelector / closest / matches and String.includes, nothing else.
//
// window[Symbol.for("paneful.rules")] = { sel(name), text(name), revision }
// - /*BUILTIN*/ is the copy built into the app, /*RULES*/ the rules in
//   effect when the panel was created (maybe a newer download).
// - On every page load and on the `paneful-rules` event, the current
//   rules are asked from Rust (the event carries nothing).
// - A selector the browser can't parse falls back to the built-in one.
(() => {
  if (window.top !== window) return;
  const KEY = Symbol.for("paneful.rules");
  if (Object.prototype.hasOwnProperty.call(window, KEY)) return;
  const BUILTIN = /*BUILTIN*/null;
  const frag = document.createDocumentFragment();
  const valid = (s) => {
    if (typeof s !== "string" || !s) return false;
    try {
      frag.querySelector(s);
      return true;
    } catch (_) {
      return false;
    }
  };
  let cur = { revision: 0, selectors: {}, text: {} };
  const take = (r) => {
    if (!r || typeof r !== "object") return;
    const sel = {};
    const text = {};
    for (const k in BUILTIN.selectors) {
      const v = r.selectors && r.selectors[k];
      sel[k] = valid(v) ? v : BUILTIN.selectors[k];
    }
    for (const k in BUILTIN.text) {
      const v = r.text && r.text[k];
      text[k] = typeof v === "string" && v ? v : BUILTIN.text[k];
    }
    cur = { revision: Number(r.revision) || 0, selectors: sel, text };
  };
  take(BUILTIN);
  take(/*RULES*/null);

  const api = Object.freeze({
    sel: (k) => cur.selectors[k],
    text: (k) => cur.text[k],
    get revision() {
      return cur.revision;
    },
  });
  Object.defineProperty(window, KEY, { value: api });

  const refresh = () => {
    const t = window.__TAURI_INTERNALS__;
    if (t) t.invoke("panel_rules", {}).then(take, () => {});
  };
  refresh();
  window.addEventListener("paneful-rules", refresh);
})();
