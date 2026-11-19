//! Windows portable mode: settings, log and WebView2 data next to the exe.
//!
//! The portable zip ships `portable.txt` beside `Paneful.exe`. When that
//! file is there, everything goes to `<exe dir>/data`:
//!
//! - `data/settings.json`
//! - `data/logs/paneful.log`
//! - `data/EBWebView/...` (WebView2 user data folder, one profile per account)
//!
//! Without the marker (installer, macOS) nothing changes: AppData as before.
//! If the marker is there but `data` can't be written (Program Files, a
//! read-only stick), the app falls back to AppData and says so in a dialog
//! and in settings.
//!
//! WebView2 encrypts cookies with a key locked by Windows DPAPI to the
//! Windows user. On another PC or another Windows account the folder still
//! loads (settings, panels) but every account has to log in again.
//!
//! First portable start: if `data` has no settings.json yet and AppData has
//! one, the AppData data is copied (never moved or deleted), so a user who
//! ran an older portable build or the installer keeps their accounts, and on
//! the same PC and Windows account also their logins.

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::OnceLock,
};

/// Marker file next to the exe that turns portable mode on.
pub const MARKER: &str = "portable.txt";
/// Data folder next to the exe.
pub const DATA: &str = "data";
/// WebView2's folder inside a user data folder.
const EBWEBVIEW: &str = "EBWebView";

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Mode {
    /// AppData (installer, macOS, no marker).
    Installed,
    /// Data in this folder next to the exe.
    Portable(PathBuf),
    /// Marker found but the folder can't be written: AppData is used.
    Fallback { dir: PathBuf, error: String },
}

static MODE: OnceLock<Mode> = OnceLock::new();

/// Decide once, before the Tauri builder (the log target depends on it).
pub fn init() -> &'static Mode {
    MODE.get_or_init(|| {
        #[cfg(windows)]
        {
            match std::env::current_exe() {
                Ok(exe) => match exe.parent() {
                    Some(dir) => detect(dir),
                    None => Mode::Installed,
                },
                Err(_) => Mode::Installed,
            }
        }
        #[cfg(not(windows))]
        {
            Mode::Installed
        }
    })
}

pub fn mode() -> &'static Mode {
    init()
}

/// Portable data folder, if portable mode is on and usable.
pub fn root() -> Option<&'static Path> {
    match mode() {
        Mode::Portable(dir) => Some(dir),
        _ => None,
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn detect(exe_dir: &Path) -> Mode {
    if !exe_dir.join(MARKER).is_file() {
        return Mode::Installed;
    }
    let dir = exe_dir.join(DATA);
    match writable(&dir) {
        Ok(()) => Mode::Portable(dir),
        Err(e) => Mode::Fallback { dir, error: e.to_string() },
    }
}

/// Create the folder and write/remove a probe file in it.
#[cfg_attr(not(windows), allow(dead_code))]
fn writable(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let probe = dir.join(".write-test");
    fs::write(&probe, b"ok")?;
    fs::remove_file(&probe)
}

/// What `migrate` did, for the log.
#[derive(Debug, PartialEq)]
pub enum Migration {
    /// data already has settings, or AppData has nothing to bring.
    Skipped,
    /// settings.json copied; `profiles` says whether EBWebView came too.
    Copied { profiles: Result<bool, String> },
    Failed(String),
}

/// First portable start: copy AppData's settings.json and WebView2 folder
/// into `root`. Never touches the AppData side. Runs before any webview
/// exists and with no lock held.
pub fn migrate(root: &Path, old_settings: &Path, old_udf: &Path) -> Migration {
    let new_settings = root.join("settings.json");
    if new_settings.exists() || !old_settings.is_file() {
        return Migration::Skipped;
    }
    // Profiles first: settings.json in place marks the migration done, so a
    // crash mid-copy retries on the next start.
    let profiles = copy_profiles(&old_udf.join(EBWEBVIEW), &root.join(EBWEBVIEW));
    let tmp = root.join("settings.json.tmp");
    match fs::copy(old_settings, &tmp).and_then(|_| fs::rename(&tmp, &new_settings)) {
        Ok(()) => Migration::Copied { profiles },
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Migration::Failed(e.to_string())
        }
    }
}

/// Ok(false): nothing to copy (no old folder, or data already has one).
fn copy_profiles(from: &Path, to: &Path) -> Result<bool, String> {
    if !from.is_dir() || to.exists() {
        return Ok(false);
    }
    // Copy beside the target and rename at the end, so a failed copy never
    // leaves a half profile that WebView2 would open. The partial folder is
    // our own copy in `data`, so removing it deletes nothing of the user's.
    let partial = to.with_extension("copying");
    let _ = fs::remove_dir_all(&partial);
    match copy_tree(from, &partial).and_then(|_| fs::rename(&partial, to)) {
        Ok(()) => Ok(true),
        Err(e) => {
            let _ = fs::remove_dir_all(&partial);
            Err(e.to_string())
        }
    }
}

/// Rebuilt by WebView2 on demand, so not worth carrying over (추론: Chromium
/// recreates its cache folders when missing). Also skips the lock file of a
/// running instance.
fn skip(name: &str) -> bool {
    matches!(
        name,
        "Cache"
            | "Code Cache"
            | "GPUCache"
            | "CacheStorage"
            | "ScriptCache"
            | "ShaderCache"
            | "GrShaderCache"
            | "GraphiteDawnCache"
            | "DawnCache"
            | "DawnGraphiteCache"
            | "DawnWebGPUCache"
            | "Crashpad"
            | "lockfile"
    )
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if skip(&name.to_string_lossy()) {
            continue;
        }
        let kind = entry.file_type()?;
        let dest = to.join(&name);
        if kind.is_dir() {
            copy_tree(&entry.path(), &dest)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &dest)?;
        }
        // symlinks: WebView2 doesn't make any; skip rather than follow.
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("paneful-portable-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn no_marker_is_installed() {
        let d = tmp("nomarker");
        assert_eq!(detect(&d), Mode::Installed);
        assert!(!d.join(DATA).exists());
    }

    #[test]
    fn marker_uses_data_next_to_exe() {
        let d = tmp("marker");
        fs::write(d.join(MARKER), "").unwrap();
        assert_eq!(detect(&d), Mode::Portable(d.join(DATA)));
        assert!(d.join(DATA).is_dir());
        assert!(!d.join(DATA).join(".write-test").exists());
    }

    #[test]
    fn unwritable_falls_back() {
        let d = tmp("ro");
        fs::write(d.join(MARKER), "").unwrap();
        // A file where the folder should be: create_dir_all fails.
        fs::write(d.join(DATA), "").unwrap();
        assert!(matches!(detect(&d), Mode::Fallback { .. }));
    }

    #[test]
    fn migrate_copies_settings_and_profiles_without_caches() {
        let d = tmp("migrate");
        let (cfg, udf, root) = (d.join("roaming"), d.join("local"), d.join("data"));
        fs::create_dir_all(&cfg).unwrap();
        fs::create_dir_all(&root).unwrap();
        fs::write(cfg.join("settings.json"), "{\"accounts\":[]}").unwrap();
        let prof = udf.join(EBWEBVIEW).join("abc");
        fs::create_dir_all(prof.join("Network")).unwrap();
        fs::create_dir_all(prof.join("Cache").join("Cache_Data")).unwrap();
        fs::write(prof.join("Network").join("Cookies"), "c").unwrap();
        fs::write(prof.join("Cache").join("Cache_Data").join("x"), "big").unwrap();
        fs::write(udf.join(EBWEBVIEW).join("Local State"), "k").unwrap();
        fs::write(udf.join(EBWEBVIEW).join("lockfile"), "").unwrap();

        let r = migrate(&root, &cfg.join("settings.json"), &udf);
        assert_eq!(r, Migration::Copied { profiles: Ok(true) });
        let new = root.join(EBWEBVIEW);
        assert_eq!(fs::read_to_string(root.join("settings.json")).unwrap(), "{\"accounts\":[]}");
        assert_eq!(fs::read_to_string(new.join("abc/Network/Cookies")).unwrap(), "c");
        assert_eq!(fs::read_to_string(new.join("Local State")).unwrap(), "k");
        assert!(!new.join("abc/Cache").exists());
        assert!(!new.join("lockfile").exists());
        assert!(!root.join("EBWebView.copying").exists());
        // Old side untouched.
        assert!(cfg.join("settings.json").exists());
        assert!(prof.join("Cache/Cache_Data/x").exists());

        // Second start: data has settings, nothing happens.
        assert_eq!(migrate(&root, &cfg.join("settings.json"), &udf), Migration::Skipped);
    }

    #[test]
    fn migrate_skips_without_old_settings() {
        let d = tmp("fresh");
        let root = d.join("data");
        fs::create_dir_all(&root).unwrap();
        assert_eq!(migrate(&root, &d.join("none/settings.json"), &d.join("none")), Migration::Skipped);
        assert!(!root.join("settings.json").exists());
    }

    #[test]
    fn migrate_without_profiles_still_copies_settings() {
        let d = tmp("noprof");
        let root = d.join("data");
        fs::create_dir_all(&root).unwrap();
        fs::write(d.join("settings.json"), "{}").unwrap();
        let r = migrate(&root, &d.join("settings.json"), &d.join("nolocal"));
        assert_eq!(r, Migration::Copied { profiles: Ok(false) });
        assert!(root.join("settings.json").exists());
    }
}
