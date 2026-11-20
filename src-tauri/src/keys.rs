//! Configurable shortcuts.
//!
//! Every shortcut is stored as an accelerator string in settings.json, the
//! same syntax the global shortcut plugin parses ("CommandOrControl+Shift+H",
//! "Control+Tab", "CommandOrControl+Slash"). In-app shortcuts are matched in
//! JavaScript (panel.js, ui/app.js), so Rust turns each one into a `Binding`
//! with the exact `KeyboardEvent.code` and modifier flags to compare against.

use serde::Serialize;
use std::collections::BTreeMap;
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

/// Global show/hide shortcut. Not in `APP`: the OS delivers it, not a page.
pub const GLOBAL: &str = "global";

/// In-app shortcuts: (id, default). Panel focus (Ctrl/Cmd+1..9) is fixed.
pub const APP: [(&str, &str); 11] = [
    ("next", "Control+Tab"),
    ("prev", "Control+Shift+Tab"),
    ("bar", "CommandOrControl+Shift+H"),
    ("help", "CommandOrControl+Slash"),
    ("capture", "CommandOrControl+Shift+S"),
    // Then a click on a post (or Enter for the page) opens it in another
    // panel. Handled in panel.js, which knows the post.
    ("send", "CommandOrControl+Shift+O"),
    // Active panel fills the window, again to go back.
    ("zoom", "CommandOrControl+Shift+F"),
    ("settings", "CommandOrControl+Comma"),
    // Page zoom of the panel the key is pressed in (or the active one).
    ("zoom_in", "CommandOrControl+Equal"),
    ("zoom_out", "CommandOrControl+Minus"),
    ("zoom_reset", "CommandOrControl+0"),
];

pub fn default_of(id: &str) -> Option<&'static str> {
    if id == GLOBAL {
        return Some(crate::settings::DEFAULT_HOTKEY);
    }
    APP.iter().find(|(a, _)| *a == id).map(|(_, d)| *d)
}

/// What a page compares a keydown against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Binding {
    /// `KeyboardEvent.code` ("KeyH", "Slash", "Tab"): the same under the
    /// Korean IME and any keyboard layout.
    pub code: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

/// One shortcut as the host UI shows it.
#[derive(Debug, Clone, Serialize)]
pub struct KeyView {
    pub id: String,
    /// None: turned off.
    pub binding: Option<Binding>,
    /// Key caps for display ("Ctrl", "Shift", "H"). Empty: off.
    pub caps: Vec<String>,
    pub default_caps: Vec<String>,
    pub is_default: bool,
}

pub fn parse(accel: &str) -> Result<Shortcut, String> {
    accel
        .trim()
        .parse::<Shortcut>()
        .map_err(|e| format!("단축키 형식 오류 \"{accel}\": {e}"))
}

pub fn binding(sc: &Shortcut) -> Binding {
    Binding {
        code: sc.key.to_string(),
        ctrl: sc.mods.contains(Modifiers::CONTROL),
        alt: sc.mods.contains(Modifiers::ALT),
        shift: sc.mods.contains(Modifiers::SHIFT),
        meta: sc.mods.contains(Modifiers::SUPER),
    }
}

fn key_cap(code: Code) -> String {
    let s = code.to_string();
    if let Some(c) = s.strip_prefix("Key") {
        return c.to_string();
    }
    if let Some(c) = s.strip_prefix("Digit") {
        return c.to_string();
    }
    let sym = match code {
        Code::Slash => "/",
        Code::Backslash => "\\",
        Code::Comma => ",",
        Code::Period => ".",
        Code::Semicolon => ";",
        Code::Quote => "'",
        Code::BracketLeft => "[",
        Code::BracketRight => "]",
        Code::Backquote => "`",
        Code::Minus => "-",
        Code::Equal => "=",
        Code::ArrowUp => "↑",
        Code::ArrowDown => "↓",
        Code::ArrowLeft => "←",
        Code::ArrowRight => "→",
        Code::Escape => "Esc",
        _ => return s,
    };
    sym.to_string()
}

/// Key caps in the platform's usual modifier order.
pub fn caps(sc: &Shortcut) -> Vec<String> {
    let mac = cfg!(target_os = "macos");
    let mut out = vec![];
    let m = sc.mods;
    if m.contains(Modifiers::CONTROL) {
        out.push(if mac { "⌃" } else { "Ctrl" }.to_string());
    }
    if m.contains(Modifiers::ALT) {
        out.push(if mac { "⌥" } else { "Alt" }.to_string());
    }
    if m.contains(Modifiers::SHIFT) {
        out.push("Shift".to_string());
    }
    if m.contains(Modifiers::SUPER) {
        out.push(if mac { "⌘" } else { "Win" }.to_string());
    }
    out.push(key_cap(sc.key));
    out
}

/// "CommandOrControl+Alt+X" -> "Ctrl+Alt+X" / "⌥+⌘+X". Unparsable: as is.
pub fn display(accel: &str) -> String {
    match parse(accel) {
        Ok(sc) => caps(&sc).join("+"),
        Err(_) => accel.to_string(),
    }
}

/// Configured accelerator for `id`: None when turned off.
/// Missing in settings = default, "" = off.
pub fn configured<'a>(map: &'a BTreeMap<String, String>, id: &str) -> Option<&'a str> {
    match map.get(id).map(|s| s.trim()) {
        None => default_of(id),
        Some("") => None,
        Some(k) => Some(k),
    }
}

fn is_function_key(code: Code) -> bool {
    matches!(
        code,
        Code::F1 | Code::F2 | Code::F3 | Code::F4 | Code::F5 | Code::F6 | Code::F7 | Code::F8
            | Code::F9 | Code::F10 | Code::F11 | Code::F12 | Code::F13 | Code::F14 | Code::F15
            | Code::F16 | Code::F17 | Code::F18 | Code::F19 | Code::F20 | Code::F21 | Code::F22
            | Code::F23 | Code::F24
    )
}

/// The fixed panel focus keys: Ctrl+1..9 (Windows) / Cmd+1..9 (macOS).
fn is_panel_focus(sc: &Shortcut) -> bool {
    let main = if cfg!(target_os = "macos") { Modifiers::SUPER } else { Modifiers::CONTROL };
    let digit = matches!(
        sc.key,
        Code::Digit1 | Code::Digit2 | Code::Digit3 | Code::Digit4 | Code::Digit5 | Code::Digit6
            | Code::Digit7 | Code::Digit8 | Code::Digit9
    );
    digit && sc.mods == main
}

/// Checks a new accelerator for `id` against the rest of `map`.
/// Returns the parsed shortcut.
pub fn validate(map: &BTreeMap<String, String>, id: &str, accel: &str) -> Result<Shortcut, String> {
    let sc = parse(accel)?;
    let typing = Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER;
    // A bare key or Shift+key would fire while typing a post.
    if !sc.mods.intersects(typing) && !is_function_key(sc.key) {
        return Err("Ctrl·Alt·⌘ 중 하나를 같이 누르는 조합만 됨 (F1~F24는 단독 가능)".into());
    }
    if is_panel_focus(&sc) {
        return Err(format!("{}는 패널 전환(고정)에 쓰는 키임", caps(&sc).join("+")));
    }
    let ids = std::iter::once(GLOBAL).chain(APP.iter().map(|(a, _)| *a));
    for other in ids.filter(|o| *o != id) {
        let Some(k) = configured(map, other) else { continue };
        if let Ok(o) = parse(k) {
            if o.mods == sc.mods && o.key == sc.key {
                return Err(format!("{}는 이미 \"{}\"에 쓰는 중", caps(&sc).join("+"), label(other)));
            }
        }
    }
    Ok(sc)
}

pub fn label(id: &str) -> &'static str {
    match id {
        GLOBAL => "창 숨기기/불러오기",
        "next" => "다음 패널",
        "prev" => "이전 패널",
        "bar" => "상단 바 보이기/숨기기",
        "help" => "단축키 창",
        "capture" => "게시물 캡쳐",
        "send" => "다른 패널에서 열기",
        "zoom" => "패널 하나만 크게 보기",
        "settings" => "설정",
        "zoom_in" => "패널 배율 키우기",
        "zoom_out" => "패널 배율 줄이기",
        "zoom_reset" => "패널 배율 100%",
        _ => "?",
    }
}

/// Bindings of every in-app shortcut that is on, for panel.js and app.js.
pub fn bindings(map: &BTreeMap<String, String>) -> BTreeMap<String, Binding> {
    APP.iter()
        .filter_map(|(id, _)| {
            let k = configured(map, id)?;
            let sc = parse(k).ok()?;
            Some((id.to_string(), binding(&sc)))
        })
        .collect()
}

/// One shortcut for the settings screen and the shortcut overlay.
pub fn view(map: &BTreeMap<String, String>, id: &str) -> KeyView {
    let sc = configured(map, id).and_then(|k| parse(k).ok());
    KeyView {
        id: id.to_string(),
        binding: sc.as_ref().map(binding),
        caps: sc.as_ref().map(caps).unwrap_or_default(),
        default_caps: default_of(id).and_then(|d| parse(d).ok()).map(|s| caps(&s)).unwrap_or_default(),
        is_default: !map.contains_key(id),
    }
}

/// Every in-app shortcut, in display order.
pub fn views(map: &BTreeMap<String, String>) -> Vec<KeyView> {
    APP.iter().map(|(id, _)| view(map, id)).collect()
}

// ---- X's own shortcuts, remapped
//
// X handles its shortcuts in its own page code, so the app can't change
// them. What it can do: in panel.js, outside text fields, swallow the key
// the user picked and hand X the original key instead (a synthetic
// keydown/keyup with X's key), and swallow X's original key once it has
// been moved or turned off. Stored in settings.json `x_keys` by id:
// missing = X's own key, "" = off, else an accelerator ("F", "Shift+L").

/// One key X expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct XStep {
    /// `KeyboardEvent.key`.
    pub key: &'static str,
    /// `KeyboardEvent.code`.
    pub code: &'static str,
    /// Legacy `keyCode`/`which`, set too in case X still reads it.
    #[serde(rename = "keyCode")]
    pub key_code: u32,
    pub shift: bool,
}

const fn step(key: &'static str, code: &'static str, key_code: u32, shift: bool) -> XStep {
    XStep { key, code, key_code, shift }
}

const G: XStep = step("g", "KeyG", 71, false);

/// X actions that can be moved: (id, label, keys X expects, X's own key
/// as an accelerator). Sequences (g then h) have no accelerator: their
/// own keys stay with X, a new key only adds one.
/// Not here (fixed): Space (scrolls natively, a synthetic one wouldn't),
/// Enter (also clicks focused buttons), Ctrl/Cmd+Enter (inside the post
/// box, where the app never touches keys).
pub const X: [(&str, &str, &[XStep], Option<&str>); 15] = [
    ("x_next", "다음 게시물", &[step("j", "KeyJ", 74, false)], Some("J")),
    ("x_prev", "이전 게시물", &[step("k", "KeyK", 75, false)], Some("K")),
    ("x_load", "새 게시물 불러오기", &[step(".", "Period", 190, false)], Some("Period")),
    ("x_like", "마음에 들어요", &[step("l", "KeyL", 76, false)], Some("L")),
    ("x_reply", "답글", &[step("r", "KeyR", 82, false)], Some("R")),
    ("x_repost", "재게시", &[step("t", "KeyT", 84, false)], Some("T")),
    ("x_post", "새 게시물", &[step("n", "KeyN", 78, false)], Some("N")),
    ("x_search", "검색", &[step("/", "Slash", 191, false)], Some("Slash")),
    ("x_dm", "쪽지", &[step("m", "KeyM", 77, false)], Some("M")),
    ("x_home", "홈", &[G, step("h", "KeyH", 72, false)], None),
    ("x_notif", "알림", &[G, step("n", "KeyN", 78, false)], None),
    ("x_profile", "내 프로필", &[G, step("p", "KeyP", 80, false)], None),
    ("x_bookmarks", "북마크", &[G, step("b", "KeyB", 66, false)], None),
    ("x_dms", "쪽지함", &[G, step("m", "KeyM", 77, false)], None),
    ("x_help", "X 전체 단축키 목록", &[step("?", "Slash", 191, true)], Some("Shift+Slash")),
];

fn x_def(id: &str) -> Option<&'static (&'static str, &'static str, &'static [XStep], Option<&'static str>)> {
    X.iter().find(|x| x.0 == id)
}

/// The key that does `id` in a panel now: the user's, else X's own for a
/// single key. None: off, or a sequence left to X.
fn x_effective(map: &BTreeMap<String, String>, id: &str) -> Option<Shortcut> {
    let (_, _, _, def) = x_def(id)?;
    match map.get(id).map(|s| s.trim()) {
        Some("") => None,
        Some(k) => parse(k).ok(),
        None => def.and_then(|d| parse(d).ok()),
    }
}

/// Which X action (label) uses `sc` now, `except` left out.
pub fn x_taken(map: &BTreeMap<String, String>, sc: &Shortcut, except: Option<&str>) -> Option<&'static str> {
    X.iter()
        .filter(|x| Some(x.0) != except)
        .find(|x| x_effective(map, x.0).is_some_and(|o| o.mods == sc.mods && o.key == sc.key))
        .map(|x| x.1)
}

/// Checks a new key for X action `id` against the app's shortcuts
/// (`app`, with the global one) and the other X actions (`xmap`).
pub fn validate_x(
    app: &BTreeMap<String, String>,
    xmap: &BTreeMap<String, String>,
    id: &str,
    accel: &str,
) -> Result<Shortcut, String> {
    let sc = parse(accel)?;
    let shown = || x_caps(&sc).join("+");
    if sc.key == Code::Escape {
        return Err("Esc는 못 씀".into());
    }
    if sc.key == Code::KeyG && sc.mods.is_empty() {
        return Err("g는 X 연속키(g → h 등)의 첫 키라 못 씀".into());
    }
    let main = if cfg!(target_os = "macos") { Modifiers::SUPER } else { Modifiers::CONTROL };
    if sc.key == Code::KeyV && sc.mods == main {
        return Err(format!("{}는 복사한 링크 열기(고정)에 쓰는 키임", shown()));
    }
    if is_panel_focus(&sc) {
        return Err(format!("{}는 패널 전환(고정)에 쓰는 키임", shown()));
    }
    let ids = std::iter::once(GLOBAL).chain(APP.iter().map(|(a, _)| *a));
    for other in ids {
        let Some(k) = configured(app, other) else { continue };
        if let Ok(o) = parse(k) {
            if o.mods == sc.mods && o.key == sc.key {
                return Err(format!("{}는 이미 \"{}\"에 쓰는 중", shown(), label(other)));
            }
        }
    }
    if let Some(l) = x_taken(xmap, &sc, Some(id)) {
        return Err(format!("{}는 이미 X \"{}\"에 쓰는 중", shown(), l));
    }
    Ok(sc)
}

/// Key caps the way X's own keys are written: "j", "?", "Shift+L".
pub fn x_caps(sc: &Shortcut) -> Vec<String> {
    if sc.key == Code::Slash && sc.mods == Modifiers::SHIFT {
        return vec!["?".into()];
    }
    let mut c = caps(sc);
    if sc.mods.is_empty() {
        if let Some(last) = c.last_mut() {
            if last.len() == 1 {
                *last = last.to_lowercase();
            }
        }
    }
    c
}

fn steps_caps(steps: &[XStep]) -> Vec<String> {
    let mut out = vec![];
    for (i, s) in steps.iter().enumerate() {
        if i > 0 {
            out.push("then".into());
        }
        out.push(s.key.to_string());
    }
    out
}

/// One X action for the settings screen and the shortcut overlay.
#[derive(Debug, Clone, Serialize)]
pub struct XKeyView {
    pub id: String,
    pub label: String,
    /// What does it now. "then" between the keys of a sequence. Empty: off.
    pub caps: Vec<String>,
    /// X's own key(s).
    pub default_caps: Vec<String>,
    pub is_default: bool,
    /// A sequence: X's own keys keep working, so it can't be turned off.
    pub seq: bool,
}

pub fn x_views(map: &BTreeMap<String, String>) -> Vec<XKeyView> {
    X.iter()
        .map(|&(id, label, steps, def)| {
            let default_caps = steps_caps(steps);
            let caps = match map.get(id).map(|s| s.trim()) {
                None => default_caps.clone(),
                Some("") if def.is_some() => vec![],
                Some(k) => parse(k).map(|sc| x_caps(&sc)).unwrap_or_else(|_| default_caps.clone()),
            };
            XKeyView {
                id: id.into(),
                label: label.into(),
                caps,
                default_caps,
                is_default: !map.contains_key(id),
                seq: def.is_none(),
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct XRemap {
    /// The key the user presses.
    pub binding: Binding,
    /// What X gets instead.
    pub send: Vec<XStep>,
}

/// What panel.js needs: keys to turn into X's keys, and X's own keys to
/// swallow (moved elsewhere or off).
#[derive(Debug, Clone, Default, Serialize)]
pub struct XKeys {
    pub remap: Vec<XRemap>,
    pub block: Vec<Binding>,
}

pub fn x_keys(map: &BTreeMap<String, String>) -> XKeys {
    let mut out = XKeys::default();
    for &(id, _, steps, def) in X.iter() {
        let Some(k) = map.get(id).map(|s| s.trim()) else { continue };
        if !k.is_empty() {
            if let Ok(sc) = parse(k) {
                out.remap.push(XRemap { binding: binding(&sc), send: steps.to_vec() });
            }
        }
        if let Some(d) = def.and_then(|d| parse(d).ok()) {
            out.block.push(binding(&d));
        }
    }
    // A key that was moved onto another action's old key is that action's
    // now (panel.js checks `remap` first), so leave it out of `block`.
    let remapped: Vec<Binding> = out.remap.iter().map(|r| r.binding.clone()).collect();
    out.block.retain(|b| !remapped.contains(b));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_parse_and_differ() {
        let map = BTreeMap::new();
        for (id, def) in APP {
            let sc = validate(&map, id, def).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert!(!is_panel_focus(&sc));
        }
        assert!(validate(&map, GLOBAL, crate::settings::DEFAULT_HOTKEY).is_ok());
    }

    #[test]
    fn binding_codes() {
        let b = binding(&parse("CommandOrControl+Shift+H").unwrap());
        assert_eq!(b.code, "KeyH");
        assert!(b.shift && !b.alt);
        assert_eq!(b.ctrl, !cfg!(target_os = "macos"));
        assert_eq!(b.meta, cfg!(target_os = "macos"));
        assert_eq!(binding(&parse("CommandOrControl+Slash").unwrap()).code, "Slash");
        assert_eq!(binding(&parse("Control+Tab").unwrap()).code, "Tab");
        assert_eq!(binding(&parse("Alt+5").unwrap()).code, "Digit5");
        assert_eq!(binding(&parse("Control+F5").unwrap()).code, "F5");
        assert_eq!(binding(&parse("CommandOrControl+Equal").unwrap()).code, "Equal");
        assert_eq!(binding(&parse("CommandOrControl+Minus").unwrap()).code, "Minus");
        assert_eq!(binding(&parse("CommandOrControl+0").unwrap()).code, "Digit0");
    }

    #[test]
    fn caps_display() {
        let c = caps(&parse("CommandOrControl+Slash").unwrap());
        assert_eq!(c.last().unwrap(), "/");
        if !cfg!(target_os = "macos") {
            assert_eq!(display("CommandOrControl+Alt+X"), "Ctrl+Alt+X");
            assert_eq!(display("Control+Shift+Tab"), "Ctrl+Shift+Tab");
        }
    }

    #[test]
    fn rejects() {
        let map = BTreeMap::new();
        assert!(validate(&map, "bar", "H").is_err());
        assert!(validate(&map, "bar", "Shift+H").is_err());
        assert!(validate(&map, "bar", "F7").is_ok());
        assert!(validate(&map, "bar", "CommandOrControl+3").is_err());
        assert!(validate(&map, "bar", "CommandOrControl+Alt+3").is_ok());
        assert!(validate(&map, "bar", "CommandOrControl+Nope").is_err());
        // Taken by another shortcut (default help key).
        let e = validate(&map, "bar", "CommandOrControl+Slash").unwrap_err();
        assert!(e.contains("단축키 창"), "{e}");
        // ... unless that one is turned off.
        let mut off = BTreeMap::new();
        off.insert("help".to_string(), String::new());
        assert!(validate(&off, "bar", "CommandOrControl+Slash").is_ok());
        // Global vs app collision both ways.
        assert!(validate(&map, "bar", crate::settings::DEFAULT_HOTKEY).is_err());
        assert!(validate(&map, GLOBAL, "Control+Tab").is_err());
    }

    #[test]
    fn configured_semantics() {
        let mut m = BTreeMap::new();
        assert_eq!(configured(&m, "bar"), Some("CommandOrControl+Shift+H"));
        m.insert("bar".into(), " ".into());
        assert_eq!(configured(&m, "bar"), None);
        m.insert("bar".into(), "Alt+B".into());
        assert_eq!(configured(&m, "bar"), Some("Alt+B"));
        assert_eq!(bindings(&m)["bar"].code, "KeyB");
    }

    #[test]
    fn x_defaults_parse_and_differ() {
        let none = BTreeMap::new();
        for (id, _, steps, def) in X {
            assert!(!steps.is_empty(), "{id}");
            if let Some(d) = def {
                let sc = parse(d).unwrap_or_else(|e| panic!("{id}: {e}"));
                assert_eq!(binding(&sc).code, steps[0].code, "{id}");
                assert_eq!(binding(&sc).shift, steps[0].shift, "{id}");
                assert_eq!(x_taken(&none, &sc, Some(id)), None, "{id}");
            }
        }
        assert!(x_keys(&none).remap.is_empty() && x_keys(&none).block.is_empty());
        assert_eq!(x_views(&none)[0].caps, vec!["j"]);
        assert_eq!(x_views(&none).last().unwrap().caps, vec!["?"]);
        assert_eq!(x_views(&none)[9].caps, vec!["g", "then", "h"]);
    }

    #[test]
    fn x_validate() {
        let app = BTreeMap::new();
        let mut x = BTreeMap::new();
        // Free key.
        assert!(validate_x(&app, &x, "x_like", "F").is_ok());
        // Another X action's key, until that one moves.
        let e = validate_x(&app, &x, "x_like", "R").unwrap_err();
        assert!(e.contains("답글"), "{e}");
        x.insert("x_reply".to_string(), "E".to_string());
        assert!(validate_x(&app, &x, "x_like", "R").is_ok());
        assert!(validate_x(&app, &x, "x_like", "E").is_err());
        // Own key back is fine.
        assert!(validate_x(&app, &x, "x_like", "L").is_ok());
        // App keys and fixed keys.
        assert!(validate_x(&app, &x, "x_like", "CommandOrControl+Slash").is_err());
        assert!(validate_x(&app, &x, "x_like", "CommandOrControl+3").is_err());
        assert!(validate_x(&app, &x, "x_like", "CommandOrControl+V").is_err());
        assert!(validate_x(&app, &x, "x_like", "G").is_err());
        assert!(validate_x(&app, &x, "x_like", "Shift+G").is_ok());
        assert!(validate_x(&app, &x, "x_like", "Escape").is_err());
        // Sequences: a single key is fine, X's keys aren't taken.
        assert!(validate_x(&app, &x, "x_home", "H").is_ok());
    }

    #[test]
    fn x_payload() {
        let mut x = BTreeMap::new();
        // Swap j and k, turn like off, home on h.
        x.insert("x_next".to_string(), "K".to_string());
        x.insert("x_prev".to_string(), "J".to_string());
        x.insert("x_like".to_string(), String::new());
        x.insert("x_home".to_string(), "H".to_string());
        let p = x_keys(&x);
        let r: Vec<(&str, Vec<&str>)> =
            p.remap.iter().map(|r| (r.binding.code.as_str(), r.send.iter().map(|s| s.key).collect())).collect();
        assert_eq!(r, vec![("KeyK", vec!["j"]), ("KeyJ", vec!["k"]), ("KeyH", vec!["g", "h"])]);
        // j and k are remapped, so only l is swallowed.
        let b: Vec<&str> = p.block.iter().map(|b| b.code.as_str()).collect();
        assert_eq!(b, vec!["KeyL"]);
        let v = x_views(&x);
        assert_eq!(v[0].caps, vec!["k"]);
        assert!(v[3].caps.is_empty() && !v[3].is_default);
        assert_eq!(v[9].caps, vec!["h"]);
        assert!(v[9].seq);
    }
}
