//! Paneful M0 spike: one window, one local host-UI webview underneath,
//! N x.com panel webviews on top, each with its own data store.
//!
//! - macOS 14+: `data_store_identifier` -> WKWebsiteDataStore(forIdentifier:)
//! - Windows: same call; the patched tauri-runtime-wry turns it into a
//!   WebView2 profile name, so all panels share one WebView2 runtime.

mod layout;
mod nav;
mod settings;

use serde::Serialize;
use settings::{Account, Settings, COLORS};
use std::{path::PathBuf, sync::Mutex};
use tauri::{
    webview::{NewWindowResponse, WebviewBuilder},
    window::WindowBuilder,
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Runtime, State, WebviewUrl, Window,
    WindowEvent,
};
use tauri_plugin_opener::OpenerExt;

const MAIN: &str = "main";
const HOST: &str = "host";
const START_URL: &str = "https://x.com/home";

struct AppState {
    settings: Mutex<Settings>,
    path: PathBuf,
}

impl AppState {
    fn save(&self, s: &Settings) {
        if let Err(e) = s.save(&self.path) {
            log::error!("saving settings failed: {e}");
        }
    }
}

fn panel_label(acc: &Account) -> String {
    format!("panel-{}", acc.id.simple())
}

#[derive(Serialize, Clone)]
struct PanelView {
    id: String,
    label: String,
    color: String,
    rect: layout::Rect,
}

#[derive(Serialize, Clone)]
struct LayoutPayload {
    win_w: f64,
    win_h: f64,
    top_bar: f64,
    gap: f64,
    min_panel: f64,
    ratios: Vec<f64>,
    panels: Vec<PanelView>,
}

fn logical_size<R: Runtime>(window: &Window<R>) -> (f64, f64) {
    let scale = window.scale_factor().unwrap_or(1.0);
    match window.inner_size() {
        Ok(s) => {
            let l = s.to_logical::<f64>(scale);
            (l.width, l.height)
        }
        Err(_) => (1280.0, 860.0),
    }
}

/// Recompute every rect, move the webviews, tell the host UI.
fn relayout<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_window(MAIN) else { return };
    let state = app.state::<AppState>();
    let s = state.settings.lock().unwrap().clone();
    let (w, h) = logical_size(&window);
    let rects = layout::panels(w, h, &s.ratios);

    if let Some(host) = app.get_webview(HOST) {
        let _ = host.set_bounds(tauri::Rect {
            position: LogicalPosition::new(0.0, 0.0).into(),
            size: LogicalSize::new(w, h).into(),
        });
    }
    let mut panels = vec![];
    for (acc, r) in s.accounts.iter().zip(rects.iter()) {
        if let Some(wv) = app.get_webview(&panel_label(acc)) {
            let _ = wv.set_bounds(tauri::Rect {
                position: LogicalPosition::new(r.x, r.y).into(),
                size: LogicalSize::new(r.w, r.h).into(),
            });
        }
        panels.push(PanelView {
            id: acc.id.to_string(),
            label: acc.label.clone(),
            color: acc.color.clone(),
            rect: *r,
        });
    }
    let payload = LayoutPayload {
        win_w: w,
        win_h: h,
        top_bar: layout::TOP_BAR,
        gap: layout::GAP,
        min_panel: layout::MIN_PANEL,
        ratios: s.ratios.clone(),
        panels,
    };
    let _ = app.emit_to(HOST, "layout", payload);
}

fn add_panel<R: Runtime>(window: &Window<R>, acc: &Account, rect: layout::Rect) -> tauri::Result<()> {
    let app = window.app_handle().clone();
    let app2 = app.clone();
    let label = panel_label(acc);
    let label2 = label.clone();

    let builder = WebviewBuilder::new(&label, WebviewUrl::External(START_URL.parse().unwrap()))
        .data_store_identifier(acc.store_id())
        .on_navigation(move |url| match nav::decide(url) {
            nav::Decision::Allow => true,
            nav::Decision::OpenExternal => {
                let _ = app.opener().open_url(url.as_str(), None::<&str>);
                false
            }
            nav::Decision::Block => false,
        })
        // target=_blank / window.open: never spawn native windows.
        // Allowed hosts load in the same panel, everything else goes to the
        // system browser.
        .on_new_window(move |url, _features| {
            match nav::decide(&url) {
                nav::Decision::Allow => {
                    if let Some(wv) = app2.get_webview(&label2) {
                        let _ = wv.navigate(url);
                    }
                }
                nav::Decision::OpenExternal => {
                    let _ = app2.opener().open_url(url.as_str(), None::<&str>);
                }
                nav::Decision::Block => {}
            }
            NewWindowResponse::Deny
        });

    let wv = window.add_child(
        builder,
        LogicalPosition::new(rect.x, rect.y),
        LogicalSize::new(rect.w, rect.h),
    )?;

    #[cfg(windows)]
    log_webview2_profile(&wv);
    #[cfg(not(windows))]
    let _ = wv;
    Ok(())
}

/// M0 check: confirm the patch really put each panel in its own WebView2
/// profile (ProfileName == account uuid hex, distinct ProfilePath).
#[cfg(windows)]
fn log_webview2_profile<R: Runtime>(wv: &tauri::Webview<R>) {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_13;
    use windows::core::Interface;
    let label = wv.label().to_string();
    let _ = wv.with_webview(move |pw| unsafe {
        let res = (|| -> windows::core::Result<(String, String)> {
            let core = pw.controller().CoreWebView2()?;
            let core13: ICoreWebView2_13 = core.cast()?;
            let profile = core13.Profile()?;
            let mut name = windows::core::PWSTR::null();
            profile.ProfileName(&mut name)?;
            let mut path = windows::core::PWSTR::null();
            profile.ProfilePath(&mut path)?;
            Ok((
                webview2_com::take_pwstr(name),
                webview2_com::take_pwstr(path),
            ))
        })();
        match res {
            Ok((name, path)) => log::info!("[{label}] WebView2 profile name={name} path={path}"),
            Err(e) => log::warn!("[{label}] could not read WebView2 profile: {e}"),
        }
    });
}

/// Windows: delete profile folders of removed accounts before any webview
/// starts. Folder layout `<udf>/EBWebView/<ProfileName>` is 추론 and is
/// confirmed by the ProfilePath log above during M0.
#[cfg(windows)]
fn purge_pending_profiles(app: &tauri::App, s: &mut Settings) {
    let Ok(udf) = app.path().app_local_data_dir() else { return };
    s.pending_profile_deletes.retain(|name| {
        let dir = udf.join("EBWebView").join(name);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => false,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => {
                log::warn!("profile {name} not deleted yet: {e}");
                true
            }
        }
    });
}

// ---------------------------------------------------------------- commands
// Only the host webview has IPC (capabilities/host.json). Commands are async
// because add_child from a sync command deadlocks on WebView2.

#[tauri::command]
async fn refresh_layout(app: AppHandle) {
    relayout(&app);
}

#[tauri::command]
async fn set_ratios(app: AppHandle, state: State<'_, AppState>, ratios: Vec<f64>) -> Result<(), String> {
    let Some(window) = app.get_window(MAIN) else { return Err("no window".into()) };
    {
        let mut s = state.settings.lock().unwrap();
        if ratios.len() != s.accounts.len() || ratios.iter().any(|r| !r.is_finite() || *r <= 0.0) {
            return Err("bad ratios".into());
        }
        let (w, _) = logical_size(&window);
        s.ratios = layout::clamp_ratios(w, &ratios);
    }
    relayout(&app);
    Ok(())
}

/// Persist ratios once a drag ends (set_ratios fires on every pointer move).
#[tauri::command]
async fn commit_layout(state: State<'_, AppState>) -> Result<(), String> {
    let s = state.settings.lock().unwrap().clone();
    state.save(&s);
    Ok(())
}

#[tauri::command]
async fn reload_panel(app: AppHandle, id: String) -> Result<(), String> {
    let label = format!("panel-{}", id.replace('-', ""));
    let wv = app.get_webview(&label).ok_or("no such panel")?;
    wv.reload().map_err(|e| e.to_string())
}

#[tauri::command]
async fn add_account(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let window = app.get_window(MAIN).ok_or("no window")?;
    let acc = {
        let mut s = state.settings.lock().unwrap();
        let n = s.accounts.len();
        let acc = Account::new(format!("계정 {}", n + 1), COLORS[n % COLORS.len()]);
        s.accounts.push(acc.clone());
        s.ratios = vec![1.0; n + 1];
        s.fix_ratios();
        state.save(&s);
        acc
    };
    let rect = layout::Rect { x: 0.0, y: layout::TOP_BAR, w: 10.0, h: 10.0 };
    add_panel(&window, &acc, rect).map_err(|e| e.to_string())?;
    relayout(&app);
    Ok(())
}

#[tauri::command]
async fn remove_account(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let acc = {
        let mut s = state.settings.lock().unwrap();
        let idx = s
            .accounts
            .iter()
            .position(|a| a.id.to_string() == id)
            .ok_or("no such account")?;
        let acc = s.accounts.remove(idx);
        s.ratios = vec![1.0; s.accounts.len()];
        s.fix_ratios();
        #[cfg(windows)]
        s.pending_profile_deletes.push(acc.profile_name());
        state.save(&s);
        acc
    };
    if let Some(wv) = app.get_webview(&panel_label(&acc)) {
        wv.close().map_err(|e| e.to_string())?;
    }
    relayout(&app);

    // macOS: drop the WKWebsiteDataStore now (M0 check item).
    #[cfg(target_os = "macos")]
    {
        if let Err(e) = app.remove_data_store(acc.store_id()).await {
            log::warn!("remove_data_store failed: {e}");
            return Err(format!("계정은 지웠는데 저장소 삭제 실패: {e}"));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- entry

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("paneful".into()),
                    }),
                ])
                .build(),
        )
        .setup(|app| {
            let path = app.path().app_config_dir()?.join("settings.json");
            #[allow(unused_mut)]
            let mut s = Settings::load(&path);
            #[cfg(windows)]
            purge_pending_profiles(app, &mut s);
            s.save(&path)?;
            app.manage(AppState {
                settings: Mutex::new(s.clone()),
                path,
            });

            let window = WindowBuilder::new(app, MAIN)
                .title("Paneful")
                .inner_size(1280.0, 860.0)
                .min_inner_size(400.0, 300.0)
                .build()?;
            let (w, h) = logical_size(&window);

            // Host UI first so the panels stack above it; it stays visible
            // in the top bar and in the gaps between panels.
            window.add_child(
                WebviewBuilder::new(HOST, WebviewUrl::App("index.html".into())),
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(w, h),
            )?;

            let rects = layout::panels(w, h, &s.ratios);
            for (acc, r) in s.accounts.iter().zip(rects) {
                add_panel(&window, acc, r)?;
            }

            let handle = app.handle().clone();
            window.on_window_event(move |e| {
                if matches!(e, WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }) {
                    relayout(&handle);
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            refresh_layout,
            set_ratios,
            commit_layout,
            reload_panel,
            add_account,
            remove_account
        ])
        .run(tauri::generate_context!())
        .expect("error while running paneful");
}
