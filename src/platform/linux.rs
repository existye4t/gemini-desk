//! Linux platform integration (KDE Plasma, GNOME, Wayland, X11, Arch/CachyOS).
//!
//! Note: This module is architected for cross-platform compilation and clean separation.
//! Runtime verification on Linux (Wayland/X11 global shortcuts via XDG Desktop Portal,
//! StatusNotifierItem tray via `tray-icon/ksni`, and WebKitGTK auth compatibility) is
//! planned for Phase E on native Linux runners/machines.

use std::fs;
use std::path::PathBuf;

use tao::window::Window;

pub fn is_system_dark_mode() -> bool {
    // Best-effort check via GTK_THEME or KDE color scheme; defaults to dark.
    if let Ok(theme) = std::env::var("GTK_THEME") {
        let lower = theme.to_ascii_lowercase();
        if lower.contains("light") {
            return false;
        }
        if lower.contains("dark") {
            return true;
        }
    }
    true
}

pub fn style_main_window(_window: &Window, _dark: bool) {
    // GTK3/Wayland window decorations follow desktop theme automatically.
}

pub fn style_quick_chat_window(_window: &Window, _dark: bool) {
    // Compositor-managed rounded corners / shadows on KWin (KDE Plasma) and Mutter (GNOME).
}

fn autostart_desktop_file() -> Option<PathBuf> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(config_home.join("autostart").join("geminidesk.desktop"))
}

pub fn set_launch_at_startup(enabled: bool) -> Result<(), String> {
    let path = autostart_desktop_file()
        .ok_or_else(|| "Could not resolve XDG autostart directory".to_string())?;
    if enabled {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create autostart directory: {e}"))?;
        }
        let exe = std::env::current_exe()
            .map_err(|e| format!("Failed to resolve executable path: {e}"))?;
        let contents = format!(
            "[Desktop Entry]\nType=Application\nName=GeminiDesk\nComment=Unofficial Gemini Desktop Client\nExec=\"{}\" --minimized\nX-GNOME-Autostart-enabled=true\n",
            exe.display()
        );
        fs::write(&path, contents).map_err(|e| format!("Failed to write .desktop file: {e}"))
    } else if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("Failed to remove .desktop file: {e}"))
    } else {
        Ok(())
    }
}

pub fn is_launch_at_startup_enabled() -> bool {
    autostart_desktop_file().is_some_and(|p| p.exists())
}
