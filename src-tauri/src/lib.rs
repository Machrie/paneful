//! Paneful M0 spike: one window, one local host-UI webview underneath,
//! N x.com panel webviews on top, each with its own data store.
//!
//! - macOS 14+: `data_store_identifier` -> WKWebsiteDataStore(forIdentifier:)
//! - Windows: same call; the patched tauri-runtime-wry turns it into a
//!   WebView2 profile name, so all panels share one WebView2 runtime.

mod layout;
mod media;
mod nav;
mod settings;
mod tray;

use serde::Serialize;
use settings::{Account, Persist, Settings, COLORS};
use std::{path::PathBuf, sync::Mutex};
use uuid::Uuid;
use tauri::{
    webview::{NewWindowResponse, WebviewBuilder},
    window::WindowBuilder,
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Runtime, State, Webview, WebviewUrl,
    Window, WindowEvent,
};
use tauri_plugin_opener::OpenerExt;

pub(crate) const MAIN: &str = "main";
const HOST: &str = "host";
const START_URL: &str = "https://x.com/home";
/// Injected into every panel's main frame: reports focus and the app's own
/// shortcuts. The panel capability lets x.com reach only those two commands.
const PANEL_JS: &str = include_str!("panel.js");

struct AppState {
    settings: Mutex<Settings>,
    /// Account whose panel last had keyboard focus. Not persisted.
    active: Mutex<Option<Uuid>>,
    /// Shortcut overlay open: panels are hidden so the host UI can draw it.
    help: Mutex<bool>,
    /// Global show/hide shortcut as registered (None: off or failed).
    hotkey: Option<String>,
    /// Why the shortcut isn't registered, shown once in the host UI.
    hotkey_error: Option<String>,
    path: PathBuf,
    persist: Persist,
}

impl AppState {
    fn save(&self, s: &Settings) {
        if self.persist == Persist::No {
            log::warn!("settings.json was unreadable at start; not overwriting it");
            return;
        }
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
    active: bool,
    rect: layout::Rect,
}

#[derive(Serialize, Clone)]
struct LayoutPayload {
    win_w: f64,
    win_h: f64,
    /// Top bar shown (else only the thin strip).
    bar: bool,
    /// Shortcut overlay open.
    help: bool,
    top: f64,
    gap: f64,
    min_panel: f64,
    ratios: Vec<f64>,
    panels: Vec<PanelView>,
    /// Global show/hide shortcut, as key caps for the overlay. Empty: none.
    hotkey: Vec<String>,
    hotkey_error: Option<String>,
}

/// Bar is forced on with no accounts, otherwise "+ 계정" would be unreachable
/// except by shortcut.
fn bar_visible(s: &Settings) -> bool {
    s.show_bar || s.accounts.is_empty()
}

fn top_of(s: &Settings) -> f64 {
    if bar_visible(s) {
        layout::TOP_BAR
    } else {
        layout::TOP_STRIP
    }
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
    let active = *state.active.lock().unwrap();
    let help = *state.help.lock().unwrap();
    let (w, h) = logical_size(&window);
    let top = top_of(&s);
    let rects = layout::panels(w, h, top, &s.ratios);

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
            // Native webviews sit above the host UI; the overlay can only
            // show while they are hidden.
            let _ = if help { wv.hide() } else { wv.show() };
        }
        panels.push(PanelView {
            id: acc.id.to_string(),
            label: acc.label.clone(),
            color: acc.color.clone(),
            active: active == Some(acc.id),
            rect: *r,
        });
    }
    let payload = LayoutPayload {
        win_w: w,
        win_h: h,
        bar: bar_visible(&s),
        help,
        top,
        gap: layout::GAP,
        min_panel: layout::MIN_PANEL,
        ratios: s.ratios.clone(),
        panels,
        hotkey: state.hotkey.as_deref().map(tray::keys).unwrap_or_default(),
        hotkey_error: state.hotkey_error.clone(),
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
        .initialization_script(PANEL_JS)
        .initialization_script(media::SCRIPT)
        .on_navigation(move |url| match nav::decide(url) {
            nav::Decision::Allow => true,
            nav::Decision::OpenExternal => {
                // Windows: WebView2 NavigationStarting is top-level only, so
                // this is a real page navigation and goes to the system browser.
                // macOS: wry runs this for iframes too, so opening externally
                // would pop a browser tab per off-list iframe. Just cancel.
                #[cfg(windows)]
                let _ = app.opener().open_url(url.as_str(), None::<&str>);
                #[cfg(not(windows))]
                log::info!("blocked off-list navigation: {url}");
                false
            }
            nav::Decision::Block => false,
        })
        // target=_blank / window.open: no native windows except sign-in
        // popups (they need window.opener). Other allowed hosts load in the
        // same panel, everything else goes to the system browser.
        .on_new_window(move |url, _features| {
            // "원본 저장" button in the panel (see media.rs).
            if media::is_trigger(&url) {
                media::handle(app2.clone(), label2.clone(), url);
                return NewWindowResponse::Deny;
            }
            if nav::is_auth_popup(&url) {
                // Same WebView2 profile / WKWebViewConfiguration as the panel,
                // so the login lands in this account's store.
                return NewWindowResponse::Allow;
            }
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

fn apply_ratios(app: &AppHandle, state: &AppState, ratios: &[f64]) -> Result<Settings, String> {
    let window = app.get_window(MAIN).ok_or("no window")?;
    let snapshot = {
        let mut s = state.settings.lock().unwrap();
        if ratios.len() != s.accounts.len() || ratios.iter().any(|r| !r.is_finite() || *r <= 0.0) {
            return Err("bad ratios".into());
        }
        let (w, _) = logical_size(&window);
        s.ratios = layout::clamp_ratios(w, ratios);
        s.clone()
    };
    relayout(app);
    Ok(snapshot)
}

/// Live update while dragging (not saved).
#[tauri::command]
async fn set_ratios(app: AppHandle, state: State<'_, AppState>, ratios: Vec<f64>) -> Result<(), String> {
    apply_ratios(&app, &state, &ratios).map(|_| ())
}

/// Drag ended: apply the final ratios and persist them in one step, so a
/// late set_ratios can't be saved over (or under) the final position.
#[tauri::command]
async fn commit_layout(app: AppHandle, state: State<'_, AppState>, ratios: Vec<f64>) -> Result<(), String> {
    let s = apply_ratios(&app, &state, &ratios)?;
    state.save(&s);
    Ok(())
}

/// Give keyboard focus to the panel of `id`, so X's own shortcuts (j/k/...)
/// go there, and mark it active right away (its focus report follows).
fn focus_account<R: Runtime>(app: &AppHandle<R>, id: Uuid) {
    let state = app.state::<AppState>();
    let label = format!("panel-{}", id.simple());
    if let Some(wv) = app.get_webview(&label) {
        let _ = wv.set_focus();
    }
    let changed = state.active.lock().unwrap().replace(id) != Some(id);
    if changed {
        relayout(app);
    }
}

/// Keyboard focus to the active panel (after the window is shown again).
fn refocus_active<R: Runtime>(app: &AppHandle<R>) {
    let id = *app.state::<AppState>().active.lock().unwrap();
    if let Some(id) = id {
        focus_account(app, id);
    }
}

/// Panel webview got keyboard focus (click, Tab, or focus_account).
/// Called from PANEL_JS; the caller is identified by its webview label,
/// not by anything the page sends.
#[tauri::command]
async fn panel_focused(app: AppHandle, state: State<'_, AppState>, webview: Webview) -> Result<(), String> {
    let id = {
        let s = state.settings.lock().unwrap();
        s.accounts
            .iter()
            .find(|a| panel_label(a) == webview.label())
            .map(|a| a.id)
            .ok_or("not a panel")?
    };
    let changed = state.active.lock().unwrap().replace(id) != Some(id);
    if changed {
        relayout(&app);
    }
    Ok(())
}

/// App shortcuts, from a panel (PANEL_JS) or the host UI:
/// - `focus` + `index`: Ctrl/Cmd+1..9
/// - `next` / `prev`: Ctrl+Tab / Ctrl+Shift+Tab (wraps)
/// - `bar`: Ctrl/Cmd+Shift+H, show/hide the top bar
/// - `help`: Ctrl/Cmd+/, open/close the shortcut overlay
/// - `refocus`: host UI was clicked, hand focus back to the active panel
/// - `tray`: hide the window to the tray (top bar button)
#[tauri::command]
async fn shortcut(
    app: AppHandle,
    state: State<'_, AppState>,
    action: String,
    index: Option<usize>,
) -> Result<(), String> {
    if action == "help" {
        let open = {
            let mut h = state.help.lock().unwrap();
            *h = !*h;
            *h
        };
        relayout(&app);
        if open {
            if let Some(host) = app.get_webview(HOST) {
                let _ = host.set_focus();
            }
        } else if let Some(id) = *state.active.lock().unwrap() {
            focus_account(&app, id);
        }
        return Ok(());
    }
    if action == "tray" {
        tray::hide(&app);
        return Ok(());
    }
    // Any other shortcut closes the overlay.
    let was_open = std::mem::replace(&mut *state.help.lock().unwrap(), false);
    if was_open {
        relayout(&app);
    }
    if action == "bar" {
        let snapshot = {
            let mut s = state.settings.lock().unwrap();
            s.show_bar = !s.show_bar;
            s.clone()
        };
        state.save(&snapshot);
        relayout(&app);
        if was_open {
            if let Some(id) = *state.active.lock().unwrap() {
                focus_account(&app, id);
            }
        }
        return Ok(());
    }
    let ids: Vec<Uuid> = state.settings.lock().unwrap().accounts.iter().map(|a| a.id).collect();
    if ids.is_empty() {
        return Ok(());
    }
    let active = *state.active.lock().unwrap();
    let cur = active.and_then(|a| ids.iter().position(|i| *i == a));
    let n = ids.len();
    let target = match action.as_str() {
        "focus" => match index {
            Some(i) if i < n => i,
            _ => return Ok(()),
        },
        "next" => cur.map_or(0, |c| (c + 1) % n),
        "prev" => cur.map_or(n - 1, |c| (c + n - 1) % n),
        "refocus" => cur.unwrap_or(0),
        _ => return Err("unknown shortcut".into()),
    };
    focus_account(&app, ids[target]);
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
    // Create the webview first and only then record the account, so a
    // failed add_child leaves no panel-less account behind.
    let mut acc = Account::new("", COLORS[0]);
    let rect = layout::Rect { x: 0.0, y: layout::TOP_STRIP, w: 10.0, h: 10.0 };
    add_panel(&window, &acc, rect).map_err(|e| e.to_string())?;
    {
        let mut s = state.settings.lock().unwrap();
        let n = s.accounts.len();
        acc.label = format!("계정 {}", n + 1);
        acc.color = COLORS[n % COLORS.len()].into();
        s.accounts.push(acc);
        let (w, _) = logical_size(&window);
        s.ratios = layout::clamp_ratios(w, &layout::with_added(&s.ratios));
        let snapshot = s.clone();
        drop(s);
        state.save(&snapshot);
    }
    relayout(&app);
    Ok(())
}

#[tauri::command]
async fn remove_account(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let window = app.get_window(MAIN).ok_or("no window")?;
    let acc = {
        let mut s = state.settings.lock().unwrap();
        let idx = s
            .accounts
            .iter()
            .position(|a| a.id.to_string() == id)
            .ok_or("no such account")?;
        let acc = s.accounts.remove(idx);
        let mut active = state.active.lock().unwrap();
        if *active == Some(acc.id) {
            *active = None;
        }
        drop(active);
        let (w, _) = logical_size(&window);
        s.ratios = layout::clamp_ratios(w, &layout::with_removed(&s.ratios, idx));
        s.fix_ratios();
        #[cfg(windows)]
        s.pending_profile_deletes.push(acc.profile_name());
        #[cfg(target_os = "macos")]
        s.pending_store_deletes.push(acc.id);
        let snapshot = s.clone();
        drop(s);
        state.save(&snapshot);
        acc
    };
    if let Some(wv) = app.get_webview(&panel_label(&acc)) {
        wv.close().map_err(|e| e.to_string())?;
    }
    relayout(&app);

    // macOS: try to drop the WKWebsiteDataStore now; whatever fails stays in
    // pending_store_deletes and is retried on next start.
    #[cfg(target_os = "macos")]
    purge_pending_stores(&app).await;
    Ok(())
}

/// macOS: remove every pending WKWebsiteDataStore, forget the ones that went.
#[cfg(target_os = "macos")]
async fn purge_pending_stores(app: &AppHandle) {
    let state = app.state::<AppState>();
    let ids = state.settings.lock().unwrap().pending_store_deletes.clone();
    let mut removed = vec![];
    for id in ids {
        match app.remove_data_store(*id.as_bytes()).await {
            Ok(()) => removed.push(id),
            Err(e) => log::warn!("data store {id} not removed yet: {e}"),
        }
    }
    if removed.is_empty() {
        return;
    }
    let snapshot = {
        let mut s = state.settings.lock().unwrap();
        s.pending_store_deletes.retain(|id| !removed.contains(id));
        s.clone()
    };
    state.save(&snapshot);
}

// ---------------------------------------------------------------- entry

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(tray::on_shortcut)
                .build(),
        )
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
            let (mut s, persist) = Settings::load(&path);
            #[cfg(windows)]
            purge_pending_profiles(app, &mut s);
            let (hotkey, hotkey_error) = match tray::register(app.handle(), s.hotkey()) {
                Ok(k) => (k, None),
                Err(e) => {
                    log::warn!("{e}");
                    (None, Some(e))
                }
            };
            let state = AppState {
                settings: Mutex::new(s.clone()),
                active: Mutex::new(s.accounts.first().map(|a| a.id)),
                help: Mutex::new(false),
                hotkey,
                hotkey_error,
                path,
                persist,
            };
            state.save(&s);
            app.manage(state);

            let window = WindowBuilder::new(app, MAIN)
                .title("Paneful")
                .inner_size(1280.0, 860.0)
                .min_inner_size(400.0, 300.0)
                // Dark title bar (Windows) / appearance (macOS) to match
                // X's dark theme and the host UI.
                .theme(Some(tauri::Theme::Dark))
                .build()?;
            let (w, h) = logical_size(&window);
            // Not fatal: without a tray icon the app still works, it just
            // can't be hidden from the top bar button (see tray::hide).
            if let Err(e) = tray::create(app.handle(), app.state::<AppState>().hotkey.as_deref()) {
                log::error!("tray icon failed: {e}");
            }

            // Host UI first so the panels stack above it; it stays visible
            // in the top bar and in the gaps between panels.
            window.add_child(
                WebviewBuilder::new(HOST, WebviewUrl::App("index.html".into())),
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(w, h),
            )?;

            let rects = layout::panels(w, h, top_of(&s), &s.ratios);
            for (acc, r) in s.accounts.iter().zip(rects) {
                add_panel(&window, acc, r)?;
            }
            // Each new webview grabs focus; start on the first panel instead.
            if let Some(acc) = s.accounts.first() {
                if let Some(wv) = app.get_webview(&panel_label(acc)) {
                    let _ = wv.set_focus();
                }
            }

            let handle = app.handle().clone();
            window.on_window_event(move |e| {
                if matches!(e, WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }) {
                    relayout(&handle);
                }
            });

            // macOS: retry data stores of removed accounts. Runs once the
            // event loop is up (remove_data_store hops to the main thread).
            #[cfg(target_os = "macos")]
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move { purge_pending_stores(&handle).await });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            refresh_layout,
            set_ratios,
            commit_layout,
            reload_panel,
            add_account,
            remove_account,
            panel_focused,
            shortcut
        ])
        .run(tauri::generate_context!())
        .expect("error while running paneful");
}
