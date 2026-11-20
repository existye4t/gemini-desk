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
use crate::performance::PerformanceMetrics;
use crate::platform;
use crate::ui_assets::{UI_PROTOCOL, handle_protocol_request, is_local_ui_url, page_url};
use crate::webview::runtime_version_string;

/// Typed IPC messages received strictly from the local, isolated Settings WebView.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SettingsIpcMessage {
    Ready,
    SaveConfig { config: Box<AppConfig> },
    ResetTemplates,
    RequestMetrics,
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
    pub initial_tab: Option<String>,
    pub toast: Option<String>,
}

fn utility_bg_rgba(is_dark: bool) -> (u8, u8, u8, u8) {
    if is_dark {
        (0x0B, 0x0B, 0x0C, 255)
    } else {
        (0xFF, 0xFF, 0xFF, 255)
    }
}

/// Lazy-created, strictly isolated Settings window.
/// Serves locally bundled SIL OFL 1.1 Geist fonts and CSS from memory via `geminidesk://`.
/// Destroyed on close so zero background RAM is consumed when idle.
pub struct SettingsWindow {
    window: Window,
    webview: WebView,
    pending_tab: Option<String>,
}

impl SettingsWindow {
    pub fn open(
        target: &EventLoopWindowTarget<AppEvent>,
        web_context: &mut WebContext,
        is_dark: bool,
        initial_tab: Option<String>,
        proxy: EventLoopProxy<AppEvent>,
    ) -> Result<Self, String> {
        let mut wb = WindowBuilder::new()
            .with_title("Settings — GeminiDesk")
            .with_inner_size(LogicalSize::new(840.0, 600.0))
            .with_min_inner_size(LogicalSize::new(720.0, 500.0))
            .with_visible(false);

        if let Some(win_icon) = icon::window_icon() {
            wb = wb.with_window_icon(Some(win_icon));
        }

        let window = wb
            .build(target)
            .map_err(|e| format!("Failed to create settings window: {e}"))?;

        platform::style_utility_window(&window, is_dark);

        let ipc_proxy = proxy.clone();
        let builder = WebViewBuilder::new_with_web_context(web_context)
            .with_background_color(utility_bg_rgba(is_dark))
            .with_custom_protocol(UI_PROTOCOL.to_string(), |_id, req| {
                handle_protocol_request(req)
            })
            .with_url(page_url("settings.html"))
            .with_navigation_handler(|uri| is_local_ui_url(&uri))
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

        Ok(Self {
            window,
            webview,
            pending_tab: initial_tab,
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    pub fn focus_with_tab(&mut self, tab: Option<String>) {
        if tab.is_some() {
            self.pending_tab = tab;
        }
        self.window.set_visible(true);
        platform::focus_window(&self.window);
    }

    pub fn apply_theme(&self, is_dark: bool) {
        platform::style_utility_window(&self.window, is_dark);
        let _ = self.webview.set_background_color(utility_bg_rgba(is_dark));
    }

    pub fn push_state(
        &mut self,
        config: &AppConfig,
        hotkeys: &HotkeyStatus,
        effective_dark: bool,
        profile_path: String,
        cache_size_bytes: u64,
        toast: Option<String>,
    ) {
        let initial_tab = self.pending_tab.take();
        let payload = SettingsViewState {
            config: config.clone(),
            hotkeys: hotkeys.clone(),
            effective_dark,
            version: env!("CARGO_PKG_VERSION"),
            webview_version: runtime_version_string(),
            profile_path,
            cache_size_bytes,
            initial_tab,
            toast,
        };
        if let Ok(json) = serde_json::to_string(&payload) {
            let script = format!("window.__geminidesk_apply_state({json});");
            if let Err(err) = self.webview.evaluate_script(&script) {
                warn!(?err, "Failed to push state to Settings UI");
            }
        }
    }

    pub fn push_metrics(&self, metrics: &PerformanceMetrics) {
        if let Ok(json) = serde_json::to_string(metrics) {
            let script = format!("window.__geminidesk_apply_metrics({json});");
            if let Err(err) = self.webview.evaluate_script(&script) {
                warn!(?err, "Failed to push metrics to Settings UI");
            }
        }
    }
}
