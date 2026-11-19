//! "원본 저장": user-initiated download of the original image / highest
//! bitrate video of one post.
//!
//! The panel script (media.js) shows a save button on post media. A trusted
//! click on it calls `window.open(TRIGGER?...)`; panels have no IPC, so the
//! panel's `on_new_window` handler catches that URL, denies the window and
//! hands it to `handle`. Rust then downloads exactly that one file into
//! the save folder (settings, default `<Downloads>/Paneful`) and reports
//! back to the panel with a DOM event.
//!
//! Only `pbs.twimg.com/media/...` and `video.twimg.com/...mp4` are accepted,
//! so a page that opens the trigger URL itself can at most save one public
//! twimg file per call. There is no batch mode on purpose.

use serde::Serialize;
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Duration,
};
use tauri::{AppHandle, Manager, Runtime};
use url::Url;

pub const SCRIPT: &str = include_str!("media.js");

const TRIGGER_HOST: &str = "paneful.invalid";
const TRIGGER_PATH: &str = "/save-media";
const IMAGE_MAX: u64 = 100 * 1024 * 1024;
const VIDEO_MAX: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Image,
    Video,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SaveRequest {
    pub kind: Kind,
    /// Canonical download URL (image: `name=orig`).
    pub url: String,
    pub file_name: String,
}

pub fn is_trigger(url: &Url) -> bool {
    url.scheme() == "https" && url.host_str() == Some(TRIGGER_HOST) && url.path() == TRIGGER_PATH
}

fn query<'a>(url: &'a Url, key: &str) -> Option<std::borrow::Cow<'a, str>> {
    url.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn is_token(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `https://pbs.twimg.com/media/<id>[.<ext>][:<size>]?format=<fmt>&name=<size>`
/// -> (`...?format=<fmt>&name=orig`, id, fmt)
pub fn image_orig(src: &Url) -> Option<(String, String, String)> {
    if src.scheme() != "https" || src.host_str() != Some("pbs.twimg.com") {
        return None;
    }
    let rest = src.path().strip_prefix("/media/")?;
    let rest = rest.split(':').next()?;
    let (id, ext) = match rest.split_once('.') {
        Some((id, ext)) => (id, Some(ext)),
        None => (rest, None),
    };
    if !is_token(id, 64) {
        return None;
    }
    let fmt = query(src, "format")
        .map(|f| f.to_ascii_lowercase())
        .or_else(|| ext.map(|e| e.to_ascii_lowercase()))
        .unwrap_or_else(|| "jpg".into());
    let fmt = match fmt.as_str() {
        "jpg" | "jpeg" => "jpg",
        "png" => "png",
        "webp" => "webp",
        "gif" => "gif",
        _ => return None,
    };
    Some((
        format!("https://pbs.twimg.com/media/{id}?format={fmt}&name=orig"),
        id.to_string(),
        fmt.to_string(),
    ))
}

/// `https://video.twimg.com/{ext_tw_video,amplify_video,tweet_video}/.../<name>.mp4`
/// -> (url without query noise except `tag`, file stem)
pub fn video_url(src: &Url) -> Option<(String, String)> {
    if src.scheme() != "https" || src.host_str() != Some("video.twimg.com") {
        return None;
    }
    let path = src.path();
    let first = path.trim_start_matches('/').split('/').next()?;
    if !matches!(first, "ext_tw_video" | "amplify_video" | "tweet_video") {
        return None;
    }
    let last = path.rsplit('/').next()?;
    let stem = last.strip_suffix(".mp4")?;
    if !is_token(stem, 128) || path.contains("..") {
        return None;
    }
    let mut out = format!("https://video.twimg.com{path}");
    if let Some(tag) = query(src, "tag").filter(|t| is_token(t, 8)) {
        out.push_str("?tag=");
        out.push_str(&tag);
    }
    Some((out, stem.to_string()))
}

/// Parse a trigger URL. `src` is the media URL; `user`, `status`, `n` are
/// optional naming hints from the page and are only used if they look sane.
pub fn parse(url: &Url) -> Result<SaveRequest, &'static str> {
    if !is_trigger(url) {
        return Err("not a save request");
    }
    let src = query(url, "src").ok_or("no src")?;
    let src = Url::parse(&src).map_err(|_| "bad src")?;

    let user = query(url, "user").filter(|u| is_token(u, 15) && !u.contains('-'));
    let status = query(url, "status").filter(|s| !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()));
    let n = query(url, "n").filter(|s| matches!(s.as_ref(), "1" | "2" | "3" | "4"));
    let prefix = match (&user, &status) {
        (Some(u), Some(s)) => Some(format!("{u}_{s}")),
        _ => None,
    };

    if let Some((orig, id, fmt)) = image_orig(&src) {
        let stem = match (&prefix, &n) {
            (Some(p), Some(n)) => format!("{p}_{n}"),
            (Some(p), None) => format!("{p}_{id}"),
            _ => id,
        };
        return Ok(SaveRequest { kind: Kind::Image, url: orig, file_name: format!("{stem}.{fmt}") });
    }
    if let Some((v, stem)) = video_url(&src) {
        let stem = match &prefix {
            Some(p) => format!("{p}_{stem}"),
            None => stem,
        };
        return Ok(SaveRequest { kind: Kind::Video, url: v, file_name: format!("{stem}.mp4") });
    }
    Err("unsupported media url")
}

/// `name.ext`, `name (2).ext`, ... : first one that doesn't exist yet,
/// created atomically so two saves never share a file.
pub(crate) fn create_unique(dir: &Path, file_name: &str) -> std::io::Result<(PathBuf, fs::File)> {
    let (stem, ext) = file_name.rsplit_once('.').unwrap_or((file_name, ""));
    for i in 1..=999 {
        let name = if i == 1 { file_name.to_string() } else { format!("{stem} ({i}).{ext}") };
        let path = dir.join(&name);
        match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(f) => return Ok((path, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("too many files with the same name"))
}

pub(crate) fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .build()
            .expect("http client")
    })
}

fn in_flight() -> &'static Mutex<HashSet<String>> {
    static S: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

async fn download(dir: PathBuf, req: &SaveRequest) -> Result<PathBuf, String> {
    let resp = client().get(&req.url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let (want, max) = match req.kind {
        Kind::Image => ("image/", IMAGE_MAX),
        Kind::Video => ("video/", VIDEO_MAX),
    };
    if !ctype.starts_with(want) {
        return Err(format!("unexpected content-type {ctype}"));
    }
    if resp.content_length().is_some_and(|l| l > max) {
        return Err("file too large".into());
    }

    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (path, mut file) = create_unique(&dir, &req.file_name).map_err(|e| e.to_string())?;
    let res: Result<(), String> = async {
        let mut resp = resp;
        let mut total = 0u64;
        while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
            total += chunk.len() as u64;
            if total > max {
                return Err("file too large".into());
            }
            file.write_all(&chunk).map_err(|e| e.to_string())?;
        }
        file.flush().map_err(|e| e.to_string())
    }
    .await;
    if let Err(e) = res {
        drop(file);
        let _ = fs::remove_file(&path);
        return Err(e);
    }
    Ok(path)
}

#[derive(Serialize)]
struct Report<'a> {
    src: &'a str,
    ok: bool,
    message: String,
}

fn report<R: Runtime>(app: &AppHandle<R>, label: &str, src: &str, ok: bool, message: String) {
    let Some(wv) = app.get_webview(label) else { return };
    let detail = serde_json::to_string(&Report { src, ok, message }).unwrap_or_else(|_| "{}".into());
    let _ = wv.eval(format!(
        "window.dispatchEvent(new CustomEvent('paneful-media-saved',{{detail:{detail}}}))"
    ));
}

/// `<Downloads>/Paneful`, used when no folder is set in settings.
pub fn default_save_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    app.path().download_dir().map(|d| d.join("Paneful")).map_err(|e| e.to_string())
}

/// Folder for media downloads and post captures: the one set in settings,
/// else the default.
/// Only reads settings (short lock, no window calls under it).
pub fn save_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let custom = app.state::<crate::AppState>().settings.lock().unwrap().save_dir.clone();
    match custom.filter(|d| !d.trim().is_empty()) {
        Some(d) => Ok(PathBuf::from(d)),
        None => default_save_dir(app),
    }
}

/// Called from the panel's `on_new_window` for a trigger URL.
pub fn handle<R: Runtime>(app: AppHandle<R>, label: String, url: Url) {
    let src = query(&url, "src").map(|s| s.into_owned()).unwrap_or_default();
    let req = match parse(&url) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("media save rejected: {e}");
            report(&app, &label, &src, false, e.into());
            return;
        }
    };
    if !in_flight().lock().unwrap().insert(req.url.clone()) {
        return; // same file already downloading: ignore double clicks
    }
    tauri::async_runtime::spawn(async move {
        let res = match save_dir(&app) {
            Ok(d) => download(d, &req).await,
            Err(e) => Err(e),
        };
        in_flight().lock().unwrap().remove(&req.url);
        match res {
            Ok(path) => {
                log::info!("saved {} -> {}", req.url, path.display());
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                report(&app, &label, &src, true, name);
            }
            Err(e) => {
                log::warn!("saving {} failed: {e}", req.url);
                report(&app, &label, &src, false, e);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trig(src: &str, extra: &str) -> Url {
        let mut u = Url::parse("https://paneful.invalid/save-media").unwrap();
        u.query_pairs_mut().append_pair("src", src);
        let mut u = u.to_string();
        u.push_str(extra);
        Url::parse(&u).unwrap()
    }

    #[test]
    fn images() {
        let p = |s: &str| image_orig(&Url::parse(s).unwrap()).map(|t| t.0);
        assert_eq!(
            p("https://pbs.twimg.com/media/GabcDEF12_-?format=jpg&name=small").as_deref(),
            Some("https://pbs.twimg.com/media/GabcDEF12_-?format=jpg&name=orig")
        );
        assert_eq!(
            p("https://pbs.twimg.com/media/Gabc?format=png&name=900x900").as_deref(),
            Some("https://pbs.twimg.com/media/Gabc?format=png&name=orig")
        );
        assert_eq!(
            p("https://pbs.twimg.com/media/Gabc.jpg:large").as_deref(),
            Some("https://pbs.twimg.com/media/Gabc?format=jpg&name=orig")
        );
        assert_eq!(p("https://pbs.twimg.com/media/Gabc?format=svg"), None);
        assert_eq!(p("https://pbs.twimg.com/profile_images/1/a.jpg"), None);
        assert_eq!(p("http://pbs.twimg.com/media/Gabc?format=jpg"), None);
        assert_eq!(p("https://evil.com/media/Gabc?format=jpg"), None);
        assert_eq!(p("https://pbs.twimg.com/media/../x?format=jpg"), None);
    }

    #[test]
    fn videos() {
        let p = |s: &str| video_url(&Url::parse(s).unwrap()).map(|t| t.0);
        assert_eq!(
            p("https://video.twimg.com/ext_tw_video/123/pu/vid/avc1/1280x720/AbC-d_e.mp4?tag=12").as_deref(),
            Some("https://video.twimg.com/ext_tw_video/123/pu/vid/avc1/1280x720/AbC-d_e.mp4?tag=12")
        );
        assert_eq!(
            p("https://video.twimg.com/tweet_video/GxYz.mp4").as_deref(),
            Some("https://video.twimg.com/tweet_video/GxYz.mp4")
        );
        assert_eq!(p("https://video.twimg.com/ext_tw_video/123/pu/pl/x.m3u8"), None);
        assert_eq!(p("https://video.twimg.com/other/x.mp4"), None);
        assert_eq!(p("https://video.twimg.com.evil.io/tweet_video/x.mp4"), None);
    }

    #[test]
    fn requests() {
        let r = parse(&trig(
            "https://pbs.twimg.com/media/Gabc?format=jpg&name=small",
            "&user=jack&status=20&n=2",
        ))
        .unwrap();
        assert_eq!(r.kind, Kind::Image);
        assert_eq!(r.file_name, "jack_20_2.jpg");

        // bad hints are dropped, media id is used instead
        let r = parse(&trig(
            "https://pbs.twimg.com/media/Gabc?format=jpg",
            "&user=../../x&status=20&n=9",
        ))
        .unwrap();
        assert_eq!(r.file_name, "Gabc.jpg");

        let r = parse(&trig(
            "https://video.twimg.com/amplify_video/9/vid/avc1/1920x1080/Zz.mp4",
            "&user=jack&status=20",
        ))
        .unwrap();
        assert_eq!(r.kind, Kind::Video);
        assert_eq!(r.file_name, "jack_20_Zz.mp4");

        assert!(parse(&trig("https://x.com/home", "")).is_err());
        assert!(parse(&Url::parse("https://paneful.invalid/other?src=x").unwrap()).is_err());
        assert!(!is_trigger(&Url::parse("http://paneful.invalid/save-media").unwrap()));
    }

    #[test]
    fn unique_names() {
        let dir = std::env::temp_dir().join(format!("paneful-test-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&dir).unwrap();
        let (a, _) = create_unique(&dir, "x.jpg").unwrap();
        let (b, _) = create_unique(&dir, "x.jpg").unwrap();
        assert_eq!(a.file_name().unwrap(), "x.jpg");
        assert_eq!(b.file_name().unwrap(), "x (2).jpg");
        let _ = fs::remove_dir_all(&dir);
    }
}
