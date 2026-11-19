//! New version notice. Asks GitHub for the latest release and, when it is
//! newer than this build, tells the host UI (top bar pill, strip marker,
//! settings) and the tray menu. Notice only: the release page opens in the
//! system browser, nothing is downloaded or installed here.
//!
//! The request goes out from Rust through the shared cookie-less client
//! (media::client), never from a webview. Failures (offline, rate limit)
//! are logged and otherwise ignored. Nothing here holds a lock across an
//! await or while calling into windows / the tray.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_opener::OpenerExt;

use crate::AppState;

/// `/releases/latest` already leaves out drafts and pre-releases.
const LATEST: &str = "https://api.github.com/repos/Machrie/paneful/releases/latest";
pub const RELEASES: &str = "https://github.com/Machrie/paneful/releases/latest";
/// First check a little after start, so it never competes with the panels
/// loading; then every few hours for windows that stay open for days.
/// Unauthenticated API limit is 60 requests per hour per IP.
const FIRST_DELAY: Duration = Duration::from_secs(20);
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// Testing only: pretend to be this version, so the current release shows
/// up as new (`PANEFUL_PRETEND_VERSION=0.0.1`).
const PRETEND_ENV: &str = "PANEFUL_PRETEND_VERSION";

/// What the last check found. Not persisted.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Status {
    /// A check is running right now.
    pub checking: bool,
    /// Latest release version ("0.0.5", no "v") when it is newer than ours.
    pub newer: Option<String>,
    /// Its release page.
    pub url: Option<String>,
    /// Last check finished and found nothing newer.
    pub up_to_date: bool,
    /// Last check failed (only shown in settings, after "지금 확인").
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// Version this build compares against.
pub fn current<R: Runtime>(app: &AppHandle<R>) -> String {
    if let Ok(v) = std::env::var(PRETEND_ENV) {
        let v = v.trim().trim_start_matches('v');
        if parse(v).is_some() {
            return v.to_string();
        }
    }
    app.package_info().version.to_string()
}

/// "v1.2.3" / "1.2.3" -> [1, 2, 3]. Anything with a pre-release or build
/// suffix ("1.2.3-beta") or non-numbers is not a version we announce.
fn parse(v: &str) -> Option<Vec<u64>> {
    let v = v.trim().trim_start_matches('v');
    if v.is_empty() {
        return None;
    }
    v.split('.').map(|p| p.parse::<u64>().ok()).collect()
}

/// `latest` is a higher version than `cur`.
fn is_newer(latest: &str, cur: &str) -> bool {
    let (Some(mut a), Some(mut b)) = (parse(latest), parse(cur)) else { return false };
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

/// The notice the top bar / tray should show: a newer version the user
/// hasn't skipped, with checking turned on.
pub fn notice(s: &crate::settings::Settings, st: &Status) -> Option<String> {
    if !s.update_check {
        return None;
    }
    let v = st.newer.as_ref()?;
    (s.skip_version.as_deref() != Some(v.as_str())).then(|| v.clone())
}

async fn fetch(cur: &str) -> Result<Option<(String, String)>, String> {
    let resp = crate::media::client()
        .get(LATEST)
        .header(reqwest::header::USER_AGENT, format!("Paneful/{cur}"))
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| format!("연결 실패: {e}"))?;
    let code = resp.status();
    if code == reqwest::StatusCode::NOT_FOUND {
        return Ok(None); // no release yet
    }
    if !code.is_success() {
        return Err(match code.as_u16() {
            403 | 429 => "GitHub 요청 한도 초과, 나중에 다시".into(),
            c => format!("GitHub 응답 {c}"),
        });
    }
    let body = resp.bytes().await.map_err(|e| format!("응답 읽기 실패: {e}"))?;
    let r: Release = serde_json::from_slice(&body).map_err(|e| format!("응답 해석 실패: {e}"))?;
    if r.draft || r.prerelease {
        return Ok(None);
    }
    let v = r.tag_name.trim().trim_start_matches('v').to_string();
    if !is_newer(&v, cur) {
        return Ok(None);
    }
    // Only ever open a page on github.com.
    let url = if r.html_url.starts_with("https://github.com/") { r.html_url } else { RELEASES.into() };
    Ok(Some((v, url)))
}

/// Run one check and publish the result. `manual`: from "지금 확인", which
/// also runs with checking turned off and reports failures.
pub async fn check<R: Runtime>(app: &AppHandle<R>, manual: bool) {
    let state = app.state::<AppState>();
    if !manual && !state.settings.lock().unwrap().update_check {
        return;
    }
    {
        let mut st = state.update.lock().unwrap();
        if st.checking {
            return;
        }
        st.checking = true;
        st.error = None;
    }
    if manual {
        crate::relayout(app);
    }
    let cur = current(app);
    let res = fetch(&cur).await;
    {
        let mut st = state.update.lock().unwrap();
        st.checking = false;
        match res {
            Ok(Some((v, url))) => {
                log::info!("update: {v} available (running {cur})");
                st.newer = Some(v);
                st.url = Some(url);
                st.up_to_date = false;
            }
            Ok(None) => {
                log::info!("update: {cur} is the latest");
                st.newer = None;
                st.url = None;
                st.up_to_date = true;
            }
            Err(e) => {
                // Keep whatever an earlier check found.
                log::warn!("update check failed: {e}");
                if manual {
                    st.error = Some(e);
                }
            }
        }
    }
    publish(app);
}

/// Tell the host UI and the tray what is current.
pub fn publish<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<AppState>();
    let v = {
        let s = state.settings.lock().unwrap();
        let st = state.update.lock().unwrap();
        notice(&s, &st)
    };
    crate::tray::set_update(app, v.as_deref());
    crate::relayout(app);
}

/// Background loop, started once from setup. A plain thread that sleeps,
/// so no async worker is held for hours.
pub fn spawn<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    let r = std::thread::Builder::new().name("update-check".into()).spawn(move || {
        std::thread::sleep(FIRST_DELAY);
        loop {
            tauri::async_runtime::block_on(check(&app, false));
            std::thread::sleep(EVERY);
        }
    });
    if let Err(e) = r {
        log::warn!("update check thread not started: {e}");
    }
}

/// Release page of the version on offer (or the releases page).
pub fn open<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let url = app
        .state::<AppState>()
        .update
        .lock()
        .unwrap()
        .url
        .clone()
        .unwrap_or_else(|| RELEASES.into());
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare() {
        assert!(is_newer("v0.0.5", "0.0.4"));
        assert!(is_newer("0.1.0", "0.0.9"));
        assert!(is_newer("0.0.10", "0.0.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("v0.0.4", "0.0.4"));
        assert!(!is_newer("0.0.4.0", "0.0.4"));
        assert!(!is_newer("0.0.3", "0.0.4"));
        assert!(!is_newer("0.0.5-beta", "0.0.4"));
        assert!(!is_newer("latest", "0.0.4"));
        assert!(!is_newer("", "0.0.4"));
    }

    #[test]
    fn notice_respects_skip_and_switch() {
        let mut s = crate::settings::Settings::default();
        let st = Status { newer: Some("0.0.5".into()), ..Default::default() };
        assert_eq!(notice(&s, &st).as_deref(), Some("0.0.5"));
        s.skip_version = Some("0.0.5".into());
        assert_eq!(notice(&s, &st), None);
        s.skip_version = Some("0.0.4".into());
        assert_eq!(notice(&s, &st).as_deref(), Some("0.0.5"));
        s.update_check = false;
        assert_eq!(notice(&s, &st), None);
        assert_eq!(notice(&crate::settings::Settings::default(), &Status::default()), None);
    }

    #[test]
    fn release_json() {
        let r: Release = serde_json::from_str(
            r#"{"tag_name":"v0.0.5","html_url":"https://github.com/Machrie/paneful/releases/tag/v0.0.5","prerelease":false,"draft":false,"assets":[]}"#,
        )
        .unwrap();
        assert_eq!(r.tag_name, "v0.0.5");
        assert!(!r.prerelease && !r.draft);
    }
}
