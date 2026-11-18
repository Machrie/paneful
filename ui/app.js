// Host UI. Runs in the local "host" webview underneath the x.com panels.
// Panels are native child webviews, so this page only shows through in the
// top bar and in the gaps between panels; the gaps are the drag handles.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let L = null; // last layout payload from Rust

function render() {
  const tabs = document.getElementById("tabs");
  const divs = document.getElementById("dividers");
  tabs.replaceChildren();
  divs.replaceChildren();
  if (!L) return;
  const pad = 8; // #bar padding-left
  L.panels.forEach((p, i) => {
    const t = document.createElement("div");
    t.className = "tab";
    t.style.left = `${p.rect.x - pad}px`;
    t.style.width = `${p.rect.w}px`;
    const dot = document.createElement("span");
    dot.className = "dot";
    dot.style.background = p.color;
    const name = document.createElement("span");
    name.textContent = p.label;
    const reload = document.createElement("button");
    reload.textContent = "새로고침";
    reload.onclick = () => invoke("reload_panel", { id: p.id });
    const del = document.createElement("button");
    del.textContent = "삭제";
    del.onclick = async () => {
      if (!confirm(`${p.label} 삭제? 로그인 정보·쿠키까지 지워짐`)) return;
      try { await invoke("remove_account", { id: p.id }); }
      catch (e) { alert(e); }
    };
    t.append(dot, name, reload, del);
    tabs.append(t);

    if (i < L.panels.length - 1) {
      const d = document.createElement("div");
      d.className = "divider";
      d.style.left = `${p.rect.x + p.rect.w}px`;
      d.style.width = `${L.gap}px`;
      d.addEventListener("pointerdown", (e) => startDrag(e, d, i));
      divs.append(d);
    }
  });
}

// Drag the divider between panel i and i+1. Only those two ratios change.
function startDrag(e, el, i) {
  el.setPointerCapture(e.pointerId);
  el.classList.add("drag");
  const startX = e.clientX;
  const start = L.ratios.slice();
  const usable = L.win_w - L.gap * (L.ratios.length - 1);
  const pair = start[i] + start[i + 1];
  const min = Math.min(L.min_panel / usable, pair / 2);
  let pending = false;
  const move = (ev) => {
    const dr = (ev.clientX - startX) / usable;
    const a = Math.max(min, Math.min(pair - min, start[i] + dr));
    const r = start.slice();
    r[i] = a;
    r[i + 1] = pair - a;
    if (pending) return; // one IPC in flight at a time
    pending = true;
    invoke("set_ratios", { ratios: r }).finally(() => (pending = false));
  };
  const up = () => {
    el.classList.remove("drag");
    el.removeEventListener("pointermove", move);
    invoke("commit_layout");
  };
  el.addEventListener("pointermove", move);
  el.addEventListener("pointerup", up, { once: true });
  el.addEventListener("pointercancel", up, { once: true });
}

document.getElementById("add").onclick = () => invoke("add_account").catch(alert);

listen("layout", (ev) => { L = ev.payload; render(); });
invoke("refresh_layout");
