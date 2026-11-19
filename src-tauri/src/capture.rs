//! Post capture: pick the first and last post in a panel, get one PNG of
//! that range, anonymized by default.
//!
//! 1. capture.js (panel) scrolls through the range and reads each post from
//!    the DOM. It calls `capture_posts`, the only new command x.com may call.
//! 2. Rust cleans the posts up, hides the panels and hands them to the host
//!    UI (`capture` event), the same way the shortcut overlay is shown.
//! 3. The host UI (ui/capture.js) draws the posts as cards on canvases. The
//!    images it needs come through `capture_image`: cookie-less, public
//!    pbs.twimg.com files only, like "원본 저장".
//! 4. `capture_export` gets the canvases as PNG strips, joins them into one
//!    image and saves it to `<Downloads>/Paneful` and/or the clipboard.

use serde::{Deserialize, Serialize};
use std::{io::Write, path::PathBuf};
use tauri::{ipc::InvokeBody, AppHandle, Emitter, Manager, Runtime, State, Webview};
use tauri_plugin_clipboard_manager::ClipboardExt;
use url::Url;

use crate::{media, AppState, HOST};

pub const SCRIPT: &str = include_str!("capture.js");

const MAX_POSTS: usize = 200;
const MAX_SEGS: usize = 400;
const MAX_TEXT: usize = 30_000;
const IMAGE_MAX: u64 = 20 * 1024 * 1024;
/// Output size cap (RGBA bytes): the host UI lowers its scale to stay under it.
const MAX_PIXELS: u64 = 150_000_000;

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct Seg {
    pub t: String,
    /// Mentioned handle (without @).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub m: Option<String>,
    /// Link or hashtag.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub l: bool,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Media {
    pub src: String,
    #[serde(default)]
    pub video: bool,
    pub w: f64,
    pub h: f64,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Card {
    #[serde(default)]
    pub lines: Vec<String>,
    #[serde(default)]
    pub img: Option<Media>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Quote {
    pub name: String,
    pub handle: String,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub text: Vec<Seg>,
    #[serde(default)]
    pub media: Vec<Media>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Post {
    pub name: String,
    pub handle: String,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub text: Vec<Seg>,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub media: Vec<Media>,
    #[serde(default)]
    pub card: Option<Card>,
    #[serde(default)]
    pub quote: Option<Quote>,
}

#[derive(Serialize, Clone)]
struct CapturePayload {
    posts: Vec<Post>,
    /// Stopped before the last post (post limit or end of page).
    cut: bool,
}

// ---------------------------------------------------------------- cleanup
// Everything here comes from x.com: keep sizes bounded and drop image URLs
// that `capture_image` would refuse anyway.

fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

fn is_handle(h: &str) -> bool {
    !h.is_empty() && h.len() <= 15 && h.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn clean_text(segs: Vec<Seg>) -> Vec<Seg> {
    let mut total = 0;
    let mut out = vec![];
    for s in segs.into_iter().take(MAX_SEGS) {
        if total >= MAX_TEXT {
            break;
        }
        let t = clip(&s.t, MAX_TEXT - total);
        total += t.len();
        let m = s.m.filter(|h| is_handle(h));
        out.push(Seg { t, m, l: s.l });
    }
    out
}

fn clean_media(v: Vec<Media>) -> Vec<Media> {
    v.into_iter()
        .filter(|m| image_url(&m.src).is_some())
        .filter(|m| m.w.is_finite() && m.h.is_finite() && m.w > 0.0 && m.h > 0.0)
        .take(4)
        .collect()
}

fn clean_avatar(a: Option<String>) -> Option<String> {
    a.filter(|s| image_url(s).is_some())
}

fn clean_time(t: Option<String>) -> Option<String> {
    t.filter(|s| s.len() <= 40 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b".:-+".contains(&b)))
}

fn clean_handle(h: String) -> String {
    if is_handle(&h) {
        h
    } else {
        String::new()
    }
}

pub fn clean(posts: Vec<Post>) -> Vec<Post> {
    posts
        .into_iter()
        .take(MAX_POSTS)
        .map(|p| Post {
            name: clip(&p.name, 200),
            handle: clean_handle(p.handle),
            avatar: clean_avatar(p.avatar),
            time: clean_time(p.time),
            text: clean_text(p.text),
            truncated: p.truncated,
            media: clean_media(p.media),
            card: p.card.map(|c| Card {
                lines: c.lines.into_iter().take(3).map(|l| clip(&l, 300)).collect(),
                img: c.img.and_then(|m| clean_media(vec![m]).pop()),
            }),
            quote: p.quote.map(|q| Quote {
                name: clip(&q.name, 200),
                handle: clean_handle(q.handle),
                avatar: clean_avatar(q.avatar),
                time: clean_time(q.time),
                text: clean_text(q.text),
                media: clean_media(q.media),
            }),
        })
        .collect()
}

/// Public post images only: `https://pbs.twimg.com/<kind>/...`.
pub fn image_url(s: &str) -> Option<Url> {
    let u = Url::parse(s).ok()?;
    if u.scheme() != "https" || u.host_str() != Some("pbs.twimg.com") || u.port().is_some() {
        return None;
    }
    if !u.username().is_empty() || u.password().is_some() || u.path().contains("..") {
        return None;
    }
    let first = u.path().trim_start_matches('/').split('/').next()?;
    matches!(
        first,
        "media" | "profile_images" | "ext_tw_video_thumb" | "amplify_video_thumb" | "tweet_video_thumb" | "card_img"
    )
    .then_some(u)
}

// ---------------------------------------------------------------- strips -> one PNG

/// Body: u32 LE count, then per strip u32 LE length + PNG bytes. All strips
/// have the same width and are stacked top to bottom.
pub fn split_strips(body: &[u8]) -> Result<Vec<&[u8]>, String> {
    let mut rest = body;
    let mut take = |n: usize| -> Result<&[u8], String> {
        if rest.len() < n {
            return Err("truncated body".into());
        }
        let (a, b) = rest.split_at(n);
        rest = b;
        Ok(a)
    };
    let count = u32::from_le_bytes(take(4)?.try_into().unwrap()) as usize;
    if count == 0 || count > 1000 {
        return Err("bad strip count".into());
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let len = u32::from_le_bytes(take(4)?.try_into().unwrap()) as usize;
        out.push(take(len)?);
    }
    Ok(out)
}

fn decode_rgba(png_bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(png_bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let (w, h) = {
        let i = reader.info();
        (i.width, i.height)
    };
    if w as u64 * h as u64 * 4 > MAX_PIXELS * 4 {
        return Err("image too large".into());
    }
    let mut buf = vec![0; reader.output_buffer_size().ok_or("image too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("unexpected indexed png".into()),
    };
    Ok((w, h, rgba))
}

/// Stack the strips into one RGBA image.
pub fn stitch(strips: &[&[u8]]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut width = 0;
    let mut height: u32 = 0;
    let mut out: Vec<u8> = vec![];
    for s in strips {
        let (w, h, rgba) = decode_rgba(s)?;
        if width == 0 {
            width = w;
        } else if w != width {
            return Err("strips differ in width".into());
        }
        height = height.checked_add(h).ok_or("image too large")?;
        if width as u64 * height as u64 > MAX_PIXELS {
            return Err("image too large".into());
        }
        out.extend_from_slice(&rgba);
    }
    Ok((width, height, out))
}

pub fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = vec![];
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut wr = enc.write_header().map_err(|e| e.to_string())?;
        wr.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

fn file_stem(s: Option<&str>) -> String {
    match s {
        Some(s) if !s.is_empty() && s.len() <= 60 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') => {
            s.to_string()
        }
        _ => "paneful_capture".into(),
    }
}

// ---------------------------------------------------------------- commands

/// From capture.js in a panel: show the preview in the host UI.
#[tauri::command]
pub async fn capture_posts(
    app: AppHandle,
    state: State<'_, AppState>,
    webview: Webview,
    posts: Vec<Post>,
    cut: bool,
) -> Result<(), String> {
    if !webview.label().starts_with("panel-") {
        return Err("not a panel".into());
    }
    let posts = clean(posts);
    if posts.is_empty() {
        return Err("no posts".into());
    }
    log::info!("capture: {} posts from {}", posts.len(), webview.label());
    *state.help.lock().unwrap() = false;
    *state.capture.lock().unwrap() = true;
    crate::relayout(&app);
    let _ = app.emit_to(HOST, "capture", CapturePayload { posts, cut });
    if let Some(host) = app.get_webview(HOST) {
        let _ = host.set_focus();
    }
    Ok(())
}

/// Host UI closed the preview.
#[tauri::command]
pub async fn capture_close(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    close(&app, &state);
    Ok(())
}

pub fn close<R: Runtime>(app: &AppHandle<R>, state: &AppState) {
    let was = std::mem::replace(&mut *state.capture.lock().unwrap(), false);
    if was {
        crate::relayout(app);
        let active = *state.active.lock().unwrap();
        if let Some(id) = active {
            crate::focus_account(app, id);
        }
    }
}

/// One public pbs.twimg.com image for the preview, without cookies.
#[tauri::command]
pub async fn capture_image(url: String) -> Result<tauri::ipc::Response, String> {
    let u = image_url(&url).ok_or("unsupported image url")?;
    let resp = media::client().get(u).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !ctype.starts_with("image/") {
        return Err(format!("unexpected content-type {ctype}"));
    }
    if resp.content_length().is_some_and(|l| l > IMAGE_MAX) {
        return Err("image too large".into());
    }
    let mut resp = resp;
    let mut buf = vec![];
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        buf.extend_from_slice(&chunk);
        if buf.len() as u64 > IMAGE_MAX {
            return Err("image too large".into());
        }
    }
    Ok(tauri::ipc::Response::new(buf))
}

/// The bytes of a binary invoke. Normally the body is raw (custom protocol
/// IPC). When the webview's custom protocol IPC has failed once, Tauri falls
/// back to postMessage for the rest of the page's life and sends a
/// Uint8Array as a JSON array of numbers instead (seen on Windows: "expected
/// raw body"). Accept both.
pub fn body_bytes(body: &InvokeBody) -> Result<Vec<u8>, String> {
    match body {
        InvokeBody::Raw(b) => Ok(b.clone()),
        InvokeBody::Json(v) => serde_json::from_value::<Vec<u8>>(v.clone()).map_err(|_| "expected PNG bytes".to_string()),
    }
}

/// Raw body = PNG strips (see `split_strips`). Headers: `x-action` = `save`,
/// `copy` or `both`; `x-name` = file name stem. Returns the saved file name
/// (empty when only copied).
#[tauri::command]
pub async fn capture_export<R: Runtime>(app: AppHandle<R>, request: tauri::ipc::Request<'_>) -> Result<String, String> {
    let body = body_bytes(request.body())?;
    let header = |k: &str| request.headers().get(k).and_then(|v| v.to_str().ok()).map(str::to_string);
    let action = header("x-action").unwrap_or_default();
    let (save, copy) = match action.as_str() {
        "save" => (true, false),
        "copy" => (false, true),
        "both" => (true, true),
        _ => return Err("bad action".into()),
    };
    let stem = file_stem(header("x-name").as_deref());

    // Decoding / encoding a tall image takes a while: off the IPC thread.
    let (w, h, rgba) = tauri::async_runtime::spawn_blocking(move || {
        let strips = split_strips(&body)?;
        stitch(&strips)
    })
    .await
    .map_err(|e| e.to_string())??;

    let mut saved = String::new();
    if copy {
        let img = tauri::image::Image::new(&rgba, w, h);
        app.clipboard().write_image(&img).map_err(|e| format!("clipboard: {e}"))?;
    }
    if save {
        let dir: PathBuf = media::save_dir(&app)?;
        let path = tauri::async_runtime::spawn_blocking(move || -> Result<PathBuf, String> {
            let png = encode_png(w, h, &rgba)?;
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let (path, mut f) = media::create_unique(&dir, &format!("{stem}.png")).map_err(|e| e.to_string())?;
            if let Err(e) = f.write_all(&png).and_then(|_| f.flush()) {
                drop(f);
                let _ = std::fs::remove_file(&path);
                return Err(e.to_string());
            }
            Ok(path)
        })
        .await
        .map_err(|e| e.to_string())??;
        log::info!("capture saved -> {}", path.display());
        saved = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    }
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_urls() {
        assert!(image_url("https://pbs.twimg.com/media/Gabc?format=jpg&name=medium").is_some());
        assert!(image_url("https://pbs.twimg.com/profile_images/1/a_bigger.jpg").is_some());
        assert!(image_url("https://pbs.twimg.com/card_img/1/x?format=jpg&name=small").is_some());
        assert!(image_url("https://pbs.twimg.com/amplify_video_thumb/1/img/x.jpg").is_some());
        assert!(image_url("http://pbs.twimg.com/media/Gabc").is_none());
        assert!(image_url("https://pbs.twimg.com:444/media/Gabc").is_none());
        assert!(image_url("https://pbs.twimg.com.evil.io/media/Gabc").is_none());
        assert!(image_url("https://pbs.twimg.com/other/Gabc").is_none());
        assert!(image_url("https://abs.twimg.com/emoji/v2/svg/1f600.svg").is_none());
        assert!(image_url("https://u:p@pbs.twimg.com/media/Gabc").is_none());
    }

    #[test]
    fn cleaning() {
        let p: Post = serde_json::from_value(serde_json::json!({
            "name": "A".repeat(500),
            "handle": "../x",
            "avatar": "https://evil.io/a.jpg",
            "time": "2024-01-01T00:00:00.000Z<script>",
            "text": [{"t": "hi "}, {"t": "@bob", "m": "bob"}, {"t": "@x", "m": "bad handle!"}],
            "media": [{"src": "https://pbs.twimg.com/media/A?name=small", "w": 4, "h": 3},
                      {"src": "javascript:alert(1)", "w": 1, "h": 1},
                      {"src": "https://pbs.twimg.com/media/B", "w": 0, "h": 3}],
            "quote": {"name": "Q", "handle": "qq", "text": [], "media": []}
        }))
        .unwrap();
        let p = &clean(vec![p])[0];
        assert_eq!(p.name.len(), 200);
        assert_eq!(p.handle, "");
        assert_eq!(p.avatar, None);
        assert_eq!(p.time, None);
        assert_eq!(p.text[1].m.as_deref(), Some("bob"));
        assert_eq!(p.text[2].m, None);
        assert_eq!(p.media.len(), 1);
        assert_eq!(p.quote.as_ref().unwrap().handle, "qq");
        assert_eq!(clip("가나다", 4), "가");
    }

    fn strip(w: u32, h: u32, v: u8) -> Vec<u8> {
        encode_png(w, h, &vec![v; (w * h * 4) as usize]).unwrap()
    }

    #[test]
    fn stitching() {
        let a = strip(3, 2, 10);
        let b = strip(3, 1, 20);
        let mut body = 2u32.to_le_bytes().to_vec();
        for s in [&a, &b] {
            body.extend_from_slice(&(s.len() as u32).to_le_bytes());
            body.extend_from_slice(s);
        }
        let strips = split_strips(&body).unwrap();
        let (w, h, rgba) = stitch(&strips).unwrap();
        assert_eq!((w, h), (3, 3));
        assert_eq!(rgba.len(), 36);
        assert_eq!(rgba[0], 10);
        assert_eq!(rgba[35], 20);

        let c = strip(4, 1, 0);
        assert!(stitch(&[&a, &c]).is_err());
        assert!(split_strips(&body[..body.len() - 1]).is_err());
        assert!(split_strips(&0u32.to_le_bytes()).is_err());
    }

    #[test]
    fn bodies() {
        assert_eq!(body_bytes(&InvokeBody::Raw(vec![1, 2, 255])).unwrap(), vec![1, 2, 255]);
        assert_eq!(body_bytes(&InvokeBody::Json(serde_json::json!([1, 2, 255]))).unwrap(), vec![1, 2, 255]);
        assert!(body_bytes(&InvokeBody::Json(serde_json::json!([1, 256]))).is_err());
        assert!(body_bytes(&InvokeBody::Json(serde_json::json!({"a": 1}))).is_err());
    }

    #[test]
    fn stems() {
        assert_eq!(file_stem(Some("paneful_20261118-0942")), "paneful_20261118-0942");
        assert_eq!(file_stem(Some("../x")), "paneful_capture");
        assert_eq!(file_stem(None), "paneful_capture");
    }
}
