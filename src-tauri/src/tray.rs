//! Hide to the tray (Windows) / menu bar (macOS), and the global shortcut
//! that shows and hides the window from any app.
//!
//! Hidden means gone from the taskbar and Alt+Tab (Windows: a hidden window
//! has no taskbar button) or from the Dock and Cmd+Tab (macOS: Accessory
//! activation policy while hidden). The tray icon stays, so the app can be
//! brought back or quit.

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, Runtime,
};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

use crate::{keys, unread, MAIN};

/// What the global shortcut / tray click should do.
#[derive(Clone, Copy)]
pub enum Toggle {
    /// Global shortcut: hide only when the window is in front with focus,
    /// otherwise bring it forward (also when it was just behind other windows).
    Shortcut,
    /// Tray click: the click itself takes focus away from the window
    /// (Windows), so only visibility counts.
    Tray,
}

pub fn hide<R: Runtime>(app: &AppHandle<R>) {
    let Some(w) = app.get_window(MAIN) else { return };
    // No tray icon and no shortcut: nothing could bring the window back.
    let tray = app.tray_by_id("main").is_some();
    let hotkey = app.state::<crate::AppState>().hotkey.lock().unwrap().is_some();
    if !tray && !hotkey {
        log::warn!("not hiding: no tray icon and no global shortcut");
        return;
    }
    // Sound off (and panels hidden) before the window goes, so nothing
    // plays on once it is out of sight.
    crate::set_window_hidden(app, true);
    let _ = w.hide();
    #[cfg(target_os = "macos")]
    {
        // NSApp hide hands focus back to the previous app; Accessory drops
        // the Dock icon and the Cmd+Tab entry.
        let _ = app.hide();
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
}

pub fn show<R: Runtime>(app: &AppHandle<R>) {
    let Some(w) = app.get_window(MAIN) else { return };
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
        let _ = app.show();
    }
    let _ = w.show();
    // Hiding drops the taskbar button and the overlay with it.
    #[cfg(windows)]
    if BADGE.load(Ordering::Relaxed) {
        set_overlay(&w, true);
    }
    crate::set_window_hidden(app, false);
    if w.is_minimized().unwrap_or(false) {
        let _ = w.unminimize();
    }
    let _ = w.set_focus();
    // Keyboard focus back into the active panel so X's shortcuts work at once.
    crate::refocus_active(app);
}

/// Runs `toggle` on a worker thread instead of inside the tray / hotkey
/// callback. Those callbacks run on the main thread, where every window and
/// webview call (show, focus, WebView2 MoveFocus, relayout) executes nested
/// inside the callback. From a worker the same calls are queued to the event
/// loop and run one by one, the way the async commands already do it.
pub fn toggle_later<R: Runtime>(app: &AppHandle<R>, how: Toggle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // One toggle at a time, so fast repeats don't both read "visible".
        // Only worker threads take this lock, never the main thread.
        static BUSY: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _busy = BUSY.lock().unwrap_or_else(|e| e.into_inner());
        toggle(&app, how)
    });
}

pub fn toggle<R: Runtime>(app: &AppHandle<R>, how: Toggle) {
    let label = match how {
        Toggle::Shortcut => "hotkey",
        Toggle::Tray => "tray",
    };
    log::info!("toggle {label}: start");
    toggle_inner(app, how);
    log::info!("toggle {label}: end");
}

fn toggle_inner<R: Runtime>(app: &AppHandle<R>, how: Toggle) {
    let Some(w) = app.get_window(MAIN) else { return };
    let shown = w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false);
    let in_front = match how {
        Toggle::Shortcut => shown && w.is_focused().unwrap_or(false),
        Toggle::Tray => shown,
    };
    if in_front {
        hide(app);
    } else {
        show(app);
    }
}

fn tooltip(hotkey: Option<&str>) -> String {
    match hotkey {
        Some(k) => format!("Paneful ({})", keys::display(k)),
        None => "Paneful".into(),
    }
}

/// Tooltip, icon badge, taskbar overlay (Windows) / Dock badge (macOS),
/// menu bar number (macOS) and window title after the global shortcut, an
/// unread count or the badge setting changed. Reads the state, lets go of
/// every lock, then talks to the tray and the window.
pub fn refresh<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<crate::AppState>();
    let hotkey = state.hotkey.lock().unwrap().clone();
    let badges = state.settings.lock().unwrap().unread_badge;
    let lines = crate::unread_lines(&state);

    let mut tip = tooltip(hotkey.as_deref());
    for (label, u) in &lines {
        tip.push('\n');
        tip.push_str(&format!("{label}: {}", u.text()));
    }
    // Windows cuts tooltips at 127 characters.
    if tip.chars().count() > 120 {
        tip = tip.chars().take(119).collect::<String>() + "…";
    }
    // Badges off: the count only goes in the tooltip (and the app's own bar).
    let total = if badges {
        unread::total(lines.iter().flat_map(|(_, u)| [u.n.as_str(), u.dm.as_str()]))
    } else {
        String::new()
    };

    // Last values handed out, so nothing is set again when only one count moved.
    static LAST: std::sync::Mutex<Option<(String, String)>> = std::sync::Mutex::new(None);
    let now = (tip.clone(), total.clone());
    let prev = {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::replace(&mut *last, Some(now.clone()))
    };
    if prev.as_ref() == Some(&now) {
        return;
    }
    let badge_changed = prev.as_ref().is_none_or(|p| p.1.is_empty() != total.is_empty());
    let total_changed = prev.as_ref().is_none_or(|p| p.1 != total);
    BADGE.store(!total.is_empty(), Ordering::Relaxed);

    if let Some(t) = app.tray_by_id("main") {
        let _ = t.set_tooltip(Some(&tip));
        if badge_changed {
            if let Some(icon) = app.default_window_icon() {
                let img = if total.is_empty() {
                    icon.clone().to_owned()
                } else {
                    let rgba = unread::with_badge(icon.rgba(), icon.width(), icon.height());
                    tauri::image::Image::new_owned(rgba, icon.width(), icon.height())
                };
                if let Err(e) = t.set_icon(Some(img)) {
                    log::warn!("tray icon not updated: {e}");
                }
            }
        }
        // macOS: the number next to the menu bar icon. Not on Windows.
        #[cfg(target_os = "macos")]
        if total_changed {
            let _ = t.set_title(if total.is_empty() { None } else { Some(total.as_str()) });
        }
    }
    if let Some(w) = app.get_window(MAIN) {
        // Taskbar / Dock window list, like a browser tab: "(3) Paneful".
        if total_changed {
            let title = if total.is_empty() { "Paneful".to_string() } else { format!("({total}) Paneful") };
            let _ = w.set_title(&title);
        }
        #[cfg(windows)]
        if badge_changed {
            set_overlay(&w, !total.is_empty());
        }
        // Dock icon badge (shows while the window is out; hidden = no Dock icon).
        #[cfg(target_os = "macos")]
        if total_changed {
            if let Err(e) = w.set_badge_label(if total.is_empty() { None } else { Some(total.clone()) }) {
                log::warn!("dock badge not updated: {e}");
            }
        }
    }
}

/// Badge currently on (something unread and badges turned on).
static BADGE: AtomicBool = AtomicBool::new(false);

/// Windows: red "!" over the taskbar button. Off: none.
#[cfg(windows)]
fn set_overlay<R: Runtime>(w: &tauri::Window<R>, on: bool) {
    let img = on.then(|| tauri::image::Image::new_owned(unread::overlay(32), 32, 32));
    if let Err(e) = w.set_overlay_icon(img) {
        log::warn!("taskbar badge not updated: {e}");
    }
}

/// With a newer release on offer (`update`, "0.0.5"), a "새 버전 받기" item
/// goes on top.
fn menu<R: Runtime>(app: &AppHandle<R>, update: Option<&str>) -> tauri::Result<Menu<R>> {
    let toggle_item = MenuItem::with_id(app, "toggle", "Paneful 보이기/숨기기", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", "설정", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "종료", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    match update {
        Some(v) => {
            let up = MenuItem::with_id(app, "update", format!("새 버전 v{v} {}", if cfg!(target_os = "macos") { "업데이트" } else { "받기" }), true, None::<&str>)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            Menu::with_items(app, &[&up, &sep2, &toggle_item, &settings_item, &sep, &quit])
        }
        None => Menu::with_items(app, &[&toggle_item, &settings_item, &sep, &quit]),
    }
}

/// New version notice changed (None: nothing to announce).
pub fn set_update<R: Runtime>(app: &AppHandle<R>, update: Option<&str>) {
    static SHOWN: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
    if shown.as_deref() == update {
        return;
    }
    let Some(t) = app.tray_by_id("main") else { return };
    match menu(app, update) {
        Ok(m) => {
            if let Err(e) = t.set_menu(Some(m)) {
                log::warn!("tray menu not updated: {e}");
                return;
            }
            *shown = update.map(str::to_string);
        }
        Err(e) => log::warn!("tray menu not built: {e}"),
    }
}

/// Tray / menu bar icon with "보이기/숨기기", "설정" and "종료" (and "새 버전
/// 받기" while one is on offer, see set_update).
/// `hotkey` is only shown as a hint in the tooltip.
pub fn create<R: Runtime>(app: &AppHandle<R>, hotkey: Option<&str>) -> tauri::Result<()> {
    let menu = menu(app, None)?;

    let mut b = TrayIconBuilder::with_id("main")
        .tooltip(tooltip(hotkey))
        .menu(&menu)
        .on_menu_event(|app, e| match e.id().as_ref() {
            "toggle" => toggle_later(app, Toggle::Tray),
            "update" => {
                if let Err(e) = crate::update::open(app) {
                    log::warn!("opening the release page failed: {e}");
                }
            }
            "settings" => {
                // Worker thread, same reason as toggle_later.
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    show(&app);
                    crate::open_settings(&app);
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            // Windows: left click toggles, right click opens the menu.
            // macOS: any click opens the menu (menu bar convention).
            #[cfg(not(target_os = "macos"))]
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = e
            {
                toggle_later(tray.app_handle(), Toggle::Tray);
            }
            #[cfg(target_os = "macos")]
            let _ = (tray, e);
        });
    #[cfg(not(target_os = "macos"))]
    {
        b = b.show_menu_on_left_click(false);
    }
    if let Some(icon) = app.default_window_icon() {
        b = b.icon(icon.clone());
    }
    b.build(app)?;
    Ok(())
}

/// Settings screen is waiting for a key press: the global shortcut must not
/// hide the window under it.
pub static RECORDING: AtomicBool = AtomicBool::new(false);

/// Global shortcut handler (registered with the plugin builder).
pub fn on_shortcut<R: Runtime>(app: &AppHandle<R>, _s: &Shortcut, e: ShortcutEvent) {
    if e.state == ShortcutState::Pressed && !RECORDING.load(Ordering::Relaxed) {
        toggle_later(app, Toggle::Shortcut);
    }
}

/// Register the show/hide shortcut. Ok(None) when turned off in settings.
pub fn register<R: Runtime>(app: &AppHandle<R>, hotkey: Option<&str>) -> Result<Option<String>, String> {
    let Some(k) = hotkey else { return Ok(None) };
    let sc = keys::parse(k)?;
    app.global_shortcut()
        .register(sc)
        .map_err(|e| format!("전역 단축키 {} 등록 실패 (다른 앱이 쓰는 중일 수 있음): {e}", keys::display(k)))?;
    Ok(Some(k.to_string()))
}

pub fn unregister<R: Runtime>(app: &AppHandle<R>, hotkey: &str) {
    if let Ok(sc) = keys::parse(hotkey) {
        if let Err(e) = app.global_shortcut().unregister(sc) {
            log::warn!("unregistering {hotkey} failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hotkey_parses() {
        let s: Result<Shortcut, _> = crate::settings::DEFAULT_HOTKEY.parse();
        assert!(s.is_ok());
    }

    #[test]
    fn display_keys() {
        let d = keys::display("CommandOrControl+Alt+X");
        if cfg!(target_os = "macos") {
            assert_eq!(d, "⌥+⌘+X");
        } else {
            assert_eq!(d, "Ctrl+Alt+X");
        }
    }
}
