//! System notifications (Notification Center on macOS, a toast on
//! Windows) when a panel's 알림 or 쪽지 count goes up. The counts are the
//! ones X already shows (unread.rs), so the notification can only say how
//! many, not who did what: nothing is requested from X for this.
//!
//! Not shown when the count only appeared because the page just loaded
//! (panel.js says so), while the app window is in front (the tabs show the
//! counts already), or more often than once per GAP per account (a post
//! going viral would otherwise ring for every like). A click on it brings
//! the window back and makes that panel active. Can be turned off in
//! settings.

use std::{
    collections::BTreeMap,
    sync::{atomic::AtomicUsize, Mutex},
    time::{Duration, Instant},
};

use tauri::{AppHandle, Runtime};
use uuid::Uuid;

use crate::unread::Unread;

/// At most one notification per account in this time.
pub const GAP: Duration = Duration::from_secs(30);

/// Notifications whose click is waited for, each on a thread of its own
/// (on macOS also a 0.5 s poll on the main thread while it stays in
/// Notification Center). Past this, new ones are shown without the click.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
const MAX_WAITING: usize = 4;

#[derive(Default)]
pub struct State {
    /// Last notification per account.
    last: Mutex<BTreeMap<Uuid, Instant>>,
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
    waiting: AtomicUsize,
}

/// A cleaned count ("", "3", "20+") as a number that orders them:
/// "20+" is more than "20".
fn rank(c: &str) -> u32 {
    let (d, plus) = match c.strip_suffix('+') {
        Some(d) => (d, 1),
        None => (c, 0),
    };
    d.parse::<u32>().unwrap_or(0) * 2 + plus
}

/// What went up from `old` to `new`: (알림, 쪽지).
pub fn rose(old: &Unread, new: &Unread) -> (bool, bool) {
    (rank(&new.n) > rank(&old.n), rank(&new.dm) > rank(&old.dm))
}

/// Title and text: the account, and what is new with the counts now.
pub fn message(label: &str, n: bool, dm: bool, now: &Unread) -> (String, String) {
    let what = match (n, dm) {
        (true, true) => "알림·쪽지",
        (false, true) => "쪽지",
        _ => "알림",
    };
    (label.to_string(), format!("새 {what} ({})", now.text()))
}

impl State {
    /// May `id` get a notification at `now`? Records it if so.
    pub fn allow(&self, id: Uuid, now: Instant) -> bool {
        let mut last = self.last.lock().unwrap();
        if let Some(t) = last.get(&id) {
            if now.saturating_duration_since(*t) < GAP {
                return false;
            }
        }
        last.insert(id, now);
        true
    }
}

/// Shows the notification for `account` on a thread of its own; a click
/// on it brings the window back with that panel active. Errors are only
/// logged.
#[cfg(any(windows, target_os = "macos"))]
pub fn show<R: Runtime>(app: &AppHandle<R>, account: Uuid, title: &str, body: &str) {
    use std::sync::atomic::Ordering;
    use tauri::Manager;
    let identifier = app.config().identifier.clone();
    let mut n = notify_rust::Notification::new();
    n.summary(title).body(body);
    #[cfg(windows)]
    n.app_id(&identifier);
    #[cfg(target_os = "macos")]
    {
        // Notifications go out as this bundle (Paneful's name and icon).
        // Only once: mac-notification-sys allows a single call. A dev build
        // isn't a registered bundle, so it borrows Terminal's.
        static APP: std::sync::Once = std::sync::Once::new();
        APP.call_once(|| {
            let id = if tauri::is_dev() { "com.apple.Terminal" } else { identifier.as_str() };
            if let Err(e) = notify_rust::set_application(id) {
                log::warn!("notification app {id}: {e}");
            }
        });
    }
    let app = app.clone();
    let body = body.to_string();
    std::thread::spawn(move || {
        let state = app.state::<State>();
        let wait = state.waiting.fetch_add(1, Ordering::SeqCst) < MAX_WAITING;
        let r = n.show().and_then(|h| {
            if !wait {
                drop(h); // macOS sends it here
                return Ok(());
            }
            h.wait_for_response(|r: &notify_rust::NotificationResponse| {
                if matches!(r, notify_rust::NotificationResponse::Default) {
                    log::info!("notification clicked: panel {}", account.simple());
                    crate::tray::show(&app);
                    crate::focus_account(&app, account);
                }
            })
        });
        state.waiting.fetch_sub(1, Ordering::SeqCst);
        match r {
            Ok(()) => log::info!("notification: {body}"),
            Err(e) => log::warn!("notification failed: {e}"),
        }
    });
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn show<R: Runtime>(_app: &AppHandle<R>, _account: Uuid, _title: &str, body: &str) {
    log::info!("notification (not on this OS): {body}");
}

/// Windows: a toast needs the app's AppUserModelID registered, or Windows
/// drops it without a word. The installer's Start menu shortcut carries it,
/// the portable zip has no shortcut, so it is registered for the current
/// user here (HKCU\Software\Classes\AppUserModelId\<id>, no admin
/// needed).
#[cfg(windows)]
pub fn register_app_id(identifier: &str, name: &str) {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
    let key = HSTRING::from(format!("Software\\Classes\\AppUserModelId\\{identifier}"));
    let value: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let r = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            &key,
            &HSTRING::from("DisplayName"),
            REG_SZ.0,
            Some(value.as_ptr().cast()),
            (value.len() * 2) as u32,
        )
    };
    if r.is_err() {
        log::warn!("registering the notification app id failed: {r:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_rose() {
        let u = |n: &str, dm: &str| Unread::new(n, dm);
        assert_eq!(rose(&u("", ""), &u("1", "")), (true, false));
        assert_eq!(rose(&u("3", "1"), &u("3", "2")), (false, true));
        assert_eq!(rose(&u("3", "1"), &u("2", "1")), (false, false));
        assert_eq!(rose(&u("20", ""), &u("20+", "")), (true, false));
        assert_eq!(rose(&u("20+", ""), &u("20+", "")), (false, false));
        assert_eq!(rose(&u("20+", ""), &u("5", "")), (false, false));
        assert_eq!(rose(&u("2", "2"), &u("3", "3")), (true, true));
    }

    #[test]
    fn texts() {
        let now = Unread::new("3", "1");
        assert_eq!(message("본계", true, false, &now), ("본계".into(), "새 알림 (알림 3 · 쪽지 1)".into()));
        assert_eq!(message("본계", false, true, &now).1, "새 쪽지 (알림 3 · 쪽지 1)");
        assert_eq!(message("본계", true, true, &now).1, "새 알림·쪽지 (알림 3 · 쪽지 1)");
    }

    #[test]
    fn once_per_gap() {
        let s = State::default();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let t = Instant::now();
        assert!(s.allow(a, t));
        assert!(!s.allow(a, t + Duration::from_secs(5)));
        assert!(s.allow(b, t + Duration::from_secs(5)));
        assert!(s.allow(a, t + GAP));
    }
}
