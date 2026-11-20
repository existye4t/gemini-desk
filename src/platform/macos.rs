//! macOS platform integration (Apple Silicon primary, Intel x86_64 compatible).
//!
//! Capabilities implemented:
//! - Dark/Light system appearance detection via `NSUserDefaults` (`AppleInterfaceStyle`).
//! - Native `NSWindow` theme synchronization, rounded floating Quick Chat level & Spaces behavior,
//!   and `NSWindow::setAlphaValue` opacity for Floating Companion.
//! - Native macOS Application Menu Bar (`muda::Menu::init_for_nsapp`) providing standard
//!   `Cmd+C` / `Cmd+V` / `Cmd+X` / `Cmd+A` / `Cmd+Z` Edit actions inside WKWebView,
//!   plus GeminiDesk Settings (`Cmd+,`), New Chat (`Cmd+N`), Quick Chat, Companion, and Quit (`Cmd+Q`).
//! - LaunchAgent (`~/Library/LaunchAgents/io.geminidesk.GeminiDesk.plist`) startup registration.
//! - Unix domain socket single-instance activation (`$TMPDIR/io.geminidesk.GeminiDesk.sock`).

use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::thread;

use muda::{
    Menu, MenuItem, PredefinedMenuItem, Submenu,
    accelerator::{Accelerator, Code, Modifiers},
};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSWindow, NSWindowCollectionBehavior};
use objc2_foundation::{NSString, NSUserDefaults};
use tao::event_loop::EventLoopProxy;
use tao::platform::macos::WindowExtMacOS;
use tao::window::{Theme, Window};
use tracing::warn;

use crate::app::AppEvent;
use crate::platform::ActivationTarget;
use crate::tray::{
    MENU_CAPTURE_SCREENSHOT, MENU_CLIPBOARD_ACTIONS, MENU_COMPANION, MENU_NEW_CHAT,
    MENU_OPEN_GEMINI, MENU_QUICK_CHAT, MENU_QUIT, MENU_SETTINGS,
};

pub const MACOS_BUNDLE_ID: &str = "io.geminidesk.GeminiDesk";

/// Queries macOS `NSUserDefaults` (`AppleInterfaceStyle`) for system dark mode.
pub fn is_system_dark_mode() -> bool {
    let defaults = NSUserDefaults::standardUserDefaults();
    let key = NSString::from_str("AppleInterfaceStyle");
    defaults
        .stringForKey(&key)
        .map(|val| val.to_string().eq_ignore_ascii_case("Dark"))
        .unwrap_or(false)
}

pub fn style_main_window(window: &Window, dark: bool) {
    window.set_theme(Some(if dark { Theme::Dark } else { Theme::Light }));
    window.set_has_shadow(true);
    window.set_allows_automatic_window_tabbing(false);
}

pub fn style_quick_chat_window(window: &Window, dark: bool) {
    window.set_theme(Some(if dark { Theme::Dark } else { Theme::Light }));
    window.set_has_shadow(true);
    let ptr = window.ns_window() as *mut NSWindow;
    if !ptr.is_null() {
        unsafe {
            let ns_win = &*ptr;
            // Allow Quick Chat to appear smoothly on the active macOS Space and over full-screen apps.
            ns_win.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::FullScreenAuxiliary,
            );
            ns_win.setMovableByWindowBackground(true);
        }
    }
}

pub fn set_window_opacity(window: &Window, opacity_pct: u8) {
    let alpha = (opacity_pct.clamp(55, 100) as f64) / 100.0;
    let ptr = window.ns_window() as *mut NSWindow;
    if !ptr.is_null() {
        unsafe {
            (*ptr).setAlphaValue(alpha);
        }
    }
}

pub fn focus_window(window: &Window) {
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }
    window.set_focus();
}

pub fn is_window_still_active_or_owned(window: &Window) -> bool {
    let ptr = window.ns_window() as *mut NSWindow;
    if ptr.is_null() {
        return false;
    }
    unsafe { (*ptr).isKeyWindow() }
}

/// Installs the native macOS application menu bar so standard Edit shortcuts (`Cmd+C/V/X/A/Z`)
/// work reliably inside WKWebView alongside GeminiDesk app controls (`Cmd+,`, `Cmd+N`, `Cmd+Q`).
pub fn install_app_menu() -> Option<Menu> {
    let menu = Menu::new();

    let app_menu = Submenu::new("GeminiDesk", true);
    let _ = app_menu.append(&PredefinedMenuItem::about(Some("About GeminiDesk"), None));
    let _ = app_menu.append(&PredefinedMenuItem::separator());
    let _ = app_menu.append(&MenuItem::with_id(
        MENU_SETTINGS,
        "Settings...",
        true,
        Some(Accelerator::new(Modifiers::META, Code::Comma)),
    ));
    let _ = app_menu.append(&PredefinedMenuItem::separator());
    let _ = app_menu.append(&PredefinedMenuItem::hide(None));
    let _ = app_menu.append(&PredefinedMenuItem::hide_others(None));
    let _ = app_menu.append(&PredefinedMenuItem::show_all(None));
    let _ = app_menu.append(&PredefinedMenuItem::separator());
    let _ = app_menu.append(&MenuItem::with_id(
        MENU_QUIT,
        "Quit GeminiDesk",
        true,
        Some(Accelerator::new(Modifiers::META, Code::KeyQ)),
    ));

    let edit_menu = Submenu::new("Edit", true);
    let _ = edit_menu.append(&PredefinedMenuItem::undo(None));
    let _ = edit_menu.append(&PredefinedMenuItem::redo(None));
    let _ = edit_menu.append(&PredefinedMenuItem::separator());
    let _ = edit_menu.append(&PredefinedMenuItem::cut(None));
    let _ = edit_menu.append(&PredefinedMenuItem::copy(None));
    let _ = edit_menu.append(&PredefinedMenuItem::paste(None));
    let _ = edit_menu.append(&PredefinedMenuItem::select_all(None));

    let window_menu = Submenu::new("Window", true);
    let _ = window_menu.append(&MenuItem::with_id(
        MENU_OPEN_GEMINI,
        "Open Gemini",
        true,
        None,
    ));
    let _ = window_menu.append(&MenuItem::with_id(
        MENU_QUICK_CHAT,
        "Quick Chat",
        true,
        None,
    ));
    let _ = window_menu.append(&MenuItem::with_id(
        MENU_COMPANION,
        "Floating Companion",
        true,
        None,
    ));
    let _ = window_menu.append(&MenuItem::with_id(
        MENU_NEW_CHAT,
        "New Chat",
        true,
        Some(Accelerator::new(Modifiers::META, Code::KeyN)),
    ));
    let _ = window_menu.append(&PredefinedMenuItem::separator());
    let _ = window_menu.append(&MenuItem::with_id(
        MENU_CAPTURE_SCREENSHOT,
        "Capture Screenshot",
        true,
        None,
    ));
    let _ = window_menu.append(&MenuItem::with_id(
        MENU_CLIPBOARD_ACTIONS,
        "Clipboard Actions",
        true,
        None,
    ));
    let _ = window_menu.append(&PredefinedMenuItem::separator());
    let _ = window_menu.append(&PredefinedMenuItem::minimize(None));
    let _ = window_menu.append(&PredefinedMenuItem::close_window(None));

    let _ = menu.append(&app_menu);
    let _ = menu.append(&edit_menu);
    let _ = menu.append(&window_menu);

    menu.init_for_nsapp();
    Some(menu)
}

// ---------------------------------------------------------------------------
// LaunchAgent user login startup (works for both signed & development bundles)
// ---------------------------------------------------------------------------

fn launch_agent_plist() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    Some(
        home.join("Library")
            .join("LaunchAgents")
            .join(format!("{MACOS_BUNDLE_ID}.plist")),
    )
}

pub fn set_launch_at_startup(enabled: bool) -> Result<(), String> {
    let path = launch_agent_plist()
        .ok_or_else(|| "Could not resolve ~/Library/LaunchAgents".to_string())?;
    if enabled {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create LaunchAgents directory: {e}"))?;
        }
        let exe = std::env::current_exe()
            .map_err(|e| format!("Failed to resolve executable path: {e}"))?;
        let escaped = exe
            .display()
            .to_string()
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let plist = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
               <key>Label</key>\n\
               <string>{MACOS_BUNDLE_ID}</string>\n\
               <key>ProgramArguments</key>\n\
               <array>\n\
                 <string>{escaped}</string>\n\
                 <string>--minimized</string>\n\
               </array>\n\
               <key>RunAtLoad</key>\n\
               <true/>\n\
             </dict>\n\
             </plist>\n"
        );
        fs::write(&path, plist).map_err(|e| format!("Failed to write LaunchAgent plist: {e}"))
    } else if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("Failed to remove LaunchAgent plist: {e}"))
    } else {
        Ok(())
    }
}

pub fn is_launch_at_startup_enabled() -> bool {
    launch_agent_plist().is_some_and(|p| p.exists())
}

// ---------------------------------------------------------------------------
// Single-instance Unix domain socket activation
// ---------------------------------------------------------------------------

fn socket_path() -> PathBuf {
    std::env::temp_dir().join(format!("{MACOS_BUNDLE_ID}.sock"))
}

fn encode_target(target: ActivationTarget) -> &'static [u8] {
    match target {
        ActivationTarget::MainWindow => b"main\n",
        ActivationTarget::QuickChat => b"quick_chat\n",
        ActivationTarget::Companion => b"companion\n",
        ActivationTarget::Settings => b"settings\n",
        ActivationTarget::ClipboardActions => b"clipboard_actions\n",
        ActivationTarget::AskSelection => b"ask_selection\n",
        ActivationTarget::Screenshot => b"screenshot\n",
    }
}

fn decode_target(line: &str) -> Option<AppEvent> {
    match line.trim() {
        "main" => Some(AppEvent::ShowMainWindow),
        "quick_chat" => Some(AppEvent::ToggleQuickChat),
        "companion" => Some(AppEvent::ToggleCompanion),
        "settings" => Some(AppEvent::OpenSettings(None)),
        "clipboard_actions" => Some(AppEvent::InvokeClipboardActions),
        "ask_selection" => Some(AppEvent::InvokeAskSelection),
        "screenshot" => Some(AppEvent::InvokeScreenshot),
        _ => None,
    }
}

pub struct SingleInstanceHandle {
    path: PathBuf,
    listener: Option<UnixListener>,
}

impl Drop for SingleInstanceHandle {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn acquire_single_instance(target: ActivationTarget) -> Option<SingleInstanceHandle> {
    let path = socket_path();
    if path.exists() {
        if let Ok(mut stream) = UnixStream::connect(&path) {
            let _ = stream.write_all(encode_target(target));
            let _ = stream.flush();
            return None;
        }
        let _ = fs::remove_file(&path);
    }

    match UnixListener::bind(&path) {
        Ok(listener) => Some(SingleInstanceHandle {
            path,
            listener: Some(listener),
        }),
        Err(err) => {
            warn!(
                ?err,
                "Could not bind macOS single-instance socket; continuing"
            );
            Some(SingleInstanceHandle {
                path,
                listener: None,
            })
        }
    }
}

pub fn start_activation_listener(
    handle: &mut SingleInstanceHandle,
    proxy: EventLoopProxy<AppEvent>,
) {
    let Some(listener) = handle.listener.take() else {
        return;
    };
    thread::Builder::new()
        .name("geminidesk-ipc-wake".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut buf = [0u8; 64];
                if let Ok(n) = stream.read(&mut buf)
                    && let Ok(text) = std::str::from_utf8(&buf[..n])
                    && let Some(event) = decode_target(text)
                {
                    let _ = proxy.send_event(event);
                }
            }
        })
        .ok();
}
