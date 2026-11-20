//! macOS platform integration (Apple Silicon & Intel, AppKit / WKWebView).
//!
//! Note: Architected for cross-platform compilation and clean separation.
//! Runtime verification on macOS (menu bar / Dock behavior, Accessibility permission
//! prompts for global shortcuts, Login Items via SMAppService) is planned for Phase E.

use tao::window::Window;

pub fn is_system_dark_mode() -> bool {
    true
}

pub fn style_main_window(_window: &Window, _dark: bool) {
    // Native NSWindow appearance handled via AppKit / tao theme.
}

pub fn style_quick_chat_window(_window: &Window, _dark: bool) {
    // Native NSPanel / floating window styling planned for Phase E macOS validation.
}

pub fn set_launch_at_startup(_enabled: bool) -> Result<(), String> {
    Err("Launch at startup on macOS requires a signed app bundle (Phase E)".to_string())
}

pub fn is_launch_at_startup_enabled() -> bool {
    false
}
