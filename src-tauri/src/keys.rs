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
pub const APP: [(&str, &str); 5] = [
    ("next", "Control+Tab"),
    ("prev", "Control+Shift+Tab"),
    ("bar", "CommandOrControl+Shift+H"),
    ("help", "CommandOrControl+Slash"),
    ("settings", "CommandOrControl+Comma"),
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
        "settings" => "설정",
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
}
