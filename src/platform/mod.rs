#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

use tao::window::Window;

use crate::config::{QuickChatPosition, ThemePreference};

/// Physical pixel rectangle describing a display's usable work area (excluding taskbars/docks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkArea {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Computes the Spotlight-style Quick Chat physical position and size on `work_area`
/// honoring the user's configured logical dimensions and placement preset.
pub fn compute_quick_chat_rect(
    work_area: WorkArea,
    scale_factor: f64,
    logical_width: u32,
    logical_height: u32,
    position: QuickChatPosition,
) -> (i32, i32, u32, u32) {
    let scale = scale_factor.clamp(1.0, 4.0);
    let desired_w = (logical_width as f64 * scale).round() as i32;
    let desired_h = (logical_height as f64 * scale).round() as i32;
    let margin = (24.0 * scale).round() as i32;

    let max_w = (work_area.width - margin * 2).max(420);
    let max_h = (work_area.height - margin * 2).max(380);

    let width = desired_w.min(max_w);
    let height = desired_h.min(max_h);

    let (raw_x, raw_y) = match position {
        QuickChatPosition::TopCenter => {
            let x = work_area.x + (work_area.width - width) / 2;
            let top_offset = ((work_area.height as f64) * 0.12).round() as i32;
            (x, work_area.y + top_offset)
        }
        QuickChatPosition::Center => (
            work_area.x + (work_area.width - width) / 2,
            work_area.y + (work_area.height - height) / 2,
        ),
        QuickChatPosition::TopRight => (
            work_area.x + work_area.width - width - margin,
            work_area.y + margin,
        ),
        QuickChatPosition::BottomRight => (
            work_area.x + work_area.width - width - margin,
            work_area.y + work_area.height - height - margin,
        ),
    };

    let max_x = work_area.x + (work_area.width - width).max(0);
    let max_y = work_area.y + (work_area.height - height).max(0);
    let x = raw_x.clamp(work_area.x, max_x);
    let y = raw_y.clamp(work_area.y, max_y);

    (x, y, width as u32, height as u32)
}

/// Computes the default docked position/size of the persistent Floating Companion window
/// on the right side of the active monitor.
pub fn default_companion_rect(work_area: WorkArea, scale_factor: f64) -> (i32, i32, u32, u32) {
    let scale = scale_factor.clamp(1.0, 4.0);
    let margin = (20.0 * scale).round() as i32;
    let desired_w = (490.0 * scale).round() as i32;
    let desired_h = (700.0 * scale).round() as i32;

    let width = desired_w.min((work_area.width - margin * 2).max(360));
    let height = desired_h.min((work_area.height - margin * 2).max(420));

    let x = work_area.x + work_area.width - width - margin;
    let y = work_area.y + (work_area.height - height) / 2;
    (x, y, width as u32, height as u32)
}

/// Snaps a window position smoothly to the edges of `work_area` if within `threshold` physical px.
/// Returns `Some((snapped_x, snapped_y))` only when at least one coordinate changed.
pub fn snap_position_to_work_area(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    work_area: WorkArea,
    threshold: i32,
) -> Option<(i32, i32)> {
    let mut nx = x;
    let mut ny = y;

    let right_edge = work_area.x + work_area.width - width;
    let bottom_edge = work_area.y + work_area.height - height;

    if (x - work_area.x).abs() <= threshold {
        nx = work_area.x;
    } else if (x - right_edge).abs() <= threshold {
        nx = right_edge;
    }

    if (y - work_area.y).abs() <= threshold {
        ny = work_area.y;
    } else if (y - bottom_edge).abs() <= threshold {
        ny = bottom_edge;
    }

    if nx != x || ny != y {
        Some((nx, ny))
    } else {
        None
    }
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

/// Applies native OS styling to the Gemini main/companion windows (#131314 dark / #ffffff light).
pub fn style_main_window(window: &Window, dark: bool) {
    #[cfg(target_os = "windows")]
    windows::style_window_with_caption(
        window,
        dark,
        if dark {
            (0x13, 0x13, 0x14)
        } else {
            (0xFF, 0xFF, 0xFF)
        },
    );
    #[cfg(target_os = "linux")]
    linux::style_main_window(window, dark);
    #[cfg(target_os = "macos")]
    macos::style_main_window(window, dark);
}

/// Applies quiet Apple-inspired surface caption styling (#0B0B0C dark / #FFFFFF light)
/// for native utility windows (Settings, Prompt Review, Screenshot Preview).
pub fn style_utility_window(window: &Window, dark: bool) {
    #[cfg(target_os = "windows")]
    windows::style_window_with_caption(
        window,
        dark,
        if dark {
            (0x0B, 0x0B, 0x0C)
        } else {
            (0xFF, 0xFF, 0xFF)
        },
    );
    #[cfg(target_os = "linux")]
    linux::style_main_window(window, dark);
    #[cfg(target_os = "macos")]
    macos::style_main_window(window, dark);
}

/// Applies native OS styling to the borderless floating Quick Chat card.
pub fn style_quick_chat_window(window: &Window, dark: bool) {
    #[cfg(target_os = "windows")]
    windows::style_quick_chat_window(window, dark);
    #[cfg(target_os = "linux")]
    linux::style_quick_chat_window(window, dark);
    #[cfg(target_os = "macos")]
    macos::style_quick_chat_window(window, dark);
}

/// Sets window opacity in percentage (`55..=100`). Uses zero compositing overhead at 100%.
pub fn set_window_opacity(window: &Window, opacity_pct: u8) {
    #[cfg(target_os = "windows")]
    windows::set_window_opacity(window, opacity_pct);
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, opacity_pct);
    }
}

/// Returns the usable work area of the monitor where the user is currently active.
pub fn active_monitor_work_area(fallback_window: &Window) -> WorkArea {
    #[cfg(target_os = "windows")]
    if let Some(area) = windows::active_monitor_work_area() {
        return area;
    }

    work_area_for_window(fallback_window)
}

/// Returns the usable work area of the monitor containing `window`.
pub fn work_area_for_window(window: &Window) -> WorkArea {
    #[cfg(target_os = "windows")]
    if let Some(area) = windows::monitor_work_area_for_window(window) {
        return area;
    }

    if let Some(monitor) = window
        .current_monitor()
        .or_else(|| window.primary_monitor())
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

/// Returns the scale factor of the monitor containing `area`.
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

/// Returns true if `window` is still active or owns/parents the current foreground window.
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

/// Secondary launch activation mode sent to an already-running primary instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationTarget {
    MainWindow,
    QuickChat,
    Companion,
    Settings,
}

/// Guard holding a single-instance OS lock for the lifetime of the primary process.
pub struct SingleInstanceGuard {
    #[cfg(target_os = "windows")]
    _inner: windows::SingleInstanceHandle,
}

pub fn acquire_single_instance(target: ActivationTarget) -> Option<SingleInstanceGuard> {
    #[cfg(target_os = "windows")]
    {
        windows::acquire_single_instance(target).map(|_inner| SingleInstanceGuard { _inner })
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = target;
        Some(SingleInstanceGuard {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_chat_rect_honors_positions_and_clamping() {
        let area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let (x, y, w, h) =
            compute_quick_chat_rect(area, 1.0, 780, 580, QuickChatPosition::TopCenter);
        assert_eq!(w, 780);
        assert_eq!(h, 580);
        assert_eq!(x, (1920 - 780) / 2);
        assert!((100..=160).contains(&y));

        let (rx, ry, _, _) =
            compute_quick_chat_rect(area, 1.0, 780, 580, QuickChatPosition::BottomRight);
        assert_eq!(rx, 1920 - 780 - 24);
        assert_eq!(ry, 1040 - 580 - 24);
    }

    #[test]
    fn quick_chat_rect_clamps_to_small_display_and_secondary_monitor() {
        let secondary = WorkArea {
            x: -1366,
            y: 0,
            width: 1366,
            height: 728,
        };
        let (x, y, w, h) =
            compute_quick_chat_rect(secondary, 1.5, 900, 640, QuickChatPosition::Center);
        assert!(w <= 1366);
        assert!(h <= 728);
        assert!(x >= secondary.x);
        assert!(x + w as i32 <= secondary.x + secondary.width);
        assert!(y >= secondary.y);
        assert!(y + h as i32 <= secondary.y + secondary.height);
    }

    #[test]
    fn companion_snaps_to_nearest_edges_within_threshold() {
        let area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        assert_eq!(
            snap_position_to_work_area(12, 500, 440, 680, area, 20),
            Some((0, 500))
        );
        assert_eq!(
            snap_position_to_work_area(1470, 350, 440, 680, area, 20),
            Some((1480, 360))
        );
        assert_eq!(
            snap_position_to_work_area(600, 200, 440, 680, area, 20),
            None
        );
    }
}
