//! settings.json in app_config_dir. Written atomically (tmp file + rename).

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io, path::Path};
use uuid::Uuid;

pub const SETTINGS_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: Uuid,
    pub label: String,
    pub color: String,
    /// Page zoom of this panel (1.0 = 100%). Missing in older files = 1.0.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub zoom: f64,
}

fn one() -> f64 {
    1.0
}

fn is_one(z: &f64) -> bool {
    (*z - 1.0).abs() < 1e-9
}

impl Account {
    pub fn new(label: impl Into<String>, color: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            label: label.into(),
            color: color.into(),
            zoom: 1.0,
        }
    }

    /// 16-byte store id. macOS: WKWebsiteDataStore identifier.
    /// Windows (patched runtime): hex of these bytes = WebView2 profile name.
    pub fn store_id(&self) -> [u8; 16] {
        *self.id.as_bytes()
    }

    /// WebView2 profile name the patched runtime derives from `store_id`.
    #[allow(dead_code)]
    pub fn profile_name(&self) -> String {
        self.store_id().iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub version: u32,
    pub accounts: Vec<Account>,
    /// Relative panel widths, one per account. Normalised on load.
    pub ratios: Vec<f64>,
    /// Windows only: profiles to delete on next start, before any webview
    /// exists (the runtime keeps the folder locked while running).
    #[serde(default)]
    pub pending_profile_deletes: Vec<String>,
    /// macOS only: WKWebsiteDataStores whose removal failed (or hasn't run
    /// yet). Retried on every start until it succeeds.
    #[serde(default)]
    pub pending_store_deletes: Vec<Uuid>,
    /// Top bar (account names, reload/delete, + account) shown. Hidden by
    /// default; Ctrl/Cmd+Shift+H or a click on the top strip shows it.
    #[serde(default)]
    pub show_bar: bool,
    /// Global show/hide shortcut, Tauri accelerator syntax
    /// ("CommandOrControl+Alt+X"). Missing = DEFAULT_HOTKEY, "" = off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotkey: Option<String>,
    /// In-app shortcuts by id (see keys::APP), accelerator syntax.
    /// Missing id = default, "" = off.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keys: BTreeMap<String, String>,
    /// X's own shortcuts moved to other keys, by id (see keys::X).
    /// Missing id = X's own key, "" = off.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub x_keys: BTreeMap<String, String>,
    /// Folder for downloaded media and captures. Missing = <Downloads>/Paneful.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_dir: Option<String>,
    /// Look for a newer release on GitHub at start and every few hours.
    #[serde(default = "yes")]
    pub update_check: bool,
    /// Release the user chose to skip ("0.0.5"): no notice for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_version: Option<String>,
    /// Download fixes to the X screen rules (rules.rs) from the repository.
    #[serde(default = "yes")]
    pub rules_update: bool,
    /// Red "!" on the tray icon and the taskbar button (Windows) / number on
    /// the Dock and menu bar icons (macOS) while something is unread. The
    /// window title's "(3)" goes with it. Off: counts only in the tooltip.
    #[serde(default = "yes")]
    pub unread_badge: bool,
    /// System notification (Notification Center / toast) when a panel's
    /// 알림 or 쪽지 count goes up (notify.rs).
    #[serde(default = "yes")]
    pub notify: bool,
    /// Ctrl + wheel (Windows) / Cmd + wheel (macOS) zooms a panel. Off by
    /// default on macOS: with Cmd held for another shortcut, a brush of the
    /// trackpad is a wheel and ran the zoom up and down. The keys stay.
    #[serde(default = "wheel_zoom_default")]
    pub wheel_zoom: bool,
}

pub fn wheel_zoom_default() -> bool {
    !cfg!(target_os = "macos")
}

fn yes() -> bool {
    true
}

/// Ctrl+Alt+X (Windows) / Cmd+Option+X (macOS). Not an X shortcut (those
/// are bare keys and Ctrl/Cmd+Enter) and not one of the app's own.
pub const DEFAULT_HOTKEY: &str = "CommandOrControl+Alt+X";

/// Whether settings.json may be written back after `load`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Persist {
    /// File missing, fine, or recovered: saving is safe.
    Yes,
    /// File exists but could not be read (permissions, locked, ...).
    /// Saving would overwrite data we never saw, so don't.
    No,
}

pub const COLORS: [&str; 6] = ["#1d9bf0", "#f91880", "#00ba7c", "#ffd400", "#7856ff", "#ff7a00"];

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            accounts: vec![
                Account::new("계정 1", COLORS[0]),
                Account::new("계정 2", COLORS[1]),
            ],
            ratios: vec![1.0, 1.0],
            pending_profile_deletes: vec![],
            pending_store_deletes: vec![],
            show_bar: false,
            hotkey: None,
            keys: BTreeMap::new(),
            x_keys: BTreeMap::new(),
            save_dir: None,
            update_check: true,
            skip_version: None,
            rules_update: true,
            unread_badge: true,
            notify: true,
            wheel_zoom: wheel_zoom_default(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> (Self, Persist) {
        let (mut s, persist) = match fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(s) => (s, Persist::Yes),
                Err(e) => {
                    let _ = fs::copy(path, path.with_extension("json.bak"));
                    match Self::salvage(&text) {
                        Some(s) => {
                            log::warn!("settings.json damaged ({e}), backup kept, accounts recovered");
                            (s, Persist::Yes)
                        }
                        None => {
                            log::warn!("settings.json unreadable ({e}), backup kept, using defaults");
                            (Settings::default(), Persist::Yes)
                        }
                    }
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => (Settings::default(), Persist::Yes),
            Err(e) => {
                log::error!("cannot read settings.json ({e}); running on defaults without saving");
                (Settings::default(), Persist::No)
            }
        };
        s.migrate();
        s.fix_ratios();
        for a in &mut s.accounts {
            a.zoom = crate::zoom::clamp(a.zoom);
        }
        (s, persist)
    }

    /// Valid JSON with a bad field somewhere: keep every account that still
    /// parses, so their ids (= login data stores) are not orphaned.
    fn salvage(text: &str) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let accounts: Vec<Account> = v
            .get("accounts")?
            .as_array()?
            .iter()
            .filter_map(|a| serde_json::from_value(a.clone()).ok())
            .collect();
        if accounts.is_empty() {
            return None;
        }
        fn field<T: serde::de::DeserializeOwned + Default>(v: &serde_json::Value, k: &str) -> T {
            v.get(k)
                .and_then(|x| serde_json::from_value(x.clone()).ok())
                .unwrap_or_default()
        }
        Some(Settings {
            version: SETTINGS_VERSION,
            accounts,
            ratios: field(&v, "ratios"),
            pending_profile_deletes: field(&v, "pending_profile_deletes"),
            pending_store_deletes: field(&v, "pending_store_deletes"),
            show_bar: field(&v, "show_bar"),
            hotkey: field(&v, "hotkey"),
            keys: field(&v, "keys"),
            x_keys: field(&v, "x_keys"),
            save_dir: field(&v, "save_dir"),
            update_check: v.get("update_check").and_then(|x| x.as_bool()).unwrap_or(true),
            skip_version: field(&v, "skip_version"),
            rules_update: v.get("rules_update").and_then(|x| x.as_bool()).unwrap_or(true),
            unread_badge: v.get("unread_badge").and_then(|x| x.as_bool()).unwrap_or(true),
            notify: v.get("notify").and_then(|x| x.as_bool()).unwrap_or(true),
            wheel_zoom: v.get("wheel_zoom").and_then(|x| x.as_bool()).unwrap_or_else(wheel_zoom_default),
        })
    }

    fn migrate(&mut self) {
        // v1 is the first version. Future migrations go here.
        self.version = SETTINGS_VERSION;
    }

    /// Shortcut to register, or None when turned off.
    pub fn hotkey(&self) -> Option<&str> {
        match self.hotkey.as_deref().map(str::trim) {
            None => Some(DEFAULT_HOTKEY),
            Some("") => None,
            Some(k) => Some(k),
        }
    }

    /// Every shortcut by id, the global one included under keys::GLOBAL.
    pub fn key_map(&self) -> BTreeMap<String, String> {
        let mut m = self.keys.clone();
        if let Some(k) = &self.hotkey {
            m.insert(crate::keys::GLOBAL.into(), k.clone());
        }
        m
    }

    pub fn fix_ratios(&mut self) {
        let n = self.accounts.len();
        if self.ratios.len() != n || self.ratios.iter().any(|r| !r.is_finite() || *r <= 0.0) {
            self.ratios = vec![1.0; n];
        }
        let sum: f64 = self.ratios.iter().sum();
        if sum > 0.0 {
            for r in &mut self.ratios {
                *r /= sum;
            }
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        fs::write(&tmp, text)?;
        fs::rename(&tmp, path)
    }
}

/// Move panel `from` to place `to` (clamped), with its width share.
/// false: nothing moved.
pub fn move_panel(s: &mut Settings, from: usize, to: usize) -> bool {
    let n = s.accounts.len();
    if from >= n || n < 2 {
        return false;
    }
    let to = to.min(n - 1);
    if to == from {
        return false;
    }
    let acc = s.accounts.remove(from);
    s.accounts.insert(to, acc);
    if s.ratios.len() == n {
        let r = s.ratios.remove(from);
        s.ratios.insert(to, r);
    } else {
        s.fix_ratios();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_keeps_widths() {
        let mut s = Settings::default();
        s.accounts.push(Account::new("계정 3", COLORS[2]));
        s.ratios = vec![0.5, 0.3, 0.2];
        let ids: Vec<Uuid> = s.accounts.iter().map(|a| a.id).collect();
        assert!(move_panel(&mut s, 0, 2));
        assert_eq!(s.accounts.iter().map(|a| a.id).collect::<Vec<_>>(), vec![ids[1], ids[2], ids[0]]);
        assert_eq!(s.ratios, vec![0.3, 0.2, 0.5]);
        assert!(!move_panel(&mut s, 2, 99));
        assert!(move_panel(&mut s, 1, 0));
        assert_eq!(s.accounts[0].id, ids[2]);
        assert!(!move_panel(&mut s, 5, 0));
    }

    #[test]
    fn ratios_normalised() {
        let mut s = Settings::default();
        s.ratios = vec![3.0, 1.0];
        s.fix_ratios();
        assert!((s.ratios[0] - 0.75).abs() < 1e-9);
        s.ratios = vec![1.0];
        s.fix_ratios();
        assert_eq!(s.ratios, vec![0.5, 0.5]);
    }

    #[test]
    fn profile_name_is_32_hex() {
        let a = Account::new("a", "#000");
        let p = a.profile_name();
        assert_eq!(p.len(), 32);
        assert_eq!(p, a.id.simple().to_string());
    }

    #[test]
    fn roundtrip() {
        let dir = std::env::temp_dir().join(format!("paneful-test-{}", Uuid::new_v4()));
        let path = dir.join("settings.json");
        let s = Settings::default();
        s.save(&path).unwrap();
        let (l, persist) = Settings::load(&path);
        assert_eq!(persist, Persist::Yes);
        assert_eq!(l.accounts[0].id, s.accounts[0].id);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn salvage_keeps_accounts() {
        let dir = std::env::temp_dir().join(format!("paneful-test-{}", Uuid::new_v4()));
        let path = dir.join("settings.json");
        let s = Settings::default();
        let mut v = serde_json::to_value(&s).unwrap();
        v["ratios"] = serde_json::json!("broken");
        v["accounts"].as_array_mut().unwrap().push(serde_json::json!({"id": "nope"}));
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, v.to_string()).unwrap();
        let (l, persist) = Settings::load(&path);
        assert_eq!(persist, Persist::Yes);
        assert_eq!(l.accounts.len(), 2);
        assert_eq!(l.accounts[1].id, s.accounts[1].id);
        assert_eq!(l.ratios.len(), 2);
        assert!(path.with_extension("json.bak").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_file_is_writable() {
        let path = std::env::temp_dir().join(format!("paneful-missing-{}.json", Uuid::new_v4()));
        let (s, persist) = Settings::load(&path);
        assert_eq!(persist, Persist::Yes);
        assert_eq!(s.accounts.len(), 2);
    }

    #[test]
    fn hotkey_default_and_off() {
        let mut s = Settings::default();
        assert_eq!(s.hotkey(), Some(DEFAULT_HOTKEY));
        s.hotkey = Some(" ".into());
        assert_eq!(s.hotkey(), None);
        s.hotkey = Some("Ctrl+Shift+F12".into());
        assert_eq!(s.hotkey(), Some("Ctrl+Shift+F12"));
        // Old settings.json without the key still loads.
        let v = serde_json::json!({"version": 1, "accounts": [], "ratios": []});
        let l: Settings = serde_json::from_value(v).unwrap();
        assert_eq!(l.hotkey(), Some(DEFAULT_HOTKEY));
        assert!(l.keys.is_empty() && l.save_dir.is_none());
        assert!(l.update_check && l.skip_version.is_none());
        assert!(l.rules_update);
        assert!(l.unread_badge);
        assert!(l.notify);
        assert_eq!(l.wheel_zoom, wheel_zoom_default());
    }

    #[test]
    fn zoom_default_and_kept() {
        // Old account entries have no zoom.
        let a: Account = serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(), "label": "a", "color": "#000"
        }))
        .unwrap();
        assert_eq!(a.zoom, 1.0);
        // 100% isn't written, anything else is.
        assert!(serde_json::to_value(&a).unwrap().get("zoom").is_none());
        let b = Account { zoom: 0.8, ..a };
        assert_eq!(serde_json::to_value(&b).unwrap()["zoom"], 0.8);
    }

    #[test]
    fn key_map_has_global() {
        let mut s = Settings::default();
        assert!(!s.key_map().contains_key(crate::keys::GLOBAL));
        s.hotkey = Some(String::new());
        s.keys.insert("bar".into(), "Alt+B".into());
        let m = s.key_map();
        assert_eq!(m[crate::keys::GLOBAL], "");
        assert_eq!(m["bar"], "Alt+B");
    }
}
