use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Instant;

use tao::event_loop::EventLoopProxy;
use tao::window::Window;
use tracing::{info, warn};
use wry::{
    NewWindowResponse, PageLoadEvent, PermissionResponse, WebContext, WebView, WebViewBuilder,
};

#[cfg(target_os = "windows")]
use tao::platform::windows::WindowExtWindows;
#[cfg(target_os = "windows")]
use wry::{MemoryUsageLevel, Theme as WryTheme, WebViewBuilderExtWindows, WebViewExtWindows};

#[cfg(target_os = "linux")]
use tao::platform::unix::WindowExtUnix;
#[cfg(target_os = "linux")]
use wry::{WebViewBuilderExtUnix, WebViewExtUnix};

#[cfg(target_os = "macos")]
use tao::platform::macos::WindowExtMacOS;
#[cfg(target_os = "macos")]
use wry::WebViewExtMacOS;

use crate::app::AppEvent;
use crate::config::{PerformanceMode, ThemePreference};
use crate::navigation::{
    NavigationDecision, NewWindowDecision, classify_navigation, classify_new_window,
    redact_url_for_log,
};
use crate::productivity::encode_base64;

pub const GEMINI_APP_URL: &str = "https://gemini.google.com/app";

/// Host window kind encoded into an `AtomicU8` for lock-free accelerator key routing.
pub const HOST_MAIN: u8 = 0;
pub const HOST_QUICK_CHAT: u8 = 1;
pub const HOST_COMPANION: u8 = 2;

/// Encapsulates the embedded system WebView so rendering engine details remain cleanly
/// separated from desktop shell orchestration.
pub struct WebViewEngine {
    webview: WebView,
    low_memory: AtomicBool,
    suspended: Arc<AtomicBool>,
    is_navigating: Arc<AtomicBool>,
}

impl WebViewEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        window: &Window,
        web_context: &mut WebContext,
        theme_pref: ThemePreference,
        perf_mode: PerformanceMode,
        is_dark: bool,
        active_host_mode: Arc<AtomicU8>,
        auth_flow_active: Arc<AtomicBool>,
        startup_clock: Instant,
        proxy: EventLoopProxy<AppEvent>,
    ) -> wry::Result<Self> {
        let bg = background_rgba(is_dark);
        let title_proxy = proxy.clone();
        let load_proxy = proxy.clone();
        let first_nav_logged = Arc::new(AtomicBool::new(false));
        let first_started_logged = Arc::new(AtomicBool::new(false));
        let first_finished_logged = Arc::new(AtomicBool::new(false));
        let is_navigating = Arc::new(AtomicBool::new(true));

        let nav_flag = Arc::clone(&first_nav_logged);
        let load_started_flag = Arc::clone(&first_started_logged);
        let load_finished_flag = Arc::clone(&first_finished_logged);
        let load_nav_state = Arc::clone(&is_navigating);

        let builder = WebViewBuilder::new_with_web_context(web_context)
            .with_url(GEMINI_APP_URL)
            .with_background_color(bg)
            .with_clipboard(true)
            .with_hotkeys_zoom(true)
            .with_permission_handler(|kind| {
                info!(?kind, "Permission requested by Gemini web content");
                PermissionResponse::Default
            })
            .with_on_page_load_handler(move |event, uri| {
                let safe_url = redact_url_for_log(&uri);
                match event {
                    PageLoadEvent::Started => {
                        load_nav_state.store(true, Ordering::Relaxed);
                        if !load_started_flag.swap(true, Ordering::Relaxed) {
                            info!(
                                elapsed_ms = startup_clock.elapsed().as_millis(),
                                url = %safe_url,
                                "Startup phase: first page load started"
                            );
                        } else {
                            info!(state = "started", url = %safe_url, "Page load state changed");
                        }
                        let _ = load_proxy.send_event(AppEvent::PageLoadStateChanged {
                            finished: false,
                            uri,
                        });
                    }
                    PageLoadEvent::Finished => {
                        load_nav_state.store(false, Ordering::Relaxed);
                        if !load_finished_flag.swap(true, Ordering::Relaxed) {
                            info!(
                                elapsed_ms = startup_clock.elapsed().as_millis(),
                                url = %safe_url,
                                "Startup phase: first page load finished (interactive)"
                            );
                        } else {
                            info!(state = "finished", url = %safe_url, "Page load state changed");
                        }
                        let _ = load_proxy.send_event(AppEvent::PageLoadStateChanged {
                            finished: true,
                            uri,
                        });
                    }
                }
            })
            .with_navigation_handler(move |uri| {
                let safe_url = redact_url_for_log(&uri);
                match classify_navigation(&uri) {
                    NavigationDecision::AllowInApp => {
                        if !nav_flag.swap(true, Ordering::Relaxed) {
                            info!(
                                elapsed_ms = startup_clock.elapsed().as_millis(),
                                url = %safe_url,
                                "Startup phase: first navigation dispatched"
                            );
                        } else {
                            info!(url = %safe_url, "Allowing in-app navigation");
                        }
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
            })
            .with_download_started_handler(|uri, suggested_path| {
                let safe_url = redact_url_for_log(&uri);
                if let Some(dirs) = directories::UserDirs::new()
                    && let Some(download_dir) = dirs.download_dir()
                {
                    let filename = suggested_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(sanitize_download_filename)
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "gemini-download".to_string());
                    *suggested_path = download_dir.join(filename);
                }
                info!(url = %safe_url, dest = %suggested_path.display(), "Starting Gemini download");
                true
            });

        #[cfg(target_os = "windows")]
        let builder = builder
            .with_theme(to_wry_theme(theme_pref))
            .with_browser_accelerator_keys(true);

        #[cfg(not(target_os = "windows"))]
        let _ = theme_pref;

        #[cfg(target_os = "linux")]
        let webview = {
            let vbox = window
                .default_vbox()
                .ok_or_else(|| wry::Error::UnsupportedWindowHandle)?;
            builder.build_gtk(vbox)?
        };

        #[cfg(not(target_os = "linux"))]
        let webview = builder.build(window)?;

        #[cfg(target_os = "windows")]
        {
            let _ = perf_mode;
            apply_lean_webview2_settings(&webview);
            install_windows_accelerators(&webview, active_host_mode, auth_flow_active, proxy);
        }

        #[cfg(target_os = "linux")]
        {
            apply_webkitgtk_settings(&webview, perf_mode, proxy.clone());
            install_linux_accelerators(&webview, active_host_mode, auth_flow_active, proxy);
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        let _ = (perf_mode, active_host_mode, auth_flow_active, proxy);

        Ok(Self {
            webview,
            low_memory: AtomicBool::new(false),
            suspended: Arc::new(AtomicBool::new(false)),
            is_navigating,
        })
    }

    /// Reparents the single live WebView into `target_window`, preserving active
    /// authentication, streaming responses, and conversation state with zero extra renderers.
    pub fn attach_to_window(&self, target_window: &Window) -> wry::Result<()> {
        #[cfg(target_os = "windows")]
        {
            self.webview.reparent(target_window.hwnd())?;
            self.sync_bounds_to_window(target_window);
        }
        #[cfg(target_os = "linux")]
        {
            use gtk::prelude::WidgetExt;
            if let Some(vbox) = target_window.default_vbox() {
                self.webview.reparent(vbox)?;
                // Reset any previous Quick Chat child size request so resizable Main/Companion
                // windows can shrink freely below Quick Chat's dimensions, and request an
                // immediate frame allocation + paint on the new top-level window.
                let wk = self.webview.webview();
                wk.set_size_request(-1, -1);
                vbox.show_all();
                wk.queue_resize();
                wk.queue_draw();
            }
            self.sync_bounds_to_window(target_window);
        }
        #[cfg(target_os = "macos")]
        {
            let ns_win = target_window.ns_window() as *mut objc2_app_kit::NSWindow;
            if !ns_win.is_null() {
                self.webview.reparent(ns_win)?;
            }
            self.sync_bounds_to_window(target_window);
        }
        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        {
            let _ = target_window;
        }
        Ok(())
    }

    /// Synchronizes both the native WebView host child HWND and the WebView2 controller
    /// bounds to `target_window`'s exact inner client size after reparenting or showing.
    pub fn sync_bounds_to_window(&self, target_window: &Window) {
        self.sync_bounds_to_window_with_inset(target_window, 0);
    }

    /// Synchronizes WebView bounds while reserving `top_inset_px` at the top of the client area
    /// (used on Windows when the native account-navigation header bar is visible).
    pub fn sync_bounds_to_window_with_inset(&self, target_window: &Window, top_inset_px: i32) {
        let size = target_window.inner_size();
        let top = top_inset_px.max(0);
        if size.width > 0 && size.height as i32 > top {
            let avail_h = (size.height as i32 - top).max(1) as u32;
            let bounds = wry::Rect {
                position: tao::dpi::PhysicalPosition::new(0, top).into(),
                size: tao::dpi::PhysicalSize::new(size.width, avail_h).into(),
            };
            if let Err(err) = self.webview.set_bounds(bounds) {
                warn!(?err, "Failed to sync WebView bounds to target window");
            }
            #[cfg(target_os = "windows")]
            {
                use windows::Win32::Foundation::{HWND, RECT};
                use windows::Win32::UI::WindowsAndMessaging::{
                    GetClientRect, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
                };
                let parent_hwnd = HWND(target_window.hwnd() as _);
                let mut rc = RECT::default();
                if unsafe { GetClientRect(parent_hwnd, &mut rc) }.is_ok() {
                    let w = rc.right - rc.left;
                    let h = (rc.bottom - rc.top - top).max(1);
                    if w > 0 && h > 0 {
                        let controller = self.webview.controller();
                        let _ = unsafe {
                            controller.SetBounds(RECT {
                                left: 0,
                                top: 0,
                                right: w,
                                bottom: h,
                            })
                        };
                        let mut host_hwnd = HWND::default();
                        if unsafe { controller.ParentWindow(&mut host_hwnd) }.is_ok()
                            && !host_hwnd.0.is_null()
                        {
                            let _ = unsafe {
                                SetWindowPos(
                                    host_hwnd,
                                    None,
                                    0,
                                    top,
                                    w,
                                    h,
                                    SWP_NOACTIVATE | SWP_NOZORDER,
                                )
                            };
                        }
                    }
                }
            }
        }
    }

    pub fn set_visible(&self, visible: bool) {
        #[cfg(target_os = "linux")]
        {
            use gtk::prelude::WidgetExt;
            let wk = self.webview.webview();
            if wk.is_visible() == visible {
                return;
            }
            if let Err(err) = self.webview.set_visible(visible) {
                warn!(?err, visible, "Failed to update WebView visibility");
            }
            if visible {
                wk.show();
                wk.queue_draw();
            } else {
                wk.hide();
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            if let Err(err) = self.webview.set_visible(visible) {
                warn!(?err, visible, "Failed to update WebView visibility");
            }
        }
    }

    pub fn focus(&self) {
        if let Err(err) = self.webview.focus() {
            warn!(?err, "Failed to focus WebView");
        }
    }

    /// Returns whether the native WebView has a previous page in its back-forward history.
    pub fn can_go_back(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            use webkit2gtk::WebViewExt;
            self.webview.webview().can_go_back()
        }
        #[cfg(target_os = "windows")]
        {
            let wv = self.webview.webview();
            let mut can = windows::core::BOOL::default();
            unsafe { wv.CanGoBack(&mut can) }.is_ok() && can.as_bool()
        }
        #[cfg(target_os = "macos")]
        {
            true
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            false
        }
    }

    /// Steps back one entry in native WebView history (preserving cached Gemini pages without
    /// unnecessary full reloads). Returns `true` if history navigation was dispatched.
    pub fn go_back(&self) -> bool {
        self.resume_if_suspended();
        #[cfg(target_os = "linux")]
        {
            use webkit2gtk::WebViewExt;
            let wk = self.webview.webview();
            if wk.can_go_back() {
                wk.go_back();
                return true;
            }
            false
        }
        #[cfg(target_os = "windows")]
        {
            let wv = self.webview.webview();
            self.can_go_back() && unsafe { wv.GoBack() }.is_ok()
        }
        #[cfg(target_os = "macos")]
        {
            self.webview
                .evaluate_script("window.history.back();")
                .is_ok()
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            false
        }
    }

    pub fn load_new_chat(&self) {
        self.resume_if_suspended();
        if let Err(err) = self.webview.load_url(GEMINI_APP_URL) {
            warn!(?err, "Failed to load Gemini home URL");
        }
    }

    /// Navigates cleanly back to a strictly validated `https://gemini.google.com/...` URL
    /// (preserving the user's previous conversation/account slot when returning from auth).
    pub fn navigate_gemini_url(&self, url: &str) -> Result<(), String> {
        let safe = crate::auth::sanitize_gemini_return_url(url)
            .ok_or_else(|| "Rejected non-Gemini return URL".to_string())?;
        self.resume_if_suspended();
        self.webview
            .load_url(&safe)
            .map_err(|err| format!("Failed to return to Gemini: {err}"))
    }

    /// Navigates the single shared WebView to a strictly validated official Google authentication
    /// URL (`AccountChooser`, `AddSession`, `ServiceLogin`, or `Logout`).
    pub fn navigate_auth_url(&self, url: &str) -> Result<(), String> {
        crate::auth::validate_auth_navigation_url(url)
            .map_err(|code| format!("Rejected unsafe auth URL ({code:?})"))?;
        self.resume_if_suspended();
        self.webview
            .load_url(url)
            .map_err(|err| format!("Failed to load Google auth URL: {err}"))
    }

    /// Non-invasive read-only check verifying that pressing `Escape` during an account flow
    /// will NOT interfere with focused Google login inputs, selects, or modal dialogs.
    pub fn check_escape_safe_to_cancel(&self, proxy: EventLoopProxy<AppEvent>) {
        let script = r#"(function() {
  try {
    const ae = document.activeElement;
    if (ae) {
      const tag = (ae.tagName || '').toUpperCase();
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || ae.isContentEditable) {
        return 'unsafe';
      }
      if (ae.closest && ae.closest('[role="dialog"], [aria-modal="true"], [role="menu"], [role="listbox"]')) {
        return 'unsafe';
      }
    }
    if (document.querySelector('[role="dialog"][aria-modal="true"], [aria-modal="true"]')) {
      return 'unsafe';
    }
    return 'safe';
  } catch (_) {
    return 'unsafe';
  }
})();"#;
        let _ = self
            .webview
            .evaluate_script_with_callback(script, move |raw| {
                let unwrapped = serde_json::from_str::<String>(raw.trim())
                    .unwrap_or_else(|_| raw.trim().to_string());
                if unwrapped == "safe" {
                    let _ = proxy.send_event(AppEvent::CancelAuthFlow);
                }
            });
    }

    /// Event-driven, non-invasive read-only session probe executed strictly on `PageLoadEvent::Finished`.
    /// Never reads cookies, passwords, form inputs, or authentication tokens.
    pub fn probe_auth_state(&self, generation: u64, proxy: EventLoopProxy<AppEvent>) {
        let probe = r#"(function() {
  try {
    const host = (window.location.hostname || '').toLowerCase();
    if (host.startsWith('accounts.google.')) {
      const bodyText = ((document.body && document.body.innerText) || '').toLowerCase();
      if (
        bodyText.includes('this browser or app may not be secure') ||
        bodyText.includes('bu tarayıcı veya uygulama güvenli olmayabilir') ||
        bodyText.includes('disallowed_useragent')
      ) {
        return 'blocked';
      }
      return 'in_progress';
    }
    if (host === 'gemini.google.com') {
      const acctEl = document.querySelector(
        'a[aria-label*="Google Account"], button[aria-label*="Google Account"], a[aria-label*="Google Hesabı"], button[aria-label*="Google Hesabı"], a[href*="SignOutOptions"]'
      );
      if (acctEl) {
        const lbl = (acctEl.getAttribute('aria-label') || '').replace(/\s+/g, ' ').trim();
        return lbl ? ('authenticated:' + lbl) : 'authenticated';
      }
      const signInEl = document.querySelector(
        'a[href*="accounts.google.com/ServiceLogin"], a[href*="accounts.google.com/AccountChooser"], a[href*="accounts.google.com/v3/signin"]'
      );
      if (signInEl) {
        return 'signed_out';
      }
    }
    return 'in_progress';
  } catch (_) {
    return 'in_progress';
  }
})();"#;
        let _ = self
            .webview
            .evaluate_script_with_callback(probe, move |raw| {
                let result = crate::auth::parse_dom_probe_payload(&raw);
                let _ = proxy.send_event(AppEvent::AuthProbeCompleted { generation, result });
            });
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

    pub fn is_low_memory(&self) -> bool {
        self.low_memory.load(Ordering::Relaxed)
    }

    pub fn is_suspended(&self) -> bool {
        self.suspended.load(Ordering::Relaxed)
    }

    /// Returns an official PID -> Role map from Microsoft WebView2's `ICoreWebView2Environment8::GetProcessInfos()`.
    /// Used exclusively on demand while Settings -> Performance is open.
    pub fn process_role_map(&self) -> HashMap<u32, &'static str> {
        #[cfg(target_os = "windows")]
        {
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                COREWEBVIEW2_PROCESS_KIND, COREWEBVIEW2_PROCESS_KIND_BROWSER,
                COREWEBVIEW2_PROCESS_KIND_GPU, COREWEBVIEW2_PROCESS_KIND_RENDERER,
                COREWEBVIEW2_PROCESS_KIND_SANDBOX_HELPER, COREWEBVIEW2_PROCESS_KIND_UTILITY,
                ICoreWebView2Environment8,
            };
            use windows::core::Interface;

            let mut map = HashMap::new();
            let env = self.webview.environment();
            if let Ok(env8) = env.cast::<ICoreWebView2Environment8>()
                && let Ok(coll) = unsafe { env8.GetProcessInfos() }
            {
                let mut count = 0u32;
                if unsafe { coll.Count(&mut count) }.is_ok() {
                    for i in 0..count {
                        if let Ok(info) = unsafe { coll.GetValueAtIndex(i) } {
                            let mut pid = 0i32;
                            let mut kind = COREWEBVIEW2_PROCESS_KIND::default();
                            if unsafe { info.ProcessId(&mut pid) }.is_ok()
                                && unsafe { info.Kind(&mut kind) }.is_ok()
                                && pid > 0
                            {
                                let label = if kind == COREWEBVIEW2_PROCESS_KIND_BROWSER {
                                    "Browser"
                                } else if kind == COREWEBVIEW2_PROCESS_KIND_RENDERER {
                                    "Renderer"
                                } else if kind == COREWEBVIEW2_PROCESS_KIND_GPU {
                                    "GPU Process"
                                } else if kind == COREWEBVIEW2_PROCESS_KIND_UTILITY
                                    || kind == COREWEBVIEW2_PROCESS_KIND_SANDBOX_HELPER
                                {
                                    "Utility / Net"
                                } else {
                                    "WebView2 Aux"
                                };
                                map.insert(pid as u32, label);
                            }
                        }
                    }
                }
            }
            map
        }
        #[cfg(not(target_os = "windows"))]
        {
            HashMap::new()
        }
    }

    /// Applies live WebView engine tuning for the active `PerformanceMode` (never touches
    /// cookies, persistent sessions, or active conversations).
    pub fn apply_performance_mode(&self, mode: PerformanceMode) {
        #[cfg(target_os = "linux")]
        {
            use webkit2gtk::{CacheModel, WebContextExt, WebViewExt};
            if let Some(ctx) = self.webview.webview().context() {
                let model = match mode {
                    PerformanceMode::Efficiency => CacheModel::DocumentViewer,
                    PerformanceMode::Balanced | PerformanceMode::Performance => {
                        CacheModel::WebBrowser
                    }
                };
                ctx.set_cache_model(model);
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = mode;
    }

    /// Switches WebView between Normal and Low memory usage modes.
    /// - Windows: `ICoreWebView2MemoryUsageTargetLevel` (`Low` / `Normal`).
    /// - Linux: trims host glibc arenas without evicting WebKit's decoded `MEMORY_CACHE` on
    ///   transient hides (preventing asset re-decode hitches on Quick Chat / Main restores;
    ///   `MEMORY_CACHE` trimming is reserved for delayed Efficiency idle suspension).
    pub fn set_low_memory(&self, low: bool) {
        let prev = self.low_memory.swap(low, Ordering::Relaxed);
        #[cfg(target_os = "windows")]
        {
            let _ = prev;
            let level = if low {
                MemoryUsageLevel::Low
            } else {
                MemoryUsageLevel::Normal
            };
            if let Err(err) = self.webview.set_memory_usage_level(level) {
                warn!(?err, low, "Failed to set WebView2 memory usage level");
            }
        }
        #[cfg(target_os = "linux")]
        {
            if low && !prev {
                crate::platform::trim_allocator();
            }
        }
        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        let _ = prev;
    }

    /// Checks whether the WebView is currently playing audio (e.g. Gemini Live / TTS readout).
    fn is_playing_audio(&self) -> bool {
        #[cfg(target_os = "windows")]
        {
            use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_8;
            use windows::core::Interface;

            let wv = self.webview.webview();
            if let Ok(wv8) = wv.cast::<ICoreWebView2_8>() {
                let mut playing = windows::core::BOOL::default();
                if unsafe { wv8.IsDocumentPlayingAudio(&mut playing) }.is_ok() {
                    return playing.as_bool();
                }
            }
        }
        #[cfg(target_os = "linux")]
        {
            use webkit2gtk::WebViewExt;
            self.webview.webview().is_playing_audio()
        }
        #[cfg(not(target_os = "linux"))]
        false
    }

    /// Evaluates whether the hidden WebView is genuinely idle (no in-flight navigation,
    /// no active audio/Gemini Live, and no active streaming stop/busy control) before
    /// requesting `TrySuspend`. Always favors preserving active Gemini work over aggressive suspension.
    pub fn check_safe_to_suspend(&self, generation: u64, proxy: EventLoopProxy<AppEvent>) {
        if self.suspended.load(Ordering::Relaxed) {
            return;
        }
        if self.is_navigating.load(Ordering::Relaxed) {
            info!("Deferring Efficiency suspension: navigation in flight");
            let _ = proxy.send_event(AppEvent::SafeSuspendResult {
                generation,
                safe: false,
            });
            return;
        }
        if self.is_playing_audio() {
            info!("Deferring Efficiency suspension: audio playback active");
            let _ = proxy.send_event(AppEvent::SafeSuspendResult {
                generation,
                safe: false,
            });
            return;
        }

        // Non-invasive standard ARIA check for active streaming / busy generation state.
        let probe = r#"(function() {
  try {
    if (document.querySelector('[aria-busy="true"]')) return "busy";
    const btns = document.querySelectorAll('button[aria-label]');
    for (const b of btns) {
      const lbl = (b.getAttribute('aria-label') || '').toLowerCase();
      if (lbl.includes('stop response') || lbl.includes('stop generating') || lbl.includes('yanıtı durdur') || lbl.includes('durdur')) {
        const r = b.getBoundingClientRect();
        if (r.width > 0 && r.height > 0) return "streaming";
      }
    }
    return "idle";
  } catch (_) {
    return "idle";
  }
})();"#;

        let res = self.webview.evaluate_script_with_callback(probe, move |val| {
            let safe = val.contains("idle");
            if !safe {
                info!(state = %val, "Deferring Efficiency suspension: active Gemini work detected");
            }
            let _ = proxy.send_event(AppEvent::SafeSuspendResult { generation, safe });
        });
        if res.is_err() {
            // If script probe couldn't run, prefer safety and skip suspension.
            warn!("Could not probe Gemini activity; skipping Efficiency suspension");
        }
    }

    /// Suspends or trims idle WebView resources after `check_safe_to_suspend` confirms idle state.
    /// - Windows: `ICoreWebView2_3::TrySuspend`.
    /// - Linux: clears WebKitGTK transient `MEMORY_CACHE` + trims host glibc arenas while keeping
    ///   cookies, sessions, and active conversations 100% intact.
    pub fn try_suspend(&self) {
        if !self.suspended.load(Ordering::Relaxed) {
            #[cfg(target_os = "windows")]
            {
                use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_3;
                use webview2_com::TrySuspendCompletedHandler;
                use windows::core::Interface;

                let flag = Arc::clone(&self.suspended);
                let wv = self.webview.webview();
                if let Ok(wv3) = wv.cast::<ICoreWebView2_3>() {
                    let handler =
                        TrySuspendCompletedHandler::create(Box::new(move |hr, is_succ| {
                            if hr.is_ok() && is_succ {
                                flag.store(true, Ordering::Relaxed);
                                info!("WebView2 suspended cleanly in Efficiency mode");
                            }
                            Ok(())
                        }));
                    if let Err(err) = unsafe { wv3.TrySuspend(&handler) } {
                        warn!(?err, "WebView2 TrySuspend call failed");
                    }
                }
            }
            #[cfg(target_os = "linux")]
            {
                trim_webkitgtk_memory_cache(&self.webview);
                crate::platform::trim_allocator();
                self.suspended.store(true, Ordering::Relaxed);
                info!("WebKitGTK idle memory cache trimmed cleanly in Efficiency mode");
            }
        }
    }

    /// Resumes WebView immediately if suspended or idle-trimmed.
    pub fn resume_if_suspended(&self) {
        if self.suspended.swap(false, Ordering::Relaxed) {
            #[cfg(target_os = "windows")]
            {
                use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_3;
                use windows::core::Interface;

                let wv = self.webview.webview();
                if let Ok(wv3) = wv.cast::<ICoreWebView2_3>() {
                    if let Err(err) = unsafe { wv3.Resume() } {
                        warn!(?err, "WebView2 Resume call failed");
                    } else {
                        info!("WebView2 resumed from Efficiency suspension");
                    }
                }
            }
            #[cfg(target_os = "linux")]
            {
                use gtk::prelude::WidgetExt;
                let wk = self.webview.webview();
                if wk.is_visible() {
                    wk.queue_draw();
                }
                info!("WebKitGTK resumed from Efficiency idle state");
            }
        }
    }

    /// User-triggered, non-persistent insertion of a reviewed prompt into Gemini's standard
    /// ARIA textbox (`[role="textbox"][contenteditable="true"]`). Never auto-submits.
    pub fn insert_prompt_into_composer(&self, prompt: &str) {
        let Ok(json_text) = serde_json::to_string(prompt) else {
            return;
        };
        let script = format!(
            r#"(function() {{
  try {{
    const el = document.querySelector('[role="textbox"][contenteditable="true"], textarea[aria-label], [contenteditable="true"]');
    if (!el) return "no_editor";
    el.focus();
    const sel = window.getSelection();
    if (sel && el.isContentEditable) {{
      const range = document.createRange();
      range.selectNodeContents(el);
      range.collapse(false);
      sel.removeAllRanges();
      sel.addRange(range);
    }}
    const ok = document.execCommand('insertText', false, {json_text});
    return ok ? "inserted" : "clipboard_fallback";
  }} catch (e) {{
    return "error";
  }}
}})();"#
        );
        let _ = self.webview.evaluate_script_with_callback(&script, |res| {
            info!(result = %res, "Prompt composer handoff completed");
        });
    }

    /// User-triggered, non-persistent image attachment into Gemini's standard composer using
    /// a standard `ClipboardEvent('paste', { clipboardData })` with `screenshot.png`.
    /// Never auto-submits; user inspects the attached thumbnail before sending.
    pub fn attach_screenshot_to_composer(&self, png_bytes: &[u8], optional_prompt: Option<&str>) {
        let b64 = encode_base64(png_bytes);
        let prompt_json = optional_prompt
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| serde_json::to_string(s).ok())
            .unwrap_or_else(|| "null".to_string());

        let script = format!(
            r#"(function() {{
  try {{
    const el = document.querySelector('[role="textbox"][contenteditable="true"], [contenteditable="true"]');
    if (!el) return "no_editor";
    el.focus();
    const bin = atob("{b64}");
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    const file = new File([bytes], "screenshot.png", {{ type: "image/png" }});
    const dt = new DataTransfer();
    dt.items.add(file);
    const pasteEv = new ClipboardEvent("paste", {{
      bubbles: true,
      cancelable: true,
      clipboardData: dt
    }});
    el.dispatchEvent(pasteEv);
    const prompt = {prompt_json};
    if (prompt) {{
      document.execCommand("insertText", false, prompt);
    }}
    return "dispatched";
  }} catch (e) {{
    return "error:" + String(e);
  }}
}})();"#
        );
        let _ = self.webview.evaluate_script_with_callback(&script, |res| {
            info!(result = %res, "Screenshot composer attachment completed");
        });
    }

    /// Focuses Gemini's accessible prompt textbox when summoning Quick Chat or Companion and
    /// cleanly collapses narrow-viewport expanded navigation drawers using standard ARIA attributes.
    pub fn focus_composer_for_quick_chat(&self) {
        let script = r#"(function() {
  function collapseAndFocus() {
    try {
      if (window.innerWidth < 960) {
        const toggle = document.querySelector('button[aria-expanded="true"][aria-controls], button[aria-expanded="true"][aria-label]');
        if (toggle) {
          const rect = toggle.getBoundingClientRect();
          if (rect.width > 0 && rect.height > 0 && rect.left < 360 && rect.top < 96) {
            toggle.click();
          }
        }
      }
      const el = document.querySelector('[role="textbox"][contenteditable="true"], [contenteditable="true"]');
      if (el) el.focus();
    } catch (_) {}
  }
  collapseAndFocus();
  setTimeout(collapseAndFocus, 120);
})();"#;
        let _ = self.webview.evaluate_script(script);
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

/// Disables unnecessary Edge browser features (built-in autofill, Edge password manager,
/// pinch-zoom, and back-swipe gestures) for tighter privacy and lower host overhead while
/// leaving Google authentication, cookies, media, and Gemini functionality 100% intact.
#[cfg(target_os = "windows")]
fn apply_lean_webview2_settings(webview: &WebView) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Settings4, ICoreWebView2Settings5, ICoreWebView2Settings6,
    };
    use windows::core::Interface;

    let wv = webview.webview();
    if let Ok(settings) = unsafe { wv.Settings() } {
        if let Ok(s4) = settings.cast::<ICoreWebView2Settings4>() {
            let _ = unsafe { s4.SetIsGeneralAutofillEnabled(false) };
            let _ = unsafe { s4.SetIsPasswordAutosaveEnabled(false) };
        }
        if let Ok(s5) = settings.cast::<ICoreWebView2Settings5>() {
            let _ = unsafe { s5.SetIsPinchZoomEnabled(false) };
        }
        if let Ok(s6) = settings.cast::<ICoreWebView2Settings6>() {
            let _ = unsafe { s6.SetIsSwipeNavigationEnabled(false) };
        }
    }
}

/// Hooks native WebView2 `AcceleratorKeyPressed` events (zero persistent JS injection):
/// - `Esc` (in Quick Chat mode) -> hides Quick Chat immediately
/// - `Esc` (in Main Window during account flow) -> passes through to Google AND runs safe-cancel check
/// - `Alt + Left` -> navigates back cleanly (or returns to Gemini from account chooser)
/// - `Ctrl + Shift + E` (in Quick Chat or Companion) -> expands into Full Window
/// - `Ctrl + Shift + T` (in Companion) -> toggles Always on Top immediately
/// - `Ctrl + ,` -> opens GeminiDesk Settings
#[cfg(target_os = "windows")]
fn install_windows_accelerators(
    webview: &WebView,
    active_host_mode: Arc<AtomicU8>,
    auth_flow_active: Arc<AtomicBool>,
    proxy: EventLoopProxy<AppEvent>,
) {
    use webview2_com::AcceleratorKeyPressedEventHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
        COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VK_CONTROL, VK_E, VK_ESCAPE, VK_LEFT, VK_MENU, VK_OEM_COMMA, VK_SHIFT, VK_T,
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
            let alt_down = (GetKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0;
            let host = active_host_mode.load(Ordering::Relaxed);

            if vkey == VK_ESCAPE.0 as u32 && host == HOST_QUICK_CHAT {
                let _ = args.SetHandled(true);
                let _ = proxy.send_event(AppEvent::HideQuickChat);
                return Ok(());
            }

            // Never override Escape inside Google's login inputs or dialogs: let the key
            // pass through normally while requesting a read-only focus/dialog check.
            if vkey == VK_ESCAPE.0 as u32
                && host == HOST_MAIN
                && auth_flow_active.load(Ordering::Relaxed)
            {
                let _ = proxy.send_event(AppEvent::AuthEscapePressed);
                return Ok(());
            }

            if alt_down && !ctrl_down && !shift_down && vkey == VK_LEFT.0 as u32 {
                let _ = args.SetHandled(true);
                let _ = proxy.send_event(AppEvent::AuthNavigateBack);
                return Ok(());
            }

            if ctrl_down && !shift_down && vkey == VK_OEM_COMMA.0 as u32 {
                let _ = args.SetHandled(true);
                let _ = proxy.send_event(AppEvent::OpenSettings(None));
                return Ok(());
            }

            if ctrl_down
                && shift_down
                && vkey == VK_E.0 as u32
                && (host == HOST_QUICK_CHAT || host == HOST_COMPANION)
            {
                let _ = args.SetHandled(true);
                let _ = proxy.send_event(AppEvent::ShowMainWindow);
                return Ok(());
            }

            if ctrl_down && shift_down && vkey == VK_T.0 as u32 && host == HOST_COMPANION {
                let _ = args.SetHandled(true);
                let _ = proxy.send_event(AppEvent::ToggleCompanionAlwaysOnTop);
                return Ok(());
            }
        }
        Ok(())
    }));

    if let Err(err) = unsafe { controller.add_AcceleratorKeyPressed(&handler, &mut token) } {
        warn!(?err, "Failed to install WebView2 accelerator key handler");
    }
}

#[cfg(target_os = "linux")]
fn trim_webkitgtk_memory_cache(webview: &WebView) {
    use webkit2gtk::{WebViewExt, WebsiteDataManagerExtManual, WebsiteDataTypes};
    let wk = webview.webview();
    if let Some(dm) = wk.website_data_manager() {
        // Strictly clears transient in-memory decoded resource cache ONLY — never touches
        // cookies, LocalStorage, IndexedDB, or active conversation state.
        dm.clear(
            WebsiteDataTypes::MEMORY_CACHE,
            gtk::glib::TimeSpan::from_seconds(0),
            gtk::gio::Cancellable::NONE,
            |_| {},
        );
    }
}

#[cfg(target_os = "linux")]
fn apply_webkitgtk_settings(
    webview: &WebView,
    perf_mode: PerformanceMode,
    proxy: EventLoopProxy<AppEvent>,
) {
    use webkit2gtk::{
        CacheModel, CookieAcceptPolicy, CookieManagerExt, HardwareAccelerationPolicy, SettingsExt,
        WebContextExt, WebViewExt, WebsiteDataManagerExt,
    };

    let wk = webview.webview();
    let hw_policy = if std::env::var_os("GEMINIDESK_SOFTWARE_RENDER").is_some() {
        HardwareAccelerationPolicy::Never
    } else {
        HardwareAccelerationPolicy::Always
    };
    if let Some(settings) = wk.settings() {
        // Enable media streams, persistent HTML5 storage, and allowlisted auth popups for genuine
        // Gemini features. Never spoof User-Agent or bypass Google's authentication policies.
        settings.set_enable_media_stream(true);
        settings.set_enable_mediasource(true);
        settings.set_enable_html5_local_storage(true);
        settings.set_enable_html5_database(true);
        settings.set_javascript_can_access_clipboard(true);
        settings.set_javascript_can_open_windows_automatically(true);
        settings.set_enable_smooth_scrolling(true);
        settings.set_enable_webgl(true);
        settings.set_enable_page_cache(true);
        settings.set_enable_developer_extras(cfg!(debug_assertions));
        settings.set_hardware_acceleration_policy(hw_policy);
    }

    if let Some(ctx) = wk.context() {
        let model = match perf_mode {
            PerformanceMode::Efficiency => CacheModel::DocumentViewer,
            PerformanceMode::Balanced | PerformanceMode::Performance => CacheModel::WebBrowser,
        };
        ctx.set_cache_model(model);
    }

    let webkit_ver = wry::webview_version().unwrap_or_else(|_| "unknown".to_string());
    let dmabuf_disabled = std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").ok();
    let dmabuf_force_shm = std::env::var("WEBKIT_DMABUF_RENDERER_FORCE_SHM").ok();
    info!(
        %webkit_ver,
        ?hw_policy,
        ?perf_mode,
        ?dmabuf_disabled,
        ?dmabuf_force_shm,
        "Configured WebKitGTK rendering pipeline"
    );

    // Surface network/TLS/load failures during Google authentication flows so users see
    // a clean Retry + Back to Gemini bar instead of a dead error page.
    wk.connect_load_failed(move |_, _load_event, failing_uri, error| {
        let safe_url = redact_url_for_log(failing_uri);
        warn!(url = %safe_url, err = %error, "WebKitGTK page load failed");
        let _ = proxy.send_event(AppEvent::PageLoadFailed {
            uri: failing_uri.to_string(),
            error: error.message().to_string(),
        });
        false
    });

    // Because top-level navigation is strictly allowlisted to official Google domains in
    // `navigation.rs`, disabling WebKit ITP and allowing Google cross-subdomain SSO cookies
    // (`accounts.google.*`, `ogs.google.com`, `gemini.google.com`) preserves persistent sessions
    // and the Google Account Switcher across restarts.
    if let Some(data_manager) = wk.website_data_manager() {
        data_manager.set_itp_enabled(false);
        if let Some(cookie_manager) = data_manager.cookie_manager() {
            cookie_manager.set_accept_policy(CookieAcceptPolicy::Always);
        }
    }
}

/// Hooks native GTK `key-press-event` on the Linux `WebKitWebView` (zero persistent JS injection):
/// - `Esc` (in Quick Chat mode) -> hides Quick Chat immediately
/// - `Esc` (in Main Window during account flow) -> proceeds to Google AND runs safe-cancel check
/// - `Alt + Left` -> navigates back cleanly (or returns to Gemini from account chooser)
/// - `Ctrl + Shift + E` (in Quick Chat or Companion) -> expands into Full Window
/// - `Ctrl + Shift + T` (in Companion) -> toggles Always on Top immediately
/// - `Ctrl + ,` -> opens GeminiDesk Settings
#[cfg(target_os = "linux")]
fn install_linux_accelerators(
    webview: &WebView,
    active_host_mode: Arc<AtomicU8>,
    auth_flow_active: Arc<AtomicBool>,
    proxy: EventLoopProxy<AppEvent>,
) {
    use gtk::gdk::{ModifierType, keys::constants as keys};
    use gtk::glib::Propagation;
    use gtk::prelude::WidgetExt;

    let wk = webview.webview();
    wk.connect_key_press_event(move |_, event| {
        let keyval = event.keyval();
        let state = event.state();
        let ctrl_down = state.contains(ModifierType::CONTROL_MASK);
        let shift_down = state.contains(ModifierType::SHIFT_MASK);
        let alt_down = state.contains(ModifierType::MOD1_MASK);
        let host = active_host_mode.load(Ordering::Relaxed);

        if keyval == keys::Escape && host == HOST_QUICK_CHAT {
            let _ = proxy.send_event(AppEvent::HideQuickChat);
            return Propagation::Stop;
        }

        // Never override Escape inside Google's login inputs or modal dialogs: allow normal
        // propagation to WebKitGTK while scheduling a read-only focus/dialog check.
        if keyval == keys::Escape && host == HOST_MAIN && auth_flow_active.load(Ordering::Relaxed) {
            let _ = proxy.send_event(AppEvent::AuthEscapePressed);
            return Propagation::Proceed;
        }

        if alt_down && !ctrl_down && !shift_down && keyval == keys::Left {
            let _ = proxy.send_event(AppEvent::AuthNavigateBack);
            return Propagation::Stop;
        }

        if ctrl_down && !shift_down && !alt_down && keyval == keys::comma {
            let _ = proxy.send_event(AppEvent::OpenSettings(None));
            return Propagation::Stop;
        }

        if ctrl_down
            && shift_down
            && !alt_down
            && (keyval == keys::E || keyval == keys::e)
            && (host == HOST_QUICK_CHAT || host == HOST_COMPANION)
        {
            let _ = proxy.send_event(AppEvent::ShowMainWindow);
            return Propagation::Stop;
        }

        if ctrl_down
            && shift_down
            && !alt_down
            && (keyval == keys::T || keyval == keys::t)
            && host == HOST_COMPANION
        {
            let _ = proxy.send_event(AppEvent::ToggleCompanionAlwaysOnTop);
            return Propagation::Stop;
        }

        Propagation::Proceed
    });
}

/// Strips path separators, traversal tokens, and reserved OS device names from downloads.
fn sanitize_download_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            _ => c,
        })
        .collect();
    let trimmed = cleaned.trim_matches('.').trim();
    if trimmed.is_empty() {
        return "gemini-download".to_string();
    }
    let stem = trimmed.split('.').next().unwrap_or(trimmed);
    let upper_stem = stem.to_ascii_uppercase();
    let is_windows_reserved = matches!(
        upper_stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    let safe = if is_windows_reserved {
        format!("_{trimmed}")
    } else {
        trimmed.to_string()
    };
    safe.chars().take(180).collect()
}

pub fn runtime_version_string() -> String {
    wry::webview_version().unwrap_or_else(|_| "Unknown".to_string())
}
