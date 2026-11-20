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

fn main() {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY: Called on the main thread at process entry before any worker threads,
        // GTK, or WebKitGTK subprocesses exist.
        // WebKitGTK 2.42+ binds its DMABuf GdkGLContext to the first realized top-level
        // GdkWindow's wl_surface on Wayland, which blanks the WebView when reparented
        // into Quick Chat or Floating Companion windows. Disabling the DMABuf renderer
        // uses WebKitGTK's standard Wayland/Cairo backing store so single-WebView
        // reparenting across top-level GTK windows remains seamless.
        unsafe {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
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
