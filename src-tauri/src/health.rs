//! Dead panel recovery. When a panel's web content process dies the panel
//! goes blank; this notices it and brings the panel back, at most
//! `MAX_TRIES` times per `WINDOW` so a page that keeps crashing doesn't loop.
//!
//! - Windows: WebView2 `ProcessFailed` (hooked in `watch`).
//!   - renderer exited / unresponsive: reload the panel.
//!   - browser process exited: every panel's WebView2 is gone, so the panel
//!     webview is closed and built again (`crate::recreate_panel`).
//!   - GPU / utility / iframe renderer: WebView2 restarts those itself, only
//!     logged.
//! - macOS: `webViewWebContentProcessDidTerminate:` through
//!   `Builder::on_web_content_process_terminate` (`on_terminated`). Without
//!   the hook tauri-runtime-wry already reloads, but with no limit and no
//!   notice; this replaces that.
//!
//! The host UI hears about each recovery through the `panel-health` event,
//! and the settings screen shows the last one per panel (`last`).
//! The lock here is never held while touching a webview.

use serde::Serialize;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager, Runtime};

pub const MAX_TRIES: usize = 3;
pub const WINDOW: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Bring it back (reload or rebuild); `n`-th try within `WINDOW`.
    Recover { n: usize },
    /// Too many in a row: leave it, the user can press 새로고침.
    GiveUp,
}

/// Recovery times of one panel, oldest first. Pure, for tests.
pub fn decide(history: &mut Vec<Instant>, now: Instant) -> Action {
    history.retain(|t| now.duration_since(*t) < WINDOW);
    if history.len() >= MAX_TRIES {
        return Action::GiveUp;
    }
    history.push(now);
    Action::Recover { n: history.len() }
}

#[derive(Default)]
struct Panel {
    tries: Vec<Instant>,
    /// Auto recovery stopped (GiveUp) until the user reloads by hand.
    stopped: bool,
    /// Windows: the WebView2 is gone for good, a plain reload can't help.
    dead: bool,
    /// Last event, for the settings screen.
    last: Option<Last>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Last {
    /// Unix ms.
    pub at: u64,
    pub text: String,
}

static PANELS: Mutex<Option<HashMap<String, Panel>>> = Mutex::new(None);

fn with<T>(label: &str, f: impl FnOnce(&mut Panel) -> T) -> T {
    let mut g = PANELS.lock().unwrap();
    f(g.get_or_insert_with(HashMap::new).entry(label.to_string()).or_default())
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Last recovery event of a panel (settings screen).
pub fn last(label: &str) -> Option<Last> {
    PANELS.lock().unwrap().as_ref().and_then(|m| m.get(label)).and_then(|p| p.last.clone())
}

/// Panel webview needs rebuilding rather than a reload (Windows, browser
/// process gone and auto recovery stopped).
pub fn is_dead(label: &str) -> bool {
    PANELS.lock().unwrap().as_ref().and_then(|m| m.get(label)).is_some_and(|p| p.dead)
}

/// The user reloaded by hand: start counting again.
pub fn reset(label: &str) {
    with(label, |p| {
        p.tries.clear();
        p.stopped = false;
        p.dead = false;
    });
}

/// Panel removed or rebuilt.
pub fn forget(label: &str) {
    if let Some(m) = PANELS.lock().unwrap().as_mut() {
        m.remove(label);
    }
}

#[derive(Serialize, Clone)]
struct Event {
    id: String,
    label: String,
    text: String,
    gave_up: bool,
}

/// Account id and display name of a panel webview label.
fn account_of<R: Runtime>(app: &AppHandle<R>, label: &str) -> Option<(String, String)> {
    let state = app.state::<crate::AppState>();
    let s = state.settings.lock().unwrap();
    s.accounts
        .iter()
        .find(|a| crate::panel_label(a) == label)
        .map(|a| (a.id.to_string(), a.label.clone()))
}

/// Something killed `label`'s page (`why`, for the log and the notice).
/// Decides, records and tells the host UI. `dead`: the webview itself is
/// gone (Windows browser process). Returns whether to recover now.
pub fn report<R: Runtime>(app: &AppHandle<R>, label: &str, why: &str, dead: bool) -> bool {
    let Some((id, name)) = account_of(app, label) else {
        // Host UI or a sign-in popup: always bring it back, nothing to show.
        log::warn!("[{label}] {why}: not a panel, reloading");
        return true;
    };
    let action = with(label, |p| {
        if p.stopped {
            p.dead |= dead;
            return None;
        }
        let a = decide(&mut p.tries, Instant::now());
        if a == Action::GiveUp {
            p.stopped = true;
            p.dead |= dead;
        }
        Some(a)
    });
    let Some(action) = action else {
        log::warn!("[{label}] {why}: auto recovery already stopped");
        return false;
    };
    let (text, ok) = match action {
        Action::Recover { n } => (format!("'{name}' 패널이 죽어서 다시 띄움 ({n}/{MAX_TRIES}, {why})"), true),
        Action::GiveUp => (
            format!("'{name}' 패널이 계속 죽어서 자동 복구 멈춤 ({why}) · 새로고침 버튼으로 다시 시도"),
            false,
        ),
    };
    log::warn!("[{label}] {text}");
    with(label, |p| p.last = Some(Last { at: now_ms(), text: text.clone() }));
    let _ = app.emit_to(crate::HOST, "panel-health", Event { id, label: name, text, gave_up: !ok });
    ok
}

/// Windows: hook WebView2 ProcessFailed on a new panel.
#[cfg(windows)]
pub fn watch<R: Runtime>(wv: &tauri::Webview<R>) {
    use webview2_com::{
        Microsoft::Web::WebView2::Win32::*,
        ProcessFailedEventHandler,
    };
    use windows::core::Interface;

    let app = wv.app_handle().clone();
    let label = wv.label().to_string();
    let _ = wv.with_webview(move |pw| unsafe {
        let core = match pw.controller().CoreWebView2() {
            Ok(c) => c,
            Err(e) => {
                log::warn!("[{label}] ProcessFailed not hooked: {e}");
                return;
            }
        };
        let label2 = label.clone();
        let handler = ProcessFailedEventHandler::create(Box::new(move |sender, args| {
            let Some(args) = args else { return Ok(()) };
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
            args.ProcessFailedKind(&mut kind)?;
            // Exit code / reason, for the log only (runtime 1.0.992+).
            let detail = args
                .cast::<ICoreWebView2ProcessFailedEventArgs2>()
                .ok()
                .map(|a2| {
                    let mut code = 0i32;
                    let mut reason = COREWEBVIEW2_PROCESS_FAILED_REASON::default();
                    let _ = a2.ExitCode(&mut code);
                    let _ = a2.Reason(&mut reason);
                    format!(" exit={code} reason={}", reason.0)
                })
                .unwrap_or_default();
            log::warn!("[{label2}] WebView2 ProcessFailed kind={}{detail}", kind.0);
            if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
                || kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE
            {
                let why = if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED {
                    "렌더러 종료"
                } else {
                    "응답 없음"
                };
                if report(&app, &label2, why, false) {
                    if let Some(core) = sender {
                        let _ = core.Reload();
                    }
                }
            } else if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED {
                if report(&app, &label2, "WebView2 종료", true) {
                    // Not from inside this webview's own event handler, and
                    // add_child must not run on the main thread.
                    let app = app.clone();
                    let label = label2.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = crate::recreate_panel(&app, &label).await {
                            log::error!("[{label}] rebuild failed: {e}");
                        }
                    });
                }
            }
            Ok(())
        }));
        let mut token = 0i64;
        if let Err(e) = core.add_ProcessFailed(&handler, &mut token) {
            log::warn!("[{label}] ProcessFailed not hooked: {e}");
        }
    });
}

#[cfg(not(windows))]
pub fn watch<R: Runtime>(_wv: &tauri::Webview<R>) {}

/// macOS: WKWebView's web content process is gone (crash, or killed under
/// memory pressure). Runs on the main thread.
#[cfg(target_os = "macos")]
pub fn on_terminated<R: Runtime>(wv: &tauri::Webview<R>) {
    let app = wv.app_handle().clone();
    if report(&app, wv.label(), "웹 콘텐츠 프로세스 종료", false) {
        if let Err(e) = wv.reload() {
            log::error!("[{}] reload failed: {e}", wv.label());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stops_after_max_tries_in_window() {
        let t0 = Instant::now();
        let mut h = vec![];
        assert_eq!(decide(&mut h, t0), Action::Recover { n: 1 });
        assert_eq!(decide(&mut h, t0 + Duration::from_secs(1)), Action::Recover { n: 2 });
        assert_eq!(decide(&mut h, t0 + Duration::from_secs(2)), Action::Recover { n: 3 });
        assert_eq!(decide(&mut h, t0 + Duration::from_secs(3)), Action::GiveUp);
        assert_eq!(decide(&mut h, t0 + Duration::from_secs(4)), Action::GiveUp);
    }

    #[test]
    fn old_tries_expire() {
        let t0 = Instant::now();
        let mut h = vec![];
        for i in 0..MAX_TRIES {
            decide(&mut h, t0 + Duration::from_secs(i as u64));
        }
        let later = t0 + WINDOW + Duration::from_secs(1);
        // Tries at +0s and +1s are out of the window now, +2s is still in.
        assert_eq!(decide(&mut h, later), Action::Recover { n: 2 });
    }

    #[test]
    fn reset_clears_stop() {
        let label = "panel-test-reset";
        with(label, |p| {
            p.stopped = true;
            p.dead = true;
            p.tries = vec![Instant::now(); MAX_TRIES];
        });
        assert!(is_dead(label));
        reset(label);
        assert!(!is_dead(label));
        with(label, |p| assert!(p.tries.is_empty() && !p.stopped));
        forget(label);
    }
}
