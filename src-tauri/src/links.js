// Injected into every x.com panel (main frame only). Left click on a link
// to another site (X wraps those in t.co): open it in the system browser.
//
// Right click > "open in new window" already reaches the system browser via
// on_new_window, but a plain left click did nothing on Windows (v0.0.4).
// 추론: x.com handles the click itself in a way the panel's new-window
// handler cannot turn into the real address. So the click is taken here,
// before x.com sees it, and sent as a window.open with the real address,
// the same route the right click takes. lib.rs logs what arrives.
(() => {
  if (window.top !== window) return;
  // Keep in sync with nav.rs ALLOWED: these stay inside the panel.
  const INSIDE = ["x.com", "twitter.com", "twimg.com", "arkoselabs.com", "google.com", "gstatic.com", "apple.com"];
  const inside = (h) => INSIDE.some((d) => h === d || h.endsWith("." + d));

  window.addEventListener(
    "click",
    (e) => {
      if (!e.isTrusted || e.button !== 0 || e.defaultPrevented) return;
      const a = e.target instanceof Element ? e.target.closest("a") : null;
      if (!a || typeof a.href !== "string" || !a.href) return;
      let u;
      try {
        u = new URL(a.href, location.href);
      } catch {
        return;
      }
      if (u.protocol !== "https:" && u.protocol !== "http:") return;
      if (inside(u.hostname.replace(/\.$/, "").toLowerCase())) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      window.open(u.href, "_blank");
    },
    true,
  );
})();
