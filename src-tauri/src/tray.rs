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
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

use crate::MAIN;

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
    let hotkey = app.state::<crate::AppState>().hotkey.is_some();
    if !tray && !hotkey {
        log::warn!("not hiding: no tray icon and no global shortcut");
        return;
    }
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

/// Tray / menu bar icon with "보이기/숨기기" and "종료".
/// `hotkey` is only shown as a hint in the tooltip.
pub fn create<R: Runtime>(app: &AppHandle<R>, hotkey: Option<&str>) -> tauri::Result<()> {
    let toggle_item = MenuItem::with_id(app, "toggle", "Paneful 보이기/숨기기", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "종료", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&toggle_item, &sep, &quit])?;
    let tooltip = match hotkey {
        Some(k) => format!("Paneful ({})", display(k)),
        None => "Paneful".into(),
    };

    let mut b = TrayIconBuilder::with_id("main")
        .tooltip(tooltip)
        .menu(&menu)
        .on_menu_event(|app, e| match e.id().as_ref() {
            "toggle" => toggle_later(app, Toggle::Tray),
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

/// Global shortcut handler (registered with the plugin builder).
pub fn on_shortcut<R: Runtime>(app: &AppHandle<R>, _s: &Shortcut, e: ShortcutEvent) {
    if e.state == ShortcutState::Pressed {
        toggle_later(app, Toggle::Shortcut);
    }
}

/// Register the show/hide shortcut. Ok(None) when turned off in settings.
pub fn register<R: Runtime>(app: &AppHandle<R>, hotkey: Option<&str>) -> Result<Option<String>, String> {
    let Some(k) = hotkey else { return Ok(None) };
    let sc: Shortcut = k.parse().map_err(|e| format!("단축키 형식 오류 \"{k}\": {e}"))?;
    app.global_shortcut()
        .register(sc)
        .map_err(|e| format!("전역 단축키 {} 등록 실패 (다른 앱이 쓰는 중일 수 있음): {e}", display(k)))?;
    Ok(Some(k.to_string()))
}

/// "CommandOrControl+Alt+X" -> ["Ctrl", "Alt", "X"] / ["⌘", "⌥", "X"].
pub fn keys(accel: &str) -> Vec<String> {
    let mac = cfg!(target_os = "macos");
    accel
        .split('+')
        .map(|p| {
            let t = p.trim();
            let s = match t.to_ascii_lowercase().as_str() {
                "commandorcontrol" | "cmdorctrl" | "commandorctrl" | "cmdorcontrol" => {
                    if mac { "⌘" } else { "Ctrl" }
                }
                "command" | "cmd" | "super" | "meta" => if mac { "⌘" } else { "Win" },
                "control" | "ctrl" => if mac { "⌃" } else { "Ctrl" },
                "alt" | "option" => if mac { "⌥" } else { "Alt" },
                "shift" => "Shift",
                _ => return t.to_ascii_uppercase(),
            };
            s.to_string()
        })
        .collect()
}

pub fn display(accel: &str) -> String {
    keys(accel).join("+")
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
        let d = display("CommandOrControl+Alt+X");
        if cfg!(target_os = "macos") {
            assert_eq!(d, "⌘+⌥+X");
        } else {
            assert_eq!(d, "Ctrl+Alt+X");
        }
    }
}
