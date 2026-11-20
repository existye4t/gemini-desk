#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

use tao::window::Window;

use crate::config::ThemePreference;

/// Physical pixel rectangle describing a display's usable work area (excluding taskbars/docks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkArea {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Computes the ideal Spotlight-style Quick Chat physical position and size on `work_area`.
/// Centered horizontally near the upper portion (~13% from top) and clamped safely inside bounds.
pub fn compute_quick_chat_rect(work_area: WorkArea, scale_factor: f64) -> (i32, i32, u32, u32) {
    let scale = scale_factor.clamp(1.0, 4.0);
    let desired_w = (760.0 * scale).round() as i32;
    let desired_h = (580.0 * scale).round() as i32;

    let max_w = (work_area.width - (32.0 * scale).round() as i32).max(420);
    let max_h = (work_area.height - (48.0 * scale).round() as i32).max(420);

    let width = desired_w.min(max_w);
    let height = desired_h.min(max_h);

    let x = work_area.x + (work_area.width - width) / 2;
    let top_offset = ((work_area.height as f64) * 0.13).round() as i32;
    let max_y = work_area.y + (work_area.height - height).max(0);
    let y = (work_area.y + top_offset).min(max_y).max(work_area.y);

    (x, y, width as u32, height as u32)
}

/// Resolves `ThemePreference` into an effective dark-mode boolean.
pub fn resolve_is_dark(pref: ThemePreference) -> bool {
    match pref {
        ThemePreference::Dark => true,
        ThemePreference::Light => false,
        ThemePreference::System => is_system_dark_mode(),
    }
}

/// Returns whether the OS prefers dark mode for applications.
pub fn is_system_dark_mode() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::is_system_dark_mode()
    }
    #[cfg(target_os = "linux")]
    {
        linux::is_system_dark_mode()
    }
    #[cfg(target_os = "macos")]
    {
        macos::is_system_dark_mode()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        true
    }
}

/// Applies native OS styling to the main application window (dark title bar, caption tint, corners).
pub fn style_main_window(window: &Window, dark: bool) {
    #[cfg(target_os = "windows")]
    windows::style_main_window(window, dark);
    #[cfg(target_os = "linux")]
    linux::style_main_window(window, dark);
    #[cfg(target_os = "macos")]
    macos::style_main_window(window, dark);
}

/// Applies native OS styling to the floating Quick Chat card (rounded corners, border, shadow).
pub fn style_quick_chat_window(window: &Window, dark: bool) {
    #[cfg(target_os = "windows")]
    windows::style_quick_chat_window(window, dark);
    #[cfg(target_os = "linux")]
    linux::style_quick_chat_window(window, dark);
    #[cfg(target_os = "macos")]
    macos::style_quick_chat_window(window, dark);
}

/// Returns the usable work area of the monitor where the user is currently active.
pub fn active_monitor_work_area(fallback_window: &Window) -> WorkArea {
    #[cfg(target_os = "windows")]
    if let Some(area) = windows::active_monitor_work_area() {
        return area;
    }

    if let Some(monitor) = fallback_window
        .current_monitor()
        .or_else(|| fallback_window.primary_monitor())
    {
        let pos = monitor.position();
        let size = monitor.size();
        return WorkArea {
            x: pos.x,
            y: pos.y,
            width: size.width as i32,
            height: size.height as i32,
        };
    }

    WorkArea {
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
    }
}

/// Returns the scale factor of the monitor containing `(x, y)` (best-effort fallback to window DPI).
pub fn monitor_scale_for_work_area(window: &Window, area: WorkArea) -> f64 {
    let center_x = area.x + area.width / 2;
    let center_y = area.y + area.height / 2;
    for monitor in window.available_monitors() {
        let pos = monitor.position();
        let size = monitor.size();
        if center_x >= pos.x
            && center_x < pos.x + size.width as i32
            && center_y >= pos.y
            && center_y < pos.y + size.height as i32
        {
            return monitor.scale_factor();
        }
    }
    window.scale_factor()
}

/// Checks whether a saved window rectangle is still reasonably visible on at least one active monitor.
pub fn is_geometry_on_any_monitor(
    window: &Window,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> bool {
    let rect_right = x.saturating_add(width as i32);
    let rect_bottom = y.saturating_add(height as i32);

    for monitor in window.available_monitors() {
        let pos = monitor.position();
        let size = monitor.size();
        let mon_right = pos.x.saturating_add(size.width as i32);
        let mon_bottom = pos.y.saturating_add(size.height as i32);

        let overlap_w = rect_right.min(mon_right) - x.max(pos.x);
        let overlap_h = rect_bottom.min(mon_bottom) - y.max(pos.y);
        if overlap_w >= 120 && overlap_h >= 120 {
            return true;
        }
    }
    false
}

/// Brings a window reliably to the foreground and gives it OS keyboard focus.
pub fn focus_window(window: &Window) {
    #[cfg(target_os = "windows")]
    windows::focus_window(window);
    #[cfg(not(target_os = "windows"))]
    {
        window.set_focus();
    }
}

/// Returns true if `window` is still active or owns/parents the current foreground window
/// (e.g. WebView2 child surface, owned file picker, permission prompt, or auth popup).
pub fn is_window_still_active_or_owned(window: &Window) -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::is_window_still_active_or_owned(window)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        false
    }
}

/// Enables or disables automatic launch at OS user login.
pub fn set_launch_at_startup(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        windows::set_launch_at_startup(enabled)
    }
    #[cfg(target_os = "linux")]
    {
        linux::set_launch_at_startup(enabled)
    }
    #[cfg(target_os = "macos")]
    {
        macos::set_launch_at_startup(enabled)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let _ = enabled;
        Err("Launch at startup is not supported on this platform".to_string())
    }
}

/// Checks whether launch at startup is currently registered in the OS.
pub fn is_launch_at_startup_enabled() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::is_launch_at_startup_enabled()
    }
    #[cfg(target_os = "linux")]
    {
        linux::is_launch_at_startup_enabled()
    }
    #[cfg(target_os = "macos")]
    {
        macos::is_launch_at_startup_enabled()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        false
    }
}

/// Guard holding a single-instance OS lock for the lifetime of the primary process.
pub struct SingleInstanceGuard {
    #[cfg(target_os = "windows")]
    _inner: windows::SingleInstanceHandle,
}

/// Attempts to claim single-instance ownership.
/// Returns `Some(guard)` if this is the primary instance, or `None` if an existing
/// instance was signaled to wake up and this process should exit cleanly.
pub fn acquire_single_instance(activate_quick_chat: bool) -> Option<SingleInstanceGuard> {
    #[cfg(target_os = "windows")]
    {
        windows::acquire_single_instance(activate_quick_chat)
            .map(|_inner| SingleInstanceGuard { _inner })
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = activate_quick_chat;
        Some(SingleInstanceGuard {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_chat_rect_is_centered_horizontally_and_upper_aligned() {
        let area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let (x, y, w, h) = compute_quick_chat_rect(area, 1.0);
        assert_eq!(w, 760);
        assert_eq!(h, 580);
        assert_eq!(x, (1920 - 760) / 2);
        assert!((100..=200).contains(&y), "unexpected top offset y={y}");
    }

    #[test]
    fn quick_chat_rect_clamps_to_small_display_and_secondary_monitor() {
        let secondary = WorkArea {
            x: -1366,
            y: 0,
            width: 1366,
            height: 728,
        };
        let (x, y, w, h) = compute_quick_chat_rect(secondary, 1.5);
        assert!(w <= 1366);
        assert!(h <= 728);
        assert!(x >= secondary.x);
        assert!(x + w as i32 <= secondary.x + secondary.width);
        assert!(y >= secondary.y);
        assert!(y + h as i32 <= secondary.y + secondary.height);
    }
}
