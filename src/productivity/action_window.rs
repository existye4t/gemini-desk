use serde::{Deserialize, Serialize};
use tao::dpi::LogicalSize;
use tao::event_loop::{EventLoopProxy, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder, WindowId};
use tracing::warn;
use wry::{WebContext, WebView, WebViewBuilder};

#[cfg(target_os = "windows")]
use wry::WebViewBuilderExtWindows;

use crate::app::AppEvent;
use crate::config::ClipboardActionTemplate;
use crate::icon;
use crate::platform;
use crate::productivity::{CaptureSource, ScreenshotData, encode_base64};

const ACTION_PANEL_HTML: &str = include_str!("../ui/action_panel.html");

/// Typed IPC messages received strictly from the isolated local Action Panel WebView.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionIpcMessage {
    Ready,
    ConfirmText { prompt: String },
    CopyText { prompt: String },
    ConfirmScreenshot { prompt: String },
    CopyScreenshot,
    RetakeScreenshot,
    Cancel,
}

/// Content prepared for the Action Panel window.
#[derive(Debug, Clone)]
pub enum ActionPanelContent {
    Selection {
        captured_text: String,
        source: CaptureSource,
        truncated: bool,
        templates: Vec<ClipboardActionTemplate>,
    },
    Clipboard {
        captured_text: String,
        source: CaptureSource,
        truncated: bool,
        templates: Vec<ClipboardActionTemplate>,
    },
    Screenshot(ScreenshotData),
}

#[derive(Debug, Serialize)]
struct ActionInitPayload {
    mode: &'static str,
    effective_dark: bool,
    badge: String,
    notice: Option<String>,
    notice_warn: bool,
    captured_text: String,
    templates: Vec<ClipboardActionTemplate>,
    image_width: u32,
    image_height: u32,
    image_base64: String,
}

fn utility_bg(is_dark: bool) -> (u8, u8, u8, u8) {
    if is_dark {
        (0x0B, 0x0B, 0x0C, 255)
    } else {
        (0xFF, 0xFF, 0xFF, 255)
    }
}

/// Lazy-created quiet Apple-style sheet for reviewing selections, clipboard actions,
/// or screenshot captures before explicitly handing them to Gemini.
/// Destroyed immediately upon confirm or cancel (zero idle RAM).
pub struct ActionWindow {
    window: Window,
    webview: WebView,
    content: ActionPanelContent,
    effective_dark: bool,
}

impl ActionWindow {
    pub fn open(
        target: &EventLoopWindowTarget<AppEvent>,
        web_context: &mut WebContext,
        is_dark: bool,
        content: ActionPanelContent,
        proxy: EventLoopProxy<AppEvent>,
    ) -> Result<Self, String> {
        let (title, width, height) = match &content {
            ActionPanelContent::Selection { .. } => ("Ask Gemini with Selection", 600.0, 440.0),
            ActionPanelContent::Clipboard { .. } => {
                ("Clipboard Actions — GeminiDesk", 620.0, 460.0)
            }
            ActionPanelContent::Screenshot(_) => ("Screenshot Preview — GeminiDesk", 640.0, 520.0),
        };

        let mut wb = WindowBuilder::new()
            .with_title(title)
            .with_inner_size(LogicalSize::new(width, height))
            .with_min_inner_size(LogicalSize::new(480.0, 360.0))
            .with_always_on_top(true)
            .with_visible(false);

        if let Some(ic) = icon::window_icon() {
            wb = wb.with_window_icon(Some(ic));
        }

        let window = wb
            .build(target)
            .map_err(|e| format!("Failed to build action window: {e}"))?;

        platform::style_utility_window(&window, is_dark);

        let ipc_proxy = proxy.clone();
        let builder = WebViewBuilder::new_with_web_context(web_context)
            .with_background_color(utility_bg(is_dark))
            .with_html(ACTION_PANEL_HTML)
            .with_navigation_handler(|uri| uri == "about:blank" || uri.starts_with("data:"))
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
            .with_ipc_handler(move |req| {
                match serde_json::from_str::<ActionIpcMessage>(req.body()) {
                    Ok(msg) => {
                        let _ = ipc_proxy.send_event(AppEvent::ActionIpc(msg));
                    }
                    Err(err) => {
                        warn!(?err, "Ignored malformed Action Panel IPC message");
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
            .map_err(|e| format!("Failed to build action panel webview: {e}"))?;

        window.set_visible(true);
        platform::focus_window(&window);

        Ok(Self {
            window,
            webview,
            content,
            effective_dark: is_dark,
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    pub fn screenshot_data(&self) -> Option<&ScreenshotData> {
        match &self.content {
            ActionPanelContent::Screenshot(data) => Some(data),
            _ => None,
        }
    }

    pub fn push_initial_content(&self) {
        let payload = match &self.content {
            ActionPanelContent::Selection {
                captured_text,
                source,
                truncated,
                templates,
            } => {
                let (badge, notice, warn) = match source {
                    CaptureSource::UiAutomation => (
                        "UI Automation".into(),
                        if *truncated {
                            Some("Selection was truncated to your configured maximum length.".into())
                        } else {
                            None
                        },
                        false,
                    ),
                    CaptureSource::ClipboardFallback => (
                        "Clipboard Fallback".into(),
                        Some("Captured via non-destructive clipboard fallback; prior clipboard restored.".into()),
                        false,
                    ),
                    CaptureSource::ProtectedField => (
                        "Protected Field".into(),
                        Some("Capture blocked: the focused input is a protected password field.".into()),
                        true,
                    ),
                    _ => (
                        "Manual Paste".into(),
                        Some("No accessible text selection was detected. Paste or type your text below.".into()),
                        false,
                    ),
                };
                ActionInitPayload {
                    mode: "selection",
                    effective_dark: self.effective_dark,
                    badge,
                    notice,
                    notice_warn: warn,
                    captured_text: captured_text.clone(),
                    templates: templates.clone(),
                    image_width: 0,
                    image_height: 0,
                    image_base64: String::new(),
                }
            }
            ActionPanelContent::Clipboard {
                captured_text,
                source,
                truncated,
                templates,
            } => {
                let (badge, notice, warn) = match source {
                    CaptureSource::Clipboard => (
                        format!("{} chars", captured_text.chars().count()),
                        if *truncated {
                            Some("Clipboard text was truncated to your configured maximum length.".into())
                        } else {
                            None
                        },
                        false,
                    ),
                    CaptureSource::ClipboardEmpty => (
                        "Empty".into(),
                        Some("Your clipboard has no plain text right now. Paste or type text below.".into()),
                        false,
                    ),
                    _ => (
                        "Unsupported".into(),
                        Some("Clipboard contains non-text or protected data. Paste plain text below.".into()),
                        true,
                    ),
                };
                ActionInitPayload {
                    mode: "clipboard",
                    effective_dark: self.effective_dark,
                    badge,
                    notice,
                    notice_warn: warn,
                    captured_text: captured_text.clone(),
                    templates: templates.clone(),
                    image_width: 0,
                    image_height: 0,
                    image_base64: String::new(),
                }
            }
            ActionPanelContent::Screenshot(data) => ActionInitPayload {
                mode: "screenshot",
                effective_dark: self.effective_dark,
                badge: format!("{} × {} px", data.width, data.height),
                notice: None,
                notice_warn: false,
                captured_text: String::new(),
                templates: Vec::new(),
                image_width: data.width,
                image_height: data.height,
                image_base64: encode_base64(&data.png_bytes),
            },
        };

        if let Ok(json) = serde_json::to_string(&payload) {
            let script = format!("window.__geminidesk_init_action({json});");
            if let Err(err) = self.webview.evaluate_script(&script) {
                warn!(?err, "Failed to push initial content to Action Panel");
            }
        }
    }
}
