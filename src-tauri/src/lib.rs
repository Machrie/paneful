//! Paneful M0 spike: one window, one local host-UI webview underneath,
//! N x.com panel webviews on top, each with its own data store.
//!
//! - macOS 14+: `data_store_identifier` -> WKWebsiteDataStore(forIdentifier:)
//! - Windows: same call; the patched tauri-runtime-wry turns it into a
//!   WebView2 profile name, so all panels share one WebView2 runtime.

mod keys;
mod capture;
mod snap;
mod layout;
mod health;
mod media;
mod memory;
mod nav;
mod rules;
mod portable;
mod settings;
mod tray;
mod notify;
mod unread;
mod update;
mod zoom;

use serde::Serialize;
use settings::{Account, Persist, Settings, COLORS};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    sync::Mutex,
};
use uuid::Uuid;
use tauri::{
    webview::{NewWindowResponse, WebviewBuilder},
    window::WindowBuilder,
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Runtime, State, Webview, WebviewUrl,
    Window, WindowEvent,
};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

pub(crate) const MAIN: &str = "main";
pub(crate) const HOST: &str = "host";
const START_URL: &str = "https://x.com/home";
/// Injected into every panel's main frame: reports focus and the app's own
/// shortcuts. The panel capability lets x.com reach only those commands.
/// `/*KEYS*/null` is replaced with the shortcut bindings at creation.
const PANEL_JS: &str = include_str!("panel.js");
/// Left click on an off-site link -> window.open with the real address.
const LINKS_JS: &str = include_str!("links.js");
/// X screen rules (rules.rs) for media.js / capture.js. `/*BUILTIN*/null`
/// and `/*RULES*/null` are replaced with JSON data at creation.
const RULES_JS: &str = include_str!("rules.js");

pub(crate) struct AppState {
    settings: Mutex<Settings>,
    /// Account whose panel last had keyboard focus. Not persisted.
    active: Mutex<Option<Uuid>>,
    /// Shortcut overlay open: panels are hidden so the host UI can draw it.
    help: Mutex<bool>,
    /// Settings screen open: panels are hidden the same way.
    settings_open: Mutex<bool>,
    /// Capture preview open: panels are hidden the same way.
    capture: Mutex<bool>,
    /// Active panel fills the window, the others are hidden. Not persisted.
    zoom: Mutex<bool>,
    /// Window hidden to the tray: panels are hidden too, so WebView2 /
    /// WKWebView treat the pages as not visible (no painting, throttled
    /// timers). Pages keep running.
    hidden: Mutex<bool>,
    /// Global show/hide shortcut as registered (None: off or failed).
    hotkey: Mutex<Option<String>>,
    /// Why the shortcut isn't registered (start or last change attempt).
    hotkey_error: Mutex<Option<String>>,
    /// Last new version check (update.rs).
    update: Mutex<update::Status>,
    /// X screen rules in effect, and the last download check (rules.rs).
    rules: Mutex<rules::Rules>,
    rules_status: Mutex<rules::Status>,
    /// Unread counts per account as its panel shows them (unread.rs).
    /// Only accounts with something unread are in here. Not persisted.
    unread: Mutex<BTreeMap<Uuid, unread::Unread>>,
    /// Panels turned off by hand (패널 끄기): the account stays, its webview
    /// is closed until 다시 켜기. Not persisted, every panel starts on.
    off: Mutex<HashSet<Uuid>>,
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
    /// Page zoom (1.0 = 100%).
    zoom: f64,
    /// Unread counts X shows in this panel.
    unread: unread::Unread,
    /// On screen (false: hidden while another panel is zoomed).
    shown: bool,
    /// Turned off by hand (no webview, the host UI shows 다시 켜기 there).
    off: bool,
}

#[derive(Serialize, Clone)]
struct LayoutPayload {
    win_w: f64,
    win_h: f64,
    /// Top bar shown (else only the thin strip).
    bar: bool,
    /// Shortcut overlay open.
    help: bool,
    /// Capture preview open.
    capture: bool,
    /// One panel fills the window (see AppState::zoom).
    zoom: bool,
    top: f64,
    gap: f64,
    min_panel: f64,
    ratios: Vec<f64>,
    panels: Vec<PanelView>,
    /// Settings screen open.
    settings: bool,
    /// Global show/hide shortcut, as key caps for the overlay. Empty: none.
    hotkey: Vec<String>,
    hotkey_error: Option<String>,
    /// Global shortcut as configured (may differ from `hotkey` if it failed).
    global: keys::KeyView,
    /// In-app shortcuts, display order.
    keys: Vec<keys::KeyView>,
    /// X's own shortcuts as moved in settings, display order.
    x_keys: Vec<keys::XKeyView>,
    /// Save folder in effect, and whether it is the default one.
    save_dir: String,
    save_dir_default: bool,
    /// Portable data folder next to the exe (None: AppData).
    data_dir: Option<String>,
    /// Portable marker found but its folder isn't writable.
    data_warning: Option<String>,
    /// This build's version, for settings.
    version: String,
    /// New version check turned on in settings.
    update_check: bool,
    /// Newer version to announce in the top bar / strip (not skipped).
    update_notice: Option<String>,
    /// Last check, for the settings screen.
    update: update::Status,
    /// Version the user skipped.
    skip_version: Option<String>,
    /// Download X screen rule fixes from the repository.
    rules_update: bool,
    /// Unread badges on the tray / taskbar / Dock icons.
    unread_badge: bool,
    /// System notifications for new 알림 / 쪽지.
    notify: bool,
    /// Modifier + wheel zooms a panel (settings checkbox).
    wheel_zoom: bool,
    /// Rules in effect and the last check, for the settings screen.
    rules: rules::Status,
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

/// Size the app lays out in: the window's inner size minus the title bar
/// area that the content view reaches under (macOS, see `titlebar`).
fn logical_size<R: Runtime>(window: &Window<R>) -> (f64, f64) {
    let scale = window.scale_factor().unwrap_or(1.0);
    match window.inner_size() {
        Ok(s) => {
            let l = s.to_logical::<f64>(scale);
            (l.width, (l.height - titlebar()).max(0.0))
        }
        Err(_) => (1280.0, 860.0),
    }
}

/// macOS: the content view can cover the title bar (seen on macOS 27: content
/// view 1470x923 = whole window, contentLayoutRect 891 high). Then
/// inner_size counts the title bar too, and everything laid out from y=0
/// sat under it: the 6pt strip and most of the top bar were hidden, and
/// WebKit inset the panels' pages by the rest (26pt), which put the X page
/// right where the strip should be. Every webview is placed this much lower,
/// and the layout height is that much smaller. Stored as f64 bits; 0 elsewhere.
static TITLEBAR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn titlebar() -> f64 {
    f64::from_bits(TITLEBAR.load(std::sync::atomic::Ordering::Relaxed))
}

/// Re-read the title bar height. Main thread only (AppKit). True if it changed.
#[cfg(target_os = "macos")]
fn measure_titlebar<R: Runtime>(window: &Window<R>) -> bool {
    use objc2_app_kit::NSWindow;
    let Ok(ptr) = window.ns_window() else { return false };
    let t = unsafe {
        let w: &NSWindow = &*(ptr as *const NSWindow);
        let Some(cv) = w.contentView() else { return false };
        let full = cv.frame().size.height;
        let lr = w.contentLayoutRect();
        // Unflipped window coordinates: the covered part is on top.
        (full - (lr.origin.y + lr.size.height)).clamp(0.0, full / 2.0)
    };
    let old = TITLEBAR.swap(t.to_bits(), std::sync::atomic::Ordering::Relaxed);
    if f64::from_bits(old) != t {
        log::info!("title bar over content: {t}pt");
        return true;
    }
    false
}

#[cfg(not(target_os = "macos"))]
fn measure_titlebar<R: Runtime>(_window: &Window<R>) -> bool {
    false
}

/// Recompute every rect, move the webviews, tell the host UI.
pub(crate) fn relayout<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_window(MAIN) else { return };
    let state = app.state::<AppState>();
    let s = state.settings.lock().unwrap().clone();
    let active = *state.active.lock().unwrap();
    let help = *state.help.lock().unwrap();
    let settings_open = *state.settings_open.lock().unwrap();
    let hotkey = state.hotkey.lock().unwrap().clone();
    let hotkey_error = state.hotkey_error.lock().unwrap().clone();
    let capture = *state.capture.lock().unwrap();
    let zoom = {
        // Nothing to zoom with one panel; don't let it come back on later.
        let mut z = state.zoom.lock().unwrap();
        if s.accounts.len() < 2 {
            *z = false;
        }
        *z
    };
    let hidden = *state.hidden.lock().unwrap();
    let up = state.update.lock().unwrap().clone();
    let rules_st = {
        let st = state.rules_status.lock().unwrap().clone();
        rules::status_of(&state.rules.lock().unwrap(), &st)
    };
    let unread = state.unread.lock().unwrap().clone();
    let off = state.off.lock().unwrap().clone();
    let (w, h) = logical_size(&window);
    let top = top_of(&s);
    let mut rects = layout::panels(w, h, top, &s.ratios);
    // Zoomed: the active panel (first one if none) takes the whole area.
    let zoomed = if zoom && s.accounts.len() > 1 {
        let ids: Vec<Uuid> = s.accounts.iter().map(|a| a.id).collect();
        active.filter(|a| ids.contains(a)).or(ids.first().copied())
    } else {
        None
    };
    if let Some(z) = zoomed {
        if let Some(i) = s.accounts.iter().position(|a| a.id == z) {
            rects[i] = layout::Rect { x: 0.0, y: top, w, h: (h - top).max(0.0) };
        }
    }

    if let Some(host) = app.get_webview(HOST) {
        let _ = host.set_bounds(tauri::Rect {
            position: LogicalPosition::new(0.0, titlebar()).into(),
            size: LogicalSize::new(w, h).into(),
        });
    }
    let mut panels = vec![];
    for (acc, r) in s.accounts.iter().zip(rects.iter()) {
        if let Some(wv) = app.get_webview(&panel_label(acc)) {
            let _ = wv.set_bounds(tauri::Rect {
                position: LogicalPosition::new(r.x, r.y + titlebar()).into(),
                size: LogicalSize::new(r.w, r.h).into(),
            });
        }
        let shown = zoomed.is_none_or(|z| z == acc.id);
        if let Some(wv) = app.get_webview(&panel_label(acc)) {
            // Native webviews sit above the host UI; the overlays can only
            // show while they are hidden.
            let _ = if help || settings_open || capture || hidden || !shown { wv.hide() } else { wv.show() };
            // Again after each move: Tahoe may add its views later.
            #[cfg(target_os = "macos")]
            clip_panel(&wv);
        }
        panels.push(PanelView {
            id: acc.id.to_string(),
            label: acc.label.clone(),
            color: acc.color.clone(),
            active: active == Some(acc.id),
            rect: *r,
            zoom: acc.zoom,
            unread: unread.get(&acc.id).cloned().unwrap_or_default(),
            shown,
            off: off.contains(&acc.id),
        });
    }
    let payload = LayoutPayload {
        win_w: w,
        win_h: h,
        bar: bar_visible(&s),
        help,
        capture,
        zoom: zoomed.is_some(),
        top,
        gap: layout::GAP,
        min_panel: layout::MIN_PANEL,
        ratios: s.ratios.clone(),
        panels,
        settings: settings_open,
        hotkey: hotkey.as_deref().and_then(|k| keys::parse(k).ok()).map(|k| keys::caps(&k)).unwrap_or_default(),
        hotkey_error,
        global: keys::view(&s.key_map(), keys::GLOBAL),
        keys: keys::views(&s.keys),
        x_keys: keys::x_views(&s.x_keys),
        save_dir: media::save_dir(app).map(|d| d.display().to_string()).unwrap_or_default(),
        save_dir_default: s.save_dir.is_none(),
        data_dir: portable::root().map(|d| d.display().to_string()),
        data_warning: fallback_warning(),
        version: update::current(app),
        update_check: s.update_check,
        update_notice: update::notice(&s, &up),
        update: up,
        skip_version: s.skip_version.clone(),
        rules_update: s.rules_update,
        unread_badge: s.unread_badge,
        notify: s.notify,
        wheel_zoom: s.wheel_zoom,
        rules: rules_st,
    };
    let _ = app.emit_to(HOST, "layout", payload);
}

/// In-app shortcut bindings as JSON, for panel.js and the `paneful-keys` event.
fn bindings_json(s: &Settings) -> String {
    serde_json::to_string(&keys::bindings(&s.keys)).unwrap_or_else(|_| "null".into())
}

/// Moved X shortcuts as JSON, for panel.js and the `paneful-xkeys` event.
fn x_keys_json(s: &Settings) -> String {
    serde_json::to_string(&keys::x_keys(&s.x_keys)).unwrap_or_else(|_| "null".into())
}

fn add_panel<R: Runtime>(window: &Window<R>, acc: &Account, rect: layout::Rect, keys_json: &str) -> tauri::Result<()> {
    let app = window.app_handle().clone();
    let rules_js = {
        let state = app.state::<AppState>();
        let cur = state.rules.lock().unwrap();
        RULES_JS
            .replace("/*BUILTIN*/null", &rules::json(&rules::builtin()))
            .replace("/*RULES*/null", &rules::json(&cur))
    };
    let x_json = x_keys_json(&app.state::<AppState>().settings.lock().unwrap());
    let wheel = app.state::<AppState>().settings.lock().unwrap().wheel_zoom;
    let app2 = app.clone();
    let label = panel_label(acc);
    let label2 = label.clone();

    let builder = WebviewBuilder::new(&label, WebviewUrl::External(START_URL.parse().unwrap()))
        .data_store_identifier(acc.store_id())
        .initialization_script(rules_js)
        .initialization_script(PANEL_JS
            .replace("/*KEYS*/null", keys_json)
            .replace("/*XKEYS*/null", &x_json)
            .replace("/*WHEEL*/true", if wheel { "true" } else { "false" }))
        .initialization_script(media::SCRIPT)
        .initialization_script(capture::SCRIPT)
        .initialization_script(LINKS_JS)
        // File drops from Explorer / Finder go to the page, so X's composer
        // takes them as attachments. With Tauri's handler on, wry swallows
        // them: on Windows it turns off WebView2's external drop and puts
        // its own IDropTarget on the panel, on macOS Tauri's callback
        // returns true, which tells WKWebView the drop is handled. Nothing
        // here listens for Tauri's DragDrop events. A drop where X doesn't
        // take it is stopped in links.js.
        .disable_drag_drop_handler()
        // Zoom again after each page load: WebView2 may drop the zoom factor
        // when a page loads (Chromium keeps zoom per site). Same value again
        // is a no-op.
        .on_page_load(|wv, _| {
            // The init script carries the wheel setting from when the panel
            // was made; a reload after a change would bring the old one back.
            let wheel = wv.app_handle().state::<AppState>().settings.lock().unwrap().wheel_zoom;
            let _ = wv.eval(wheel_js(wheel));
            let z = panel_zoom(&wv.app_handle().state::<AppState>(), wv.label());
            if let Some(z) = z.filter(|z| *z != 1.0) {
                log::info!("[{}] page load: zoom {}% again", wv.label(), zoom::percent(z));
                let _ = wv.set_zoom(z);
            }
        })
        .on_navigation(move |url| match nav::decide(url) {
            nav::Decision::Allow => true,
            nav::Decision::OpenExternal => {
                // Windows: WebView2 NavigationStarting is top-level only, so
                // this is a real page navigation and goes to the system browser.
                // macOS: wry runs this for iframes too, so opening externally
                // would pop a browser tab per off-list iframe. Just cancel.
                #[cfg(windows)]
                open_external(&app, url, "navigation");
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
            // window.open() with no address (filled in later by the page):
            // nothing to send anywhere, and loading about:blank in the
            // panel would blank it.
            if url.scheme() == "about" {
                log::info!("[{label2}] new window {url}: denied");
                return NewWindowResponse::Deny;
            }
            match nav::decide(&url) {
                nav::Decision::Allow => {
                    log::info!("[{label2}] new window {url}: same panel");
                    if let Some(wv) = app2.get_webview(&label2) {
                        let _ = wv.navigate(url);
                    }
                }
                nav::Decision::OpenExternal => open_external(&app2, &url, "new window"),
                nav::Decision::Block => log::info!("[{label2}] new window {url}: blocked"),
            }
            NewWindowResponse::Deny
        });

    let wv = window.add_child(
        with_udf(builder),
        LogicalPosition::new(rect.x, rect.y + titlebar()),
        LogicalSize::new(rect.w, rect.h),
    )?;

    if acc.zoom != 1.0 {
        log::info!("[{}] created at zoom {}%", wv.label(), zoom::percent(acc.zoom));
        let _ = wv.set_zoom(acc.zoom);
    }
    #[cfg(windows)]
    log_webview2_profile(&wv);
    #[cfg(target_os = "macos")]
    clip_panel(&wv);
    health::watch(&wv);
    Ok(())
}

/// macOS: keep a panel's drawing inside its own frame and log where it
/// really is. On Earl's Mac (macOS 27) the panels' frames are right (6pt
/// strip left free above them, see the log) but the X page shows up above
/// them, over the strip and the top bar. Since the macOS 14 SDK,
/// NSView.clipsToBounds defaults to NO, and Tahoe adds scroll edge
/// "pockets" near the title bar. The log line (written whenever something
/// in it changes) has the numbers needed to find which part is shifted.
#[cfg(target_os = "macos")]
fn clip_panel<R: Runtime>(wv: &tauri::Webview<R>) {
    use objc2::runtime::AnyObject;
    use objc2_app_kit::NSView;
    use objc2_foundation::{NSEdgeInsets, NSObjectProtocol, NSRect};
    use objc2_web_kit::WKWebView;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    fn r(f: NSRect) -> String {
        format!("({},{} {}x{})", f.origin.x, f.origin.y, f.size.width, f.size.height)
    }
    fn ins(e: NSEdgeInsets) -> String {
        format!("(t{} l{} b{} r{})", e.top, e.left, e.bottom, e.right)
    }
    fn class_name(v: &NSView) -> String {
        v.class().name().to_string_lossy().into_owned()
    }
    /// Hide Tahoe's scroll pockets anywhere inside the panel.
    unsafe fn hide_pockets(v: &NSView, label: &str) {
        for sub in v.subviews().iter() {
            let name = class_name(&sub);
            if name.contains("ScrollPocket") {
                log::info!("[{label}] hiding {name} {}", r(sub.frame()));
                sub.setHidden(true);
            }
            hide_pockets(&sub, label);
        }
    }
    /// Subviews two levels down, with frames and layer frames.
    unsafe fn tree(v: &NSView, depth: usize, out: &mut Vec<String>) {
        for sub in v.subviews().iter() {
            let layer: *mut AnyObject = objc2::msg_send![&*sub, layer];
            let lf = if layer.is_null() {
                "-".to_string()
            } else {
                let f: NSRect = objc2::msg_send![layer, frame];
                r(f)
            };
            out.push(format!(
                "{}{} frame={} layer={} hidden={}",
                "  ".repeat(depth),
                class_name(&sub),
                r(sub.frame()),
                lf,
                sub.isHidden()
            ));
            if depth < 1 {
                tree(&sub, depth + 1, out);
            }
        }
    }
    static LAST: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

    let label = wv.label().to_string();
    let _ = wv.with_webview(move |pw| unsafe {
        let view: &WKWebView = &*(pw.inner() as *const WKWebView);
        view.setClipsToBounds(true);
        hide_pockets(view, &label);

        let mut parts = vec![format!("frame={}", r(view.frame()))];
        let layer: *mut AnyObject = objc2::msg_send![view, layer];
        if !layer.is_null() {
            let f: NSRect = objc2::msg_send![layer, frame];
            parts.push(format!("layer={}", r(f)));
        }
        parts.push(format!("safeArea={}", ins(view.safeAreaInsets())));
        if view.respondsToSelector(objc2::sel!(obscuredContentInsets)) {
            parts.push(format!("obscured={}", ins(view.obscuredContentInsets())));
        }
        if view.respondsToSelector(objc2::sel!(_topContentInset)) {
            let t: f64 = objc2::msg_send![view, _topContentInset];
            parts.push(format!("topInset={t}"));
        }
        if view.respondsToSelector(objc2::sel!(_automaticallyAdjustsContentInsets)) {
            let a: bool = objc2::msg_send![view, _automaticallyAdjustsContentInsets];
            parts.push(format!("autoInsets={a}"));
        }
        if let Some(sv) = view.superview() {
            parts.push(format!("parent={} flipped={}", r(sv.frame()), sv.isFlipped()));
        }
        if let Some(w) = view.window() {
            parts.push(format!("window={} layoutRect={}", r(w.frame()), r(w.contentLayoutRect())));
            if let Some(cv) = w.contentView() {
                parts.push(format!("contentView={} {}", class_name(&cv), r(cv.frame())));
            }
        }
        let mut sub = vec![];
        tree(view, 0, &mut sub);
        let line = format!("{}; subviews: {}", parts.join(" "), sub.join(" | "));
        let mut last = LAST.get_or_init(Default::default).lock().unwrap();
        if last.get(&label) != Some(&line) {
            log::info!("[{label}] {line}");
            last.insert(label, line);
        }
    });
}

/// Portable mode: every webview (host and panels) uses the WebView2 user
/// data folder next to the exe. Same folder for all, so they keep sharing
/// one WebView2 environment (the runtime keys its web contexts by it).
/// Installed: Tauri's default, app_local_data_dir.
fn with_udf<R: Runtime>(b: WebviewBuilder<R>) -> WebviewBuilder<R> {
    match portable::root() {
        Some(dir) => b.data_directory(dir.to_path_buf()),
        None => b,
    }
}

/// Text for the dialog and settings when the portable folder can't be used.
fn fallback_warning() -> Option<String> {
    match portable::mode() {
        portable::Mode::Fallback { dir, error } => Some(format!(
            "{} 폴더에 쓸 수 없어서 포터블 대신 AppData에 저장 중 ({error}). 쓰기 가능한 곳(바탕화면, USB 등)으로 폴더째 옮겨서 실행해라.",
            dir.display()
        )),
        _ => None,
    }
}

/// Close a panel's webview and build it again in place (Windows: its
/// WebView2 browser process is gone, so reload can't help).
pub(crate) async fn recreate_panel<R: Runtime>(app: &AppHandle<R>, label: &str) -> Result<(), String> {
    let window = app.get_window(MAIN).ok_or("no window")?;
    let (acc, keys_json) = {
        let s = app.state::<AppState>().settings.lock().unwrap().clone();
        let acc = s.accounts.iter().find(|a| panel_label(a) == label).cloned().ok_or("no such panel")?;
        (acc, bindings_json(&s))
    };
    if let Some(wv) = app.get_webview(label) {
        let _ = wv.close();
    }
    // close() drops the label at once; wait a little in case it didn't.
    for _ in 0..30 {
        if app.get_webview(label).is_none() {
            break;
        }
        let _ = tauri::async_runtime::spawn_blocking(|| std::thread::sleep(std::time::Duration::from_millis(100))).await;
    }
    if app.get_webview(label).is_some() {
        return Err("old panel did not close".into());
    }
    log::info!("[{label}] rebuilding panel");
    let rect = layout::Rect { x: 0.0, y: layout::TOP_STRIP, w: 10.0, h: 10.0 };
    add_panel(&window, &acc, rect, &keys_json).map_err(|e| e.to_string())?;
    relayout(app);
    Ok(())
}

/// Zoom of the panel with this webview label (None: no such panel).
/// Lock held only for the lookup.
pub(crate) fn panel_zoom(state: &AppState, label: &str) -> Option<f64> {
    let s = state.settings.lock().unwrap();
    s.accounts.iter().find(|a| panel_label(a) == label).map(|a| a.zoom)
}

/// Account of the panel with this webview label.
fn panel_id(state: &AppState, label: &str) -> Option<Uuid> {
    let s = state.settings.lock().unwrap();
    s.accounts.iter().find(|a| panel_label(a) == label).map(|a| a.id)
}

/// (account label, counts) of every panel with something unread, in panel
/// order, for the tray. Locks are let go before returning.
pub(crate) fn unread_lines(state: &AppState) -> Vec<(String, unread::Unread)> {
    let accounts: Vec<(Uuid, String)> = state
        .settings
        .lock()
        .unwrap()
        .accounts
        .iter()
        .map(|a| (a.id, a.label.clone()))
        .collect();
    let unread = state.unread.lock().unwrap().clone();
    accounts
        .into_iter()
        .filter_map(|(id, label)| unread.get(&id).map(|u| (label, u.clone())))
        .collect()
}

/// Hand an off-list address to the system browser. paneful.log keeps each
/// one, so a link that "does nothing" shows whether it got here.
fn open_external<R: Runtime>(app: &AppHandle<R>, url: &tauri::Url, via: &str) {
    match app.opener().open_url(url.as_str(), None::<&str>) {
        Ok(()) => log::info!("{via} {url}: system browser"),
        Err(e) => log::warn!("{via} {url}: system browser failed: {e}"),
    }
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
    let udf = match portable::root() {
        Some(dir) => dir.to_path_buf(),
        None => match app.path().app_local_data_dir() {
            Ok(d) => d,
            Err(_) => return,
        },
    };
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
// The host webview may call these (capabilities/host.json); x.com panels only
// panel_focused, panel_unread, panel_view, panel_stuck, shortcut, panel_keys, panel_xkeys, paste_link, open_link, send_link and capture_begin/shot/drop/finish
// (capabilities/panel.json). Commands are async
// because add_child from a sync command deadlocks on WebView2.

#[tauri::command]
async fn refresh_layout(app: AppHandle) {
    relayout(&app);
}

fn apply_ratios(app: &AppHandle, state: &AppState, ratios: &[f64]) -> Result<Settings, String> {
    let window = app.get_window(MAIN).ok_or("no window")?;
    // Window getters wait for the main thread, and the main thread locks
    // `settings` in relayout (resize, tray show): never call them with a lock held.
    let (w, _) = logical_size(&window);
    let snapshot = {
        let mut s = state.settings.lock().unwrap();
        if ratios.len() != s.accounts.len() || ratios.iter().any(|r| !r.is_finite() || *r <= 0.0) {
            return Err("bad ratios".into());
        }
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
pub(crate) fn focus_account<R: Runtime>(app: &AppHandle<R>, id: Uuid) {
    let state = app.state::<AppState>();
    let label = format!("panel-{}", id.simple());
    // Mark it active (and lay out) first: while zoomed, that is what shows
    // the panel, and a hidden webview can't take focus.
    let changed = state.active.lock().unwrap().replace(id) != Some(id);
    if changed {
        relayout(app);
    }
    if let Some(wv) = app.get_webview(&label) {
        let _ = wv.set_focus();
    }
}

/// Keyboard focus to the active panel (after the window is shown again).
fn refocus_active<R: Runtime>(app: &AppHandle<R>) {
    let id = *app.state::<AppState>().active.lock().unwrap();
    if let Some(id) = id {
        focus_account(app, id);
    }
}

/// The calling panel's unread counts changed (panel.js reads them off the
/// page; see unread.rs). The caller is the panel by its webview label.
/// `fresh`: the page loaded only moments ago, so a count is not news (no
/// system notification for it).
#[tauri::command]
async fn panel_unread(
    app: AppHandle,
    state: State<'_, AppState>,
    webview: Webview,
    n: String,
    dm: String,
    fresh: Option<bool>,
) -> Result<(), String> {
    let id = panel_id(&state, webview.label()).ok_or("not a panel")?;
    let u = unread::Unread::new(&n, &dm);
    let old = {
        let mut m = state.unread.lock().unwrap();
        let old = if u.is_empty() { m.remove(&id) } else { m.insert(id, u.clone()) };
        old.unwrap_or_default()
    };
    if old != u {
        relayout(&app);
        tray::refresh(&app);
        if !fresh.unwrap_or(false) {
            notify_unread(&app, &state, id, &old, &u);
        }
    }
    Ok(())
}

/// System notification for a count that went up (notify.rs), unless they
/// are off, the window is in front (the tabs show it) or one just went out
/// for this account.
fn notify_unread(app: &AppHandle, state: &AppState, id: Uuid, old: &unread::Unread, now: &unread::Unread) {
    let (n, dm) = notify::rose(old, now);
    if !(n || dm) {
        return;
    }
    let label = {
        let s = state.settings.lock().unwrap();
        if !s.notify {
            return;
        }
        s.accounts.iter().find(|a| a.id == id).map(|a| a.label.clone()).unwrap_or_default()
    };
    let hidden = *state.hidden.lock().unwrap();
    let in_front = !hidden
        && app
            .get_window(MAIN)
            .map(|w| w.is_focused().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
            .unwrap_or(false);
    if in_front {
        return;
    }
    if !app.state::<notify::State>().allow(id, std::time::Instant::now()) {
        log::info!("notification for {} skipped: one went out less than {}s ago", label, notify::GAP.as_secs());
        return;
    }
    let (title, body) = notify::message(&label, n, dm, now);
    notify::show(app, id, &title, &body);
}

/// The calling panel's page scale changed by itself or by the app (panel.js
/// watches devicePixelRatio, visualViewport.scale and the CSS width). Log
/// only: next to the app's own zoom lines it tells an app zoom from one the
/// webview did on its own (pinch magnification, a system zoom, ...).
#[tauri::command]
async fn panel_view(state: State<'_, AppState>, webview: Webview, dpr: f64, scale: f64, w: f64) -> Result<(), String> {
    let z = panel_zoom(&state, webview.label()).ok_or("not a panel")?;
    log::info!(
        "[{}] view: devicePixelRatio {dpr:.3}, viewport scale {scale:.3}, css width {w:.0} (app zoom {}%)",
        webview.label(),
        zoom::percent(z)
    );
    Ok(())
}

/// Panel webview got keyboard focus (click, Tab, or focus_account).
/// Called from PANEL_JS; the caller is identified by its webview label,
/// not by anything the page sends.
#[tauri::command]
async fn panel_focused(app: AppHandle, state: State<'_, AppState>, webview: Webview) -> Result<(), String> {
    let id = panel_id(&state, webview.label()).ok_or("not a panel")?;
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
/// - `settings`: Ctrl/Cmd+, open/close the settings screen
/// (keys of next/prev/bar/help/settings/capture are configurable, see keys.rs)
/// - `refocus`: host UI was clicked, hand focus back to the active panel
/// - `tray`: hide the window to the tray (top bar button)
/// - `capture`: Ctrl/Cmd+Shift+S from the host UI or the top bar button:
///   start picking posts in the active panel (capture.js)
/// - `zoom_in` / `zoom_out` / `zoom_reset`: Ctrl/Cmd + = / - / 0, page zoom
///   of the panel the key was pressed in; from the host UI the panel at
///   `index` (tab button) or the active one
/// - `reload`: Ctrl/Cmd+R, reload the panel the key was pressed in (from the
///   host UI the active one), same as the 새로고침 button
#[tauri::command]
async fn shortcut(
    app: AppHandle,
    state: State<'_, AppState>,
    webview: Webview,
    action: String,
    index: Option<usize>,
    via: Option<String>,
) -> Result<(), String> {
    // Paired start/end lines: after a hang, a start without its end in
    // paneful.log names the shortcut that got stuck.
    // `via`: what the caller says set it off (panel.js: "wheel ..." for the
    // wheel zoom), for the log only. Cut short and kept to plain text.
    let via: String = via
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .take(80)
        .collect();
    log::info!("shortcut {action} {index:?} from {} {via}: start", webview.label());
    let caller = panel_id(&state, webview.label());
    let r = run_shortcut(&app, &state, &action, index, caller);
    log::info!("shortcut {action}: end");
    r
}

/// Open/close the shortcut overlay and the settings screen (at most one).
/// Open: keyboard focus to the host UI that draws it; closed: back to the
/// active panel.
fn set_overlay<R: Runtime>(app: &AppHandle<R>, state: &AppState, help: bool, settings: bool) {
    *state.help.lock().unwrap() = help;
    *state.settings_open.lock().unwrap() = settings;
    if !settings {
        tray::RECORDING.store(false, std::sync::atomic::Ordering::Relaxed);
    }
    relayout(app);
    if help || settings {
        if let Some(host) = app.get_webview(HOST) {
            let _ = host.set_focus();
        }
    } else {
        // Copy the id out first: in edition 2021 an `if let` on
        // `*m.lock().unwrap()` keeps the guard for the whole block, and
        // focus_account locks `active` again -> self-deadlock.
        let active = *state.active.lock().unwrap();
        if let Some(id) = active {
            focus_account(app, id);
        }
    }
}

/// Tray menu "설정": open the settings screen (window already shown).
pub(crate) fn open_settings<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<AppState>();
    capture::close(app, &state);
    set_overlay(app, &state, false, true);
}

/// Zoom one panel a step in (`dir` 1), out (-1) or back to 100% (0), save
/// it, and show the new value in the panel for a moment.
fn zoom_panel<R: Runtime>(app: &AppHandle<R>, state: &AppState, id: Uuid, dir: i32) {
    let label = format!("panel-{}", id.simple());
    let (z, snapshot) = {
        let mut s = state.settings.lock().unwrap();
        let Some(acc) = s.accounts.iter_mut().find(|a| a.id == id) else { return };
        let z = zoom::step(acc.zoom, dir);
        let changed = acc.zoom != z;
        log::info!(
            "[{label}] zoom {}% -> {}% (dir {dir}{})",
            zoom::percent(acc.zoom),
            zoom::percent(z),
            if changed { "" } else { ", at limit" }
        );
        acc.zoom = z;
        (z, changed.then(|| s.clone()))
    };
    if let Some(snap) = &snapshot {
        state.save(snap);
    }
    if let Some(wv) = app.get_webview(&label) {
        if snapshot.is_some() {
            let _ = wv.set_zoom(z);
        }
        // Shown even at the limit, so a key that "does nothing" says why.
        let _ = wv.eval(format!(
            "window.dispatchEvent(new CustomEvent('paneful-zoom',{{detail:{}}}))",
            zoom::percent(z)
        ));
    }
    if snapshot.is_some() {
        relayout(app);
    }
}

fn run_shortcut(
    app: &AppHandle,
    state: &AppState,
    action: &str,
    index: Option<usize>,
    caller: Option<Uuid>,
) -> Result<(), String> {
    let app = app.clone();
    // Capture preview open: the host UI handles its own keys; any app
    // shortcut that still gets here closes it first.
    if *state.capture.lock().unwrap() {
        capture::close(&app, state);
        if action == "capture" {
            return Ok(());
        }
    }
    if action == "capture" {
        let was_help = std::mem::replace(&mut *state.help.lock().unwrap(), false);
        let was_settings = std::mem::replace(&mut *state.settings_open.lock().unwrap(), false);
        if was_settings {
            tray::RECORDING.store(false, std::sync::atomic::Ordering::Relaxed);
        }
        if was_help || was_settings {
            relayout(&app);
        }
        let ids: Vec<Uuid> = state.settings.lock().unwrap().accounts.iter().map(|a| a.id).collect();
        let active = *state.active.lock().unwrap();
        let Some(id) = active.filter(|a| ids.contains(a)).or(ids.first().copied()) else {
            return Ok(());
        };
        focus_account(&app, id);
        if let Some(wv) = app.get_webview(&format!("panel-{}", id.simple())) {
            let _ = wv.eval("window.dispatchEvent(new Event('paneful-capture'))");
        }
        return Ok(());
    }
    if action == "help" {
        let open = !*state.help.lock().unwrap();
        set_overlay(&app, state, open, false);
        return Ok(());
    }
    if action == "settings" {
        let open = !*state.settings_open.lock().unwrap();
        set_overlay(&app, state, false, open);
        return Ok(());
    }
    if action == "tray" {
        tray::hide(&app);
        return Ok(());
    }
    // Any other shortcut closes the overlays.
    let was_help = std::mem::replace(&mut *state.help.lock().unwrap(), false);
    let was_settings = std::mem::replace(&mut *state.settings_open.lock().unwrap(), false);
    let was_open = was_help || was_settings;
    if was_settings {
        tray::RECORDING.store(false, std::sync::atomic::Ordering::Relaxed);
    }
    if was_open {
        relayout(&app);
    }
    if action == "zoom" {
        let on = {
            let mut z = state.zoom.lock().unwrap();
            *z = !*z;
            *z
        };
        log::info!("zoom {on}");
        relayout(&app);
        let active = *state.active.lock().unwrap(); // see set_overlay
        let ids: Vec<Uuid> = state.settings.lock().unwrap().accounts.iter().map(|a| a.id).collect();
        if let Some(id) = active.filter(|a| ids.contains(a)).or(ids.first().copied()) {
            focus_account(&app, id);
        }
        return Ok(());
    }
    if action == "send" || action == "paste" {
        // From the host UI: the active panel does the work (works out what
        // to send / asks before opening the copied link).
        let active = *state.active.lock().unwrap();
        let ids: Vec<Uuid> = state.settings.lock().unwrap().accounts.iter().map(|a| a.id).collect();
        if let Some(id) = active.filter(|a| ids.contains(a)).or(ids.first().copied()) {
            focus_account(&app, id);
            if let Some(wv) = app.get_webview(&format!("panel-{}", id.simple())) {
                let _ = wv.eval(format!("window.dispatchEvent(new Event('paneful-{action}'))"));
            }
        }
        return Ok(());
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
            let active = *state.active.lock().unwrap(); // see "help" above
            if let Some(id) = active {
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
    let dir = match action {
        "zoom_in" => Some(1),
        "zoom_out" => Some(-1),
        "zoom_reset" => Some(0),
        _ => None,
    };
    if action == "reload" {
        // Same target rule as zoom: the panel the key was pressed in, else
        // the active one. Async: a dead panel is rebuilt (recreate_panel).
        let target = caller
            .or_else(|| index.and_then(|i| ids.get(i).copied()))
            .or(active.filter(|a| ids.contains(a)))
            .or(ids.first().copied());
        if let Some(id) = target {
            let label = format!("panel-{}", id.simple());
            if caller.is_none() {
                focus_account(&app, id);
            }
            tauri::async_runtime::spawn(async move {
                if let Err(e) = reload_label(&app, &label).await {
                    log::error!("[{label}] reload failed: {e}");
                }
            });
        }
        return Ok(());
    }
    if let Some(dir) = dir {
        // A panel zooms itself; the host UI names a tab or means the active one.
        let target = caller
            .or_else(|| index.and_then(|i| ids.get(i).copied()))
            .or(active.filter(|a| ids.contains(a)))
            .or(ids.first().copied());
        if let Some(id) = target {
            zoom_panel(&app, state, id, dir);
            // From the host UI (tab button, key with the bar focused): focus
            // goes to that panel, as for any other click up there.
            if caller.is_none() {
                focus_account(&app, id);
            }
        }
        return Ok(());
    }
    let cur = active.and_then(|a| ids.iter().position(|i| *i == a));
    let n = ids.len();
    let target = match action {
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

/// What panel.js does after the "paste" shortcut: ask before opening `url`, or show
/// `msg` briefly. Both None: nothing to do.
#[derive(Serialize, Default)]
struct Pasted {
    url: Option<String>,
    msg: Option<String>,
}

fn is_panel(state: &AppState, webview: &Webview) -> bool {
    let s = state.settings.lock().unwrap();
    s.accounts.iter().any(|a| panel_label(a) == webview.label())
}

/// The "paste" shortcut in a panel (panel.js, Ctrl/Cmd+O by default): if the
/// clipboard holds one X link, hand back the cleaned-up x.com address so
/// the panel can ask before going there (open_link). Nothing navigates
/// here. The caller is the panel by its webview label.
/// No lock is held while touching the clipboard.
#[tauri::command]
async fn paste_link(app: AppHandle, state: State<'_, AppState>, webview: Webview) -> Result<Pasted, String> {
    if !is_panel(&state, &webview) {
        return Err("not a panel".into());
    }
    // Image or empty clipboard: read_text fails, nothing to do.
    let text = match tauri_plugin_clipboard_manager::ClipboardExt::clipboard(&app).read_text() {
        Ok(t) => t,
        Err(e) => {
            log::info!("paste_link in {}: no text ({e})", webview.label());
            return Ok(Pasted::default());
        }
    };
    let msg = |m: &str| Ok(Pasted { url: None, msg: Some(m.into()) });
    let r = nav::x_link(&text);
    // Kind only: the clipboard text itself stays out of the log.
    log::info!("paste_link in {}: {}", webview.label(), match &r {
        Ok(_) => "x link",
        Err(nav::LinkError::NotLink) => "not a link",
        Err(nav::LinkError::OtherSite) => "other site",
        Err(nav::LinkError::ShortLink) => "t.co",
    });
    match r {
        Ok(url) => Ok(Pasted { url: Some(url.to_string()), msg: None }),
        Err(nav::LinkError::NotLink) => Ok(Pasted::default()),
        Err(nav::LinkError::OtherSite) => msg("X 링크가 아님 · x.com·twitter.com 링크만 열 수 있음"),
        Err(nav::LinkError::ShortLink) => msg("t.co 단축 링크는 못 엶 · 게시물 주소(x.com/…)를 복사해서 붙여넣기"),
    }
}

/// The user said yes to the address paste_link returned: open it in the
/// calling panel. Checked again with nav::x_link, so only x.com pages open
/// this way. No lock is held while touching the webview.
#[tauri::command]
async fn open_link(state: State<'_, AppState>, webview: Webview, url: String) -> Result<(), String> {
    if !is_panel(&state, &webview) {
        return Err("not a panel".into());
    }
    let url = nav::x_link(&url).map_err(|_| "X 링크가 아님".to_string())?;
    log::info!("open_link: opening {url} in {}", webview.label());
    go_to(&webview, &url)
}

/// Open `url` (already checked with nav::x_link) in a panel the way X moves
/// between its own pages: panel.js changes the address in the page and X
/// draws it (history.pushState + popstate), no new document. A full load of
/// a post address (webview.navigate) left macOS panels on X's loading logo,
/// Cmd+R loaded the same address again, and only a restart (back at
/// x.com/home) brought them back.
/// panel.js takes the event only on x.com once X has started; otherwise
/// (another page, X not up yet, no panel.js) the snippet loads the address.
fn go_to<R: Runtime>(wv: &Webview<R>, url: &url::Url) -> Result<(), String> {
    wv.eval(go_js(url)).map_err(|e| e.to_string())
}

fn go_js(url: &url::Url) -> String {
    // A JSON string is a JS string literal; Url output is percent-encoded,
    // so no U+2028/2029 either.
    let u = serde_json::to_string(url.as_str()).unwrap_or_default();
    format!(
        "(()=>{{const u={u};const e=new CustomEvent('paneful-go',{{detail:u,cancelable:true}});\
         if(window.dispatchEvent(e))location.assign(u);}})()"
    )
}

/// The calling panel sat on X's loading logo and panel.js sent it to
/// x.com/home (`why`: "timer" or "reload"). Log only.
#[tauri::command]
async fn panel_stuck(state: State<'_, AppState>, webview: Webview, path: String, why: String) -> Result<(), String> {
    if !is_panel(&state, &webview) {
        return Err("not a panel".into());
    }
    let path: String = path.chars().filter(|c| !c.is_control()).take(300).collect();
    let why: String = why.chars().filter(char::is_ascii_alphanumeric).take(20).collect();
    log::info!("[{}] stuck on X's loading screen at {path} ({why}): going to x.com/home", webview.label());
    Ok(())
}

/// One panel a link can be sent to.
#[derive(Serialize)]
struct Target {
    id: String,
    label: String,
    color: String,
    /// Place on screen, from 0 (Ctrl/Cmd+1 is 0).
    index: usize,
    /// Share of the window width (the panel layout drawn when asking).
    share: f64,
}

/// What panel.js does after send_link.
#[derive(Serialize, Default)]
struct Sent {
    /// Opened in this panel (its name).
    to: Option<String>,
    /// More than one other panel: ask which one.
    targets: Vec<Target>,
    /// With `targets`: the calling panel, so the layout can be drawn whole.
    me: Option<Target>,
    msg: Option<String>,
}

/// "다른 패널에서 열기" (panel.js): open `url` in the panel of `to`, or,
/// with no `to`, in the only other panel. With several others, hand back the
/// list so the panel can ask. Only x.com pages (nav::x_link), and no lock
/// is held while touching webviews. The caller is the panel by its label.
#[tauri::command]
async fn send_link(app: AppHandle, state: State<'_, AppState>, webview: Webview, url: String, to: Option<String>) -> Result<Sent, String> {
    let (me, mine, others): (Uuid, Target, Vec<(Uuid, Target)>) = {
        let s = state.settings.lock().unwrap();
        let sum: f64 = s.ratios.iter().sum();
        let mut all: Vec<(Uuid, Target)> = s
            .accounts
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let share = match s.ratios.get(i) {
                    Some(r) if sum > 0.0 => r / sum,
                    _ => 1.0 / s.accounts.len() as f64,
                };
                (a.id, Target { id: a.id.to_string(), label: a.label.clone(), color: a.color.clone(), index: i, share })
            })
            .collect();
        let at = s.accounts.iter().position(|a| panel_label(a) == webview.label()).ok_or("not a panel")?;
        let (me, mine) = all.remove(at);
        (me, mine, all)
    };
    let url = nav::x_link(&url).map_err(|_| "X 링크가 아님".to_string())?;
    let pick = match &to {
        Some(t) => others.iter().find(|(_, o)| &o.id == t).ok_or("그 패널이 없음")?,
        None => match others.len() {
            0 => return Ok(Sent { msg: Some("다른 패널이 없음 · 상단 바의 ＋ 계정으로 추가".into()), ..Default::default() }),
            1 => &others[0],
            _ => return Ok(Sent { targets: others.into_iter().map(|(_, t)| t).collect(), me: Some(mine), ..Default::default() }),
        },
    };
    let (id, label) = (pick.0, pick.1.label.clone());
    let wv = app.get_webview(&format!("panel-{}", id.simple())).ok_or_else(|| {
        if state.off.lock().unwrap().contains(&id) { "그 패널은 꺼져 있음" } else { "그 패널이 없음" }
    })?;
    log::info!("send_link: {url} from {me} to {id}");
    go_to(&wv, &url)?;
    focus_account(&app, id);
    Ok(Sent { to: Some(label), ..Default::default() })
}

/// Longest panel name, in characters.
const MAX_LABEL: usize = 30;

/// Settings: new name for a panel.
#[tauri::command]
async fn rename_account(app: AppHandle, state: State<'_, AppState>, id: String, label: String) -> Result<(), String> {
    let label: String = label.trim().chars().filter(|c| !c.is_control()).collect();
    if label.is_empty() {
        return Err("이름이 비어 있음".into());
    }
    if label.chars().count() > MAX_LABEL {
        return Err(format!("이름은 {MAX_LABEL}자까지"));
    }
    let snapshot = {
        let mut s = state.settings.lock().unwrap();
        let acc = s.accounts.iter_mut().find(|a| a.id.to_string() == id).ok_or("no such account")?;
        acc.label = label;
        s.clone()
    };
    state.save(&snapshot);
    relayout(&app);
    Ok(())
}

/// Settings: move a panel to place `to` (0 = leftmost). Its width share
/// moves with it; Ctrl/Cmd+1..9 follow the new order.
#[tauri::command]
async fn move_account(app: AppHandle, state: State<'_, AppState>, id: String, to: usize) -> Result<(), String> {
    let snapshot = {
        let mut s = state.settings.lock().unwrap();
        let from = s.accounts.iter().position(|a| a.id.to_string() == id).ok_or("no such account")?;
        if !settings::move_panel(&mut s, from, to) {
            return Ok(());
        }
        s.clone()
    };
    state.save(&snapshot);
    relayout(&app);
    Ok(())
}

/// Window hidden to the tray (`hidden`) or back. Hiding: every playing
/// video/audio is paused (panel.js) and, on Windows, the panel is muted
/// as well, so nothing can be heard while hidden; the panels are hidden
/// too (relayout), which lets the webviews stop painting. Pages keep
/// running (unread counts still update). Shown again: unmuted, but paused
/// media stay paused.
/// Called before the window hides and after it shows, never on the main
/// thread and with no lock held (tray.rs).
pub(crate) fn set_window_hidden<R: Runtime>(app: &AppHandle<R>, hidden: bool) {
    let state = app.state::<AppState>();
    *state.hidden.lock().unwrap() = hidden;
    let labels: Vec<String> = state.settings.lock().unwrap().accounts.iter().map(panel_label).collect();
    for label in &labels {
        let Some(wv) = app.get_webview(label) else { continue };
        let _ = wv.eval(format!("window.dispatchEvent(new CustomEvent('paneful-hidden',{{detail:{hidden}}}))"));
        #[cfg(windows)]
        set_muted(&wv, hidden);
    }
    relayout(app);
}

/// Windows: WebView2's own mute, which also covers sound from frames the
/// page script can't reach (embedded players).
#[cfg(windows)]
fn set_muted<R: Runtime>(wv: &tauri::Webview<R>, muted: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_8;
    use windows::core::Interface;
    let label = wv.label().to_string();
    let _ = wv.with_webview(move |pw| unsafe {
        let r = pw
            .controller()
            .CoreWebView2()
            .and_then(|c| c.cast::<ICoreWebView2_8>())
            .and_then(|c| c.SetIsMuted(muted));
        if let Err(e) = r {
            log::warn!("[{label}] mute {muted} failed: {e}");
        }
    });
}

/// 새로고침 (tab button, settings memory list). Also restarts the dead panel
/// recovery count, and rebuilds the panel if its WebView2 is gone.
#[tauri::command]
async fn reload_panel(app: AppHandle, id: String) -> Result<(), String> {
    reload_label(&app, &format!("panel-{}", id.replace('-', ""))).await
}

/// Reload one panel by webview label (button, menu, `reload` shortcut).
/// A panel turned off is turned back on instead.
async fn reload_label<R: Runtime>(app: &AppHandle<R>, label: &str) -> Result<(), String> {
    if is_off(app, label) {
        return start_label(app, label).await;
    }
    let dead = health::is_dead(label);
    health::reset(label);
    if dead {
        return recreate_panel(app, label).await;
    }
    let wv = app.get_webview(label).ok_or("no such panel")?;
    wv.reload().map_err(|e| e.to_string())
}

/// Account id of a panel webview label, from any thread.
fn account_id_of<R: Runtime>(app: &AppHandle<R>, label: &str) -> Option<Uuid> {
    panel_id(&app.state::<AppState>(), label)
}

/// Panel turned off by hand (패널 끄기). health.rs leaves these alone.
pub(crate) fn is_off<R: Runtime>(app: &AppHandle<R>, label: &str) -> bool {
    account_id_of(app, label).is_some_and(|id| app.state::<AppState>().off.lock().unwrap().contains(&id))
}

/// 패널 끄기 (tab button, settings memory list): close the panel's webview
/// and leave the account in place, so a stuck panel can be stopped without
/// quitting the app, and started again with 다시 켜기 (`start_panel`).
/// Closing the webview doesn't need the page to answer, so it works on a
/// frozen panel too. macOS: its web content process is also killed if no
/// other webview shares it, in case a hung one doesn't exit on its own.
#[tauri::command]
async fn stop_panel(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let uid = {
        let s = state.settings.lock().unwrap();
        s.accounts.iter().find(|a| a.id.to_string() == id).map(|a| a.id).ok_or("no such account")?
    };
    let label = format!("panel-{}", uid.simple());
    // Marked first: a termination notice from the closing page is ignored
    // (health.rs), not answered with a reload.
    if !state.off.lock().unwrap().insert(uid) {
        return Ok(());
    }
    log::info!("[{label}] stopping panel");
    state.unread.lock().unwrap().remove(&uid);
    health::forget(&label);
    #[cfg(target_os = "macos")]
    let pid = memory::own_pid(&app, &label).await;
    if let Some(wv) = app.get_webview(&label) {
        if let Err(e) = wv.close() {
            state.off.lock().unwrap().remove(&uid);
            return Err(e.to_string());
        }
    }
    #[cfg(target_os = "macos")]
    if let Some(pid) = pid {
        // Sent after close(), so the page's delegate is already gone.
        let r = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        log::info!("[{label}] web content process {pid} killed: {}", r == 0);
    }
    relayout(&app);
    tray::refresh(&app);
    Ok(())
}

/// 다시 켜기: build a panel turned off with `stop_panel` again.
#[tauri::command]
async fn start_panel(app: AppHandle, id: String) -> Result<(), String> {
    start_label(&app, &format!("panel-{}", id.replace('-', ""))).await
}

async fn start_label<R: Runtime>(app: &AppHandle<R>, label: &str) -> Result<(), String> {
    let id = account_id_of(app, label).ok_or("no such panel")?;
    log::info!("[{label}] starting panel");
    health::forget(label);
    app.state::<AppState>().off.lock().unwrap().remove(&id);
    if let Err(e) = recreate_panel(app, label).await {
        // Still no webview: keep showing it as off, so 다시 켜기 stays.
        if app.get_webview(label).is_none() {
            app.state::<AppState>().off.lock().unwrap().insert(id);
            relayout(app);
        }
        return Err(e);
    }
    focus_account(app, id);
    Ok(())
}

/// Settings screen: memory per panel (memory.rs).
#[tauri::command]
async fn panel_memory(app: AppHandle) -> memory::Report {
    memory::measure(&app).await
}

#[tauri::command]
async fn add_account(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let window = app.get_window(MAIN).ok_or("no window")?;
    // Create the webview first and only then record the account, so a
    // failed add_child leaves no panel-less account behind.
    let mut acc = Account::new("", COLORS[0]);
    let rect = layout::Rect { x: 0.0, y: layout::TOP_STRIP, w: 10.0, h: 10.0 };
    let keys_json = bindings_json(&state.settings.lock().unwrap().clone());
    add_panel(&window, &acc, rect, &keys_json).map_err(|e| e.to_string())?;
    let (w, _) = logical_size(&window); // not under the lock, see apply_ratios
    {
        let mut s = state.settings.lock().unwrap();
        let n = s.accounts.len();
        acc.label = format!("계정 {}", n + 1);
        acc.color = COLORS[n % COLORS.len()].into();
        s.accounts.push(acc);
        s.ratios = layout::clamp_ratios(w, &layout::with_added(&s.ratios));
        let snapshot = s.clone();
        drop(s);
        state.save(&snapshot);
    }
    // The new panel should be seen (it starts on the login page).
    *state.zoom.lock().unwrap() = false;
    relayout(&app);
    Ok(())
}

#[tauri::command]
async fn remove_account(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let window = app.get_window(MAIN).ok_or("no window")?;
    let (w, _) = logical_size(&window); // not under the lock, see apply_ratios
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
        state.unread.lock().unwrap().remove(&acc.id);
        state.off.lock().unwrap().remove(&acc.id);
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
    health::forget(&panel_label(&acc));
    if let Some(wv) = app.get_webview(&panel_label(&acc)) {
        wv.close().map_err(|e| e.to_string())?;
    }
    relayout(&app);
    tray::refresh(&app);

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

// ---------------------------------------------------------------- settings
// Never hold `settings` while calling into windows, webviews, the dialog or
// the global shortcut plugin: those wait for the main thread, which locks
// `settings` in relayout.

/// Current in-app shortcut bindings. panel.js asks on every page load, since
/// its initialization script keeps the bindings the panel was created with.
#[tauri::command]
async fn panel_keys(state: State<'_, AppState>) -> Result<BTreeMap<String, keys::Binding>, String> {
    Ok(keys::bindings(&state.settings.lock().unwrap().keys))
}

/// X shortcuts as moved in settings. panel.js asks on every page load, like
/// `panel_keys`.
#[tauri::command]
async fn panel_xkeys(state: State<'_, AppState>) -> Result<keys::XKeys, String> {
    Ok(keys::x_keys(&state.settings.lock().unwrap().x_keys))
}

/// Tell every panel the new X key map.
fn push_xkeys<R: Runtime>(app: &AppHandle<R>, s: &Settings) {
    let json = x_keys_json(s);
    for acc in &s.accounts {
        if let Some(wv) = app.get_webview(&panel_label(acc)) {
            let _ = wv.eval(format!(
                "window.dispatchEvent(new CustomEvent('paneful-xkeys',{{detail:{json}}}))"
            ));
        }
    }
}

/// Move one X shortcut. `accel`: None = back to X's own key, "" = off.
#[tauri::command]
async fn set_x_key(app: AppHandle, state: State<'_, AppState>, id: String, accel: Option<String>) -> Result<(), String> {
    let Some(&(_, _, _, def)) = keys::X.iter().find(|x| x.0 == id) else {
        return Err("unknown shortcut".into());
    };
    let mut accel = accel.map(|a| a.trim().to_string());
    let (app_map, x_map) = {
        let s = state.settings.lock().unwrap();
        (s.key_map(), s.x_keys.clone())
    };
    match accel.as_deref() {
        None => {}
        Some("") if def.is_none() => return Err("연속키는 X가 직접 받아서 끌 수 없음".into()),
        Some("") => {}
        Some(k) => {
            let sc = keys::validate_x(&app_map, &x_map, &id, k)?;
            // X's own key again: same as no change.
            if def.and_then(|d| keys::parse(d).ok()).is_some_and(|d| d.mods == sc.mods && d.key == sc.key) {
                accel = None;
            }
        }
    }
    let snapshot = {
        let mut s = state.settings.lock().unwrap();
        match accel {
            None => s.x_keys.remove(&id),
            Some(a) => s.x_keys.insert(id.clone(), a),
        };
        s.clone()
    };
    state.save(&snapshot);
    push_xkeys(&app, &snapshot);
    relayout(&app);
    Ok(())
}

/// Tell every panel the new bindings.
fn push_keys<R: Runtime>(app: &AppHandle<R>, s: &Settings) {
    let json = bindings_json(s);
    for acc in &s.accounts {
        if let Some(wv) = app.get_webview(&panel_label(acc)) {
            let _ = wv.eval(format!(
                "window.dispatchEvent(new CustomEvent('paneful-keys',{{detail:{json}}}))"
            ));
        }
    }
}

/// Change one shortcut. `accel`: None = back to default, "" = off.
#[tauri::command]
async fn set_key(app: AppHandle, state: State<'_, AppState>, id: String, accel: Option<String>) -> Result<(), String> {
    if keys::default_of(&id).is_none() {
        return Err("unknown shortcut".into());
    }
    let accel = accel.map(|a| a.trim().to_string());
    let map = state.settings.lock().unwrap().key_map();
    // What the shortcut will be once stored (None: off).
    let effective = match &accel {
        None => keys::default_of(&id).map(str::to_string),
        Some(a) if a.is_empty() => None,
        Some(a) => Some(a.clone()),
    };
    if let Some(k) = &effective {
        let sc = keys::validate(&map, &id, k)?;
        let x_map = state.settings.lock().unwrap().x_keys.clone();
        if let Some(l) = keys::x_taken(&x_map, &sc, None) {
            return Err(format!("{}는 이미 X \"{}\"에 쓰는 중", keys::caps(&sc).join("+"), l));
        }
    }

    if id == keys::GLOBAL {
        let registered = state.hotkey.lock().unwrap().clone();
        if registered != effective {
            if let Some(old) = &registered {
                tray::unregister(&app, old);
            }
            match tray::register(&app, effective.as_deref()) {
                Ok(k) => {
                    *state.hotkey.lock().unwrap() = k;
                    *state.hotkey_error.lock().unwrap() = None;
                }
                Err(e) => {
                    log::warn!("{e}");
                    // Keep the old one working.
                    let back = tray::register(&app, registered.as_deref()).ok().flatten();
                    *state.hotkey.lock().unwrap() = back;
                    *state.hotkey_error.lock().unwrap() = Some(e.clone());
                    relayout(&app);
                    return Err(e);
                }
            }
        }
        let snapshot = {
            let mut s = state.settings.lock().unwrap();
            s.hotkey = accel;
            s.clone()
        };
        state.save(&snapshot);
        tray::refresh(&app);
    } else {
        let snapshot = {
            let mut s = state.settings.lock().unwrap();
            match accel {
                None => s.keys.remove(&id),
                Some(a) => s.keys.insert(id.clone(), a),
            };
            s.clone()
        };
        state.save(&snapshot);
        push_keys(&app, &snapshot);
    }
    relayout(&app);
    Ok(())
}

/// Settings screen is waiting for a key: don't let the global shortcut
/// hide the window meanwhile.
#[tauri::command]
async fn set_recording(on: bool) {
    tray::RECORDING.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Folder picker for the save folder. Ok(false): cancelled.
#[tauri::command]
async fn pick_save_dir(app: AppHandle, state: State<'_, AppState>) -> Result<bool, String> {
    let start = media::save_dir(&app).ok();
    let mut dialog = app.dialog().file().set_title("저장 폴더 선택");
    if let Some(d) = start.filter(|d| d.is_dir()) {
        dialog = dialog.set_directory(d);
    }
    if let Some(w) = app.get_window(MAIN) {
        dialog = dialog.set_parent(&w);
    }
    // Blocks until the dialog closes; keep it off the async workers.
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_folder())
        .await
        .map_err(|e| e.to_string())?;
    let Some(picked) = picked else { return Ok(false) };
    let dir = picked.into_path().map_err(|e| e.to_string())?;
    if !dir.is_dir() {
        return Err(format!("폴더가 아님: {}", dir.display()));
    }
    let snapshot = {
        let mut s = state.settings.lock().unwrap();
        s.save_dir = Some(dir.to_string_lossy().into_owned());
        s.clone()
    };
    state.save(&snapshot);
    relayout(&app);
    Ok(true)
}

/// Back to `<Downloads>/Paneful`.
#[tauri::command]
async fn reset_save_dir(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let snapshot = {
        let mut s = state.settings.lock().unwrap();
        s.save_dir = None;
        s.clone()
    };
    state.save(&snapshot);
    relayout(&app);
    Ok(())
}

/// Show the save folder in Explorer / Finder (created if missing).
#[tauri::command]
async fn open_save_dir(app: AppHandle) -> Result<(), String> {
    let dir = media::save_dir(&app)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("폴더를 만들 수 없음: {e}"))?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Portable data folder in Explorer.
#[tauri::command]
async fn open_data_dir(app: AppHandle) -> Result<(), String> {
    let dir = portable::root().ok_or("포터블 모드가 아님")?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- update

/// "지금 확인" in settings. Returns once the check is done; the result
/// arrives with the next layout event.
#[tauri::command]
async fn check_update(app: AppHandle) {
    update::check(&app, true).await;
}

/// Settings checkbox: look for new versions at all.
#[tauri::command]
async fn set_update_check(app: AppHandle, state: State<'_, AppState>, on: bool) -> Result<(), String> {
    let snapshot = {
        let mut s = state.settings.lock().unwrap();
        s.update_check = on;
        s.clone()
    };
    state.save(&snapshot);
    update::publish(&app);
    if on {
        let found = state.update.lock().unwrap().newer.is_some();
        if !found {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { update::check(&app, false).await });
        }
    }
    Ok(())
}

/// "이 버전 건너뛰기": no more notices for the version on offer. `undo`
/// clears the skip again.
#[tauri::command]
async fn skip_update(app: AppHandle, state: State<'_, AppState>, undo: Option<bool>) -> Result<(), String> {
    let newer = state.update.lock().unwrap().newer.clone();
    let snapshot = {
        let mut s = state.settings.lock().unwrap();
        s.skip_version = if undo.unwrap_or(false) { None } else { newer.or(s.skip_version.take()) };
        s.clone()
    };
    state.save(&snapshot);
    update::publish(&app);
    Ok(())
}

/// X screen rules in effect. rules.js asks on every page load and on the
/// `paneful-rules` event. Data only (selectors and URL fragments).
#[tauri::command]
async fn panel_rules(state: State<'_, AppState>) -> Result<rules::Rules, String> {
    Ok(state.rules.lock().unwrap().clone())
}

/// "지금 확인" for the X screen rules.
#[tauri::command]
async fn check_rules(app: AppHandle) {
    rules::check(&app, true).await;
}

/// Settings checkbox: download rule fixes at all. Off = built-in rules.
#[tauri::command]
async fn set_rules_update(app: AppHandle, state: State<'_, AppState>, on: bool) -> Result<(), String> {
    let snapshot = {
        let mut s = state.settings.lock().unwrap();
        s.rules_update = on;
        s.clone()
    };
    state.save(&snapshot);
    rules::set_enabled(&app, on);
    relayout(&app);
    Ok(())
}

/// Settings checkbox: unread badges on the tray / taskbar / Dock icons.
#[tauri::command]
async fn set_unread_badge(app: AppHandle, state: State<'_, AppState>, on: bool) -> Result<(), String> {
    let snapshot = {
        let mut s = state.settings.lock().unwrap();
        s.unread_badge = on;
        s.clone()
    };
    state.save(&snapshot);
    tray::refresh(&app);
    relayout(&app);
    Ok(())
}

/// Settings checkbox: system notifications for new 알림 / 쪽지.
#[tauri::command]
async fn set_notify(app: AppHandle, state: State<'_, AppState>, on: bool) -> Result<(), String> {
    let snapshot = {
        let mut s = state.settings.lock().unwrap();
        s.notify = on;
        s.clone()
    };
    state.save(&snapshot);
    log::info!("notifications {on}");
    #[cfg(windows)]
    if on {
        notify::register_app_id(&app.config().identifier, "Paneful");
    }
    relayout(&app);
    Ok(())
}

/// Tell a panel whether modifier + wheel zooms (panel.js `paneful-wheel`).
fn wheel_js(on: bool) -> String {
    format!("window.dispatchEvent(new CustomEvent('paneful-wheel',{{detail:{on}}}))")
}

/// Settings checkbox: Ctrl/Cmd + wheel zooms a panel. The zoom keys stay.
#[tauri::command]
async fn set_wheel_zoom(app: AppHandle, state: State<'_, AppState>, on: bool) -> Result<(), String> {
    let snapshot = {
        let mut s = state.settings.lock().unwrap();
        s.wheel_zoom = on;
        s.clone()
    };
    state.save(&snapshot);
    log::info!("wheel zoom {on}");
    for acc in &snapshot.accounts {
        if let Some(wv) = app.get_webview(&panel_label(acc)) {
            let _ = wv.eval(wheel_js(on));
        }
    }
    relayout(&app);
    Ok(())
}

/// Release page in the system browser.
#[tauri::command]
async fn open_update(app: AppHandle) -> Result<(), String> {
    update::open(&app)
}

/// Settings "복사" (macOS): put the terminal install line on the clipboard.
/// Fixed text only, the UI can't put anything else there.
#[tauri::command]
async fn copy_install_cmd(app: AppHandle) -> Result<(), String> {
    tauri_plugin_clipboard_manager::ClipboardExt::clipboard(&app)
        .write_text(update::INSTALL_MAC)
        .map_err(|e| format!("복사 실패: {e}"))
}

// ---------------------------------------------------------------- entry

/// Release builds abort on panic (Cargo.toml), so a panic kills the whole
/// app with nothing on screen. Put the message and place in the log first
/// so a crash report can say where it happened.
fn log_panics() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current().name().unwrap_or("?").to_string();
        log::error!("panic on thread '{thread}': {info}");
        log::logger().flush();
        prev(info);
    }));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Before the builder: the log file goes next to the exe in portable mode.
    let log_target = match portable::init() {
        portable::Mode::Portable(dir) => tauri_plugin_log::TargetKind::Folder {
            path: dir.join("logs"),
            file_name: Some("paneful".into()),
        },
        _ => tauri_plugin_log::TargetKind::LogDir { file_name: Some("paneful".into()) },
    };
    let builder = tauri::Builder::default();
    // Replaces tauri-runtime-wry's own unlimited silent reload (health.rs).
    #[cfg(target_os = "macos")]
    let builder = builder.on_web_content_process_terminate(health::on_terminated);
    builder
        .plugin(tauri_plugin_opener::init())
        // Folder picker in settings. Called from Rust only; no JS permission.
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(tray::on_shortcut)
                .build(),
        )
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                // Default is 40 KB and KeepOne: the file is wiped when full,
                // and the per-relayout diagnostics fill 40 KB in minutes, so
                // what happened before was gone by the time anyone looked.
                // 2 MB, and the last 3 full files kept (renamed with a date).
                .max_file_size(2_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(3))
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(log_target),
                ])
                .build(),
        )
        .setup(|app| {
            log_panics();
            let path = match portable::mode() {
                portable::Mode::Portable(dir) => {
                    // First portable start: bring AppData's data over (copy only).
                    let old = app.path().app_config_dir()?.join("settings.json");
                    let old_udf = app.path().app_local_data_dir()?;
                    match portable::migrate(dir, &old, &old_udf) {
                        portable::Migration::Skipped => {}
                        r => log::info!("portable: copied AppData data -> {r:?}"),
                    }
                    log::info!("portable: data in {}", dir.display());
                    dir.join("settings.json")
                }
                other => {
                    if let portable::Mode::Fallback { dir, error } = other {
                        log::warn!("portable: {} not writable ({error}); using AppData", dir.display());
                    }
                    app.path().app_config_dir()?.join("settings.json")
                }
            };
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
                settings_open: Mutex::new(false),
                hotkey: Mutex::new(hotkey),
                hotkey_error: Mutex::new(hotkey_error),
                capture: Mutex::new(false),
                zoom: Mutex::new(false),
                hidden: Mutex::new(false),
                update: Mutex::new(update::Status::default()),
                rules: Mutex::new(rules::initial(app.handle(), s.rules_update)),
                rules_status: Mutex::new(rules::Status::default()),
                unread: Mutex::new(BTreeMap::new()),
                off: Mutex::new(HashSet::new()),
                path,
                persist,
            };
            state.save(&s);
            app.manage(state);
            app.manage(notify::State::default());
            #[cfg(windows)]
            if s.notify {
                notify::register_app_id(&app.config().identifier, "Paneful");
            }

            let window = WindowBuilder::new(app, MAIN)
                .title("Paneful")
                .inner_size(1280.0, 860.0)
                .min_inner_size(400.0, 300.0)
                // Dark title bar (Windows) / appearance (macOS) to match
                // X's dark theme and the host UI.
                .theme(Some(tauri::Theme::Dark))
                .build()?;
            measure_titlebar(&window);
            let (w, h) = logical_size(&window);
            // Not fatal: without a tray icon the app still works, it just
            // can't be hidden from the top bar button (see tray::hide).
            let hotkey = app.state::<AppState>().hotkey.lock().unwrap().clone();
            if let Err(e) = tray::create(app.handle(), hotkey.as_deref()) {
                log::error!("tray icon failed: {e}");
            }
            // Non-blocking: the callback runs when it's closed.
            if let Some(msg) = fallback_warning() {
                use tauri_plugin_dialog::MessageDialogKind;
                app.dialog().message(msg).title("Paneful 포터블").kind(MessageDialogKind::Warning).show(|_| {});
            }
            // New version check, off the main thread (update.rs).
            update::spawn(app.handle());
            // X screen rule fixes, same way (rules.rs).
            rules::spawn(app.handle());

            // Host UI first so the panels stack above it; it stays visible
            // in the top bar and in the gaps between panels.
            window.add_child(
                with_udf(WebviewBuilder::new(HOST, WebviewUrl::App("index.html".into()))),
                LogicalPosition::new(0.0, titlebar()),
                LogicalSize::new(w, h),
            )?;

            let rects = layout::panels(w, h, top_of(&s), &s.ratios);
            let keys_json = bindings_json(&s);
            for (acc, r) in s.accounts.iter().zip(rects) {
                add_panel(&window, acc, r, &keys_json)?;
            }
            // Each new webview grabs focus; start on the first panel instead.
            if let Some(acc) = s.accounts.first() {
                if let Some(wv) = app.get_webview(&panel_label(acc)) {
                    let _ = wv.set_focus();
                }
            }

            let handle = app.handle().clone();
            let win = window.clone();
            window.on_window_event(move |e| {
                // Runs on the main thread. The title bar area can change
                // (full screen) or only settle once the window is shown.
                let moved = measure_titlebar(&win);
                if moved || matches!(e, WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }) {
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
            stop_panel,
            start_panel,
            panel_memory,
            add_account,
            remove_account,
            panel_focused,
            panel_unread,
            panel_view,
            shortcut,
            panel_keys,
            panel_xkeys,
            paste_link,
            open_link,
            send_link,
            panel_stuck,
            rename_account,
            move_account,
            set_key,
            set_x_key,
            set_recording,
            pick_save_dir,
            reset_save_dir,
            open_save_dir,
            open_data_dir,
            capture::capture_begin,
            capture::capture_shot,
            capture::capture_drop,
            capture::capture_finish,
            capture::capture_close,
            capture::capture_slice,
            capture::capture_export,
            check_update,
            set_update_check,
            skip_update,
            open_update,
            copy_install_cmd,
            panel_rules,
            check_rules,
            set_rules_update,
            set_unread_badge,
            set_notify,
            set_wheel_zoom
        ])
        .run(tauri::generate_context!())
        .expect("error while running paneful");
}

#[cfg(test)]
mod go_tests {
    #[test]
    fn go_js_quotes_the_address() {
        let u = crate::nav::x_link("https://x.com/a/status/1?x=\"'</script>").unwrap();
        let js = super::go_js(&u);
        assert!(js.contains("paneful-go"));
        assert!(js.contains(&serde_json::to_string(u.as_str()).unwrap()));
        assert!(!js.contains('"') || js.matches('"').count() == 2, "only the JSON string's quotes: {js}");
    }
}
