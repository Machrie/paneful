// Paneful: post capture (x.com panels only). X as it looks, cut out post by
// post, with avatars, names and @handles swapped for 익명N in the masked copy.
//
// The capture shortcut (mapped in panel.js / ui/app.js, configurable) or the
// top bar button -> click the first post -> click the last post (the same
// post again, or Enter, for just that one). X keeps only the posts near the
// screen in the DOM, so the range is then walked twice by scrolling:
//
// 1. scan: number everyone in the range in order of appearance (authors,
//    quoted authors, mentions), so a name is masked even in a post above
//    its owner's first post.
// 2. shoot: from the first post down, per screen:
//    - hide X's sticky/fixed bars that overlap the column
//    - screenshot without masks, then lay the masks (own shadow root, over
//      the page, X's DOM is not touched) and screenshot again; Rust keeps
//      only the post rectangles (capture.rs)
//    - every rectangle and mask position is measured before and after each
//      screenshot; if anything moved, the shot is dropped and retaken, so a
//      frame where a mask may be off is never kept
//
// Nothing is requested from X; only what the page shows is read.
(() => {
  if (window.top !== window || window.__panefulCapture) return;
  Object.defineProperty(window, "__panefulCapture", { value: true });

  const MAX_POSTS = 150;
  const MAX_STEPS = 600;
  const STATUS_RE = /^\/([A-Za-z0-9_]{1,15})\/status\/(\d+)/;
  const HANDLE_RE = /^@([A-Za-z0-9_]{1,15})$/;
  const MENTION_RE = /(^|[^A-Za-z0-9_@])@([A-Za-z0-9_]{1,15})(?![A-Za-z0-9_])/g;
  const ANON = ["#1d9bf0", "#f91880", "#00ba7c", "#ffad1f", "#7856ff", "#ff7a00", "#e0245e", "#17bf63", "#8b98a5"];

  const call = (cmd, args) => {
    const t = window.__TAURI_INTERNALS__;
    if (!t) return Promise.reject(new Error("앱과 연결이 안 됐다. 패널을 새로고침해라"));
    return t.invoke(cmd, args || {});
  };

  // ------------------------------------------------ reading posts
  const isEmoji = (img) => /\/emoji\//.test(img.src || "");

  // Visible text of an element, emoji as their alt text.
  function textOf(root) {
    let out = "";
    const visit = (n) => {
      if (n.nodeType === 3) return void (out += n.nodeValue);
      if (n.nodeType !== 1) return;
      if (n.tagName === "IMG") return void (isEmoji(n) && (out += n.alt || ""));
      if (n.tagName === "svg" || n.tagName === "SVG") return;
      for (const c of n.childNodes) visit(c);
    };
    visit(root);
    return out.replace(/\s+/g, " ").trim();
  }

  function handleSpan(userName) {
    for (const s of userName.querySelectorAll("span")) {
      if (HANDLE_RE.test((s.textContent || "").trim())) return s;
    }
    return null;
  }

  function who(userName) {
    const hs = handleSpan(userName);
    const handle = hs ? HANDLE_RE.exec(hs.textContent.trim())[1] : "";
    const nameEl = userName.firstElementChild || userName;
    let name = textOf(nameEl);
    if (name === "@" + handle) name = "";
    return { handle, name, nameEl, handleEl: hs };
  }

  function quoteBoxOf(article) {
    const names = article.querySelectorAll('[data-testid="User-Name"]');
    for (let i = 1; i < names.length; i++) {
      const box = names[i].closest('div[role="link"]');
      if (box && article.contains(box)) return box;
    }
    return null;
  }

  function keyOf(article) {
    for (const t of article.querySelectorAll("time")) {
      const a = t.closest("a");
      const m = a && STATUS_RE.exec(new URL(a.href, location.href).pathname);
      if (m) return m[2];
    }
    const un = article.querySelector('[data-testid="User-Name"]');
    const tt = article.querySelector('[data-testid="tweetText"]');
    return "k:" + (un ? textOf(un) : "") + ":" + (tt ? textOf(tt).slice(0, 80) : "");
  }

  // Parts of one post: author, quoted post, text blocks.
  function partsOf(article) {
    const qbox = quoteBoxOf(article);
    const inQuote = (el) => !!(qbox && qbox.contains(el));
    const names = [...article.querySelectorAll('[data-testid="User-Name"]')];
    const un = names.find((u) => !inQuote(u));
    const qun = qbox && names.find((u) => inQuote(u));
    const avatarOf = (root, skip) =>
      [...root.querySelectorAll('[data-testid="Tweet-User-Avatar"]')].find((a) => !skip || !skip(a)) ||
      [...root.querySelectorAll("img")].find((i) => /\/profile_images\//.test(i.src || "") && (!skip || !skip(i)));
    return {
      author: un ? who(un) : null,
      avatar: avatarOf(article, inQuote),
      quote: qun ? who(qun) : null,
      qavatar: qbox ? avatarOf(qbox) : null,
      texts: [...article.querySelectorAll('[data-testid="tweetText"]')],
      social: [...article.querySelectorAll('[data-testid="socialContext"]')],
      // @handle links outside the name lines: text mentions, "Replying to @x"
      mentions: [...article.querySelectorAll("a")].filter(
        (a) => !a.closest('[data-testid="User-Name"]') && HANDLE_RE.test((a.textContent || "").trim()),
      ),
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

  // ------------------------------------------------ people: 익명1..N
  function newPeople() {
    const ids = new Map(); // handle (lower case) or "name:..." -> n
    const names = new Map(); // display name -> n
    const add = (handle, name) => {
      const k = handle ? handle.toLowerCase() : name ? "name:" + name : "";
      if (!k) return 0;
      if (!ids.has(k)) ids.set(k, ids.size + 1);
      const n = ids.get(k);
      // Display names are also masked where they appear in text: as shown
      // and without their emoji ("김철수 🍀" -> also "김철수").
      for (const v of [name, (name || "").replace(/[\p{Extended_Pictographic}️‍]/gu, "").trim()]) {
        if (v && v.length >= 2 && !names.has(v)) names.set(v, n);
      }
      return n;
    };
    const n = (handle, name) => ids.get(handle ? handle.toLowerCase() : "name:" + name) || add(handle, name);
    return { ids, names, add, n };
  }

  function scanPeople(P, article) {
    const p = partsOf(article);
    if (p.author) P.add(p.author.handle, p.author.name);
    if (p.quote) P.add(p.quote.handle, p.quote.name);
    for (const a of p.mentions) P.add(HANDLE_RE.exec(a.textContent.trim())[1]);
    for (const t of p.texts) for (const m of (t.textContent || "").matchAll(MENTION_RE)) P.add(m[2]);
  }

  // ------------------------------------------------ masks
  // A mask: rectangles (viewport CSS px) + what to draw over them.
  function bgOf(el) {
    // Stack of backgrounds down to the first opaque one (hover highlights
    // are translucent), so the mask matches what is under the text.
    const layers = [];
    for (let e = el; e && e.nodeType === 1; e = e.parentElement) {
      const c = getComputedStyle(e).backgroundColor;
      const m = /rgba?\(([^)]+)\)/.exec(c);
      if (!m) continue;
      const parts = m[1].split(",").map((s) => parseFloat(s));
      const a = parts.length > 3 ? parts[3] : 1;
      if (a === 0) continue;
      layers.push(c);
      if (a >= 1) break;
    }
    if (!layers.length || !/rgb\(|, 1\)$/.test(layers[layers.length - 1])) layers.push(pageBg());
    return layers;
  }
  function pageBg() {
    for (const e of [document.body, document.documentElement]) {
      const c = e && getComputedStyle(e).backgroundColor;
      if (c && !/rgba\(0, 0, 0, 0\)|transparent/.test(c)) return c;
    }
    return "rgb(0, 0, 0)";
  }

  function textMask(el, rects, text) {
    const cs = getComputedStyle(el);
    return { kind: "text", rects: [...rects].filter((r) => r.width > 0 && r.height > 0), text, cs, bg: bgOf(el) };
  }

  // Ranges of `re` matches in the text nodes of root (not inside links).
  function rangesIn(root, re) {
    const out = [];
    const w = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    for (let n = w.nextNode(); n; n = w.nextNode()) {
      if (n.parentElement && n.parentElement.closest("a")) continue;
      re.lastIndex = 0;
      for (const m of n.nodeValue.matchAll(re)) {
        const r = document.createRange();
        const at = m.index + (m.groups && m.groups.pre ? m.groups.pre.length : 0);
        const len = m[0].length - (at - m.index);
        r.setStart(n, at);
        r.setEnd(n, at + len);
        out.push({ range: r, match: m[0].slice(at - m.index), node: n });
      }
    }
    return out;
  }

  const escRe = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

  function masksFor(article, P) {
    const p = partsOf(article);
    const out = [];
    const person = (w, avatar) => {
      if (!w) return;
      const n = P.n(w.handle, w.name);
      if (avatar) {
        const img = avatar.querySelector ? avatar.querySelector("img") : null;
        const box = (img && img.getBoundingClientRect().width > 4 ? img : avatar).getBoundingClientRect();
        if (box.width > 0) out.push({ kind: "avatar", rects: [box], n, bg: bgOf(avatar.parentElement || avatar) });
      }
      if (w.nameEl) out.push(textMask(w.nameEl, [w.nameEl.getBoundingClientRect()], `익명${n}`));
      if (w.handleEl) out.push(textMask(w.handleEl, [w.handleEl.getBoundingClientRect()], `@익명${n}`));
    };
    person(p.author, p.avatar);
    person(p.quote, p.qavatar);
    for (const a of p.mentions) {
      const h = HANDLE_RE.exec(a.textContent.trim())[1];
      out.push(textMask(a, a.getClientRects(), `@익명${P.n(h)}`));
    }
    for (const s of p.social) out.push(textMask(s, [s.getBoundingClientRect()], ""));
    // Unlinked @handles and the display names of everyone in the range.
    const names = [...P.names.keys()].sort((a, b) => b.length - a.length).map(escRe);
    const re = new RegExp(
      `(?<pre>^|[^A-Za-z0-9_@])@[A-Za-z0-9_]{1,15}(?![A-Za-z0-9_])` + (names.length ? `|${names.join("|")}` : ""),
      "gu",
    );
    for (const t of p.texts) {
      for (const { range, match, node } of rangesIn(t, re)) {
        const text = match.startsWith("@") ? `@익명${P.n(match.slice(1))}` : `익명${P.names.get(match)}`;
        out.push(textMask(node.parentElement, range.getClientRects(), text));
      }
    }
    return out;
  }

  // ------------------------------------------------ overlay (own shadow root)
  let host = null;
  let root = null;
  let banner = null;
  let hover = null;
  let mark = null;
  let maskLayer = null;
  const css = (el, s) => (el.style.cssText = s);
  const mctx = document.createElement("canvas").getContext("2d");

  function ensureUi() {
    if (host && host.isConnected) return;
    host = document.createElement("div");
    css(host, "position:fixed;inset:0;z-index:2147483646;pointer-events:none;");
    root = host.attachShadow({ mode: "closed" });
    const box = (color, fill) => {
      const d = document.createElement("div");
      css(
        d,
        `position:fixed;display:none;box-sizing:border-box;border:2px solid ${color};` +
          `background:${fill};border-radius:4px;pointer-events:none;`,
      );
      return d;
    };
    maskLayer = document.createElement("div");
    css(maskLayer, "position:fixed;inset:0;pointer-events:none;");
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
    root.append(maskLayer, hover, mark, banner);
    document.documentElement.appendChild(host);
  }

  function say(text) {
    ensureUi();
    banner.textContent = text;
    banner.style.display = text ? "block" : "none";
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

  function drawMasks(masks) {
    maskLayer.replaceChildren();
    for (const m of masks) {
      m.rects.forEach((r, i) => {
        const d = document.createElement("div");
        const bg = m.bg.map((c) => `linear-gradient(${c},${c})`).join(",");
        css(
          d,
          `position:fixed;left:${r.left - 1}px;top:${r.top - 1}px;width:${r.width + 2}px;height:${r.height + 2}px;` +
            `background:${bg};overflow:hidden;`,
        );
        if (m.kind === "avatar") {
          const s = Math.min(r.width, r.height);
          const c = document.createElement("div");
          css(
            c,
            `position:absolute;left:1px;top:1px;width:${s}px;height:${s}px;border-radius:50%;` +
              `background:${ANON[(m.n - 1) % ANON.length]};color:#fff;display:flex;align-items:center;` +
              `justify-content:center;font:700 ${Math.round(s * (m.n > 99 ? 0.32 : 0.42))}px/1 ` +
              `system-ui,-apple-system,'Segoe UI',sans-serif;`,
          );
          c.textContent = String(m.n);
          d.append(c);
        } else if (i === 0 && m.text) {
          // Same font and color as the text it covers, shrunk to fit.
          const cs = m.cs;
          let size = parseFloat(cs.fontSize) || 15;
          mctx.font = `${cs.fontWeight} ${size}px ${cs.fontFamily}`;
          const w = mctx.measureText(m.text).width;
          if (w > r.width && w > 0) size = Math.max(7, (size * r.width) / w);
          const t = document.createElement("span");
          css(
            t,
            `position:absolute;left:1px;top:0;height:100%;display:flex;align-items:center;white-space:nowrap;` +
              `font-family:${cs.fontFamily};font-weight:${cs.fontWeight};font-size:${size}px;color:${cs.color};`,
          );
          t.textContent = m.text;
          d.append(t);
        }
        maskLayer.append(d);
      });
    }
  }

  // ------------------------------------------------ X's own fixed / sticky bars
  const hidden = new Map(); // element -> previous inline visibility
  function hideBars(col) {
    const xs = [col.left + 4, col.left + col.width / 2, col.right - 4];
    for (let y = 1; y < window.innerHeight; y += 16) {
      for (const x of xs) {
        for (const el of document.elementsFromPoint(x, y)) {
          if (el === host) continue;
          for (let e = el; e && e !== document.body && e !== document.documentElement; e = e.parentElement) {
            if (hidden.has(e)) break;
            const pos = getComputedStyle(e).position;
            if (pos !== "fixed" && pos !== "sticky") continue;
            if (e.querySelector('article[data-testid="tweet"]')) break;
            hidden.set(e, e.style.visibility);
            e.style.visibility = "hidden";
            break;
          }
        }
      }
    }
  }
  function showBars() {
    for (const [e, v] of hidden) e.style.visibility = v;
    hidden.clear();
  }

  // ------------------------------------------------ picking
  // idle -> start (pick first) -> end (pick last) -> busy -> idle
  let mode = "idle";
  let first = null; // { key, y }
  let hovered = null;
  let abort = false;

  function startMode() {
    if (mode === "busy") return;
    if (mode !== "idle") return stop("캡쳐 취소");
    mode = "start";
    first = null;
    say("캡쳐: 첫 게시물을 클릭 · 취소 Esc");
  }

  function stop(msg) {
    if (mode === "busy") abort = true;
    mode = "idle";
    first = null;
    hovered = null;
    if (!host) return;
    hover.style.display = "none";
    mark.style.display = "none";
    maskLayer.replaceChildren();
    host.style.pointerEvents = "none";
    if (msg) {
      say(msg);
      setTimeout(() => mode === "idle" && host && (host.style.display = "none"), 2500);
    } else {
      host.style.display = "none";
    }
  }

  const articleAt = (t) => (t instanceof Element ? t.closest('article[data-testid="tweet"]') : null);
  const absY = (a) => a.getBoundingClientRect().top + window.scrollY;
  const findByKey = (key) => articles().find((a) => keyOf(a) === key);

  function onPick(a) {
    if (mode === "start") {
      first = { key: keyOf(a), y: absY(a) };
      mode = "end";
      frame(mark, a);
      say("마지막 게시물을 클릭 (스크롤해서 찾기) · 이 게시물만이면 Enter 또는 한 번 더 클릭 · 취소 Esc");
      return;
    }
    if (mode === "end") run(first, { key: keyOf(a), y: absY(a) });
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
      frame(mark, first && findByKey(first.key));
    },
    true,
  );
  // Busy: keep the user's wheel / keys from fighting the auto scroll.
  window.addEventListener("wheel", (e) => mode === "busy" && e.preventDefault(), { capture: true, passive: false });
  window.addEventListener(
    "keydown",
    (e) => {
      if (mode === "idle") return;
      if (e.code === "Escape") {
        e.preventDefault();
        e.stopImmediatePropagation();
        stop("캡쳐 취소");
        return;
      }
      if (mode === "end" && (e.code === "Enter" || e.code === "NumpadEnter") && !e.repeat) {
        e.preventDefault();
        e.stopImmediatePropagation();
        run(first, first);
        return;
      }
      if (mode === "busy") {
        e.preventDefault();
        e.stopImmediatePropagation();
      }
    },
    true,
  );
  // The capture shortcut and the top bar button arrive as this event;
  // pressing it again while picking cancels.
  window.addEventListener("paneful-capture", startMode);

  // ------------------------------------------------ scrolling helpers
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const frames = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  const settle = async (ms) => {
    await wait(ms);
    await frames();
  };
  const scrollTo = (y) => window.scrollTo({ top: Math.max(0, y), behavior: "instant" });

  async function findArticle(target) {
    const vh = window.innerHeight;
    for (const off of [0, -0.5, -1, 0.5, 1, -2, 2, -3, 3]) {
      scrollTo(target.y - 120 + off * vh);
      await settle(200);
      if (abort) return null;
      const a = findByKey(target.key);
      if (a) return a;
    }
    return null;
  }

  // Scan pass: keys in order from top to bottom, and everyone in them.
  async function scan(top, bottom, P) {
    const keys = [];
    const seen = new Set();
    const above = new Set();
    let started = false;
    let last = null;
    if (!(await findArticle(top))) throw new Error("첫 게시물을 다시 찾지 못했다");
    let stuck = 0;
    for (let step = 0; step < MAX_STEPS && !abort; step++) {
      const arts = articles();
      const from = last ? arts.findIndex((a) => keyOf(a) === last) : -1;
      for (const a of from >= 0 ? arts.slice(from + 1) : arts) {
        const k = keyOf(a);
        if (!started) {
          if (k !== top.key) {
            above.add(k);
            continue;
          }
          started = true;
        }
        if (above.has(k) || seen.has(k)) continue;
        seen.add(k);
        keys.push(k);
        last = k;
        scanPeople(P, a);
        if (k === bottom.key || keys.length >= MAX_POSTS) return { keys, cut: k !== bottom.key };
      }
      say(`캡쳐: 범위 확인 중… ${keys.length}개`);
      const before = window.scrollY;
      window.scrollBy({ top: Math.round(window.innerHeight * 0.7), behavior: "instant" });
      await settle(150);
      if (Math.abs(window.scrollY - before) < 2) {
        if (++stuck >= 3) break;
      } else stuck = 0;
    }
    return { keys, cut: true };
  }

  // ------------------------------------------------ shoot pass
  // Rectangles of the posts in `keys` visible now, from the cursor down.
  // cursor = { i: index in keys, off: px of that post already taken }
  function plan(keys, cur) {
    const vh = window.innerHeight;
    const byKey = new Map(articles().map((a) => [keyOf(a), a]));
    const crops = [];
    let next = { ...cur };
    for (let i = cur.i; i < keys.length; i++) {
      const a = byKey.get(keys[i]);
      if (!a) break; // not rendered yet: scroll on
      const r = a.getBoundingClientRect();
      const off = i === cur.i ? cur.off : 0;
      if (off >= r.height - 0.5) {
        next = { i: i + 1, off: 0 };
        continue;
      }
      const top = r.top + off;
      if (top >= vh - 1) break;
      if (top < -1) return null; // part of it is above the screen: re-scroll
      const bottom = Math.min(r.bottom, vh);
      crops.push({ x: r.left, y: top, w: r.width, h: bottom - top, a });
      if (bottom < r.bottom - 0.5) {
        next = { i, off: off + (bottom - top) };
        break;
      }
      next = { i: i + 1, off: 0 };
    }
    return { crops, next };
  }

  // Everything a shot depends on, rounded: if this changes between before
  // and after a screenshot, the shot is not used.
  function signature(crops, masks) {
    const r = (v) => Math.round(v * 2);
    const parts = [window.scrollY, window.innerWidth, window.innerHeight];
    for (const c of crops) {
      const b = c.a.getBoundingClientRect();
      parts.push(r(b.left), r(b.top), r(b.width), r(b.height));
    }
    for (const m of masks || []) for (const x of m.rects) parts.push(r(x.left), r(x.top), r(x.width), r(x.height));
    return parts.join(",");
  }

  async function shot(crops, anon) {
    const res = await call("capture_shot", {
      viewW: window.innerWidth,
      anon,
      crops: crops.map(({ x, y, w, h }) => ({ x, y, w, h })),
    });
    return res;
  }

  async function shootStep(keys, cur, P) {
    for (let attempt = 0; attempt < 6 && !abort; attempt++) {
      const p = plan(keys, cur);
      if (!p || !p.crops.length) return null;
      const arts = [...new Set(p.crops.map((c) => c.a))];
      maskLayer.replaceChildren();
      banner.style.display = "none";
      await frames();
      const sig0 = signature(p.crops);
      const plain = await shot(p.crops, false);
      if (signature(p.crops) !== sig0) {
        await call("capture_drop", { id: plain.id });
        await settle(150);
        continue;
      }
      // Masked copy: measure, lay masks, check they are where they were
      // measured, shoot, check again.
      const masks = arts.flatMap((a) => masksFor(a, P));
      const sig1 = signature(p.crops, masks);
      drawMasks(masks);
      await frames();
      const still = () => signature(p.crops) === sig0 && signature(p.crops, remeasure(arts, P)) === sig1;
      const masked = still() ? await shot(p.crops, true) : null;
      const ok = masked && still();
      maskLayer.replaceChildren();
      if (!ok) {
        await call("capture_drop", { id: plain.id });
        if (masked) await call("capture_drop", { id: masked.id });
        await settle(150);
        continue;
      }
      return { next: p.next, full: plain.full || masked.full };
    }
    throw new Error("화면이 계속 움직여서 찍지 못했다");
  }
  const remeasure = (arts, P) => arts.flatMap((a) => masksFor(a, P));

  async function run(a, b) {
    mode = "busy";
    abort = false;
    hover.style.display = "none";
    mark.style.display = "none";
    const back = window.scrollY;
    const [top, bottom] = a.y <= b.y ? [a, b] : [b, a];
    const P = newPeople();
    let begun = false;
    let finished = false;
    window.dispatchEvent(new CustomEvent("paneful-shooting", { detail: true }));
    host.style.pointerEvents = "auto"; // no hover highlight on posts while shooting
    try {
      say("캡쳐: 범위 확인 중…");
      const { keys, cut } = await scan(top, bottom, P);
      if (abort) return;
      if (!keys.length) throw new Error("게시물을 읽지 못했다");

      await call("capture_begin");
      begun = true;
      const firstArt = await findArticle(top);
      if (!firstArt) throw new Error("첫 게시물을 다시 찾지 못했다");
      let cur = { i: 0, off: 0 };
      let full = false;
      let stuck = 0;
      let col = firstArt.getBoundingClientRect();
      for (let step = 0; step < MAX_STEPS && cur.i < keys.length && !abort && !full; step++) {
        // Put the next part to take at the top of the screen.
        const art = findByKey(keys[cur.i]);
        if (art) {
          col = art.getBoundingClientRect();
          window.scrollBy({ top: Math.round(col.top + cur.off), behavior: "instant" });
        } else {
          window.scrollBy({ top: Math.round(window.innerHeight * 0.5), behavior: "instant" });
        }
        await settle(180);
        hideBars(col);
        await frames();
        const r = await shootStep(keys, cur, P);
        say(`캡쳐: 찍는 중… ${Math.min(cur.i + 1, keys.length)}/${keys.length}`);
        if (!r) {
          if (++stuck >= 8) throw new Error("다음 게시물이 화면에 안 나온다");
          continue;
        }
        stuck = 0;
        cur = r.next;
        full = r.full;
      }
      if (abort) return;
      await call("capture_finish", {
        cssW: col.width,
        people: P.ids.size,
        cut: cut || cur.i < keys.length,
        cancel: false,
      });
      finished = true;
      stop();
    } catch (e) {
      stop("캡쳐 실패: " + String(e && e.message ? e.message : e).slice(0, 160));
    } finally {
      if (begun && !finished) call("capture_finish", { cssW: 0, people: 0, cut: true, cancel: true }).catch(() => {});
      showBars();
      if (maskLayer) maskLayer.replaceChildren();
      if (host) host.style.pointerEvents = "none";
      window.dispatchEvent(new CustomEvent("paneful-shooting", { detail: false }));
      if (mode === "busy") mode = "idle";
      scrollTo(back);
    }
  }
})();
