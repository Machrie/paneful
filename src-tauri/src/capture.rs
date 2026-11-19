//! Post capture: pick the first and last post in a panel and get one PNG of
//! that range as X shows it, with profile pictures, names and @handles
//! swapped for 익명N.
//!
//! 1. capture.js (panel) scrolls through the range. Before each step it
//!    hides X's sticky/fixed bars and lays masks (익명N) over avatars,
//!    names and handles in its own overlay, then calls `capture_shot`
//!    twice: once plain, once masked, each with the rectangles of the
//!    posts (or parts of a tall post) visible in that frame.
//! 2. `capture_shot` takes a screenshot of the panel (snap.rs), crops the
//!    rectangles out right away and keeps only those. If the page moved
//!    while the shot was taken, capture.js calls `capture_drop` and retries,
//!    so a frame where the masks might not line up is never used.
//! 3. `capture_finish` stacks the crops top to bottom (one image with masks,
//!    one without), hides the panels and opens the preview in the host UI.
//! 4. The preview (ui/capture.js) shows `capture_slice`s of either image;
//!    `capture_export` saves it to the save folder (media::save_dir) and/or
//!    copies it to the clipboard.

use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
use tauri::{AppHandle, Emitter, Manager, Runtime, State, Webview};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::{media, snap, AppState, HOST};

pub const SCRIPT: &str = include_str!("capture.js");

/// Per version (masked / plain). 40M px is e.g. 1200 x 33000.
const MAX_PIXELS: u64 = 40_000_000;
const MAX_CROPS_PER_SHOT: usize = 64;

/// One rectangle of a screenshot, in CSS px of the panel's viewport.
#[derive(Deserialize, Debug, Clone, Copy)]
pub struct Crop {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Img {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

struct Part {
    shot: u32,
    anon: bool,
    img: Img,
}

struct Session {
    label: String,
    next: u32,
    parts: Vec<Part>,
}

struct Done {
    anon: Img,
    plain: Img,
}

fn session() -> &'static Mutex<Option<Session>> {
    static S: OnceLock<Mutex<Option<Session>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

fn done() -> &'static Mutex<Option<Done>> {
    static D: OnceLock<Mutex<Option<Done>>> = OnceLock::new();
    D.get_or_init(Default::default)
}

// ---------------------------------------------------------------- pixels

fn decode_rgba(png_bytes: &[u8]) -> Result<Img, String> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(png_bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let (w, h) = {
        let i = reader.info();
        (i.width, i.height)
    };
    if w as u64 * h as u64 > 100_000_000 {
        return Err("screenshot too large".into());
    }
    let mut buf = vec![0; reader.output_buffer_size().ok_or("screenshot too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("unexpected indexed png".into()),
    };
    Ok(Img { w, h, rgba })
}

pub fn encode_png(img: &Img) -> Result<Vec<u8>, String> {
    let mut out = vec![];
    {
        let mut enc = png::Encoder::new(&mut out, img.w, img.h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut wr = enc.write_header().map_err(|e| e.to_string())?;
        wr.write_image_data(&img.rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Cut a CSS-px rectangle out of a screenshot taken of a `view_w` CSS px
/// wide viewport. None when the rectangle is empty after clamping.
pub fn crop(shot: &Img, view_w: f64, c: Crop) -> Option<Img> {
    if !view_w.is_finite() || view_w <= 0.0 || ![c.x, c.y, c.w, c.h].iter().all(|v| v.is_finite()) {
        return None;
    }
    let scale = shot.w as f64 / view_w;
    let px = |v: f64, max: u32| (v * scale).round().clamp(0.0, max as f64) as u32;
    let (x0, x1) = (px(c.x, shot.w), px(c.x + c.w, shot.w));
    let (y0, y1) = (px(c.y, shot.h), px(c.y + c.h, shot.h));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y1 {
        let row = ((y * shot.w + x0) * 4) as usize;
        rgba.extend_from_slice(&shot.rgba[row..row + (w * 4) as usize]);
    }
    Some(Img { w, h, rgba })
}

/// Stack top to bottom. Width = the first part's; a part of another width
/// (display scale changed mid-capture) is cut or padded with black.
pub fn stack<'a>(parts: impl Iterator<Item = &'a Img>) -> Option<Img> {
    let mut out: Option<Img> = None;
    for p in parts {
        let o = out.get_or_insert_with(|| Img { w: p.w, h: 0, rgba: vec![] });
        let w = o.w as usize;
        for y in 0..p.h as usize {
            let row = &p.rgba[y * p.w as usize * 4..(y + 1) * p.w as usize * 4];
            let n = row.len().min(w * 4);
            o.rgba.extend_from_slice(&row[..n]);
            for _ in 0..(w * 4 - n) / 4 {
                o.rgba.extend_from_slice(&[0, 0, 0, 255]);
            }
        }
        o.h += p.h;
    }
    out
}

/// Rows [y, y+h) of an image.
fn rows(img: &Img, y: u32, h: u32) -> Img {
    let y = y.min(img.h);
    let h = h.min(img.h - y);
    let a = (y * img.w * 4) as usize;
    let b = ((y + h) * img.w * 4) as usize;
    Img { w: img.w, h, rgba: img.rgba[a..b].to_vec() }
}

fn file_stem(s: Option<&str>) -> String {
    match s {
        Some(s) if !s.is_empty() && s.len() <= 60 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') => {
            s.to_string()
        }
        _ => "paneful_capture".into(),
    }
}

// ---------------------------------------------------------------- panel commands

fn panel_only(wv: &Webview) -> Result<(), String> {
    if wv.label().starts_with("panel-") {
        Ok(())
    } else {
        Err("not a panel".into())
    }
}

/// A new capture in this panel (drops any unfinished one).
#[tauri::command]
pub async fn capture_begin(webview: Webview) -> Result<(), String> {
    panel_only(&webview)?;
    *session().lock().unwrap() = Some(Session { label: webview.label().to_string(), next: 1, parts: vec![] });
    Ok(())
}

#[derive(Serialize)]
pub struct ShotResult {
    id: u32,
    /// Size limit reached: this shot was kept, stop after it.
    full: bool,
}

/// Screenshot the calling panel and keep `crops` of it (in order).
#[tauri::command]
pub async fn capture_shot(webview: Webview, view_w: f64, anon: bool, crops: Vec<Crop>) -> Result<ShotResult, String> {
    panel_only(&webview)?;
    if crops.is_empty() || crops.len() > MAX_CROPS_PER_SHOT {
        return Err("bad crops".into());
    }
    {
        let s = session().lock().unwrap();
        if s.as_ref().is_none_or(|s| s.label != webview.label()) {
            return Err("no capture in progress".into());
        }
    }
    let png = snap::png(&webview).await?;
    let imgs = tauri::async_runtime::spawn_blocking(move || -> Result<Vec<Img>, String> {
        let shot = decode_rgba(&png)?;
        Ok(crops.into_iter().filter_map(|c| crop(&shot, view_w, c)).collect())
    })
    .await
    .map_err(|e| e.to_string())??;
    if imgs.is_empty() {
        return Err("nothing to crop".into());
    }
    let mut guard = session().lock().unwrap();
    let s = guard.as_mut().filter(|s| s.label == webview.label()).ok_or("no capture in progress")?;
    let id = s.next;
    s.next += 1;
    for img in imgs {
        s.parts.push(Part { shot: id, anon, img });
    }
    let px: u64 = s.parts.iter().filter(|p| p.anon == anon).map(|p| p.img.w as u64 * p.img.h as u64).sum();
    Ok(ShotResult { id, full: px >= MAX_PIXELS })
}

/// The page moved while shot `id` was taken: forget its crops.
#[tauri::command]
pub async fn capture_drop(webview: Webview, id: u32) -> Result<(), String> {
    panel_only(&webview)?;
    if let Some(s) = session().lock().unwrap().as_mut().filter(|s| s.label == webview.label()) {
        s.parts.retain(|p| p.shot != id);
    }
    Ok(())
}

#[derive(Serialize, Clone)]
struct Preview {
    w: u32,
    h: u32,
    /// Width in CSS px (for the preview's display size).
    css_w: f64,
    /// People numbered 익명1..N.
    people: u32,
    /// Stopped early (size limit or end of page).
    cut: bool,
}

/// Stack the crops and open the preview. Called with `cancel` to just end.
#[tauri::command]
pub async fn capture_finish(
    app: AppHandle,
    state: State<'_, AppState>,
    webview: Webview,
    css_w: f64,
    people: u32,
    cut: bool,
    cancel: bool,
) -> Result<(), String> {
    panel_only(&webview)?;
    let s = {
        let mut g = session().lock().unwrap();
        match g.take() {
            Some(s) if s.label == webview.label() => s,
            other => {
                *g = other;
                return Err("no capture in progress".into());
            }
        }
    };
    if cancel {
        return Ok(());
    }
    let (anon, plain) = tauri::async_runtime::spawn_blocking(move || {
        let anon = stack(s.parts.iter().filter(|p| p.anon).map(|p| &p.img));
        let plain = stack(s.parts.iter().filter(|p| !p.anon).map(|p| &p.img));
        (anon, plain)
    })
    .await
    .map_err(|e| e.to_string())?;
    let (Some(anon), Some(plain)) = (anon, plain) else {
        return Err("nothing captured".into());
    };
    let preview = Preview { w: anon.w, h: anon.h, css_w, people, cut };
    log::info!("capture: {}x{} from {}", anon.w, anon.h, webview.label());
    *done().lock().unwrap() = Some(Done { anon, plain });

    *state.help.lock().unwrap() = false;
    *state.settings_open.lock().unwrap() = false;
    crate::tray::RECORDING.store(false, std::sync::atomic::Ordering::Relaxed);
    *state.capture.lock().unwrap() = true;
    crate::relayout(&app);
    let _ = app.emit_to(HOST, "capture", preview);
    if let Some(host) = app.get_webview(HOST) {
        let _ = host.set_focus();
    }
    Ok(())
}

// ---------------------------------------------------------------- host commands

/// Host UI closed the preview.
#[tauri::command]
pub async fn capture_close(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    close(&app, &state);
    Ok(())
}

pub fn close<R: Runtime>(app: &AppHandle<R>, state: &AppState) {
    let was = std::mem::replace(&mut *state.capture.lock().unwrap(), false);
    *done().lock().unwrap() = None;
    if was {
        crate::relayout(app);
        let active = *state.active.lock().unwrap();
        if let Some(id) = active {
            crate::focus_account(app, id);
        }
    }
}

/// Rows of the finished image as PNG, for the preview.
#[tauri::command]
pub async fn capture_slice(anon: bool, y: u32, h: u32) -> Result<tauri::ipc::Response, String> {
    let part = {
        let d = done().lock().unwrap();
        let d = d.as_ref().ok_or("no capture")?;
        rows(if anon { &d.anon } else { &d.plain }, y, h.min(8000))
    };
    let png = tauri::async_runtime::spawn_blocking(move || encode_png(&part))
        .await
        .map_err(|e| e.to_string())??;
    Ok(tauri::ipc::Response::new(png))
}

/// `action`: save, copy or both. `name`: file name stem. Returns the saved
/// file name (empty when only copied).
#[tauri::command]
pub async fn capture_export<R: Runtime>(app: AppHandle<R>, action: String, anon: bool, name: String) -> Result<String, String> {
    let (save, copy) = match action.as_str() {
        "save" => (true, false),
        "copy" => (false, true),
        "both" => (true, true),
        _ => return Err("bad action".into()),
    };
    let stem = file_stem(Some(&name));
    let img = {
        let d = done().lock().unwrap();
        let d = d.as_ref().ok_or("no capture")?;
        if anon { d.anon.clone() } else { d.plain.clone() }
    };

    if copy {
        let ti = tauri::image::Image::new(&img.rgba, img.w, img.h);
        app.clipboard().write_image(&ti).map_err(|e| format!("clipboard: {e}"))?;
    }
    let mut saved = String::new();
    if save {
        let dir: PathBuf = media::save_dir(&app)?;
        let path = tauri::async_runtime::spawn_blocking(move || -> Result<PathBuf, String> {
            let png = encode_png(&img)?;
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

    /// w x h image whose pixel (x, y) is [x, y, 0, 255].
    fn grid(w: u32, h: u32) -> Img {
        let mut rgba = vec![];
        for y in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(&[x as u8, y as u8, 0, 255]);
            }
        }
        Img { w, h, rgba }
    }

    #[test]
    fn cropping() {
        // 20x10 px screenshot of a 10 CSS px wide viewport: scale 2.
        let shot = grid(20, 10);
        let c = crop(&shot, 10.0, Crop { x: 1.0, y: 2.0, w: 3.0, h: 1.5 }).unwrap();
        assert_eq!((c.w, c.h), (6, 3));
        assert_eq!(&c.rgba[..4], &[2, 4, 0, 255]);
        assert_eq!(&c.rgba[c.rgba.len() - 4..], &[7, 6, 0, 255]);
        // clamped to the screenshot
        let c = crop(&shot, 10.0, Crop { x: -5.0, y: 4.0, w: 100.0, h: 100.0 }).unwrap();
        assert_eq!((c.w, c.h), (20, 2));
        assert!(crop(&shot, 10.0, Crop { x: 0.0, y: 20.0, w: 5.0, h: 5.0 }).is_none());
        assert!(crop(&shot, 0.0, Crop { x: 0.0, y: 0.0, w: 5.0, h: 5.0 }).is_none());
        assert!(crop(&shot, 10.0, Crop { x: f64::NAN, y: 0.0, w: 5.0, h: 5.0 }).is_none());
    }

    #[test]
    fn stacking() {
        let a = grid(3, 2);
        let b = grid(3, 1);
        let s = stack([&a, &b].into_iter()).unwrap();
        assert_eq!((s.w, s.h), (3, 3));
        assert_eq!(&s.rgba[2 * 12..2 * 12 + 4], &[0, 0, 0, 255]);
        // other widths: cut / padded
        let s = stack([&a, &grid(5, 1), &grid(1, 1)].into_iter()).unwrap();
        assert_eq!((s.w, s.h), (3, 4));
        assert_eq!(s.rgba.len(), 3 * 4 * 4);
        assert_eq!(&s.rgba[3 * 12 + 4..3 * 12 + 8], &[0, 0, 0, 255]);
        assert!(stack(std::iter::empty()).is_none());
        let r = rows(&grid(3, 4), 1, 10);
        assert_eq!((r.w, r.h), (3, 3));
        assert_eq!(r.rgba[1], 1);
    }

    #[test]
    fn png_roundtrip() {
        let g = grid(4, 3);
        assert_eq!(decode_rgba(&encode_png(&g).unwrap()).unwrap(), g);
    }

    #[test]
    fn stems() {
        assert_eq!(file_stem(Some("paneful_20261118-0942")), "paneful_20261118-0942");
        assert_eq!(file_stem(Some("../x")), "paneful_capture");
        assert_eq!(file_stem(None), "paneful_capture");
    }
}
