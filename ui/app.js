// Host UI. Runs in the local "host" webview underneath the x.com panels.
// Panels are native child webviews, so this page only shows through above
// the panels (top bar, or a thin strip while the bar is hidden) and in the
// gaps between panels; the gaps are the drag handles. The shortcut overlay
// and the settings screen are drawn here too, while Rust keeps the panels
// hidden.
// No confirm()/alert(): wry's macOS UI delegate doesn't implement them
// (confirm() always returns false there), so everything is inline.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const tabsEl = $("tabs");
const divsEl = $("dividers");
const statusEl = $("status");
const barEl = $("bar");
const stripEl = $("strip");
const overlayEl = $("overlay");
const settingsEl = $("settings");

const mac = /Mac/.test(navigator.platform || navigator.userAgent);
const MOD = mac ? "⌘" : "Ctrl";

let L = null; // last layout payload from Rust
let shape = ""; // panel ids/labels/colors the DOM was built for
let hotkeyErrorShown = false; // start-up hotkey error toasted once
let keysShape = ""; // shortcut set the overlay / settings rows were built for
let rec = null; // settings: shortcut waiting for a key press {id, btn}
const rowErrs = {}; // settings: last error per shortcut, kept across rebuilds

// Static, trusted markup only.
const ICONS = {
  reload: '<svg viewBox="0 0 24 24"><path d="M20 11a8 8 0 1 0-2.3 5.7"/><path d="M20 4v7h-7"/></svg>',
  trash: '<svg viewBox="0 0 24 24"><path d="M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3"/></svg>',
  keyboard: '<svg viewBox="0 0 24 24"><rect x="2.5" y="6" width="19" height="12" rx="2"/><path d="M6.5 10h.01M10 10h.01M14 10h.01M17.5 10h.01M7.5 14h9"/></svg>',
  chevronUp: '<svg viewBox="0 0 24 24"><path d="M6 15l6-6 6 6"/></svg>',
  eyeOff: '<svg viewBox="0 0 24 24"><path d="M3 3l18 18"/><path d="M10.6 5.1A10 10 0 0 1 12 5c5 0 9 4.5 10 7a13 13 0 0 1-2.9 3.9M6.6 6.6C4.3 8 2.6 10.2 2 12c1 2.5 5 7 10 7a9.6 9.6 0 0 0 5.4-1.6"/><path d="M9.9 9.9a3 3 0 0 0 4.2 4.2"/></svg>',
  close: '<svg viewBox="0 0 24 24"><path d="M6 6l12 12M18 6L6 18"/></svg>',
  gear: '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/></svg>',
  camera: '<svg viewBox="0 0 24 24"><path d="M4 8h3l2-3h6l2 3h3a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V9a1 1 0 0 1 1-1z"/><circle cx="12" cy="13" r="3.5"/></svg>',
};
function setIcon(el, name) {
  el.innerHTML = ICONS[name];
}
document.querySelectorAll("[data-icon]").forEach((el) => setIcon(el, el.dataset.icon));
// Button tooltips name the configured keys, so they follow the layout.
const keyText = (id) => {
  const k = L && L.keys.find((v) => v.id === id);
  return k && k.caps.length ? k.caps.join("+") : "";
};
const withKey = (text, k) => (k ? `${text} (${k})` : text);
function updateTitles() {
  $("help").title = withKey("단축키", keyText("help"));
  $("settings-btn").title = withKey("설정", keyText("settings"));
  $("hide").title = withKey("상단 바 숨기기", keyText("bar"));
  $("capture").title = withKey("게시물 캡쳐", keyText("capture")) + ": 첫 게시물, 마지막 게시물 순서로 클릭";
  $("tray").title = withKey("창을 트레이로 숨기기", L.hotkey.join("+"));
  stripTitle();
}

function stripTitle() {
  const bar = keyText("bar");
  const help = keyText("help");
  stripEl.title = [
    L.update_notice && `새 버전 v${L.update_notice} 있음`,
    `클릭${bar ? ` 또는 ${bar}` : ""}: 상단 바 표시`,
    help && `${help}: 단축키`,
  ]
    .filter(Boolean)
    .join(" · ");
}

// ---- new version notice (top bar pill, strip mark, settings section)
let updateShape = "";
function renderUpdate() {
  const n = L.update_notice;
  const u = L.update || {};
  const sh = JSON.stringify([n, u, L.update_check, L.skip_version, L.version, L.bar]);
  if (sh === updateShape) return;
  updateShape = sh;
  $("update-box").hidden = !n;
  $("update-mark").hidden = !n || L.bar;
  if (n) {
    $("update").textContent = `v${n} 받기`;
    $("update").title = `새 버전 v${n}: 릴리스 페이지 열기 (지금 v${L.version})`;
  }
  stripTitle();

  $("cur-ver").textContent = `v${L.version}`;
  $("up-auto").checked = L.update_check;
  $("up-check").disabled = !!u.checking;
  const st = $("up-state");
  st.classList.remove("ok", "warn");
  const skipped = u.newer && L.skip_version === u.newer;
  if (u.checking) st.textContent = "확인 중…";
  else if (u.error) {
    st.textContent = `확인 실패: ${u.error}`;
    st.classList.add("warn");
  } else if (u.newer) {
    st.textContent = `새 버전 v${u.newer} 있음${skipped ? " (건너뜀)" : ""}`;
    st.classList.add("ok");
  } else if (u.up_to_date) st.textContent = "최신 버전";
  else st.textContent = L.update_check ? "아직 확인 안 함" : "확인 꺼짐";
  $("up-open").hidden = !u.newer;
  $("up-skip").hidden = !u.newer;
  $("up-skip").textContent = skipped ? "다시 알림" : "이 버전 건너뛰기";
}
const openUpdate = () => invoke("open_update").catch(showError);
$("update").onclick = openUpdate;
$("update-skip").onclick = () => invoke("skip_update").catch(showError);
$("up-open").onclick = () => invoke("open_update").catch((e) => settingsMsg(e, true));
$("up-skip").onclick = () => {
  const undo = L.update.newer && L.skip_version === L.update.newer;
  invoke("skip_update", { undo }).catch((e) => settingsMsg(e, true));
};
$("up-check").onclick = () => invoke("check_update").catch((e) => settingsMsg(e, true));
$("up-auto").onchange = (e) =>
  invoke("set_update_check", { on: e.target.checked }).catch((err) => settingsMsg(err, true));

function showError(e) {
  statusEl.textContent = String(e);
  statusEl.hidden = false;
  clearTimeout(showError.timer);
  showError.timer = setTimeout(() => (statusEl.hidden = true), 8000);
}

function iconButton(icon, title, onclick) {
  const b = document.createElement("button");
  b.className = "icon";
  b.title = title;
  setIcon(b, icon);
  b.onclick = onclick;
  return b;
}

function pill(text, onclick) {
  const b = document.createElement("button");
  b.className = "pill";
  b.textContent = text;
  b.onclick = onclick;
  return b;
}

// Rebuild the DOM only when the set of panels changes. Plain layout updates
// (every pointer move while dragging) just move the existing elements, so
// the divider being dragged keeps its pointer capture.
function render() {
  if (!L) return;
  barEl.hidden = !L.bar;
  stripEl.hidden = L.bar;
  overlayEl.hidden = !L.help;
  settingsEl.hidden = !L.settings;
  if (!L.settings && rec) stopRec();
  document.documentElement.style.setProperty("--top", `${L.top}px`);
  const s = L.panels.map((p) => `${p.id}\u0000${p.label}\u0000${p.color}`).join("\u0001");
  const ks = JSON.stringify([L.keys, L.global, L.hotkey, L.hotkey_error, L.save_dir]);
  if (ks !== keysShape && !rec) {
    keysShape = ks;
    buildKeys();
    buildSettings();
    updateTitles();
  }
  if (!hotkeyErrorShown) {
    hotkeyErrorShown = true;
    if (L.hotkey_error) showError(L.hotkey_error);
  }
  if (s !== shape) {
    shape = s;
    build();
  }
  renderUpdate();
  place();
}

function build() {
  tabsEl.replaceChildren();
  divsEl.replaceChildren();
  stripEl.replaceChildren();
  L.panels.forEach((p, i) => {
    const seg = document.createElement("div");
    seg.className = "seg";
    seg.style.setProperty("--c", p.color);
    stripEl.append(seg);

    const t = document.createElement("div");
    t.className = "tab";
    t.style.setProperty("--c", p.color);
    // Clicking the tab itself (not its buttons) focuses that panel.
    t.addEventListener("click", (e) => {
      if (e.target.closest("button")) return;
      e.stopPropagation();
      invoke("shortcut", { action: "focus", index: i }).catch(showError);
    });
    const dot = document.createElement("span");
    dot.className = "dot";
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = p.label;
    const key = document.createElement("span");
    key.className = "key";
    if (i < 9) key.textContent = `${MOD} ${i + 1}`;

    const normal = document.createElement("span");
    normal.className = "group";
    const ask = document.createElement("span");
    ask.className = "group ask";
    ask.hidden = true;

    const del = iconButton("trash", "계정 삭제", () => {
      normal.hidden = true;
      ask.hidden = false;
    });
    del.classList.add("danger");
    normal.append(
      iconButton("reload", "새로고침", () => invoke("reload_panel", { id: p.id }).catch(showError)),
      del,
    );
    const cancel = () => {
      ask.hidden = true;
      normal.hidden = false;
    };
    const msg = document.createElement("span");
    msg.className = "warn";
    msg.textContent = "로그인·쿠키까지 지움";
    const yes = pill("삭제", async () => {
      yes.disabled = true;
      try {
        await invoke("remove_account", { id: p.id });
      } catch (e) {
        showError(e);
        yes.disabled = false;
        cancel();
      }
    });
    yes.classList.add("danger");
    ask.append(msg, yes, pill("취소", cancel));

    t.append(dot, name, key, normal, ask);
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
    t.classList.toggle("active", p.active);
    const seg = stripEl.children[i];
    seg.style.left = `${p.rect.x}px`;
    seg.style.width = `${p.rect.w}px`;
    seg.classList.toggle("active", p.active);
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

// ---- shortcut overlay
// Fixed: Ctrl/Cmd+V in a panel outside text fields (panel.js, paste_link).
const PASTE_LABEL = "복사한 X 링크 열기 (입력창 밖)";
const KEY_LABELS = {
  global: "창 숨기기/불러오기 (전역)",
  next: "다음 패널",
  prev: "이전 패널",
  bar: "상단 바 보이기/숨기기",
  help: "이 단축키 창",
  capture: "게시물 캡쳐 (첫 → 마지막 게시물 클릭)",
  settings: "설정",
};
// X's own shortcuts, only the ones that agree across several public lists
// (X has no current official page for them). Work in the focused panel.
// `then` = press one key, release, then the next (g then h).
const X_KEYS = [
  [["j"], "다음 게시물"],
  [["k"], "이전 게시물"],
  [["Space"], "아래로 스크롤"],
  [["."], "새 게시물 불러오기"],
  [["Enter"], "게시물 열기"],
  [["l"], "마음에 들어요"],
  [["r"], "답글"],
  [["t"], "재게시"],
  [["n"], "새 게시물"],
  [[mac ? "⌘" : "Ctrl", "Enter"], "게시하기"],
  [["/"], "검색"],
  [["m"], "쪽지"],
  [["g", "then", "h"], "홈"],
  [["g", "then", "n"], "알림"],
  [["g", "then", "p"], "내 프로필"],
  [["g", "then", "b"], "북마크"],
  [["g", "then", "m"], "쪽지함"],
  [["?"], "X 전체 단축키 목록"],
];
function fillKeys(dl, rows) {
  for (const [keys, label] of rows) {
    const dt = document.createElement("dt");
    keys.forEach((k, i) => {
      if (k === "then") {
        const s = document.createElement("span");
        s.className = "then";
        s.textContent = "→";
        dt.append(s);
        return;
      }
      const kbd = document.createElement("kbd");
      kbd.textContent = k;
      dt.append(kbd);
    });
    const dd = document.createElement("dd");
    dd.textContent = label;
    dl.append(dt, dd);
  }
}
// Paneful's own shortcuts come from settings via Rust, so the list is
// rebuilt whenever they change. Turned-off ones are left out.
function buildKeys() {
  const dl = $("app-keys");
  dl.replaceChildren();
  const rows = [[[MOD, "1–9"], "N번째 패널로 포커스"], [[MOD, "V"], PASTE_LABEL]];
  for (const k of L.keys) if (k.caps.length) rows.push([k.caps, KEY_LABELS[k.id]]);
  fillKeys(dl, rows);
  if (L.hotkey.length) {
    fillKeys(dl, [[L.hotkey, KEY_LABELS.global]]);
  } else {
    const dt = document.createElement("dt");
    dt.textContent = "전역 단축키";
    const dd = document.createElement("dd");
    dd.className = "warn";
    dd.textContent = L.hotkey_error || "꺼짐 (설정에서 켜기)";
    dl.append(dt, dd);
  }
}
fillKeys($("x-keys"), X_KEYS);
const toggleHelp = () => invoke("shortcut", { action: "help" }).catch(showError);
$("help").onclick = toggleHelp;
$("close").onclick = (e) => {
  e.stopPropagation();
  toggleHelp();
};
overlayEl.addEventListener("click", (e) => {
  e.stopPropagation();
  if (e.target === overlayEl) toggleHelp();
});

// ---- settings screen
const toggleSettings = () => invoke("shortcut", { action: "settings" }).catch(showError);
$("settings-btn").onclick = toggleSettings;
$("settings-close").onclick = (e) => {
  e.stopPropagation();
  toggleSettings();
};
settingsEl.addEventListener("click", (e) => {
  e.stopPropagation();
  if (e.target === settingsEl && !rec) toggleSettings();
});

function settingsMsg(text, bad) {
  const m = $("settings-msg");
  m.textContent = text || "";
  m.classList.toggle("warn", !!bad);
}

$("save-pick").onclick = async () => {
  try {
    if (await invoke("pick_save_dir")) settingsMsg("저장 폴더 바뀜");
  } catch (e) {
    settingsMsg(e, true);
  }
};
$("save-open").onclick = () => invoke("open_save_dir").catch((e) => settingsMsg(e, true));
$("save-reset").onclick = () =>
  invoke("reset_save_dir").then(() => settingsMsg("기본 폴더로 되돌림"), (e) => settingsMsg(e, true));

function buildSettings() {
  $("save-dir").textContent = L.save_dir || "(다운로드 폴더를 찾지 못함)";
  $("save-dir").title = L.save_dir;
  $("save-reset").disabled = L.save_dir_default;
  $("save-default").hidden = !L.save_dir_default;

  const box = $("key-rows");
  box.replaceChildren();
  const views = [L.global, ...L.keys];
  for (const v of views) box.append(keyRow(v));
  // Fixed ones, for reference.
  for (const [label, keys] of [
    ["N번째 패널로 포커스", [MOD, "1–9"]],
    [PASTE_LABEL, [MOD, "V"]],
  ]) {
    const row = document.createElement("div");
    row.className = "keyrow";
    const name = document.createElement("span");
    name.className = "kname";
    name.textContent = label;
    const caps = document.createElement("span");
    caps.className = "kcaps fixed";
    caps.append(kbds(keys));
    const note = document.createElement("span");
    note.className = "knote";
    note.textContent = "고정";
    row.append(name, caps, note);
    box.append(row);
  }
}

function kbds(keys) {
  const f = document.createDocumentFragment();
  keys.forEach((k) => {
    const kbd = document.createElement("kbd");
    kbd.textContent = k;
    f.append(kbd);
  });
  return f;
}

function keyRow(v) {
  const row = document.createElement("div");
  row.className = "keyrow";
  const name = document.createElement("span");
  name.className = "kname";
  name.textContent = KEY_LABELS[v.id];
  if (v.id === "global") {
    const sub = document.createElement("span");
    sub.className = "sub";
    sub.textContent = "다른 앱에서도";
    name.append(sub);
  }
  const btn = document.createElement("button");
  btn.className = "kcaps";
  btn.title = "눌러서 새 키 입력";
  const show = () => {
    btn.replaceChildren();
    if (v.caps.length) btn.append(kbds(v.caps));
    else btn.textContent = "꺼짐";
    btn.classList.toggle("off", !v.caps.length);
  };
  show();
  btn.onclick = () => startRec(v.id, btn, show);
  const def = pill("기본값", () => setKey(v.id, null));
  def.disabled = v.is_default;
  def.title = `기본값: ${v.default_caps.join("+")}`;
  const off = pill("끄기", () => setKey(v.id, ""));
  off.disabled = !v.caps.length;
  const err = document.createElement("span");
  err.className = "kerr warn";
  err.dataset.id = v.id;
  err.textContent = rowErrs[v.id] || (v.id === "global" && L.hotkey_error) || "";
  row.append(name, btn, def, off, err);
  return row;
}

function rowError(id, text) {
  if (text) rowErrs[id] = text;
  else delete rowErrs[id];
  const el = document.querySelector(`.kerr[data-id="${id}"]`);
  if (el) el.textContent = text || "";
}

async function setKey(id, accel) {
  rowError(id, "");
  settingsMsg("");
  try {
    await invoke("set_key", { id, accel });
    settingsMsg(`"${KEY_LABELS[id]}" 바뀜`);
  } catch (e) {
    rowError(id, String(e));
  }
}

function startRec(id, btn, restore) {
  if (rec) stopRec();
  rec = { id, btn, restore };
  btn.classList.add("rec");
  btn.replaceChildren();
  btn.textContent = "키 조합을 누르기 (Esc 취소)";
  rowError(id, "");
  invoke("set_recording", { on: true }).catch(() => {});
}

function stopRec() {
  if (!rec) return;
  rec.btn.classList.remove("rec");
  rec.restore();
  rec = null;
  invoke("set_recording", { on: false }).catch(() => {});
  // Changes that arrived while recording.
  if (L) render();
}

// KeyboardEvent -> accelerator string the Rust side parses.
function accelOf(e) {
  const p = [];
  if (mac) {
    if (e.ctrlKey) p.push("Control");
    if (e.altKey) p.push("Alt");
    if (e.shiftKey) p.push("Shift");
    if (e.metaKey) p.push("CommandOrControl");
  } else {
    if (e.ctrlKey) p.push("CommandOrControl");
    if (e.altKey) p.push("Alt");
    if (e.shiftKey) p.push("Shift");
    if (e.metaKey) p.push("Super");
  }
  let k = e.code;
  if (/^Key[A-Z]$/.test(k)) k = k.slice(3);
  else if (/^Digit[0-9]$/.test(k)) k = k.slice(5);
  p.push(k);
  return p.join("+");
}

// Runs before the app shortcut handler below (registered first).
window.addEventListener(
  "keydown",
  (e) => {
    if (!rec) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    if (e.repeat) return;
    if (/^(Control|Shift|Alt|Meta|OS)(Left|Right)?$/.test(e.code) || !e.code) return; // modifier alone
    const { id } = rec;
    if (e.code === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) {
      stopRec();
      return;
    }
    const accel = accelOf(e);
    stopRec();
    setKey(id, accel);
  },
  true,
);

$("capture").onclick = () => invoke("shortcut", { action: "capture" }).catch(showError);
$("add").onclick = () => invoke("add_account").catch(showError);
$("tray").onclick = () => invoke("shortcut", { action: "tray" }).catch(showError);
const toggleBar = () => invoke("shortcut", { action: "bar" }).catch(showError);
$("hide").onclick = toggleBar;
stripEl.onclick = toggleBar;

// Any click up here leaves keyboard focus on the active panel, so X's own
// shortcuts keep working without clicking back into it. Runs after the
// button handlers (bubble phase on document).
document.addEventListener("click", (e) => {
  if (e.target.closest("#help, #tray, #settings-btn, #capture")) return; // overlays keep focus here; tray hides the window; capture focuses the panel
  if (L && L.panels.length && !L.help && !L.settings && !L.capture) invoke("shortcut", { action: "refocus" }).catch(() => {});
});

// Same mapping as src-tauri/src/panel.js, for when this UI has focus.
// e.code, not e.key: stays the same under the Korean IME.
const hit = (b, e) =>
  b && e.code === b.code && e.ctrlKey === b.ctrl && e.altKey === b.alt &&
  e.shiftKey === b.shift && e.metaKey === b.meta;
function keyOf(e) {
  if (!L) return null;
  if (L.capture) return null; // capture.js handles keys in the preview
  for (const k of L.keys) if (hit(k.binding, e)) return { action: k.id };
  const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
  if (mod && !e.shiftKey && !e.altKey && /^Digit[1-9]$/.test(e.code)) {
    return { action: "focus", index: Number(e.code.slice(5)) - 1 };
  }
  if (e.code === "Escape") {
    if (L.help) return { action: "help" };
    if (L.settings) return { action: "settings" };
  }
  return null;
}
window.addEventListener(
  "keydown",
  (e) => {
    const a = keyOf(e);
    if (!a) return;
    e.preventDefault();
    if (e.repeat && ["bar", "help", "settings", "capture"].includes(a.action)) return;
    invoke("shortcut", a).catch(showError);
  },
  true,
);

listen("layout", (ev) => {
  L = ev.payload;
  render();
});
invoke("refresh_layout");
