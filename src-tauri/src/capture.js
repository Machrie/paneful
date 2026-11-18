// Paneful: post capture (x.com panels only).
//
// Ctrl/Cmd+Shift+S (or the top bar button) -> click the first post -> click
// the last post. X's timeline keeps only the posts near the screen in the
// DOM, so after both ends are picked the script scrolls from the upper one
// to the lower one itself and reads each post as it passes. Only what the
// page already shows (or already received) is read; no requests are made.
// The posts go to Rust (`capture_posts`), and the host UI draws them as
// cards, anonymizes them and makes the PNG.
(() => {
  if (window.top !== window || window.__panefulCapture) return;
  Object.defineProperty(window, "__panefulCapture", { value: true });

  const mac = /Mac/.test(navigator.platform || navigator.userAgent);
  const MAX_POSTS = 150;
  const MAX_STEPS = 600;
  const STATUS_RE = /^\/([A-Za-z0-9_]{1,15})\/status\/(\d+)/;
  const HANDLE_RE = /^@([A-Za-z0-9_]{1,15})$/;
  const MEDIA_RE = /^https:\/\/pbs\.twimg\.com\/(media|ext_tw_video_thumb|amplify_video_thumb|tweet_video_thumb|card_img)\//;
  const AVATAR_RE = /^https:\/\/pbs\.twimg\.com\/profile_images\//;

  // ------------------------------------------------ full text of long posts
  // Timelines cut long posts ("Show more"). The full text is in API
  // responses the page already received: tap them read-only, like media.js.
  const notes = new Map(); // rest_id -> text with t.co links expanded
  const MAX_NOTES = 3000;
  function noteOf(o) {
    const r = o.note_tweet && o.note_tweet.note_tweet_results && o.note_tweet.note_tweet_results.result;
    if (!r || typeof r.text !== "string") return;
    let text = r.text;
    const urls = (r.entity_set && r.entity_set.urls) || [];
    for (const u of urls) {
      if (u && typeof u.url === "string" && typeof u.display_url === "string") {
        text = text.split(u.url).join(u.display_url);
      }
    }
    notes.set(o.rest_id, text);
    if (notes.size > MAX_NOTES) notes.delete(notes.keys().next().value);
  }
  function walkApi(o, depth) {
    if (!o || typeof o !== "object" || depth > 60) return;
    if (Array.isArray(o)) {
      for (const x of o) walkApi(x, depth + 1);
      return;
    }
    if (typeof o.rest_id === "string" && o.note_tweet) noteOf(o);
    for (const k in o) {
      const v = o[k];
      if (v && typeof v === "object") walkApi(v, depth + 1);
    }
  }
  const isApi = (u) => typeof u === "string" && /^https:\/\/(?:x|twitter)\.com\/i\/api\//.test(u);
  const absorb = (text) => {
    if (typeof text !== "string" || text.indexOf("note_tweet_results") < 0) return;
    try {
      walkApi(JSON.parse(text), 0);
    } catch (_) {}
  };
  const XHR = XMLHttpRequest.prototype;
  const origOpen = XHR.open;
  XHR.open = function (method, url) {
    try {
      if (isApi(new URL(url, location.href).href)) {
        this.addEventListener("load", function () {
          try {
            if (this.responseType === "" || this.responseType === "text") absorb(this.responseText);
            else if (this.responseType === "json") walkApi(this.response, 0);
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
      if (isApi(new URL(u, location.href).href)) p.then((r) => r.clone().text().then(absorb)).catch(() => {});
    } catch (_) {}
    return p;
  };

  // ------------------------------------------------ reading one post
  const isEmoji = (img) => /\/emoji\//.test(img.src || "");

  // Text with mentions kept apart, so the host UI can anonymize them.
  // -> [{t: "text"} | {t: "@name", m: "name"} | {t: "#tag", l: true}]
  function richText(root, inName) {
    const out = [];
    // m: mentioned handle, l: link / hashtag (drawn in blue)
    const push = (t, m, l) => {
      if (!t) return;
      const last = out[out.length - 1];
      if (!m && !l && last && !last.m && !last.l) last.t += t;
      else out.push(m ? { t, m } : l ? { t, l: true } : { t });
    };
    const visit = (n) => {
      if (n.nodeType === 3) return push(n.nodeValue);
      if (n.nodeType !== 1) return;
      const tag = n.tagName;
      if (tag === "BR") return push("\n");
      if (tag === "IMG") return isEmoji(n) ? push(n.alt || "") : undefined;
      if (tag === "SVG" || tag === "svg" || tag === "STYLE" || tag === "SCRIPT") return;
      if (tag === "A") {
        const vis = (n.innerText || n.textContent || "").trim();
        const m = HANDLE_RE.exec(vis);
        if (m) return push(vis, m[1]);
        // Emoji are <img>, which innerText drops: walk the children then.
        // Otherwise innerText, which skips the hidden parts of shortened URLs.
        if (![...n.querySelectorAll("img")].some(isEmoji)) return push(n.innerText || n.textContent || "", null, !inName);
        for (const c of n.childNodes) visit(c);
        return;
      }
      for (const c of n.childNodes) visit(c);
    };
    visit(root);
    return out;
  }
  const plain = (segs) => segs.map((s) => s.t).join("");

  // Mentions in plain text (API text has no links).
  function splitMentions(text) {
    const out = [];
    const re = /(^|[^A-Za-z0-9_@])@([A-Za-z0-9_]{1,15})(?![A-Za-z0-9_])/g;
    let last = 0;
    let m;
    while ((m = re.exec(text))) {
      const at = m.index + m[1].length;
      if (at > last) out.push({ t: text.slice(last, at) });
      out.push({ t: "@" + m[2], m: m[2] });
      last = at + 1 + m[2].length;
    }
    if (last < text.length) out.push({ t: text.slice(last) });
    return out;
  }

  function nameAndHandle(userName) {
    let handle = "";
    for (const s of userName.querySelectorAll("span")) {
      const m = HANDLE_RE.exec((s.textContent || "").trim());
      if (m) {
        handle = m[1];
        break;
      }
    }
    const first = userName.firstElementChild || userName;
    let name = plain(richText(first, true)).replace(/\s+/g, " ").trim();
    if (!name || name === "@" + handle) name = handle;
    return { name: name.slice(0, 100), handle };
  }

  function mediaSrc(src) {
    // Displayed size is often "small"/"360x360": ask for one that is sharp
    // at 2x in a 600px card, but not the original.
    try {
      const u = new URL(src);
      if (u.searchParams.has("name")) u.searchParams.set("name", "medium");
      return u.href;
    } catch (_) {
      return src;
    }
  }

  function mediaIn(root, skip) {
    const out = [];
    const seen = new Set();
    for (const el of root.querySelectorAll("img, video")) {
      if (skip && skip(el)) continue;
      if (el.closest('[data-testid="card.wrapper"]')) continue;
      let src = "";
      let video = false;
      if (el.tagName === "VIDEO") {
        src = el.poster || "";
        video = true;
      } else {
        src = el.src || "";
        video = /_thumb\//.test(src) || !!el.closest('[data-testid="videoPlayer"], [data-testid="videoComponent"]');
      }
      if (!MEDIA_RE.test(src) || /\/card_img\//.test(src)) continue;
      src = mediaSrc(src);
      if (seen.has(src)) continue;
      seen.add(src);
      const box = el.closest('[data-testid="tweetPhoto"], [data-testid="videoPlayer"], [data-testid="videoComponent"]') || el;
      const r = box.getBoundingClientRect();
      let w = el.naturalWidth || el.videoWidth || r.width;
      let h = el.naturalHeight || el.videoHeight || r.height;
      if (!(w > 0 && h > 0)) (w = 16), (h = 9);
      out.push({ src, video, w, h });
      if (out.length >= 4) break;
    }
    return out;
  }

  function avatarIn(root, skip) {
    for (const img of root.querySelectorAll('[data-testid="Tweet-User-Avatar"] img, img')) {
      if (skip && skip(img)) continue;
      if (AVATAR_RE.test(img.src || "")) return img.src.replace(/_(normal|mini|bigger|x96)\./, "_bigger.");
    }
    return null;
  }

  function statusLink(root, skip) {
    for (const t of root.querySelectorAll("time")) {
      if (skip && skip(t)) continue;
      const a = t.closest("a");
      const m = a && STATUS_RE.exec(new URL(a.href, location.href).pathname);
      return { time: t.getAttribute("datetime") || null, id: m ? m[2] : null, owner: m ? m[1] : null };
    }
    return { time: null, id: null, owner: null };
  }

  function quoteBoxOf(article) {
    const names = article.querySelectorAll('[data-testid="User-Name"]');
    for (let i = 1; i < names.length; i++) {
      const box = names[i].closest('div[role="link"]');
      if (box && article.contains(box)) return box;
    }
    return null;
  }

  function extract(article) {
    const qbox = quoteBoxOf(article);
    const outside = (el) => !!(qbox && qbox.contains(el));
    const userName = [...article.querySelectorAll('[data-testid="User-Name"]')].find((u) => !outside(u));
    if (!userName) return null;
    const { name, handle } = nameAndHandle(userName);
    const st = statusLink(article, outside);

    const textEl = [...article.querySelectorAll('[data-testid="tweetText"]')].find((t) => !outside(t));
    let text = textEl ? richText(textEl) : [];
    let truncated = !!article.querySelector('[data-testid="tweet-text-show-more-link"]');
    if (truncated && st.id && notes.has(st.id)) {
      text = splitMentions(notes.get(st.id));
      truncated = false;
    }

    let card = null;
    const cw = [...article.querySelectorAll('[data-testid="card.wrapper"]')].find((c) => !outside(c));
    if (cw) {
      const lines = (cw.innerText || "")
        .split("\n")
        .map((s) => s.trim())
        .filter(Boolean)
        .slice(0, 3)
        .map((s) => s.slice(0, 200));
      const img = [...cw.querySelectorAll("img")].find((i) => MEDIA_RE.test(i.src || ""));
      card = {
        lines,
        img: img ? { src: img.src, video: false, w: img.naturalWidth || 16, h: img.naturalHeight || 9 } : null,
      };
      if (!lines.length && !card.img) card = null;
    }

    let quote = null;
    if (qbox) {
      const qn = qbox.querySelector('[data-testid="User-Name"]');
      const q = nameAndHandle(qn);
      const qt = qbox.querySelector('[data-testid="tweetText"]');
      const qs = statusLink(qbox);
      quote = {
        name: q.name,
        handle: q.handle,
        avatar: avatarIn(qbox),
        time: qs.time,
        text: qt ? richText(qt) : [],
        media: mediaIn(qbox),
      };
    }

    const key = st.id || "k:" + handle + ":" + plain(text).slice(0, 80);
    return {
      key,
      name,
      handle,
      avatar: avatarIn(article, outside),
      time: st.time,
      text,
      truncated,
      media: mediaIn(article, outside),
      card,
      quote,
    };
  }

  function articles() {
    let list = document.querySelectorAll('[data-testid="primaryColumn"] article[data-testid="tweet"]');
    if (!list.length) list = document.querySelectorAll('article[data-testid="tweet"]');
    return [...list]
      .filter((a) => !a.closest('[aria-modal="true"]'))
      .map((a) => ({ a, top: a.getBoundingClientRect().top }))
      .sort((x, y) => x.top - y.top)
      .map((x) => x.a);
  }

  // ------------------------------------------------ UI (own shadow root)
  let host = null;
  let banner = null;
  let hover = null;
  let mark = null;
  const css = (el, s) => (el.style.cssText = s);

  function ensureUi() {
    if (host && host.isConnected) return;
    host = document.createElement("div");
    css(host, "position:fixed;inset:0;z-index:2147483646;pointer-events:none;");
    const root = host.attachShadow({ mode: "closed" });
    const box = (color, fill) => {
      const d = document.createElement("div");
      css(
        d,
        `position:fixed;display:none;box-sizing:border-box;border:2px solid ${color};` +
          `background:${fill};border-radius:4px;pointer-events:none;`,
      );
      return d;
    };
    hover = box("#1d9bf0", "rgba(29,155,240,.08)");
    mark = box("#00ba7c", "rgba(0,186,124,.10)");
    banner = document.createElement("div");
    css(
      banner,
      "position:fixed;left:50%;top:12px;transform:translateX(-50%);max-width:90vw;" +
        "font:600 13px/1.4 system-ui,-apple-system,'Segoe UI','Malgun Gothic',sans-serif;color:#fff;" +
        "background:rgba(29,155,240,.95);border-radius:999px;padding:8px 16px;" +
        "box-shadow:0 2px 12px rgba(0,0,0,.4);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;",
    );
    root.append(hover, mark, banner);
    document.documentElement.appendChild(host);
  }

  function say(text) {
    ensureUi();
    banner.textContent = text;
    host.style.display = "block";
  }

  function frame(el, a) {
    if (!a || !a.isConnected) return (el.style.display = "none");
    const r = a.getBoundingClientRect();
    Object.assign(el.style, {
      display: "block",
      left: r.left + "px",
      top: r.top + "px",
      width: r.width + "px",
      height: r.height + "px",
    });
  }

  // ------------------------------------------------ state machine
  // idle -> start (pick first) -> end (pick last) -> busy (scrolling) -> idle
  let mode = "idle";
  let first = null; // { key, y }
  let hovered = null;
  let abort = false;

  const KEY_HINT = `${mac ? "⌘" : "Ctrl"}+Shift+S`;

  function startMode() {
    if (mode === "busy") return;
    if (mode !== "idle") return stop();
    mode = "start";
    first = null;
    say(`캡쳐: 첫 게시물을 클릭 · 취소 Esc 또는 ${KEY_HINT}`);
  }

  function stop(msg) {
    if (mode === "busy") abort = true;
    mode = "idle";
    first = null;
    hovered = null;
    if (!host) return;
    hover.style.display = "none";
    mark.style.display = "none";
    if (msg) {
      say(msg);
      setTimeout(() => mode === "idle" && host && (host.style.display = "none"), 2500);
    } else {
      host.style.display = "none";
    }
  }

  const articleAt = (t) => (t instanceof Element ? t.closest('article[data-testid="tweet"]') : null);
  const absY = (a) => a.getBoundingClientRect().top + window.scrollY;
  const markedArticle = () => first && articles().find((a) => (extract(a) || {}).key === first.key);

  function onPick(a) {
    const p = extract(a);
    if (!p) return;
    if (mode === "start") {
      first = { key: p.key, y: absY(a) };
      mode = "end";
      frame(mark, a);
      say("마지막 게시물을 클릭 (스크롤해서 찾기) · 같은 게시물을 다시 누르면 그 하나만 · 취소 Esc");
      return;
    }
    if (mode === "end") collect(first, { key: p.key, y: absY(a) });
  }

  // Swallow clicks on posts while picking, so X doesn't open them.
  for (const t of ["pointerdown", "mousedown", "pointerup", "mouseup", "click"]) {
    window.addEventListener(
      t,
      (e) => {
        if (mode === "idle") return;
        if (mode === "busy") {
          e.preventDefault();
          e.stopImmediatePropagation();
          return;
        }
        const a = articleAt(e.target);
        if (!a) return;
        e.preventDefault();
        e.stopImmediatePropagation();
        if (t === "click" && e.isTrusted && e.button === 0) onPick(a);
      },
      true,
    );
  }
  window.addEventListener(
    "mouseover",
    (e) => {
      if (mode !== "start" && mode !== "end") return;
      hovered = articleAt(e.target);
      frame(hover, hovered);
    },
    true,
  );
  window.addEventListener(
    "scroll",
    () => {
      if (mode !== "start" && mode !== "end") return;
      frame(hover, hovered);
      frame(mark, markedArticle());
    },
    true,
  );
  // Busy: keep the user's wheel / keys from fighting the auto scroll.
  window.addEventListener("wheel", (e) => mode === "busy" && e.preventDefault(), { capture: true, passive: false });

  window.addEventListener(
    "keydown",
    (e) => {
      const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
      if (mod && e.shiftKey && !e.altKey && e.code === "KeyS") {
        e.preventDefault();
        e.stopImmediatePropagation();
        if (!e.repeat) startMode();
        return;
      }
      if (mode === "idle") return;
      if (e.code === "Escape") {
        e.preventDefault();
        e.stopImmediatePropagation();
        stop("캡쳐 취소");
        return;
      }
      if (mode === "busy") {
        e.preventDefault();
        e.stopImmediatePropagation();
      }
    },
    true,
  );
  // Top bar button / host shortcut (Rust dispatches this event).
  window.addEventListener("paneful-capture", startMode);

  // ------------------------------------------------ auto scroll + collect
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const settle = async () => {
    await wait(220);
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  };
  const scrollTo = (y) => window.scrollTo({ top: Math.max(0, y), behavior: "instant" });

  async function findArticle(target) {
    const has = () => articles().some((a) => (extract(a) || {}).key === target.key);
    const vh = window.innerHeight;
    for (const off of [0, -0.5, -1, 0.5, 1, -2, 2, -3, 3]) {
      scrollTo(target.y - 120 + off * vh);
      await settle();
      if (abort) return false;
      if (has()) return true;
    }
    return false;
  }

  async function collect(a, b) {
    mode = "busy";
    abort = false;
    hover.style.display = "none";
    mark.style.display = "none";
    const back = window.scrollY;
    const [top, bottom] = a.y <= b.y ? [a, b] : [b, a];
    const list = [];
    const keys = new Set(); // collected
    const above = new Set(); // seen above the first post: never collected
    let lastKey = null;
    let started = false;
    let done = false;
    let cut = false;
    try {
      say("캡쳐: 게시물 모으는 중…");
      if (!(await findArticle(top))) {
        if (!abort) stop("첫 게시물을 다시 찾지 못했다. 다시 시도해라");
        return;
      }
      let stuck = 0;
      for (let step = 0; step < MAX_STEPS && !done && !abort; step++) {
        const posts = articles()
          .map(extract)
          .filter(Boolean);
        // Only what comes after the last collected post (when it is still
        // in the DOM): never pick up posts above the range again.
        const from = lastKey ? posts.findIndex((p) => p.key === lastKey) : -1;
        for (const p of from >= 0 ? posts.slice(from + 1) : posts) {
          const key = p.key;
          if (!started) {
            if (key !== top.key) {
              above.add(key);
              continue;
            }
            started = true;
          }
          if (above.has(key)) continue;
          if (!keys.has(key)) {
            keys.add(key);
            lastKey = key;
            delete p.key;
            list.push(p);
          }
          if (key === bottom.key) {
            done = true;
            break;
          }
          if (list.length >= MAX_POSTS) {
            done = cut = true;
            break;
          }
        }
        if (done) break;
        say(`캡쳐: 게시물 모으는 중… ${list.length}개`);
        const before = window.scrollY;
        window.scrollBy({ top: Math.round(window.innerHeight * 0.6), behavior: "instant" });
        await settle();
        if (Math.abs(window.scrollY - before) < 2) {
          if (++stuck >= 3) break; // bottom of the page
        } else stuck = 0;
      }
      if (abort) return;
      if (!list.length) return stop("게시물을 읽지 못했다");
      const posts = list;
      const t = window.__TAURI_INTERNALS__;
      if (!t) return stop("앱과 연결이 안 됐다. 패널을 새로고침해라");
      await t.invoke("capture_posts", { posts, cut: cut || !done });
      stop();
    } catch (e) {
      stop("캡쳐 실패: " + String(e && e.message ? e.message : e).slice(0, 120));
    } finally {
      if (mode === "busy") mode = "idle";
      scrollTo(back);
    }
  }
})();
