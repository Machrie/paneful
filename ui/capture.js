// Post capture preview. capture.js in the panel took the screenshots and
// Rust stacked them into two images, with and without masks (capture.rs);
// this shows either one, and saves / copies the one shown. The 익명화
// choice is remembered on this computer.
(() => {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const mac = /Mac/.test(navigator.platform || navigator.userAgent);

  const SLICE = 4000; // px per canvas
  const $ = (id) => document.getElementById(id);
  const capEl = $("cap");
  const viewEl = $("cap-view");
  const infoEl = $("cap-info");
  const msgEl = $("cap-msg");
  const anonEl = $("cap-anon");

  const KEY = "paneful.capture.anon";
  try {
    const v = localStorage.getItem(KEY);
    if (v !== null) anonEl.checked = v === "1";
  } catch (_) {}

  let meta = null; // { w, h, plain_h, css_w, people, cut }
  let seq = 0;
  let busy = false;

  function setMsg(text, error) {
    msgEl.textContent = text || "";
    msgEl.classList.toggle("error", !!error);
  }

  // ArrayBuffer normally; a plain number array when Tauri's IPC has fallen
  // back to postMessage.
  const bytes = (buf) => new Blob([Array.isArray(buf) ? new Uint8Array(buf) : buf]);

  async function render() {
    if (!meta) return;
    const my = ++seq;
    const anon = anonEl.checked;
    const out = [];
    const total = anon ? meta.h : meta.plain_h || meta.h;
    try {
      for (let y = 0; y < total; y += SLICE) {
        const h = Math.min(SLICE, total - y);
        const bmp = await createImageBitmap(bytes(await invoke("capture_slice", { anon, y, h })));
        if (my !== seq) return;
        const c = document.createElement("canvas");
        c.width = bmp.width;
        c.height = bmp.height;
        c.getContext("2d").drawImage(bmp, 0, 0);
        c.style.width = `${meta.css_w}px`;
        out.push(c);
      }
    } catch (e) {
      if (my === seq) setMsg("미리보기 실패: " + String(e), true);
      return;
    }
    viewEl.replaceChildren(...out);
    const who = anon ? ` · ${meta.people}명 익명 처리` : " · 원본";
    infoEl.textContent = who.slice(3) + (meta.cut ? " · 끝 게시물까지 못 감 (최대 150개, 크기 한도 또는 페이지 끝)" : "");
  }

  const pad2 = (n) => String(n).padStart(2, "0");
  function stamp() {
    const d = new Date();
    return (
      `paneful_${d.getFullYear()}${pad2(d.getMonth() + 1)}${pad2(d.getDate())}` +
      `-${pad2(d.getHours())}${pad2(d.getMinutes())}${pad2(d.getSeconds())}`
    );
  }

  async function exportImg(action) {
    if (busy || !meta) return;
    busy = true;
    setMsg(action === "copy" ? "복사 중…" : "저장 중…");
    try {
      const name = await invoke("capture_export", { action, anon: anonEl.checked, name: stamp() });
      const saved = name ? `저장 폴더에 ${name} 저장` : "";
      setMsg(action === "copy" ? "클립보드에 복사했다" : action === "both" ? `${saved}, 클립보드에 복사` : saved);
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
  anonEl.onchange = () => {
    try {
      localStorage.setItem(KEY, anonEl.checked ? "1" : "0");
    } catch (_) {}
    render();
  };

  const MOD = mac ? "⌘" : "Ctrl";
  $("cap-copy").title = `${MOD}+C`;
  $("cap-save").title = `${MOD}+S`;
  $("cap-both").title = "Enter";
  $("cap-close").title = "닫기 (Esc)";
  anonEl.parentElement.title = "끄면 원본 그대로 (찍을 때 둘 다 찍어 둠)";

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
    meta = ev.payload;
    viewEl.replaceChildren();
    infoEl.textContent = "";
    setMsg("");
    viewEl.scrollTop = 0;
    render();
  });
  listen("layout", (ev) => {
    const open = !!ev.payload.capture;
    capEl.hidden = !open;
    if (!open && meta) {
      seq++;
      meta = null;
      viewEl.replaceChildren();
    }
  });
})();
