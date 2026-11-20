use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tao::event_loop::EventLoopProxy;
use tao::window::Window;
use tracing::{info, warn};
use wry::{
    NewWindowResponse, PageLoadEvent, PermissionResponse, Theme as WryTheme, WebContext, WebView,
    WebViewBuilder,
};

#[cfg(target_os = "windows")]
use tao::platform::windows::WindowExtWindows;
#[cfg(target_os = "windows")]
use wry::{MemoryUsageLevel, WebViewBuilderExtWindows, WebViewExtWindows};

use crate::app::AppEvent;
use crate::config::ThemePreference;
use crate::navigation::{
    NavigationDecision, NewWindowDecision, classify_navigation, classify_new_window,
    redact_url_for_log,
};

pub const GEMINI_APP_URL: &str = "https://gemini.google.com/app";

/// Encapsulates the embedded system WebView so rendering engine details remain cleanly
/// separated from the rest of the desktop application.
pub struct WebViewEngine {
    webview: WebView,
}

impl WebViewEngine {
    pub fn new(
        window: &Window,
        web_context: &mut WebContext,
        theme_pref: ThemePreference,
        is_dark: bool,
        in_quick_chat: Arc<AtomicBool>,
        proxy: EventLoopProxy<AppEvent>,
    ) -> wry::Result<Self> {
        let bg = background_rgba(is_dark);
        let title_proxy = proxy.clone();

        let builder = WebViewBuilder::new_with_web_context(web_context)
            .with_url(GEMINI_APP_URL)
            .with_background_color(bg)
            .with_clipboard(true)
            .with_hotkeys_zoom(true)
            .with_permission_handler(|kind| {
                info!(?kind, "Permission requested by Gemini web content");
                PermissionResponse::Default
            })
            .with_on_page_load_handler(|event, uri| {
                let safe_url = redact_url_for_log(&uri);
                let state = match event {
                    PageLoadEvent::Started => "started",
                    PageLoadEvent::Finished => "finished",
                };
                info!(state, url = %safe_url, "Page load state changed");
            })
            .with_navigation_handler(|uri| {
                let safe_url = redact_url_for_log(&uri);
                match classify_navigation(&uri) {
                    NavigationDecision::AllowInApp => {
                        info!(url = %safe_url, "Allowing in-app navigation");
                        true
                    }
                    NavigationDecision::OpenExternal => {
                        info!(url = %safe_url, "Opening external URL in system browser");
                        if let Err(err) = open::that(&uri) {
                            warn!(url = %safe_url, ?err, "Failed to open external URL");
                        }
                        false
                    }
                    NavigationDecision::Block => {
                        warn!(url = %safe_url, "Blocked disallowed navigation");
                        false
                    }
                }
            })
            .with_new_window_req_handler(|uri, _features| {
                let safe_url = redact_url_for_log(&uri);
                match classify_new_window(&uri) {
                    NewWindowDecision::AllowInAppPopup => {
                        info!(url = %safe_url, "Allowing in-app popup");
                        NewWindowResponse::Allow
                    }
                    NewWindowDecision::OpenExternal => {
                        info!(url = %safe_url, "Opening external popup URL in system browser");
                        if let Err(err) = open::that(&uri) {
                            warn!(url = %safe_url, ?err, "Failed to open external popup URL");
                        }
                        NewWindowResponse::Deny
                    }
                    NewWindowDecision::Deny => {
                        warn!(url = %safe_url, "Denied disallowed popup");
                        NewWindowResponse::Deny
                    }
                }
            })
            .with_document_title_changed_handler(move |title| {
                let _ = title_proxy.send_event(AppEvent::DocumentTitleChanged(title));
            });

        #[cfg(target_os = "windows")]
        let builder = builder
            .with_theme(to_wry_theme(theme_pref))
            .with_browser_accelerator_keys(true);

        #[cfg(not(target_os = "windows"))]
        let _ = theme_pref;

        let webview = builder.build(window)?;

        #[cfg(target_os = "windows")]
        install_windows_accelerators(&webview, in_quick_chat, proxy);

        #[cfg(not(target_os = "windows"))]
        let _ = (in_quick_chat, proxy);

        Ok(Self { webview })
    }

    /// Reparents the single live WebView into `target_window`, preserving active
    /// authentication, streaming responses, and conversation state with zero extra renderers.
    pub fn attach_to_window(&self, target_window: &Window) -> wry::Result<()> {
        #[cfg(target_os = "windows")]
        {
            self.webview.reparent(target_window.hwnd())?;
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = target_window;
        }
        Ok(())
    }

    pub fn set_visible(&self, visible: bool) {
        if let Err(err) = self.webview.set_visible(visible) {
            warn!(?err, visible, "Failed to update WebView visibility");
        }
    }

    pub fn focus(&self) {
        if let Err(err) = self.webview.focus() {
            warn!(?err, "Failed to focus WebView");
        }
    }

    pub fn load_new_chat(&self) {
        if let Err(err) = self.webview.load_url(GEMINI_APP_URL) {
            warn!(?err, "Failed to load Gemini home URL");
        }
    }

    pub fn apply_theme(&self, pref: ThemePreference, is_dark: bool) {
        let _ = self.webview.set_background_color(background_rgba(is_dark));
        #[cfg(target_os = "windows")]
        {
            if let Err(err) = self.webview.set_theme(to_wry_theme(pref)) {
                warn!(?err, "Failed to update WebView2 theme");
            }
        }
        #[cfg(not(target_os = "windows"))]
        let _ = pref;
    }

    /// Switches WebView2 between Normal and Low memory usage target levels when hidden in tray.
    /// Does not terminate the renderer or drop cookies/sessions.
    pub fn set_low_memory(&self, low: bool) {
        #[cfg(target_os = "windows")]
        {
            let level = if low {
                MemoryUsageLevel::Low
            } else {
                MemoryUsageLevel::Normal
            };
            if let Err(err) = self.webview.set_memory_usage_level(level) {
                warn!(?err, low, "Failed to set WebView2 memory usage level");
            }
        }
        #[cfg(not(target_os = "windows"))]
        let _ = low;
    }
}

pub fn background_rgba(is_dark: bool) -> (u8, u8, u8, u8) {
    if is_dark {
        (19, 19, 20, 255) // #131314 (Gemini dark surface)
    } else {
        (255, 255, 255, 255)
    }
}

#[cfg(target_os = "windows")]
fn to_wry_theme(pref: ThemePreference) -> WryTheme {
    match pref {
        ThemePreference::System => WryTheme::Auto,
        ThemePreference::Dark => WryTheme::Dark,
        ThemePreference::Light => WryTheme::Light,
    }
}

/// Hooks native WebView2 `AcceleratorKeyPressed` events (zero JavaScript injection into Google)
/// to handle desktop shell shortcuts while the WebView owns keyboard focus:
/// - `Esc` (in Quick Chat mode only) -> hides Quick Chat immediately
/// - `Ctrl + Shift + E` (in Quick Chat mode) -> expands seamlessly into the Full Window
/// - `Ctrl + ,` -> opens GeminiDesk Settings
#[cfg(target_os = "windows")]
fn install_windows_accelerators(
    webview: &WebView,
    in_quick_chat: Arc<AtomicBool>,
    proxy: EventLoopProxy<AppEvent>,
) {
    use webview2_com::AcceleratorKeyPressedEventHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
        COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VK_CONTROL, VK_E, VK_ESCAPE, VK_OEM_COMMA, VK_SHIFT,
    };

    let controller = webview.controller();
    let mut token = 0i64;
    let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        unsafe {
            let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
            args.KeyEventKind(&mut kind)?;
            if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
            {
                return Ok(());
            }

            let mut vkey = 0u32;
            args.VirtualKey(&mut vkey)?;

            let ctrl_down = (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
            let shift_down = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;

            if vkey == VK_ESCAPE.0 as u32 && in_quick_chat.load(Ordering::Relaxed) {
                let _ = args.SetHandled(true);
                let _ = proxy.send_event(AppEvent::HideQuickChat);
                return Ok(());
            }

            if ctrl_down && !shift_down && vkey == VK_OEM_COMMA.0 as u32 {
                let _ = args.SetHandled(true);
                let _ = proxy.send_event(AppEvent::OpenSettings);
                return Ok(());
            }

            if ctrl_down
                && shift_down
                && vkey == VK_E.0 as u32
                && in_quick_chat.load(Ordering::Relaxed)
            {
                let _ = args.SetHandled(true);
                let _ = proxy.send_event(AppEvent::ShowMainWindow);
                return Ok(());
            }
        }
        Ok(())
    }));

    if let Err(err) = unsafe { controller.add_AcceleratorKeyPressed(&handler, &mut token) } {
        warn!(?err, "Failed to install WebView2 accelerator key handler");
    }
}

pub fn runtime_version_string() -> String {
    wry::webview_version().unwrap_or_else(|_| "Unknown".to_string())
}
