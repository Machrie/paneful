//! X screen rules: the CSS selectors (and a few URL fragments) that the
//! panel scripts use to find posts, names, avatars, photos, videos and the
//! unread badges on x.com. They live in `rules/x.v1.json` in the repository, not in the
//! scripts, so when X changes its page the fix is one edit there and
//! installed apps pick it up without a release.
//!
//! Safety:
//! - Only data comes in. The file is parsed into fixed maps of short ASCII
//!   strings under known names; anything else in it is dropped, and a value
//!   that fails the checks rejects the whole file. The panel scripts only
//!   ever pass these strings to querySelector / closest / matches and
//!   String.includes, never to eval, Function, innerHTML or a RegExp.
//! - The app carries its own copy (built in from the same file). A failed
//!   download, a bad file or a format this build doesn't know leaves the
//!   built-in or the last good download in use. The last good download is
//!   cached on disk for offline starts.
//! - `format` is in the file and in its name (x.v1.json). A change old apps
//!   can't read goes to x.v2.json; x.v1.json keeps getting fixes for them.
//! - `revision` only goes up: the highest of built-in / cache / download
//!   wins, so a stale cache never shadows a newer app's built-in copy.
//!
//! The request goes out from Rust through the shared cookie-less client
//! (media::client), on a background thread, like update.rs.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};
use tauri::{AppHandle, Manager, Runtime};

use crate::AppState;

/// Rules format this build reads.
pub const FORMAT: u32 = 1;
const BUILTIN: &str = include_str!("../../rules/x.v1.json");
const URL: &str = "https://raw.githubusercontent.com/Machrie/paneful/main/rules/x.v1.json";
const CACHE_FILE: &str = "x-rules.v1.json";
const FIRST_DELAY: Duration = Duration::from_secs(15);
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_BYTES: usize = 64 * 1024;
const MAX_VALUE: usize = 400;

/// Names the panel scripts look up (rules.js falls back to the built-in
/// value for any name it doesn't get). Unknown names in a file are dropped,
/// so a newer file with extra names still works here.
pub const SELECTORS: &[&str] = &[
    "post",
    "column",
    "modal",
    "statusLink",
    "userName",
    "avatar",
    "postText",
    "socialContext",
    "quoteBox",
    "photo",
    "video",
    "notifLink",
    "dmLink",
    "badge",
];
/// Plain substrings matched against image URLs (String.includes).
pub const TEXTS: &[&str] = &["avatarPath", "emojiPath"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Builtin,
    Cache,
    Remote,
}

/// Rules in effect, as handed to the panels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rules {
    pub revision: u64,
    pub source: Source,
    pub selectors: BTreeMap<String, String>,
    pub text: BTreeMap<String, String>,
}

/// For the settings screen.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Status {
    pub checking: bool,
    pub revision: u64,
    pub source: Option<Source>,
    /// Last check failed (only shown after "지금 확인").
    pub error: Option<String>,
    /// Last check finished without anything newer.
    pub up_to_date: bool,
}

#[derive(Deserialize)]
struct File {
    format: u32,
    revision: u64,
    #[serde(default)]
    selectors: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    text: BTreeMap<String, serde_json::Value>,
}

/// Short printable ASCII, nothing that could close a string or start
/// markup if a value were ever misused. X's selectors need none of these.
fn value_ok(v: &str) -> bool {
    !v.trim().is_empty()
        && v.len() <= MAX_VALUE
        && v.bytes().all(|b| (0x20..0x7f).contains(&b) && !matches!(b, b'\\' | b'`' | b'<' | b'>'))
}

fn pick(
    from: &BTreeMap<String, serde_json::Value>,
    names: &[&str],
    base: Option<&BTreeMap<String, String>>,
) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for &n in names {
        match from.get(n) {
            Some(serde_json::Value::String(s)) if value_ok(s) => {
                out.insert(n.to_string(), s.clone());
            }
            Some(_) => return Err(format!("bad value for {n}")),
            None => match base.and_then(|b| b.get(n)) {
                Some(s) => {
                    out.insert(n.to_string(), s.clone());
                }
                None => return Err(format!("missing {n}")),
            },
        }
    }
    Ok(out)
}

/// Parse and check one file. `base`: fills names the file leaves out
/// (the built-in rules); None for the built-in file itself, which must
/// have every name.
pub fn parse(bytes: &[u8], source: Source, base: Option<&Rules>) -> Result<Rules, String> {
    if bytes.len() > MAX_BYTES {
        return Err("file too large".into());
    }
    let f: File = serde_json::from_slice(bytes).map_err(|e| format!("not rules JSON: {e}"))?;
    if f.format != FORMAT {
        return Err(format!("format {} (this build reads {FORMAT})", f.format));
    }
    Ok(Rules {
        revision: f.revision,
        source,
        selectors: pick(&f.selectors, SELECTORS, base.map(|b| &b.selectors))?,
        text: pick(&f.text, TEXTS, base.map(|b| &b.text))?,
    })
}

pub fn builtin() -> Rules {
    parse(BUILTIN.as_bytes(), Source::Builtin, None).expect("built-in rules must parse")
}

fn cache_path<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    // Portable mode: next to settings.json in the data folder.
    if let Some(root) = crate::portable::root() {
        return Some(root.join(CACHE_FILE));
    }
    app.path().app_data_dir().ok().map(|d| d.join(CACHE_FILE))
}

/// Rules to start with: built-in, or the cached download if it is newer
/// (and fetching is on).
pub fn initial<R: Runtime>(app: &AppHandle<R>, fetch_on: bool) -> Rules {
    let b = builtin();
    if !fetch_on {
        return b;
    }
    let Some(p) = cache_path(app) else { return b };
    match std::fs::read(&p) {
        Ok(bytes) => match parse(&bytes, Source::Cache, Some(&b)) {
            Ok(c) if c.revision > b.revision => {
                log::info!("rules: cached revision {} (built-in {})", c.revision, b.revision);
                c
            }
            Ok(_) => b,
            Err(e) => {
                log::warn!("rules: cache ignored: {e}");
                b
            }
        },
        Err(_) => b,
    }
}

/// JSON for rules.js.
pub fn json(r: &Rules) -> String {
    serde_json::to_string(r).unwrap_or_else(|_| "null".into())
}

pub fn status_of(r: &Rules, st: &Status) -> Status {
    Status { revision: r.revision, source: Some(r.source.clone()), ..st.clone() }
}

async fn fetch(cur: &str) -> Result<Vec<u8>, String> {
    let resp = crate::media::client()
        .get(URL)
        .header(reqwest::header::USER_AGENT, format!("Paneful/{cur}"))
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| format!("연결 실패: {e}"))?;
    let code = resp.status();
    if !code.is_success() {
        return Err(format!("GitHub 응답 {}", code.as_u16()));
    }
    if resp.content_length().is_some_and(|n| n as usize > MAX_BYTES) {
        return Err("규칙 파일이 너무 큼".into());
    }
    let body = resp.bytes().await.map_err(|e| format!("응답 읽기 실패: {e}"))?;
    Ok(body.to_vec())
}

/// Download, check, adopt if newer, tell the panels. `manual`: from
/// "지금 확인", which reports failures in settings.
pub async fn check<R: Runtime>(app: &AppHandle<R>, manual: bool) {
    let state = app.state::<AppState>();
    if !state.settings.lock().unwrap().rules_update {
        return;
    }
    {
        let mut st = state.rules_status.lock().unwrap();
        if st.checking {
            return;
        }
        st.checking = true;
        st.error = None;
    }
    if manual {
        crate::relayout(app);
    }
    let cur = crate::update::current(app);
    let res = fetch(&cur).await.and_then(|bytes| {
        let base = builtin();
        parse(&bytes, Source::Remote, Some(&base))
            .map(|r| (r, bytes))
            .map_err(|e| format!("규칙 파일 거부: {e}"))
    });
    let mut changed = false;
    {
        let mut st = state.rules_status.lock().unwrap();
        st.checking = false;
        match res {
            Ok((r, bytes)) => {
                let mut active = state.rules.lock().unwrap();
                st.up_to_date = true;
                if r.revision > active.revision {
                    log::info!("rules: revision {} adopted (was {})", r.revision, active.revision);
                    if let Some(p) = cache_path(app) {
                        if let Some(d) = p.parent() {
                            let _ = std::fs::create_dir_all(d);
                        }
                        if let Err(e) = std::fs::write(&p, &bytes) {
                            log::warn!("rules: cache not written: {e}");
                        }
                    }
                    *active = r;
                    changed = true;
                } else {
                    log::info!("rules: remote revision {} not newer than {}", r.revision, active.revision);
                }
            }
            Err(e) => {
                log::warn!("rules check failed: {e}");
                if manual {
                    st.error = Some(e);
                }
            }
        }
    }
    if changed {
        push(app);
    }
    crate::relayout(app);
}

/// Tell every panel to ask for the rules again (rules.js calls
/// `panel_rules`; the event itself carries nothing to trust).
pub fn push<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<AppState>();
    let labels: Vec<String> = state.settings.lock().unwrap().accounts.iter().map(crate::panel_label).collect();
    for l in labels {
        if let Some(wv) = app.get_webview(&l) {
            let _ = wv.eval("window.dispatchEvent(new Event('paneful-rules'))");
        }
    }
}

/// Settings switch: off goes back to the built-in rules right away.
pub fn set_enabled<R: Runtime>(app: &AppHandle<R>, on: bool) {
    let state = app.state::<AppState>();
    let next = initial(app, on);
    let changed = {
        let mut active = state.rules.lock().unwrap();
        let changed = *active != next;
        *active = next;
        changed
    };
    {
        let mut st = state.rules_status.lock().unwrap();
        st.error = None;
        st.up_to_date = false;
    }
    if changed {
        push(app);
    }
    if on {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { check(&app, false).await });
    }
}

/// Background loop, started once from setup (see update::spawn).
pub fn spawn<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    let r = std::thread::Builder::new().name("rules-check".into()).spawn(move || {
        std::thread::sleep(FIRST_DELAY);
        loop {
            tauri::async_runtime::block_on(check(&app, false));
            std::thread::sleep(EVERY);
        }
    });
    if let Err(e) = r {
        log::warn!("rules check thread not started: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_has_every_name() {
        let b = builtin();
        assert_eq!(b.selectors.len(), SELECTORS.len());
        assert_eq!(b.text.len(), TEXTS.len());
        assert_eq!(b.source, Source::Builtin);
    }

    #[test]
    fn accepts_newer_and_fills_missing() {
        let b = builtin();
        let r = parse(
            br#"{"format":1,"revision":9,"selectors":{"post":"article[data-testid=\"post\"]","extra":"x"},"later":{"a":1}}"#,
            Source::Remote,
            Some(&b),
        )
        .unwrap();
        assert_eq!(r.revision, 9);
        assert_eq!(r.selectors["post"], "article[data-testid=\"post\"]");
        assert_eq!(r.selectors["photo"], b.selectors["photo"]);
        assert!(!r.selectors.contains_key("extra"));
        assert_eq!(r.text, b.text);
    }

    #[test]
    fn rejects_bad_files() {
        let b = builtin();
        let bad: &[&[u8]] = &[
            b"",
            b"not json",
            br#"{"format":2,"revision":9,"selectors":{}}"#,
            br#"{"revision":9}"#,
            br#"{"format":1,"revision":-1}"#,
            br#"{"format":1,"revision":9,"selectors":{"post":1}}"#,
            br#"{"format":1,"revision":9,"selectors":{"post":""}}"#,
            br#"{"format":1,"revision":9,"selectors":{"post":"a<img src=x onerror=alert(1)>"}}"#,
            br#"{"format":1,"revision":9,"selectors":{"post":"a`"}}"#,
            br#"{"format":1,"revision":9,"selectors":{"post":"a\\u2028"}}"#,
            br#"{"format":1,"revision":9,"selectors":{"post":"a\u2028"}}"#,
            br#"{"format":1,"revision":9,"selectors":{"post":"\u00e9"}}"#,
            br#"{"format":1,"revision":9,"text":{"emojiPath":"a\nb"}}"#,
        ];
        for f in bad {
            assert!(parse(f, Source::Remote, Some(&b)).is_err(), "{}", String::from_utf8_lossy(f));
        }
        let big = format!(r#"{{"format":1,"revision":9,"selectors":{{"post":"{}"}}}}"#, "a".repeat(MAX_VALUE + 1));
        assert!(parse(big.as_bytes(), Source::Remote, Some(&b)).is_err());
        let huge = format!(r#"{{"format":1,"revision":9,"pad":"{}"}}"#, "a".repeat(MAX_BYTES));
        assert!(parse(huge.as_bytes(), Source::Remote, Some(&b)).is_err());
    }

    #[test]
    fn json_is_data_only() {
        let j = json(&builtin());
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["source"], "builtin");
        assert!(v["selectors"]["post"].is_string());
    }
}
