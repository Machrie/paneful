//! Screenshot of what one panel webview shows right now (its viewport), as
//! PNG bytes. Tauri has no API for this, so each OS gets its own:
//!
//! - Windows: `ICoreWebView2::CapturePreview` (PNG into a memory stream)
//! - macOS: `WKWebView takeSnapshotWithConfiguration:` (NSImage -> PNG)
//!
//! Both run on the main thread through `with_webview` and report back over
//! a channel; the caller waits off the main thread.

use std::{sync::mpsc, time::Duration};
use tauri::{Runtime, Webview};

const TIMEOUT: Duration = Duration::from_secs(10);

pub async fn png<R: Runtime>(wv: &Webview<R>) -> Result<Vec<u8>, String> {
    let (tx, rx) = mpsc::channel::<Result<Vec<u8>, String>>();
    start(wv, tx)?;
    tauri::async_runtime::spawn_blocking(move || match rx.recv_timeout(TIMEOUT) {
        Ok(r) => r,
        Err(_) => Err("screenshot timed out".into()),
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(windows)]
fn start<R: Runtime>(wv: &Webview<R>, tx: mpsc::Sender<Result<Vec<u8>, String>>) -> Result<(), String> {
    use webview2_com::{
        CapturePreviewCompletedHandler,
        Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
    };
    use windows::Win32::{
        System::Com::{IStream, STREAM_SEEK_SET},
        UI::Shell::SHCreateMemStream,
    };

    fn read_all(stream: &IStream) -> Result<Vec<u8>, String> {
        unsafe {
            stream.Seek(0, STREAM_SEEK_SET, None).map_err(|e| e.to_string())?;
            let mut out = vec![];
            let mut buf = vec![0u8; 1 << 16];
            loop {
                let mut n = 0u32;
                stream
                    .Read(buf.as_mut_ptr().cast(), buf.len() as u32, Some(&mut n))
                    .ok()
                    .map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                out.extend_from_slice(&buf[..n as usize]);
            }
            Ok(out)
        }
    }

    wv.with_webview(move |pw| {
        let res = (|| -> Result<(), String> {
            unsafe {
                let core = pw.controller().CoreWebView2().map_err(|e| e.to_string())?;
                let stream = SHCreateMemStream(None).ok_or("no memory stream")?;
                let s2 = stream.clone();
                let tx2 = tx.clone();
                let handler = CapturePreviewCompletedHandler::create(Box::new(move |r| {
                    let out = r.map_err(|e| e.to_string()).and_then(|_| read_all(&s2));
                    let _ = tx2.send(out);
                    Ok(())
                }));
                core.CapturePreview(COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, &stream, &handler)
                    .map_err(|e| e.to_string())
            }
        })();
        if let Err(e) = res {
            let _ = tx.send(Err(e));
        }
    })
    .map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn start<R: Runtime>(wv: &Webview<R>, tx: mpsc::Sender<Result<Vec<u8>, String>>) -> Result<(), String> {
    use block2::RcBlock;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSDictionary, NSError};
    use objc2_web_kit::WKWebView;

    fn to_png(img: &NSImage) -> Result<Vec<u8>, String> {
        let tiff = img.TIFFRepresentation().ok_or("no TIFF representation")?;
        let rep = NSBitmapImageRep::imageRepWithData(&tiff).ok_or("no bitmap")?;
        let png = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }
            .ok_or("PNG encoding failed")?;
        Ok(png.to_vec())
    }

    wv.with_webview(move |pw| unsafe {
        let view: &WKWebView = &*(pw.inner() as *const WKWebView);
        let block = RcBlock::new(move |img: *mut NSImage, err: *mut NSError| {
            let out = match img.as_ref() {
                Some(img) => to_png(img),
                None => Err(err
                    .as_ref()
                    .map(|e| e.localizedDescription().to_string())
                    .unwrap_or_else(|| "snapshot failed".into())),
            };
            let _ = tx.send(out);
        });
        view.takeSnapshotWithConfiguration_completionHandler(None, &block);
    })
    .map_err(|e| e.to_string())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn start<R: Runtime>(_wv: &Webview<R>, _tx: mpsc::Sender<Result<Vec<u8>, String>>) -> Result<(), String> {
    Err("screenshots are only supported on Windows and macOS".into())
}
