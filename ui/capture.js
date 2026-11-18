// Post capture preview. Rust hides the panels and sends the posts that
// capture.js (in the panel) read; this page draws them as X-style cards on
// canvases, anonymized by default, and hands the canvases to Rust as PNG
// strips (`capture_export`), which joins them into one image.
//
// Everything in `posts` comes from x.com: it is only ever drawn as canvas
// text, never put into the DOM as markup.
(() => {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const mac = /Mac/.test(navigator.platform || navigator.userAgent);

  // ---- card geometry (CSS px; drawn at 2x)
  const W = 600;
  const PAD = 16;
  const AV = 40;
  const X0 = PAD + AV + 12;
  const CW = W - X0 - PAD;
  const LH = 20;
  const FAMILY =
    '-apple-system, BlinkMacSystemFont, "Segoe UI", "Malgun Gothic", "Apple SD Gothic Neo", ' +
    '"Apple Color Emoji", "Segoe UI Emoji", "Noto Color Emoji", sans-serif';
  const font = (weight, px) => `${weight} ${px}px ${FAMILY}`;
  const C = { bg: "#000", fg: "#e7e9ea", muted: "#71767b", line: "#2f3336", accent: "#1d9bf0", surface: "#16181c" };
  const ANON = ["#1d9bf0", "#f91880", "#00ba7c", "#ffad1f", "#7856ff", "#ff7a00", "#e0245e", "#17bf63", "#8b98a5"];
  const STRIP_MAX = 4000; // CSS px per canvas
  const MAX_PIXELS = 140e6; // Rust refuses above 150M

  const $ = (id) => document.getElementById(id);
  const capEl = $("cap");
  const viewEl = $("cap-view");
  const infoEl = $("cap-info");
  const msgEl = $("cap-msg");
  const anonEl = $("cap-anon");
  const timeEl = $("cap-time");

  let data = null; // { posts, cut }
  let strips = [];
  let seq = 0; // newest render wins
  let busy = false;
  const images = new Map(); // url -> Promise<ImageBitmap | null>

  function setMsg(text, error) {
    msgEl.textContent = text || "";
    msgEl.classList.toggle("error", !!error);
  }

  // ---- images: through Rust (cookie-less, pbs.twimg.com only), 6 at a time
  let running = 0;
  const queue = [];
  function pump() {
    while (running < 6 && queue.length) {
      const job = queue.shift();
      running++;
      invoke("capture_image", { url: job.url })
        .then((buf) => createImageBitmap(new Blob([buf])))
        .then(job.ok, () => job.ok(null))
        .finally(() => {
          running--;
          pump();
        });
    }
  }
  function loadImage(url) {
    if (!url) return Promise.resolve(null);
    let p = images.get(url);
    if (!p) {
      p = new Promise((ok) => queue.push({ url, ok }));
      images.set(url, p);
      pump();
    }
    return p;
  }

  // ---- anonymization: one number per person, in order of appearance
  const MENTION_RE = /(^|[^A-Za-z0-9_@])@([A-Za-z0-9_]{1,15})(?![A-Za-z0-9_])/g;
  const keyOf = (handle, name) => (handle ? handle.toLowerCase() : "name:" + name);

  function people(posts) {
    const ids = new Map(); // key -> n
    const names = new Map(); // display name -> n
    const add = (handle, name) => {
      const k = keyOf(handle, name || "");
      if (!ids.has(k)) ids.set(k, ids.size + 1);
      const n = ids.get(k);
      // Display names are replaced in plain text too: as shown, and without
      // their emoji ("김철수 🍀" -> also "김철수").
      for (const v of [name, (name || "").replace(/[\p{Extended_Pictographic}\uFE0F\u200D]/gu, "").trim()]) {
        if (v && v.length >= 2 && !names.has(v)) names.set(v, n);
      }
      return n;
    };
    const scan = (segs) => {
      for (const s of segs) {
        if (s.m) add(s.m);
        else if (!s.l) for (const m of s.t.matchAll(MENTION_RE)) add(m[2]);
      }
    };
    for (const p of posts) {
      add(p.handle, p.name);
      if (p.quote) add(p.quote.handle, p.quote.name);
      scan(p.text);
      if (p.quote) scan(p.quote.text);
    }
    return { ids, names, n: (handle, name) => ids.get(keyOf(handle, name || "")) };
  }

  const escRe = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

  // segs -> [{t, color}] for drawing
  function runs(segs, A) {
    const out = [];
    let nameRe = null;
    if (A && A.names.size) {
      const list = [...A.names.keys()].sort((a, b) => b.length - a.length).map(escRe);
      nameRe = new RegExp(list.join("|"), "g");
    }
    for (const s of segs) {
      if (s.m) {
        out.push({ t: A ? `@익명${A.ids.get(s.m.toLowerCase())}` : s.t, color: C.accent });
        continue;
      }
      if (s.l) {
        out.push({ t: s.t, color: C.accent });
        continue;
      }
      if (!A) {
        out.push({ t: s.t, color: C.fg });
        continue;
      }
      // Plain text: unlinked @handles and the display names of everyone in
      // the range are replaced too.
      let last = 0;
      for (const m of s.t.matchAll(MENTION_RE)) {
        const at = m.index + m[1].length;
        pushPlain(out, s.t.slice(last, at), A, nameRe);
        out.push({ t: `@익명${A.ids.get(m[2].toLowerCase())}`, color: C.accent });
        last = at + 1 + m[2].length;
      }
      pushPlain(out, s.t.slice(last), A, nameRe);
    }
    return out;
  }
  function pushPlain(out, t, A, nameRe) {
    if (!t) return;
    if (nameRe) t = t.replace(nameRe, (m) => `익명${A.names.get(m)}`);
    out.push({ t, color: C.fg });
  }

  function timeText(iso) {
    if (!iso || !timeEl.checked) return "";
    const d = new Date(iso);
    if (isNaN(d)) return "";
    return d.toLocaleString("ko-KR", {
      year: "numeric",
      month: "numeric",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
    });
  }

  // -> what one post (or quote) looks like with the current options
  function who(handle, name, avatar, time, A) {
    const t = timeText(time);
    if (A) {
      const n = A.n(handle, name);
      return { name: `익명${n}`, sub: t, anon: n, avatar: null };
    }
    const sub = [handle ? "@" + handle : "", t].filter(Boolean).join(" · ");
    return { name: name || handle, sub, anon: 0, avatar };
  }

  // ---- text layout
  const seg = typeof Intl.Segmenter === "function" ? new Intl.Segmenter("ko", { granularity: "word" }) : null;
  const gseg = typeof Intl.Segmenter === "function" ? new Intl.Segmenter("ko", { granularity: "grapheme" }) : null;
  const tokens = (s) => (seg ? [...seg.segment(s)].map((x) => x.segment) : s.split(/(\s+)/).filter(Boolean));
  const graphemes = (s) => (gseg ? [...gseg.segment(s)].map((x) => x.segment) : Array.from(s));

  const mctx = document.createElement("canvas").getContext("2d");

  // -> lines: [[{t, color, x}]]
  function wrap(rs, f, maxW) {
    mctx.font = f;
    const lines = [[]];
    let x = 0;
    const newline = () => {
      lines.push([]);
      x = 0;
    };
    const put = (t, color) => {
      const w = mctx.measureText(t).width;
      lines[lines.length - 1].push({ t, color, x });
      x += w;
    };
    for (const r of rs) {
      r.t.split("\n").forEach((part, i) => {
        if (i > 0) newline();
        for (const tok of tokens(part)) {
          const w = mctx.measureText(tok).width;
          if (x + w <= maxW) put(tok, r.color);
          else if (/^\s+$/.test(tok)) newline();
          else if (w <= maxW) {
            newline();
            put(tok, r.color);
          } else {
            for (const g of graphemes(tok)) {
              if (x > 0 && x + mctx.measureText(g).width > maxW) newline();
              put(g, r.color);
            }
          }
        }
      });
    }
    while (lines.length && !lines[lines.length - 1].some((r) => r.t.trim())) lines.pop();
    return lines;
  }

  function ellipsize(t, f, maxW) {
    mctx.font = f;
    if (mctx.measureText(t).width <= maxW) return t;
    const gs = graphemes(t);
    while (gs.length && mctx.measureText(gs.join("") + "…").width > maxW) gs.pop();
    return gs.join("") + "…";
  }

  // ---- drawing helpers
  function rr(ctx, x, y, w, h, r) {
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.arcTo(x + w, y, x + w, y + h, r);
    ctx.arcTo(x + w, y + h, x, y + h, r);
    ctx.arcTo(x, y + h, x, y, r);
    ctx.arcTo(x, y, x + w, y, r);
    ctx.closePath();
  }
  function cover(ctx, bmp, x, y, w, h) {
    const s = Math.max(w / bmp.width, h / bmp.height);
    const sw = w / s;
    const sh = h / s;
    ctx.drawImage(bmp, (bmp.width - sw) / 2, (bmp.height - sh) / 2, sw, sh, x, y, w, h);
  }
  function drawLines(ctx, lines, f, x, y) {
    ctx.font = f;
    ctx.textBaseline = "alphabetic";
    lines.forEach((line, i) => {
      for (const r of line) {
        ctx.fillStyle = r.color;
        ctx.fillText(r.t, x + r.x, y + i * LH + 15);
      }
    });
  }
  function drawAvatar(ctx, w, bm, x, y, size) {
    ctx.save();
    ctx.beginPath();
    ctx.arc(x + size / 2, y + size / 2, size / 2, 0, Math.PI * 2);
    ctx.clip();
    if (w.anon) {
      ctx.fillStyle = ANON[(w.anon - 1) % ANON.length];
      ctx.fillRect(x, y, size, size);
      ctx.fillStyle = "#fff";
      ctx.font = font(700, Math.round(size * (w.anon > 99 ? 0.32 : 0.42)));
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(String(w.anon), x + size / 2, y + size / 2 + 1);
      ctx.textAlign = "left";
      ctx.textBaseline = "alphabetic";
    } else {
      const b = w.avatar && bm.get(w.avatar);
      ctx.fillStyle = C.surface;
      ctx.fillRect(x, y, size, size);
      if (b) cover(ctx, b, x, y, size, size);
    }
    ctx.restore();
  }
  // name (bold) + sub (muted) on one line
  function drawHead(ctx, w, x, y, maxW, px) {
    const fn = font(700, px);
    const fs = font(400, px);
    const name = ellipsize(w.name, fn, w.sub ? maxW * 0.6 : maxW);
    ctx.font = fn;
    ctx.fillStyle = C.fg;
    ctx.fillText(name, x, y + 15);
    if (!w.sub) return;
    const nw = mctx.measureText(name).width + 4;
    mctx.font = fs;
    const sub = ellipsize(w.sub, fs, maxW - nw);
    ctx.font = fs;
    ctx.fillStyle = C.muted;
    ctx.fillText(sub, x + nw, y + 15);
  }

  function mediaGrid(media, w) {
    const n = media.length;
    if (!n) return null;
    if (n === 1) {
      const r = Math.min(Math.max(media[0].h / media[0].w, 0.4), 1.25);
      const h = Math.round(w * r);
      return { h, cells: [[0, 0, w, h]] };
    }
    const h = Math.round(w * 0.5625);
    const g = 2;
    const hw = (w - g) / 2;
    const hh = (h - g) / 2;
    const cells =
      n === 2
        ? [[0, 0, hw, h], [hw + g, 0, hw, h]]
        : n === 3
          ? [[0, 0, hw, h], [hw + g, 0, hw, hh], [hw + g, hh + g, hw, hh]]
          : [[0, 0, hw, hh], [hw + g, 0, hw, hh], [0, hh + g, hw, hh], [hw + g, hh + g, hw, hh]];
    return { h, cells };
  }
  function drawMedia(ctx, media, grid, bm, x, y, w) {
    ctx.save();
    rr(ctx, x, y, w, grid.h, 16);
    ctx.clip();
    grid.cells.forEach(([cx, cy, cw, ch], i) => {
      const m = media[i];
      ctx.fillStyle = C.surface;
      ctx.fillRect(x + cx, y + cy, cw, ch);
      const b = bm.get(m.src);
      if (b) cover(ctx, b, x + cx, y + cy, cw, ch);
      if (m.video) {
        const mx = x + cx + cw / 2;
        const my = y + cy + ch / 2;
        ctx.fillStyle = "rgba(29,155,240,.9)";
        ctx.beginPath();
        ctx.arc(mx, my, 24, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillStyle = "#fff";
        ctx.beginPath();
        ctx.moveTo(mx - 7, my - 11);
        ctx.lineTo(mx + 12, my);
        ctx.lineTo(mx - 7, my + 11);
        ctx.closePath();
        ctx.fill();
      }
    });
    ctx.restore();
    ctx.strokeStyle = C.line;
    ctx.lineWidth = 1;
    rr(ctx, x + 0.5, y + 0.5, w - 1, grid.h - 1, 16);
    ctx.stroke();
  }

  // ---- one post -> { h, draw(ctx, y) }
  function layoutPost(p, A, bm) {
    const w = who(p.handle, p.name, p.avatar, p.time, A);
    const f = font(400, 15);
    const lines = wrap(runs(p.text, A), f, CW);
    const parts = []; // [height, draw(ctx, x, y)]
    if (lines.length) parts.push([lines.length * LH, (ctx, y) => drawLines(ctx, lines, f, X0, y)]);
    if (p.truncated) {
      parts.push([LH, (ctx, y) => drawLines(ctx, [[{ t: "… (긴 글, 뒷부분 없음)", color: C.muted, x: 0 }]], f, X0, y)]);
    }
    const grid = mediaGrid(p.media, CW);
    if (grid) parts.push([12 + grid.h, (ctx, y) => drawMedia(ctx, p.media, grid, bm, X0, y + 12, CW)]);

    if (p.card) {
      const c = p.card;
      const ig = c.img ? { h: Math.round(CW * Math.min(Math.max(c.img.h / c.img.w, 0.3), 0.75)) } : null;
      const tl = [];
      c.lines.forEach((l, i) => {
        const lf = i === 0 ? font(400, 13) : font(400, 15);
        const ls = wrap([{ t: l, color: i === 0 ? C.muted : C.fg }], lf, CW - 24).slice(0, 2);
        tl.push([ls, lf]);
      });
      const th = tl.reduce((a, [ls]) => a + ls.length * LH, 0);
      const h = (ig ? ig.h : 0) + (th ? th + 20 : 0);
      if (h) {
        parts.push([
          12 + h,
          (ctx, y) => {
            const top = y + 12;
            ctx.save();
            rr(ctx, X0, top, CW, h, 16);
            ctx.clip();
            if (ig) {
              ctx.fillStyle = C.surface;
              ctx.fillRect(X0, top, CW, ig.h);
              const b = bm.get(c.img.src);
              if (b) cover(ctx, b, X0, top, CW, ig.h);
            }
            let ty = top + (ig ? ig.h : 0) + 10;
            for (const [ls, lf] of tl) {
              drawLines(ctx, ls, lf, X0 + 12, ty);
              ty += ls.length * LH;
            }
            ctx.restore();
            ctx.strokeStyle = C.line;
            rr(ctx, X0 + 0.5, top + 0.5, CW - 1, h - 1, 16);
            ctx.stroke();
          },
        ]);
      }
    }

    if (p.quote) {
      const q = p.quote;
      const qw = who(q.handle, q.name, q.avatar, q.time, A);
      const iw = CW - 24;
      const ql = wrap(runs(q.text, A), f, iw);
      const qg = mediaGrid(q.media, iw);
      const h = 12 + LH + (ql.length ? 4 + ql.length * LH : 0) + (qg ? 8 + qg.h : 0) + 12;
      parts.push([
        12 + h,
        (ctx, y) => {
          const top = y + 12;
          ctx.strokeStyle = C.line;
          ctx.lineWidth = 1;
          rr(ctx, X0 + 0.5, top + 0.5, CW - 1, h - 1, 16);
          ctx.stroke();
          let cy = top + 12;
          drawAvatar(ctx, qw, bm, X0 + 12, cy, 20);
          drawHead(ctx, qw, X0 + 12 + 24, cy, iw - 24, 15);
          cy += LH;
          if (ql.length) {
            cy += 4;
            drawLines(ctx, ql, f, X0 + 12, cy);
            cy += ql.length * LH;
          }
          if (qg) drawMedia(ctx, q.media, qg, bm, X0 + 12, cy + 8, iw);
        },
      ]);
    }

    const body = parts.reduce((a, [h]) => a + h, 0);
    const h = Math.ceil(Math.max(12 + LH + 2 + body, 12 + AV) + 12);
    return {
      h,
      draw(ctx, y) {
        drawAvatar(ctx, w, bm, PAD, y + 12, AV);
        drawHead(ctx, w, X0, y + 12, CW, 15);
        let cy = y + 12 + LH + 2;
        for (const [ph, d] of parts) {
          d(ctx, cy);
          cy += ph;
        }
      },
    };
  }

  function urlsOf(posts, anon) {
    const set = new Set();
    const media = (list) => list.forEach((m) => set.add(m.src));
    for (const p of posts) {
      if (!anon && p.avatar) set.add(p.avatar);
      media(p.media);
      if (p.card && p.card.img) set.add(p.card.img.src);
      if (p.quote) {
        if (!anon && p.quote.avatar) set.add(p.quote.avatar);
        media(p.quote.media);
      }
    }
    return [...set];
  }

  async function render() {
    if (!data) return;
    const my = ++seq;
    const anon = anonEl.checked;
    const posts = data.posts;
    const urls = urlsOf(posts, anon);
    setMsg(urls.length ? `이미지 불러오는 중… (${urls.length})` : "");
    const bm = new Map();
    await Promise.all(urls.map((u) => loadImage(u).then((b) => b && bm.set(u, b))));
    if (my !== seq) return;
    const failed = urls.length - bm.size;
    setMsg(failed ? `이미지 ${failed}개를 못 불러와 빈칸으로 둔다` : "");

    const A = anon ? people(posts) : null;
    const lays = posts.map((p) => layoutPost(p, A, bm));
    const total = lays.reduce((a, l) => a + l.h, 0);
    const scale = Math.min(2, Math.sqrt(MAX_PIXELS / (W * total)));

    const groups = [[]];
    let acc = 0;
    for (const l of lays) {
      if (acc && acc + l.h > STRIP_MAX) {
        groups.push([]);
        acc = 0;
      }
      groups[groups.length - 1].push(l);
      acc += l.h;
    }
    let done = 0;
    const out = groups.map((g) => {
      const h = g.reduce((a, l) => a + l.h, 0);
      const c = document.createElement("canvas");
      c.width = Math.round(W * scale);
      c.height = Math.round(h * scale);
      c.style.width = `${W}px`;
      const ctx = c.getContext("2d");
      ctx.scale(scale, scale);
      ctx.fillStyle = C.bg;
      ctx.fillRect(0, 0, W, h);
      let y = 0;
      for (const l of g) {
        l.draw(ctx, y);
        y += l.h;
        done++;
        if (done < lays.length) {
          ctx.fillStyle = C.line;
          ctx.fillRect(0, y - 1, W, 1);
        }
      }
      return c;
    });
    strips = out;
    viewEl.replaceChildren(...out);
    const ppl = A ? ` · ${A.ids.size}명 익명 처리` : "";
    infoEl.textContent =
      `게시물 ${posts.length}개${ppl}` + (data.cut ? " · 끝 게시물까지 못 감 (최대 150개 또는 페이지 끝)" : "");
  }

  // ---- export
  const pad2 = (n) => String(n).padStart(2, "0");
  function stamp() {
    const d = new Date();
    return (
      `paneful_${d.getFullYear()}${pad2(d.getMonth() + 1)}${pad2(d.getDate())}` +
      `-${pad2(d.getHours())}${pad2(d.getMinutes())}${pad2(d.getSeconds())}`
    );
  }
  const pngOf = (c) =>
    new Promise((ok, fail) =>
      c.toBlob((b) => (b ? b.arrayBuffer().then(ok, fail) : fail(new Error("PNG 변환 실패"))), "image/png"),
    );

  async function exportImg(action) {
    if (busy || !strips.length) return;
    busy = true;
    setMsg(action === "copy" ? "복사 중…" : "저장 중…");
    try {
      const parts = await Promise.all(strips.map(pngOf));
      const size = 4 + parts.reduce((a, p) => a + 4 + p.byteLength, 0);
      const buf = new Uint8Array(size);
      const dv = new DataView(buf.buffer);
      dv.setUint32(0, parts.length, true);
      let o = 4;
      for (const p of parts) {
        dv.setUint32(o, p.byteLength, true);
        o += 4;
        buf.set(new Uint8Array(p), o);
        o += p.byteLength;
      }
      const name = await invoke("capture_export", buf, { headers: { "x-action": action, "x-name": stamp() } });
      const saved = name ? `다운로드/Paneful/${name}에 저장` : "";
      setMsg(
        action === "copy" ? "클립보드에 복사했다" : action === "both" ? `${saved}, 클립보드에 복사` : saved,
      );
    } catch (e) {
      setMsg("실패: " + String(e), true);
    } finally {
      busy = false;
    }
  }

  const close = () => invoke("capture_close").catch(() => {});

  $("cap-copy").onclick = () => exportImg("copy");
  $("cap-save").onclick = () => exportImg("save");
  $("cap-both").onclick = () => exportImg("both");
  $("cap-close").onclick = close;
  anonEl.onchange = render;
  timeEl.onchange = render;

  const MOD = mac ? "⌘" : "Ctrl";
  $("cap-copy").title = `${MOD}+C`;
  $("cap-save").title = `${MOD}+S`;
  $("cap-both").title = "Enter";
  $("cap-close").title = "닫기 (Esc)";

  // While the preview is open this handles the keys; app.js steps aside.
  window.addEventListener(
    "keydown",
    (e) => {
      if (capEl.hidden) return;
      const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
      let act = null;
      if (e.code === "Escape") act = close;
      else if (mod && !e.shiftKey && e.code === "KeyC") act = () => exportImg("copy");
      else if (mod && !e.shiftKey && e.code === "KeyS") act = () => exportImg("save");
      else if (e.code === "Enter" && !e.target.closest("button")) act = () => exportImg("both");
      if (!act) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      if (!e.repeat) act();
    },
    true,
  );

  listen("capture", (ev) => {
    data = ev.payload;
    strips = [];
    viewEl.replaceChildren();
    infoEl.textContent = "";
    setMsg("");
    viewEl.scrollTop = 0;
    render();
  });
  listen("layout", (ev) => {
    const open = !!ev.payload.capture;
    capEl.hidden = !open;
    if (!open && data) {
      // Free the canvases and bitmaps; a new capture starts clean.
      seq++;
      data = null;
      strips = [];
      viewEl.replaceChildren();
      images.clear();
    }
  });
})();
