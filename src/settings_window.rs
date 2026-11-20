use serde::{Deserialize, Serialize};
use tao::dpi::LogicalSize;
use tao::event_loop::{EventLoopProxy, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder, WindowId};
use tracing::warn;
use wry::{WebContext, WebView, WebViewBuilder};

#[cfg(target_os = "windows")]
use wry::WebViewBuilderExtWindows;

use crate::app::AppEvent;
use crate::config::AppConfig;
use crate::hotkeys::HotkeyStatus;
use crate::icon;
use crate::platform;
use crate::webview::{background_rgba, runtime_version_string};

const SETTINGS_HTML: &str = include_str!("ui/settings.html");

/// Typed IPC messages received strictly from the local, isolated Settings WebView.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SettingsIpcMessage {
    Ready,
    SaveConfig { config: AppConfig },
    ClearCache,
    OpenProfileFolder,
    Close,
}

/// Snapshot pushed from Rust into the local Settings WebView.
#[derive(Debug, Serialize)]
pub struct SettingsViewState {
    pub config: AppConfig,
    pub hotkeys: HotkeyStatus,
    pub effective_dark: bool,
    pub version: &'static str,
    pub webview_version: String,
    pub profile_path: String,
    pub cache_size_bytes: u64,
    pub toast: Option<String>,
}

/// Lazy-created, strictly isolated Settings window.
/// Only exists while open; closing destroys the window and frees all associated resources.
pub struct SettingsWindow {
    window: Window,
    webview: WebView,
}

impl SettingsWindow {
    pub fn open(
        target: &EventLoopWindowTarget<AppEvent>,
        web_context: &mut WebContext,
        is_dark: bool,
        proxy: EventLoopProxy<AppEvent>,
    ) -> Result<Self, String> {
        let mut wb = WindowBuilder::new()
            .with_title("GeminiDesk Settings")
            .with_inner_size(LogicalSize::new(660.0, 720.0))
            .with_min_inner_size(LogicalSize::new(540.0, 540.0))
            .with_visible(false);

        if let Some(win_icon) = icon::window_icon() {
            wb = wb.with_window_icon(Some(win_icon));
        }

        let window = wb
            .build(target)
            .map_err(|e| format!("Failed to create settings window: {e}"))?;

        platform::style_main_window(&window, is_dark);

        let ipc_proxy = proxy.clone();
        let builder = WebViewBuilder::new_with_web_context(web_context)
            .with_background_color(background_rgba(is_dark))
            .with_html(SETTINGS_HTML)
            // Strictly block all external network navigation inside Settings.
            .with_navigation_handler(|uri| uri == "about:blank" || uri.starts_with("data:"))
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
            .with_ipc_handler(move |req| {
                match serde_json::from_str::<SettingsIpcMessage>(req.body()) {
                    Ok(msg) => {
                        let _ = ipc_proxy.send_event(AppEvent::SettingsIpc(msg));
                    }
                    Err(err) => {
                        warn!(?err, "Ignored malformed Settings IPC message");
                    }
                }
            });

        #[cfg(target_os = "windows")]
        let builder = builder
            .with_theme(if is_dark {
                wry::Theme::Dark
            } else {
                wry::Theme::Light
            })
            .with_browser_accelerator_keys(false);

        let webview = builder
            .build(&window)
            .map_err(|e| format!("Failed to build settings webview: {e}"))?;

        window.set_visible(true);
        platform::focus_window(&window);

        Ok(Self { window, webview })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    pub fn focus(&self) {
        self.window.set_visible(true);
        platform::focus_window(&self.window);
    }

    pub fn apply_theme(&self, is_dark: bool) {
        platform::style_main_window(&self.window, is_dark);
        let _ = self.webview.set_background_color(background_rgba(is_dark));
    }

    pub fn push_state(
        &self,
        config: &AppConfig,
        hotkeys: &HotkeyStatus,
        effective_dark: bool,
        profile_path: String,
        cache_size_bytes: u64,
        toast: Option<String>,
    ) {
        let payload = SettingsViewState {
            config: config.clone(),
            hotkeys: hotkeys.clone(),
            effective_dark,
            version: env!("CARGO_PKG_VERSION"),
            webview_version: runtime_version_string(),
            profile_path,
            cache_size_bytes,
            toast,
        };
        if let Ok(json) = serde_json::to_string(&payload) {
            let script = format!("window.__geminidesk_apply_state({json});");
            if let Err(err) = self.webview.evaluate_script(&script) {
                warn!(?err, "Failed to update Settings UI state");
            }
        }
    }
}
