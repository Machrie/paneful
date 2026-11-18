// Host UI. Runs in the local "host" webview underneath the x.com panels.
// Panels are native child webviews, so this page only shows through in the
// top bar and in the gaps between panels; the gaps are the drag handles.
// No confirm()/alert(): wry's macOS UI delegate doesn't implement them
// (confirm() always returns false there), so everything is inline.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const tabsEl = document.getElementById("tabs");
const divsEl = document.getElementById("dividers");
const statusEl = document.getElementById("status");

let L = null; // last layout payload from Rust
let shape = ""; // panel ids/labels/colors the DOM was built for

function showError(e) {
  statusEl.textContent = String(e);
  statusEl.hidden = false;
  clearTimeout(showError.timer);
  showError.timer = setTimeout(() => (statusEl.hidden = true), 8000);
}

function button(text, onclick) {
  const b = document.createElement("button");
  b.textContent = text;
  b.onclick = onclick;
  return b;
}

// Rebuild the DOM only when the set of panels changes. Plain layout updates
// (every pointer move while dragging) just move the existing elements, so
// the divider being dragged keeps its pointer capture.
function render() {
  if (!L) return;
  const s = L.panels.map((p) => `${p.id}\u0000${p.label}\u0000${p.color}`).join("\u0001");
  if (s !== shape) {
    shape = s;
    build();
  }
  place();
}

function build() {
  tabsEl.replaceChildren();
  divsEl.replaceChildren();
  L.panels.forEach((p, i) => {
    const t = document.createElement("div");
    t.className = "tab";
    const dot = document.createElement("span");
    dot.className = "dot";
    dot.style.background = p.color;
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = p.label;

    const normal = document.createElement("span");
    normal.className = "group";
    const ask = document.createElement("span");
    ask.className = "group ask";
    ask.hidden = true;

    normal.append(
      button("새로고침", () => invoke("reload_panel", { id: p.id }).catch(showError)),
      button("삭제", () => {
        normal.hidden = true;
        ask.hidden = false;
      }),
    );
    const cancel = () => {
      ask.hidden = true;
      normal.hidden = false;
    };
    const msg = document.createElement("span");
    msg.className = "warn";
    msg.textContent = "로그인·쿠키까지 지움";
    const yes = button("삭제", async () => {
      yes.disabled = true;
      try {
        await invoke("remove_account", { id: p.id });
      } catch (e) {
        showError(e);
        yes.disabled = false;
        cancel();
      }
    });
    yes.className = "danger";
    ask.append(msg, yes, button("취소", cancel));

    t.append(dot, name, normal, ask);
    tabsEl.append(t);

    if (i < L.panels.length - 1) {
      const d = document.createElement("div");
      d.className = "divider";
      d.addEventListener("pointerdown", (e) => startDrag(e, d, i));
      divsEl.append(d);
    }
  });
}

function place() {
  const pad = 8; // #bar padding-left
  L.panels.forEach((p, i) => {
    const t = tabsEl.children[i];
    t.style.left = `${p.rect.x - pad}px`;
    t.style.width = `${p.rect.w}px`;
    const d = divsEl.children[i];
    if (d) {
      d.style.left = `${p.rect.x + p.rect.w}px`;
      d.style.width = `${L.gap}px`;
    }
  });
}

// Drag the divider between panel i and i+1. Only those two ratios change.
// At most one set_ratios is in flight; the newest position is kept and sent
// when it returns, and the final position is committed (applied + saved)
// once the in-flight call has finished.
function startDrag(e, el, i) {
  if (e.button !== 0) return;
  e.preventDefault();
  el.setPointerCapture(e.pointerId);
  el.classList.add("drag");
  const startX = e.clientX;
  const start = L.ratios.slice();
  const usable = L.win_w - L.gap * (start.length - 1);
  const pair = start[i] + start[i + 1];
  const min = Math.min(L.min_panel / usable, pair / 2);
  const ratiosAt = (x) => {
    const a = Math.max(min, Math.min(pair - min, start[i] + (x - startX) / usable));
    const r = start.slice();
    r[i] = a;
    r[i + 1] = pair - a;
    return r;
  };

  let last = start; // newest computed position
  let queued = null; // position waiting to be sent
  let inFlight = Promise.resolve();
  let busy = false;
  let done = false;

  const pump = () => {
    if (busy || !queued || done) return;
    const r = queued;
    queued = null;
    busy = true;
    inFlight = invoke("set_ratios", { ratios: r })
      .catch(() => {})
      .finally(() => {
        busy = false;
        pump();
      });
  };
  const move = (ev) => {
    last = ratiosAt(ev.clientX);
    queued = last;
    pump();
  };
  const end = async (ev) => {
    if (done) return;
    done = true;
    el.classList.remove("drag");
    el.removeEventListener("pointermove", move);
    el.removeEventListener("pointerup", end);
    el.removeEventListener("pointercancel", end);
    el.removeEventListener("lostpointercapture", end);
    if (ev.type === "pointerup") last = ratiosAt(ev.clientX);
    await inFlight;
    // Panels were added/removed mid-drag: these ratios no longer fit.
    if (!L || L.ratios.length !== start.length) return;
    invoke("commit_layout", { ratios: last }).catch(showError);
  };
  el.addEventListener("pointermove", move);
  el.addEventListener("pointerup", end);
  el.addEventListener("pointercancel", end);
  el.addEventListener("lostpointercapture", end);
}

document.getElementById("add").onclick = () => invoke("add_account").catch(showError);

listen("layout", (ev) => {
  L = ev.payload;
  render();
});
invoke("refresh_layout");
