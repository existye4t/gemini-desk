#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod config;
mod hotkeys;
mod icon;
mod navigation;
mod performance;
mod platform;
mod productivity;
mod settings_window;
mod tray;
mod webview;

use tracing::error;

fn main() {
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
