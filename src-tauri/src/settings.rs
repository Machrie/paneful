//! settings.json in app_config_dir. Written atomically (tmp file + rename).

use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};
use uuid::Uuid;

pub const SETTINGS_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: Uuid,
    pub label: String,
    pub color: String,
}

impl Account {
    pub fn new(label: impl Into<String>, color: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            label: label.into(),
            color: color.into(),
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
}

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
        })
    }

    fn migrate(&mut self) {
        // v1 is the first version. Future migrations go here.
        self.version = SETTINGS_VERSION;
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
