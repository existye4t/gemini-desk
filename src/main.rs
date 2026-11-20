#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod auth;
mod config;
mod hotkeys;
mod icon;
mod navigation;
mod notifications;
mod performance;
mod platform;
mod productivity;
mod settings_window;
mod tray;
mod ui_assets;
mod updater;
mod webview;

use tracing::error;

/// Selects the safest high-performance WebKitGTK presentation transport on Linux.
///
/// Why this matters:
/// - Plain native DMABuf EGLImage binds its `GdkGLContext` to the first realized top-level
///   `GdkWindow`'s `wl_surface` on Wayland, causing reparented Quick Chat and Floating Companion
///   windows to render solid black.
/// - Setting `WEBKIT_DISABLE_DMABUF_RENDERER=1` disables accelerated compositing completely in
///   WebKitGTK 2.44–2.54, collapsing live Gemini rendering to ~13–20 FPS with high CPU usage.
/// - On WebKitGTK >= 2.44, `WEBKIT_DMABUF_RENDERER_FORCE_SHM=1` keeps full GPU-accelerated
///   compositing, threaded Skia rasterization, and async overflow scrolling while transferring
///   finished frames over SHM buffers so multi-window reparenting works seamlessly at ~90–110+ FPS.
/// - On older WebKitGTK (< 2.44, where `FORCE_SHM` does not exist) or when the user explicitly
///   requests `GEMINIDESK_SOFTWARE_RENDER`, we fall back to `WEBKIT_DISABLE_DMABUF_RENDERER=1`.
#[cfg(any(target_os = "linux", test))]
fn select_linux_webkit_renderer_env(
    webkit_version: Option<&str>,
    has_disable_dmabuf: bool,
    has_force_shm: bool,
    software_render: bool,
) -> Option<(&'static str, &'static str)> {
    if has_disable_dmabuf || has_force_shm {
        return None;
    }
    if software_render {
        return Some(("WEBKIT_DISABLE_DMABUF_RENDERER", "1"));
    }
    if webkit_supports_force_shm(webkit_version) {
        Some(("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1"))
    } else {
        Some(("WEBKIT_DISABLE_DMABUF_RENDERER", "1"))
    }
}

#[cfg(any(target_os = "linux", test))]
fn webkit_supports_force_shm(version: Option<&str>) -> bool {
    let Some(ver) = version else {
        return false;
    };
    let mut parts = ver.trim().split('.');
    let major: u32 = match parts.next().and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => return false,
    };
    let minor: u32 = match parts.next().and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => return false,
    };
    (major, minor) >= (2, 44)
}

fn main() {
    #[cfg(target_os = "linux")]
    {
        let wk_ver = wry::webview_version().ok();
        let env_choice = select_linux_webkit_renderer_env(
            wk_ver.as_deref(),
            std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some(),
            std::env::var_os("WEBKIT_DMABUF_RENDERER_FORCE_SHM").is_some(),
            std::env::var_os("GEMINIDESK_SOFTWARE_RENDER").is_some(),
        );
        if let Some((key, val)) = env_choice {
            // SAFETY: Called on the main thread at process entry before any worker threads,
            // GTK, or WebKitGTK subprocesses exist.
            unsafe {
                std::env::set_var(key, val);
            }
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let options = app::LaunchOptions::from_env();
    if let Err(err) = app::run(options) {
        error!(?err, "GeminiDesk exited with an error");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_renderer_uses_force_shm_on_modern_webkitgtk() {
        assert_eq!(
            select_linux_webkit_renderer_env(Some("2.54.3"), false, false, false),
            Some(("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1"))
        );
        assert_eq!(
            select_linux_webkit_renderer_env(Some("2.44.0"), false, false, false),
            Some(("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1"))
        );
        assert_eq!(
            select_linux_webkit_renderer_env(Some("3.0.0"), false, false, false),
            Some(("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1"))
        );
    }

    #[test]
    fn linux_renderer_falls_back_on_older_webkitgtk_or_software_flag() {
        assert_eq!(
            select_linux_webkit_renderer_env(Some("2.42.5"), false, false, false),
            Some(("WEBKIT_DISABLE_DMABUF_RENDERER", "1"))
        );
        assert_eq!(
            select_linux_webkit_renderer_env(None, false, false, false),
            Some(("WEBKIT_DISABLE_DMABUF_RENDERER", "1"))
        );
        assert_eq!(
            select_linux_webkit_renderer_env(Some("2.54.3"), false, false, true),
            Some(("WEBKIT_DISABLE_DMABUF_RENDERER", "1"))
        );
    }

    #[test]
    fn linux_renderer_respects_explicit_user_overrides() {
        assert_eq!(
            select_linux_webkit_renderer_env(Some("2.54.3"), true, false, false),
            None
        );
        assert_eq!(
            select_linux_webkit_renderer_env(Some("2.54.3"), false, true, false),
            None
        );
    }
}
