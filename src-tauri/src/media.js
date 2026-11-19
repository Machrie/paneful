// Paneful: "다운" button (saves the original file) on post images and videos (x.com panels only).
//
// - Images: the <img> src on pbs.twimg.com/media, rewritten to name=orig in Rust.
// - Videos: the page plays them through blob: streams, so the mp4 URL is read
//   from API responses the page has already received (no extra requests).
//   The highest-bitrate video/mp4 variant per media id is kept.
// Saving happens only on a trusted click; Rust fetches the file without
// cookies. Panels have no IPC: the click opens a trigger URL that the
// panel's new-window handler intercepts.
(() => {
  if (window.top !== window || window.__panefulMedia) return;
  Object.defineProperty(window, "__panefulMedia", { value: true });

  const TRIGGER = "https://paneful.invalid/save-media";
  const IMG_RE = /^https:\/\/pbs\.twimg\.com\/media\//;
  const POSTER_RE = /\/(?:ext_tw_video_thumb|amplify_video_thumb)\/(\d+)\//;
  const GIF_POSTER_RE = /\/tweet_video_thumb\/([A-Za-z0-9_-]+)\./;
  const STATUS_RE = /^\/([A-Za-z0-9_]{1,15})\/status\/(\d+)(?:\/(?:photo|video)\/(\d))?/;

  // ------------------------------------------------ video variants
  const videos = new Map(); // media id or gif key -> best mp4 url
  const MAX_VIDEOS = 2000;

  function remember(media) {
    const vi = media && media.video_info;
    if (!vi || !Array.isArray(vi.variants)) return;
    let best = null;
    for (const v of vi.variants) {
      if (!v || v.content_type !== "video/mp4" || typeof v.url !== "string") continue;
      if (!best || (v.bitrate || 0) > (best.bitrate || 0)) best = v;
    }
    if (!best) return;
    const id = typeof media.id_str === "string" ? media.id_str : null;
    if (id) videos.set(id, best.url);
    const gif = /\/tweet_video\/([A-Za-z0-9_-]+)\.mp4/.exec(best.url);
    if (gif) videos.set(gif[1], best.url);
    if (videos.size > MAX_VIDEOS) videos.delete(videos.keys().next().value);
  }

  function walk(o, depth) {
    if (!o || typeof o !== "object" || depth > 60) return;
    if (Array.isArray(o)) {
      for (const x of o) walk(x, depth + 1);
      return;
    }
    if (o.video_info) remember(o);
    for (const k in o) {
      const v = o[k];
      if (v && typeof v === "object") walk(v, depth + 1);
    }
  }

  function isApi(url) {
    return typeof url === "string" && /^https:\/\/(?:x|twitter)\.com\/i\/api\//.test(url);
  }

  function absorb(text) {
    if (typeof text !== "string" || text.indexOf("video_info") < 0) return;
    try {
      walk(JSON.parse(text), 0);
    } catch (_) {}
  }

  // Read-only taps: responses are passed through untouched.
  const XHR = XMLHttpRequest.prototype;
  const origOpen = XHR.open;
  XHR.open = function (method, url) {
    try {
      const u = new URL(url, location.href).href;
      if (isApi(u)) {
        this.addEventListener("load", function () {
          try {
            if (this.responseType === "" || this.responseType === "text") absorb(this.responseText);
            else if (this.responseType === "json") walk(this.response, 0);
          } catch (_) {}
        });
      }
    } catch (_) {}
    return origOpen.apply(this, arguments);
  };
  const origFetch = window.fetch;
  window.fetch = function (input) {
    const p = origFetch.apply(this, arguments);
    try {
      const u = typeof input === "string" ? input : input && input.url;
      if (isApi(new URL(u, location.href).href)) {
        p.then((r) => r.clone().text().then(absorb)).catch(() => {});
      }
    } catch (_) {}
    return p;
  };

  // ------------------------------------------------ media under the pointer
  // Only media uploaded to a post: the tweetPhoto box in timelines/threads,
  // or the photo under the pointer in the photo viewer. Avatars, emoji, link
  // cards and the rest of the post never match.
  function imageOf(el, x, y) {
    const box = el.closest('[data-testid="tweetPhoto"]');
    if (box) {
      const img = box.querySelector("img");
      return img && IMG_RE.test(img.src) ? { box, src: img.src } : null;
    }
    const modal = el.closest('[aria-modal="true"]');
    if (!modal || x == null) return null;
    for (const img of modal.querySelectorAll("img")) {
      if (!IMG_RE.test(img.src)) continue;
      const r = img.getBoundingClientRect();
      if (r.width >= 48 && r.height >= 48 && x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) {
        return { box: img, src: img.src };
      }
    }
    return null;
  }

  function videoOf(el) {
    const box = el.closest('[data-testid="videoPlayer"], [data-testid="videoComponent"]');
    if (!box) return null;
    const v = box.querySelector("video");
    if (!v) return null;
    if (/^https:\/\/video\.twimg\.com\/.+\.mp4/.test(v.src)) return { box, src: v.src };
    const poster = v.poster || "";
    const m = POSTER_RE.exec(poster) || GIF_POSTER_RE.exec(poster);
    return { box, src: (m && videos.get(m[1])) || null };
  }

  function statusOf(box) {
    const own = box.closest('a[href*="/status/"]');
    let m = own && STATUS_RE.exec(new URL(own.href).pathname);
    if (m) return m;
    if (box.closest('[aria-modal="true"]')) {
      m = STATUS_RE.exec(location.pathname);
      if (m) return m;
    }
    const art = box.closest("article");
    const t = art && art.querySelector('a[href*="/status/"] time');
    const a = t && t.closest("a");
    return a ? STATUS_RE.exec(new URL(a.href).pathname) : STATUS_RE.exec(location.pathname);
  }

  // ------------------------------------------------ button (own shadow root,
  // outside x.com's React tree; styles via CSSOM so the page CSP can't block)
  let host = null;
  let btn = null;
  let cur = null; // { box, kind }
  let hideTimer = 0;
  let resetTimer = 0;
  const pending = new Set();

  const LABEL = "⤓ 다운";
  const css = (el, s) => (el.style.cssText = s);

  function ensure() {
    if (host && host.isConnected) return;
    host = document.createElement("div");
    css(host, "position:fixed;top:0;left:0;z-index:2147483647;display:none;");
    const root = host.attachShadow({ mode: "closed" });
    btn = document.createElement("button");
    btn.type = "button";
    btn.textContent = LABEL;
    css(
      btn,
      "all:initial;cursor:pointer;font:600 12px/1 system-ui,sans-serif;color:#fff;" +
        "background:rgba(15,20,25,.78);border:1px solid rgba(255,255,255,.35);" +
        "border-radius:999px;padding:6px 10px;white-space:nowrap;"
    );
    for (const t of ["pointerdown", "mousedown", "mouseup", "pointerup"]) {
      btn.addEventListener(t, (e) => e.stopPropagation(), true);
    }
    btn.addEventListener("click", onClick, true);
    host.addEventListener("mouseenter", () => clearTimeout(hideTimer));
    host.addEventListener("mouseleave", scheduleHide);
    root.appendChild(btn);
    document.documentElement.appendChild(host);
  }

  function place() {
    if (!cur) return;
    const r = cur.box.getBoundingClientRect();
    if (r.width < 48 || r.height < 32) return hide();
    host.style.display = "block";
    const w = btn.offsetWidth || 70;
    host.style.left = Math.max(0, r.right - w - 8) + "px";
    host.style.top = Math.max(0, r.top + 8) + "px";
  }

  function hide() {
    clearTimeout(hideTimer);
    if (host) host.style.display = "none";
    cur = null;
  }

  function scheduleHide() {
    clearTimeout(hideTimer);
    hideTimer = setTimeout(hide, 250);
  }

  function setText(t, ms) {
    clearTimeout(resetTimer);
    btn.textContent = t;
    if (ms) resetTimer = setTimeout(() => (btn.textContent = LABEL), ms);
  }

  function onClick(e) {
    e.preventDefault();
    e.stopPropagation();
    if (!e.isTrusted || !cur) return;
    const found = cur.kind === "image" ? { src: cur.src } : videoOf(cur.box);
    if (!found || !found.src) {
      // variants not seen yet (e.g. post loaded before this script)
      setText("영상 주소 없음, 새로고침 후 다시", 2500);
      return;
    }
    const u = new URL(TRIGGER);
    u.searchParams.set("src", found.src);
    const m = statusOf(cur.box);
    if (m) {
      u.searchParams.set("user", m[1]);
      u.searchParams.set("status", m[2]);
      if (m[3] && cur.kind === "image") u.searchParams.set("n", m[3]);
    }
    pending.add(found.src);
    setText("저장 중…");
    window.open(u.href, "_blank");
  }

  window.addEventListener("paneful-media-saved", (e) => {
    const d = e.detail || {};
    if (!pending.delete(d.src) || !btn) return;
    if (d.ok) {
      setText("저장됨", 1500);
      btn.title = String(d.message || "");
    } else {
      setText("실패", 2500);
      btn.title = String(d.message || "");
    }
  });

  function onPointer(e) {
    const t = e.target;
    if (!(t instanceof Element) || t === host) return;
    const v = videoOf(t);
    const i = v ? null : imageOf(t, e.clientX, e.clientY);
    const hit = v ? { box: v.box, kind: "video" } : i ? { box: i.box, src: i.src, kind: "image" } : null;
    if (!hit) return scheduleHide();
    clearTimeout(hideTimer);
    ensure();
    if (!cur || cur.box !== hit.box) {
      cur = hit;
      if (!pending.size) setText(LABEL);
    }
    place();
  }
  document.addEventListener("mouseover", onPointer, true);
  // Photo viewer: one overlay covers the whole image area, so moving onto
  // the photo fires no mouseover. Re-check on mousemove, once per frame.
  let moveQueued = null;
  document.addEventListener(
    "mousemove",
    (e) => {
      if (!e.target.closest || !e.target.closest('[aria-modal="true"]')) return;
      if (moveQueued) return void (moveQueued = e);
      moveQueued = e;
      requestAnimationFrame(() => {
        const ev = moveQueued;
        moveQueued = null;
        onPointer(ev);
      });
    },
    true
  );
  window.addEventListener("scroll", () => cur && place(), true);
  window.addEventListener("resize", () => cur && place());
})();
