use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use muda::MenuEvent;
use tao::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use tao::event::{ElementState, Event, KeyEvent, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopWindowTarget};
use tao::keyboard::Key;
use tao::window::{Window, WindowBuilder};
use tracing::{error, info, warn};
use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};
use wry::WebContext;

use crate::auth::{
    self, AuthController, AuthDomProbeResult, AuthErrorCode, AuthReturnAction, GEMINI_APP_URL,
    SignInFlow,
};
use crate::config::{
    self, AppConfig, AppPaths, AppState, CloseBehavior, PerformanceMode, ThemePreference,
    WindowGeometry, clear_safe_cache, compute_cache_size_bytes, default_clipboard_templates,
    load_config, load_state, save_config, save_state,
};
use crate::hotkeys::{HotkeyAction, HotkeyController, HotkeyStatus};
use crate::icon;
use crate::notifications;
use crate::performance::PerformanceSampler;
use crate::platform::{self, ActivationTarget, AuthNavHeader};
use crate::productivity::CaptureSource;
use crate::productivity::action_window::{ActionIpcMessage, ActionPanelContent, ActionWindow};
use crate::productivity::clipboard::{self, ClipboardText};
use crate::productivity::screenshot::{self, CaptureOutcome};
use crate::productivity::selection::{self, SelectionOutcome};
use crate::settings_window::{SettingsIpcMessage, SettingsWindow};
use crate::tray::{
    MENU_ACCOUNT, MENU_AUTH_BACK, MENU_AUTH_CANCEL, MENU_CAPTURE_SCREENSHOT,
    MENU_CLIPBOARD_ACTIONS, MENU_COMPANION, MENU_NEW_CHAT, MENU_OPEN_GEMINI, MENU_QUICK_CHAT,
    MENU_QUIT, MENU_SETTINGS, TrayController,
};
use crate::updater::{self, UpdateStatus, UpdaterConfig, VerifiedUpdateOffer};
use crate::webview::{
    HOST_COMPANION, HOST_MAIN, HOST_QUICK_CHAT, WebViewEngine, runtime_version_string,
};

const MAIN_WINDOW_TITLE: &str = "GeminiDesk";
const QUICK_CHAT_TITLE: &str = "GeminiDesk Quick Chat";
const COMPANION_TITLE: &str = "GeminiDesk Companion";
const BLUR_GRACE_PERIOD: Duration = Duration::from_millis(300);
const SNAP_THRESHOLD_PX: i32 = 22;

#[derive(Debug, Default, Clone)]
pub struct LaunchOptions {
    pub start_minimized: bool,
    pub start_quick_chat: bool,
    pub start_companion: bool,
    pub open_settings_tab: Option<String>,
    pub open_settings: bool,
    pub start_sign_in: bool,
    pub open_clipboard_actions: bool,
    pub open_ask_selection: bool,
    pub open_screenshot: bool,
    pub preview_theme: Option<ThemePreference>,
}

impl LaunchOptions {
    pub fn from_env() -> Self {
        let mut opts = Self::default();
        for arg in std::env::args().skip(1) {
            if arg == "--minimized" || arg == "--tray" {
                opts.start_minimized = true;
            } else if arg == "--quick-chat" {
                opts.start_quick_chat = true;
            } else if arg == "--companion" {
                opts.start_companion = true;
            } else if arg == "--settings" {
                opts.open_settings = true;
            } else if arg == "--account" {
                opts.open_settings = true;
                opts.open_settings_tab = Some("account".to_string());
            } else if arg == "--sign-in" {
                opts.start_sign_in = true;
            } else if let Some(tab) = arg.strip_prefix("--settings=") {
                opts.open_settings = true;
                opts.open_settings_tab = Some(tab.to_string());
            } else if arg == "--clipboard-actions" {
                opts.open_clipboard_actions = true;
            } else if arg == "--ask-selection" {
                opts.open_ask_selection = true;
            } else if arg == "--screenshot" {
                opts.open_screenshot = true;
            } else if let Some(th) = arg.strip_prefix("--preview-theme=") {
                opts.preview_theme = match th {
                    "dark" => Some(ThemePreference::Dark),
                    "light" => Some(ThemePreference::Light),
                    "system" => Some(ThemePreference::System),
                    _ => None,
                };
            }
        }
        opts
    }

    fn activation_target(&self) -> ActivationTarget {
        if self.start_quick_chat {
            ActivationTarget::QuickChat
        } else if self.start_companion {
            ActivationTarget::Companion
        } else if self.open_settings {
            ActivationTarget::Settings
        } else if self.open_clipboard_actions {
            ActivationTarget::ClipboardActions
        } else if self.open_ask_selection {
            ActivationTarget::AskSelection
        } else if self.open_screenshot {
            ActivationTarget::Screenshot
        } else {
            ActivationTarget::MainWindow
        }
    }
}

/// Application-wide event bus driven entirely by OS callbacks (zero background polling).
#[derive(Debug)]
pub enum AppEvent {
    ShowMainWindow,
    ToggleQuickChat,
    #[allow(dead_code)]
    HideQuickChat,
    ToggleCompanion,
    #[allow(dead_code)]
    ToggleCompanionAlwaysOnTop,
    NewChat,
    OpenSettings(Option<String>),
    SettingsIpc(SettingsIpcMessage),
    ActionIpc(ActionIpcMessage),
    InvokeAskSelection,
    SelectionCaptured(SelectionOutcome),
    InvokeScreenshot,
    ScreenshotCaptured(CaptureOutcome),
    InvokeClipboardActions,
    EvaluateIdleSuspend(u64),
    SafeSuspendResult {
        generation: u64,
        safe: bool,
    },
    HotkeyPressed(u32),
    #[allow(dead_code)]
    PortalHotkeyActivated(HotkeyAction),
    #[allow(dead_code)]
    PortalShortcutsStatusUpdated {
        status: HotkeyStatus,
        permission_denied: bool,
    },
    UpdateCheckFinished {
        result: Result<Option<VerifiedUpdateOffer>, String>,
        user_initiated: bool,
    },
    UpdateDownloadFinished(Result<PathBuf, String>),
    TrayMenu(String),
    TrayLeftClick,
    PageLoadStateChanged {
        finished: bool,
        uri: String,
    },
    #[allow(dead_code)]
    PageLoadFailed {
        uri: String,
        error: String,
    },
    CancelAuthFlow,
    RetryAuthFlow,
    AuthNavigateBack,
    #[allow(dead_code)]
    AuthEscapePressed,
    VerifyAuthSession(u64),
    AuthProbeCompleted {
        generation: u64,
        result: AuthDomProbeResult,
    },
    DocumentTitleChanged(String),
    Quit,
}

/// Reusable single-slot condvar timer for Efficiency idle suspension checks.
/// Avoids spawning leaked sleeping threads on repeated window hide/show cycles.
struct IdleSuspendScheduler {
    state: Arc<(Mutex<IdleTimerState>, Condvar)>,
    worker_started: AtomicBool,
}

#[derive(Default)]
struct IdleTimerState {
    deadline: Option<(Instant, u64)>,
    shutdown: bool,
}

impl IdleSuspendScheduler {
    fn new() -> Self {
        Self {
            state: Arc::new((Mutex::new(IdleTimerState::default()), Condvar::new())),
            worker_started: AtomicBool::new(false),
        }
    }

    fn schedule(
        &self,
        delay: Duration,
        generation: u64,
        proxy: tao::event_loop::EventLoopProxy<AppEvent>,
    ) {
        if !self.worker_started.swap(true, Ordering::SeqCst) {
            let shared = Arc::clone(&self.state);
            std::thread::Builder::new()
                .name("geminidesk-idle-timer".into())
                .spawn(move || {
                    let (lock, cvar) = &*shared;
                    let mut guard = match lock.lock() {
                        Ok(g) => g,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    loop {
                        if guard.shutdown {
                            break;
                        }
                        let Some((target, generation)) = guard.deadline else {
                            guard = match cvar.wait(guard) {
                                Ok(g) => g,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                            continue;
                        };
                        let now = Instant::now();
                        if now >= target {
                            guard.deadline = None;
                            drop(guard);
                            let _ = proxy.send_event(AppEvent::EvaluateIdleSuspend(generation));
                            guard = match lock.lock() {
                                Ok(g) => g,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                        } else {
                            let wait_dur = target.saturating_duration_since(now);
                            guard = match cvar.wait_timeout(guard, wait_dur) {
                                Ok((g, _)) => g,
                                Err(poisoned) => poisoned.into_inner().0,
                            };
                        }
                    }
                })
                .ok();
        }

        let (lock, cvar) = &*self.state;
        if let Ok(mut guard) = lock.lock() {
            guard.deadline = Some((Instant::now() + delay, generation));
            cvar.notify_one();
        }
    }

    fn cancel(&self) {
        let (lock, cvar) = &*self.state;
        if let Ok(mut guard) = lock.lock()
            && guard.deadline.is_some()
        {
            guard.deadline = None;
            cvar.notify_one();
        }
    }

    fn shutdown(&self) {
        let (lock, cvar) = &*self.state;
        if let Ok(mut guard) = lock.lock() {
            guard.deadline = None;
            guard.shutdown = true;
            cvar.notify_all();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveHost {
    Main,
    QuickChat,
    Companion,
}

struct DesktopApp {
    paths: AppPaths,
    config: AppConfig,
    state: AppState,
    main_window: Window,
    quick_chat_window: Window,
    companion_window: Window,
    web_context: WebContext,
    engine: WebViewEngine,
    active_host: ActiveHost,
    active_host_mode: Arc<AtomicU8>,
    auth_flow_active: Arc<AtomicBool>,
    auth_header: Option<AuthNavHeader>,
    last_quick_chat_shown: Option<Instant>,
    snapping_companion: bool,
    background_gen: Arc<AtomicU64>,
    idle_scheduler: IdleSuspendScheduler,
    hotkeys: HotkeyController,
    tray: Option<TrayController>,
    settings_window: Option<SettingsWindow>,
    action_window: Option<ActionWindow>,
    perf_sampler: PerformanceSampler,
    auth: AuthController,
    updater_cfg: UpdaterConfig,
    update_status: UpdateStatus,
    pending_offer: Option<VerifiedUpdateOffer>,
    verified_download_path: Option<PathBuf>,
    effective_dark: bool,
    last_main_geometry: Option<WindowGeometry>,
    last_companion_geometry: Option<WindowGeometry>,
    startup_clock: Instant,
    first_title_logged: bool,
    proxy: tao::event_loop::EventLoopProxy<AppEvent>,
}

impl DesktopApp {
    fn bump_background_gen(&self) -> u64 {
        self.idle_scheduler.cancel();
        self.background_gen.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn main_top_inset(&self) -> i32 {
        self.auth_header
            .as_ref()
            .map(AuthNavHeader::top_inset_px)
            .unwrap_or(0)
    }

    fn any_gemini_window_active(&self) -> bool {
        (self.main_window.is_visible() && !self.main_window.is_minimized())
            || self.quick_chat_window.is_visible()
            || (self.companion_window.is_visible() && !self.companion_window.is_minimized())
    }

    fn wake_engine(&self) {
        self.bump_background_gen();
        self.engine.resume_if_suspended();
        self.engine.set_low_memory(false);
        self.engine.set_visible(true);
    }

    fn show_main_window(&mut self) {
        let t0 = Instant::now();
        if self.quick_chat_window.is_visible() {
            platform::hide_window(&self.quick_chat_window);
        }
        if self.companion_window.is_visible() {
            self.capture_companion_geometry();
            platform::hide_window(&self.companion_window);
        }

        if self.active_host != ActiveHost::Main {
            if let Err(err) = self.engine.attach_to_window(&self.main_window) {
                error!(?err, "Failed to reparent WebView to main window");
            } else {
                self.active_host = ActiveHost::Main;
                self.active_host_mode.store(HOST_MAIN, Ordering::Relaxed);
            }
        }

        self.wake_engine();
        if self.main_window.is_minimized() {
            self.main_window.set_minimized(false);
        }
        self.main_window.set_visible(true);
        if let Some(header) = &self.auth_header {
            header.layout(&self.main_window);
        }
        self.engine
            .sync_bounds_to_window_with_inset(&self.main_window, self.main_top_inset());
        platform::focus_window(&self.main_window);
        self.engine.focus();

        info!(
            elapsed_ms = t0.elapsed().as_millis(),
            "Main window shown and focused"
        );
    }

    fn show_quick_chat(&mut self) {
        let t0 = Instant::now();
        self.last_quick_chat_shown = Some(t0);

        if self.main_window.is_visible() {
            self.capture_main_geometry();
            platform::hide_window(&self.main_window);
        }
        if self.companion_window.is_visible() {
            self.capture_companion_geometry();
            platform::hide_window(&self.companion_window);
        }

        let work_area = platform::active_monitor_work_area(&self.main_window);
        let scale = platform::monitor_scale_for_work_area(&self.main_window, work_area);
        let (x, y, w, h) = platform::compute_quick_chat_rect(
            work_area,
            scale,
            self.config.quick_chat_width,
            self.config.quick_chat_height,
            self.config.quick_chat_position,
        );

        self.quick_chat_window
            .set_outer_position(PhysicalPosition::new(x, y));
        self.quick_chat_window
            .set_inner_size(PhysicalSize::new(w, h));

        let switched_host = self.active_host != ActiveHost::QuickChat;
        if switched_host {
            if let Err(err) = self.engine.attach_to_window(&self.quick_chat_window) {
                error!(?err, "Failed to reparent WebView to Quick Chat window");
            } else {
                self.active_host = ActiveHost::QuickChat;
                self.active_host_mode
                    .store(HOST_QUICK_CHAT, Ordering::Relaxed);
            }
        }
        #[cfg(target_os = "linux")]
        platform::linux::apply_quick_chat_geometry(
            &self.quick_chat_window,
            self.config.quick_chat_width,
            self.config.quick_chat_height,
        );

        self.wake_engine();
        self.quick_chat_window.set_visible(true);
        self.engine.sync_bounds_to_window(&self.quick_chat_window);
        platform::focus_window(&self.quick_chat_window);
        self.engine.focus();
        self.engine.focus_composer_for_quick_chat();

        info!(
            x,
            y,
            w,
            h,
            elapsed_ms = t0.elapsed().as_millis(),
            "Quick Chat shown and focused"
        );
    }

    fn hide_quick_chat(&mut self) {
        if !self.quick_chat_window.is_visible() {
            return;
        }
        platform::hide_window(&self.quick_chat_window);
        self.enter_background_state();
        info!("Quick Chat hidden");
    }

    fn toggle_quick_chat(&mut self) {
        if self.active_host == ActiveHost::QuickChat && self.quick_chat_window.is_visible() {
            self.hide_quick_chat();
        } else {
            self.show_quick_chat();
        }
    }

    fn show_companion(&mut self) {
        let t0 = Instant::now();
        if self.main_window.is_visible() {
            self.capture_main_geometry();
            platform::hide_window(&self.main_window);
        }
        if self.quick_chat_window.is_visible() {
            platform::hide_window(&self.quick_chat_window);
        }

        // If no persisted companion geometry exists yet, dock neatly on the right side of the active display.
        if self.last_companion_geometry.is_none() {
            let area = platform::active_monitor_work_area(&self.main_window);
            let scale = platform::monitor_scale_for_work_area(&self.main_window, area);
            let (x, y, w, h) = platform::default_companion_rect(area, scale);
            self.companion_window
                .set_inner_size(PhysicalSize::new(w, h));
            self.companion_window
                .set_outer_position(PhysicalPosition::new(x, y));
        }

        if self.active_host != ActiveHost::Companion {
            if let Err(err) = self.engine.attach_to_window(&self.companion_window) {
                error!(?err, "Failed to reparent WebView to Floating Companion");
            } else {
                self.active_host = ActiveHost::Companion;
                self.active_host_mode
                    .store(HOST_COMPANION, Ordering::Relaxed);
            }
        }

        self.companion_window
            .set_always_on_top(self.config.companion_always_on_top);
        platform::set_window_opacity(&self.companion_window, self.config.companion_opacity);
        self.wake_engine();
        if self.companion_window.is_minimized() {
            self.companion_window.set_minimized(false);
        }
        self.companion_window.set_visible(true);
        self.engine.sync_bounds_to_window(&self.companion_window);
        platform::focus_window(&self.companion_window);
        self.engine.focus();
        self.engine.focus_composer_for_quick_chat();

        info!(
            always_on_top = self.config.companion_always_on_top,
            opacity = self.config.companion_opacity,
            elapsed_ms = t0.elapsed().as_millis(),
            "Floating Companion shown and focused"
        );
    }

    fn hide_companion(&mut self) {
        if !self.companion_window.is_visible() {
            return;
        }
        self.capture_companion_geometry();
        self.persist_state();
        platform::hide_window(&self.companion_window);
        self.enter_background_state();
        info!("Floating Companion hidden");
    }

    fn toggle_companion(&mut self) {
        if self.active_host == ActiveHost::Companion && self.companion_window.is_visible() {
            self.hide_companion();
        } else {
            self.show_companion();
        }
    }

    fn toggle_companion_always_on_top(&mut self) {
        self.config.companion_always_on_top = !self.config.companion_always_on_top;
        self.companion_window
            .set_always_on_top(self.config.companion_always_on_top);
        let _ = save_config(&self.paths.config_file(), &self.config);
        info!(
            always_on_top = self.config.companion_always_on_top,
            "Toggled Floating Companion Always on Top"
        );
        self.push_settings_state(Some(if self.config.companion_always_on_top {
            "Companion pinned on top".into()
        } else {
            "Companion unpinned".into()
        }));
    }

    fn hide_main_to_tray(&mut self) {
        self.capture_main_geometry();
        self.persist_state();
        platform::hide_window(&self.main_window);
        self.enter_background_state();
        info!("Main window hidden to system tray");
    }

    fn enter_background_state(&self) {
        if self.any_gemini_window_active() {
            return;
        }

        self.engine.set_visible(false);

        match self.config.performance_mode {
            PerformanceMode::Performance => {
                self.engine.set_low_memory(false);
            }
            PerformanceMode::Balanced => {
                self.engine.set_low_memory(true);
            }
            PerformanceMode::Efficiency => {
                self.engine.set_low_memory(true);
                self.schedule_idle_suspend_check();
            }
        }
    }

    fn schedule_idle_suspend_check(&self) {
        let generation = self.bump_background_gen();
        let delay = Duration::from_secs(self.config.efficiency_suspend_delay_secs as u64);
        self.idle_scheduler
            .schedule(delay, generation, self.proxy.clone());
    }

    fn handle_evaluate_idle_suspend(&self, expected_gen: u64) {
        if self.config.performance_mode != PerformanceMode::Efficiency {
            return;
        }
        if self.any_gemini_window_active() {
            return;
        }
        if self.background_gen.load(Ordering::SeqCst) == expected_gen {
            self.engine
                .check_safe_to_suspend(expected_gen, self.proxy.clone());
        }
    }

    fn handle_safe_suspend_result(&self, expected_gen: u64, safe: bool) {
        if self.config.performance_mode != PerformanceMode::Efficiency
            || self.any_gemini_window_active()
            || self.background_gen.load(Ordering::SeqCst) != expected_gen
        {
            return;
        }
        if safe {
            self.engine.try_suspend();
        } else {
            // Active Gemini work detected (streaming, navigation, or audio): re-check later.
            self.schedule_idle_suspend_check();
        }
    }

    fn open_or_focus_settings(
        &mut self,
        target: &EventLoopWindowTarget<AppEvent>,
        initial_tab: Option<String>,
    ) {
        if let Some(settings) = &mut self.settings_window {
            settings.focus_with_tab(initial_tab);
            self.push_settings_state(None);
            return;
        }

        match SettingsWindow::open(
            target,
            &mut self.web_context,
            self.effective_dark,
            initial_tab,
            self.proxy.clone(),
        ) {
            Ok(win) => {
                self.settings_window = Some(win);
            }
            Err(err) => {
                error!("{err}");
            }
        }
    }

    fn push_settings_state(&mut self, toast: Option<String>) {
        if let Some(settings) = &mut self.settings_window {
            let cache_bytes = compute_cache_size_bytes(&self.paths.webview_data_dir);
            let persistence = auth::inspect_profile_persistence(&self.paths.webview_data_dir);
            settings.push_state(
                &self.config,
                self.hotkeys.status(),
                self.effective_dark,
                self.paths.webview_data_dir.display().to_string(),
                cache_bytes,
                self.auth.state().clone(),
                persistence,
                self.update_status.clone(),
                toast,
            );
        }
    }

    fn push_auth_state(&self) {
        if let Some(settings) = &self.settings_window {
            let persistence = auth::inspect_profile_persistence(&self.paths.webview_data_dir);
            settings.push_auth_state(self.auth.state(), &persistence);
        }
    }

    fn sync_auth_ui(&mut self) {
        let active = self.auth.is_account_flow_active();
        self.auth_flow_active.store(active, Ordering::Relaxed);
        let bar_state = self.auth.nav_bar_state(self.engine.can_go_back());
        if let Some(header) = &mut self.auth_header {
            header.apply_state(&self.main_window, &bar_state);
        }
        if self.active_host == ActiveHost::Main && self.main_window.is_visible() {
            self.engine
                .sync_bounds_to_window_with_inset(&self.main_window, self.main_top_inset());
        }
        self.push_auth_state();
    }

    fn execute_auth_return(&mut self, generation: u64, action: AuthReturnAction) {
        match action {
            AuthReturnAction::HistoryBack { fallback_url } => {
                let went_back = self.engine.go_back();
                if !went_back && let Err(err) = self.engine.navigate_gemini_url(&fallback_url) {
                    warn!(%err, "Fallback return to Gemini failed; loading default workspace");
                    self.engine.load_new_chat();
                } else if went_back {
                    // Schedule an honest post-return DOM check in case BFCache restores the page
                    // without emitting a new PageLoadEvent::Finished callback.
                    let p = self.proxy.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(450));
                        let _ = p.send_event(AppEvent::VerifyAuthSession(generation));
                    });
                }
            }
            AuthReturnAction::NavigateTo(url) => {
                if let Err(err) = self.engine.navigate_gemini_url(&url) {
                    warn!(%err, "Direct return to Gemini failed; loading default workspace");
                    self.engine.load_new_chat();
                }
            }
            AuthReturnAction::AlreadyInWorkspace => {
                self.engine.probe_auth_state(generation, self.proxy.clone());
            }
        }
    }

    fn start_auth_flow(&mut self, flow: SignInFlow) {
        match self.auth.begin_flow(flow) {
            Ok((url, _gen)) => {
                self.show_main_window();
                if let Err(err) = self.engine.navigate_auth_url(url) {
                    warn!(%err, "Google auth navigation failed");
                    self.auth
                        .record_error(AuthErrorCode::NetworkOrLoadError, err);
                }
                self.sync_auth_ui();
            }
            Err(code) => {
                self.auth
                    .record_error(code, format!("Rejected invalid auth flow ({code:?})"));
                self.sync_auth_ui();
            }
        }
    }

    fn retry_auth_flow(&mut self) {
        match self.auth.retry_flow() {
            Ok((url, _gen)) => {
                self.show_main_window();
                if let Err(err) = self.engine.navigate_auth_url(url) {
                    warn!(%err, "Google auth retry navigation failed");
                    self.auth
                        .record_error(AuthErrorCode::NetworkOrLoadError, err);
                }
                self.sync_auth_ui();
            }
            Err(code) => {
                self.auth
                    .record_error(code, format!("Rejected invalid auth retry ({code:?})"));
                self.sync_auth_ui();
            }
        }
    }

    fn cancel_auth_flow(&mut self) {
        let can_back = self.engine.can_go_back();
        let (generation, action) = self.auth.cancel_flow(can_back);
        self.execute_auth_return(generation, action);
        self.sync_auth_ui();
        if self.active_host == ActiveHost::Main && self.main_window.is_visible() {
            self.engine.focus();
        }
    }

    fn navigate_auth_back(&mut self) {
        if self.auth.is_account_flow_active() {
            let can_back = self.engine.can_go_back();
            let (generation, action) = self.auth.navigate_back_action(can_back);
            self.execute_auth_return(generation, action);
            self.sync_auth_ui();
        } else if self.engine.can_go_back() {
            let _ = self.engine.go_back();
        }
    }

    fn set_update_status(&mut self, status: UpdateStatus) {
        self.update_status = status.clone();
        if let Some(settings) = &self.settings_window {
            settings.push_update_status(&status);
        }
    }

    fn start_update_check(&mut self) {
        if !self.updater_cfg.is_signing_configured() {
            self.set_update_status(UpdateStatus::NotConfigured {
                message: "Automatic updates require an official signed release build (Ed25519 verifying key not embedded in this local build).".into(),
            });
            return;
        }
        self.set_update_status(UpdateStatus::Checking);
        let cfg = self.updater_cfg.clone();
        let proxy = self.proxy.clone();
        std::thread::Builder::new()
            .name("geminidesk-update-check".into())
            .spawn(move || {
                let result = updater::check_for_update(&cfg);
                let _ = proxy.send_event(AppEvent::UpdateCheckFinished {
                    result,
                    user_initiated: true,
                });
            })
            .ok();
    }

    fn start_update_download(&mut self) {
        let Some(offer) = self.pending_offer.clone() else {
            return;
        };
        self.set_update_status(UpdateStatus::Downloading {
            version: offer.version.clone(),
            downloaded_bytes: 0,
            total_bytes: offer.artifact.size,
        });
        let cfg = self.updater_cfg.clone();
        let proxy = self.proxy.clone();
        std::thread::Builder::new()
            .name("geminidesk-update-download".into())
            .spawn(move || {
                let res = updater::download_and_verify_artifact(&offer, &cfg);
                let _ = proxy.send_event(AppEvent::UpdateDownloadFinished(res));
            })
            .ok();
    }

    fn dispatch_hotkey_action(&mut self, action: HotkeyAction) {
        match action {
            HotkeyAction::ToggleQuickChat => self.toggle_quick_chat(),
            HotkeyAction::ShowFullWindow => self.show_main_window(),
            HotkeyAction::ToggleCompanion => self.toggle_companion(),
            HotkeyAction::AskSelection => {
                let _ = self.proxy.send_event(AppEvent::InvokeAskSelection);
            }
            HotkeyAction::CaptureScreenshot => {
                let _ = self.proxy.send_event(AppEvent::InvokeScreenshot);
            }
            HotkeyAction::ClipboardActions => {
                let _ = self.proxy.send_event(AppEvent::InvokeClipboardActions);
            }
        }
    }

    fn open_action_window(
        &mut self,
        target: &EventLoopWindowTarget<AppEvent>,
        content: ActionPanelContent,
    ) {
        // Drop any prior sheet so only one utility panel exists at a time.
        self.action_window = None;
        match ActionWindow::open(
            target,
            &mut self.web_context,
            self.effective_dark,
            content,
            self.proxy.clone(),
        ) {
            Ok(win) => {
                self.action_window = Some(win);
            }
            Err(err) => {
                error!("{err}");
            }
        }
    }

    fn focus_active_gemini_for_handoff(&mut self) {
        if self.main_window.is_visible() {
            self.show_main_window();
        } else if self.companion_window.is_visible() {
            self.show_companion();
        } else {
            self.show_quick_chat();
        }
    }

    fn apply_config_update(&mut self, mut new_cfg: AppConfig) {
        new_cfg.sanitize();
        let mut toast = Some("Saved".to_string());

        // 1. OS startup registration
        if new_cfg.launch_at_startup != self.config.launch_at_startup
            && let Err(err) = platform::set_launch_at_startup(new_cfg.launch_at_startup)
        {
            warn!("{err}");
            new_cfg.launch_at_startup = self.config.launch_at_startup;
            toast = Some(err);
        }

        // 2. Global shortcuts (re-register whenever shortcuts or opt-in capture toggles change)
        let shortcuts_changed = new_cfg.quick_chat_shortcut != self.config.quick_chat_shortcut
            || new_cfg.full_window_shortcut != self.config.full_window_shortcut
            || new_cfg.companion_shortcut != self.config.companion_shortcut
            || new_cfg.ask_selection_shortcut != self.config.ask_selection_shortcut
            || new_cfg.screenshot_shortcut != self.config.screenshot_shortcut
            || new_cfg.clipboard_actions_shortcut != self.config.clipboard_actions_shortcut
            || new_cfg.ask_selection_enabled != self.config.ask_selection_enabled
            || new_cfg.screenshot_enabled != self.config.screenshot_enabled
            || new_cfg.clipboard_actions_enabled != self.config.clipboard_actions_enabled;

        if shortcuts_changed {
            self.hotkeys.apply_from_config(&new_cfg);
            if let Some(tray) = &self.tray {
                tray.sync_labels(&new_cfg);
            }
        }

        // 3. Theme updates
        if new_cfg.theme != self.config.theme {
            self.effective_dark = platform::resolve_is_dark(new_cfg.theme);
            self.apply_theme_to_all_windows(new_cfg.theme);
        }

        // 4. Companion live properties
        if new_cfg.companion_always_on_top != self.config.companion_always_on_top {
            self.companion_window
                .set_always_on_top(new_cfg.companion_always_on_top);
        }
        if new_cfg.companion_opacity != self.config.companion_opacity {
            platform::set_window_opacity(&self.companion_window, new_cfg.companion_opacity);
        }
        #[cfg(target_os = "linux")]
        if new_cfg.quick_chat_width != self.config.quick_chat_width
            || new_cfg.quick_chat_height != self.config.quick_chat_height
        {
            platform::linux::apply_quick_chat_geometry(
                &self.quick_chat_window,
                new_cfg.quick_chat_width,
                new_cfg.quick_chat_height,
            );
        }

        // 5. Performance mode live update
        if new_cfg.performance_mode != self.config.performance_mode {
            self.engine.apply_performance_mode(new_cfg.performance_mode);
            match new_cfg.performance_mode {
                PerformanceMode::Performance => {
                    self.bump_background_gen();
                    self.engine.resume_if_suspended();
                    self.engine.set_low_memory(false);
                }
                PerformanceMode::Balanced | PerformanceMode::Efficiency => {
                    if !self.any_gemini_window_active() {
                        self.engine.set_low_memory(true);
                    }
                }
            }
        }

        self.config = new_cfg;
        if let Err(err) = save_config(&self.paths.config_file(), &self.config) {
            warn!(?err, "Failed to persist config.toml");
            toast = Some(format!("Failed to save config: {err}"));
        }

        self.push_settings_state(toast);
    }

    fn apply_theme_to_all_windows(&self, pref: ThemePreference) {
        platform::style_main_window(&self.main_window, self.effective_dark);
        platform::style_main_window(&self.companion_window, self.effective_dark);
        platform::style_quick_chat_window(&self.quick_chat_window, self.effective_dark);
        if let Some(header) = &self.auth_header {
            header.apply_theme(self.effective_dark);
        }
        self.engine.apply_theme(pref, self.effective_dark);
        if let Some(settings) = &self.settings_window {
            settings.apply_theme(self.effective_dark);
        }
    }

    fn capture_main_geometry(&mut self) {
        if self.main_window.is_minimized() {
            return;
        }
        let maximized = self.main_window.is_maximized();
        if !maximized && let Ok(pos) = self.main_window.outer_position() {
            let size = self.main_window.inner_size();
            if size.width >= 320 && size.height >= 320 {
                self.last_main_geometry = Some(WindowGeometry {
                    x: pos.x,
                    y: pos.y,
                    width: size.width,
                    height: size.height,
                    maximized: false,
                });
            }
        }
        if let Some(mut geom) = self.last_main_geometry {
            geom.maximized = maximized;
            self.state.main_window = Some(geom);
        }
    }

    fn capture_companion_geometry(&mut self) {
        if self.companion_window.is_minimized() {
            return;
        }
        if let Ok(pos) = self.companion_window.outer_position() {
            let size = self.companion_window.inner_size();
            if size.width >= 300 && size.height >= 300 {
                let geom = WindowGeometry {
                    x: pos.x,
                    y: pos.y,
                    width: size.width,
                    height: size.height,
                    maximized: false,
                };
                self.last_companion_geometry = Some(geom);
                self.state.companion_window = Some(geom);
            }
        }
    }

    fn snap_companion_if_needed(&mut self) {
        if !self.config.companion_snap_to_edges
            || self.snapping_companion
            || !self.companion_window.is_visible()
            || self.companion_window.is_minimized()
        {
            return;
        }
        let Ok(pos) = self.companion_window.outer_position() else {
            return;
        };
        let outer = self.companion_window.outer_size();
        let area = platform::work_area_for_window(&self.companion_window);
        if let Some((nx, ny)) = platform::snap_position_to_work_area(
            pos.x,
            pos.y,
            outer.width as i32,
            outer.height as i32,
            area,
            SNAP_THRESHOLD_PX,
        ) {
            self.snapping_companion = true;
            self.companion_window
                .set_outer_position(PhysicalPosition::new(nx, ny));
            self.snapping_companion = false;
        }
    }

    fn persist_state(&self) {
        if let Err(err) = save_state(&self.paths.state_file(), &self.state) {
            warn!(?err, "Failed to save state.toml");
        }
    }

    fn shutdown(&mut self) {
        self.bump_background_gen();
        self.idle_scheduler.shutdown();
        self.capture_main_geometry();
        self.capture_companion_geometry();
        self.persist_state();
        self.hotkeys.unregister_all();
        self.action_window = None;
        self.settings_window = None;
        self.tray = None;
        info!("GeminiDesk shutting down cleanly");
    }
}

pub fn run(options: LaunchOptions) -> Result<(), Box<dyn std::error::Error>> {
    let startup_clock = Instant::now();

    let Some(mut _instance_guard) = platform::acquire_single_instance(options.activation_target())
    else {
        info!("Existing GeminiDesk instance signaled; exiting secondary process");
        return Ok(());
    };

    let paths = AppPaths::resolve();
    updater::cleanup_stale_partial_downloads(&paths.updates_dir);
    let mut cfg = load_config(&paths.config_file());
    cfg.launch_at_startup = platform::is_launch_at_startup_enabled();
    let state = load_state(&paths.state_file());
    let theme_for_session = options.preview_theme.unwrap_or(cfg.theme);
    let effective_dark = platform::resolve_is_dark(theme_for_session);

    info!(
        version = env!("CARGO_PKG_VERSION"),
        platform = %platform::platform_runtime_label(),
        data_dir = %paths.webview_data_dir.display(),
        dark = effective_dark,
        perf_mode = ?cfg.performance_mode,
        "Starting GeminiDesk"
    );

    let mut el_builder = EventLoopBuilder::<AppEvent>::with_user_event();
    #[cfg(target_os = "linux")]
    {
        use tao::platform::unix::EventLoopBuilderExtUnix;
        el_builder.with_app_id(platform::linux::LINUX_APP_ID);
    }
    let event_loop = el_builder.build();
    #[cfg(target_os = "linux")]
    platform::linux::init_desktop_identity();
    let proxy = event_loop.create_proxy();

    _instance_guard.start_activation_listener(proxy.clone());

    #[cfg(target_os = "macos")]
    let _macos_menu_bar = platform::macos::install_app_menu();

    // Zero-polling push-based OS callbacks.
    {
        let p = proxy.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let _ = p.send_event(AppEvent::TrayMenu(event.id.0));
        }));
    }
    {
        let p = proxy.clone();
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = p.send_event(AppEvent::TrayLeftClick);
            }
        }));
    }
    {
        let p = proxy.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.state == HotKeyState::Pressed {
                let _ = p.send_event(AppEvent::HotkeyPressed(event.id));
            }
        }));
    }

    let app_icon = icon::window_icon();

    // 1. Main Window
    let mut main_wb = WindowBuilder::new()
        .with_title(MAIN_WINDOW_TITLE)
        .with_inner_size(LogicalSize::new(1220.0, 840.0))
        .with_min_inner_size(LogicalSize::new(420.0, 520.0))
        .with_visible(false);
    if let Some(ic) = app_icon.clone() {
        main_wb = main_wb.with_window_icon(Some(ic));
    }
    let main_window = main_wb.build(&event_loop)?;

    let mut restored_maximized = false;
    let mut last_main_geometry = None;
    if let Some(geom) = state.main_window
        && platform::is_geometry_on_any_monitor(
            &main_window,
            geom.x,
            geom.y,
            geom.width,
            geom.height,
        )
    {
        main_window.set_inner_size(PhysicalSize::new(geom.width, geom.height));
        main_window.set_outer_position(PhysicalPosition::new(geom.x, geom.y));
        restored_maximized = geom.maximized;
        last_main_geometry = Some(geom);
    }

    // 2. Quick Chat Spotlight Window (pre-created hidden HWND -> ~15-24ms instant summon)
    let mut quick_wb = WindowBuilder::new()
        .with_title(QUICK_CHAT_TITLE)
        .with_inner_size(LogicalSize::new(
            cfg.quick_chat_width as f64,
            cfg.quick_chat_height as f64,
        ))
        .with_decorations(false)
        .with_always_on_top(true)
        .with_visible(false);
    #[cfg(not(target_os = "linux"))]
    {
        quick_wb = quick_wb.with_resizable(false);
    }
    if let Some(ic) = app_icon.clone() {
        quick_wb = quick_wb.with_window_icon(Some(ic));
    }
    #[cfg(target_os = "windows")]
    {
        use tao::platform::windows::WindowBuilderExtWindows;
        quick_wb = quick_wb.with_skip_taskbar(true);
    }
    #[cfg(target_os = "linux")]
    {
        use tao::platform::unix::WindowBuilderExtUnix;
        quick_wb = quick_wb.with_skip_taskbar(true);
    }
    let quick_chat_window = quick_wb.build(&event_loop)?;

    // 3. Persistent Floating Companion Window (pre-created hidden HWND -> shares single WebView)
    let mut comp_wb = WindowBuilder::new()
        .with_title(COMPANION_TITLE)
        .with_inner_size(LogicalSize::new(490.0, 700.0))
        .with_min_inner_size(LogicalSize::new(380.0, 440.0))
        .with_always_on_top(cfg.companion_always_on_top)
        .with_visible(false);
    if let Some(ic) = app_icon {
        comp_wb = comp_wb.with_window_icon(Some(ic));
    }
    let companion_window = comp_wb.build(&event_loop)?;

    let mut last_companion_geometry = None;
    if let Some(geom) = state.companion_window
        && platform::is_geometry_on_any_monitor(
            &companion_window,
            geom.x,
            geom.y,
            geom.width,
            geom.height,
        )
    {
        companion_window.set_inner_size(PhysicalSize::new(geom.width, geom.height));
        companion_window.set_outer_position(PhysicalPosition::new(geom.x, geom.y));
        last_companion_geometry = Some(geom);
    }

    // Style all three native windows BEFORE displaying any of them (zero white flash).
    platform::style_main_window(&main_window, effective_dark);
    platform::style_main_window(&companion_window, effective_dark);
    platform::style_quick_chat_window(&quick_chat_window, effective_dark);
    #[cfg(target_os = "linux")]
    platform::linux::apply_quick_chat_geometry(
        &quick_chat_window,
        cfg.quick_chat_width,
        cfg.quick_chat_height,
    );
    platform::set_window_opacity(&companion_window, cfg.companion_opacity);

    #[cfg(target_os = "windows")]
    platform::windows::install_activation_subclass(&main_window, proxy.clone());

    info!(
        elapsed_ms = startup_clock.elapsed().as_millis(),
        "Startup phase: native windows ready"
    );

    // 4. Shared WebContext preserving existing Google sign-in profile
    let mut web_context = WebContext::new(Some(paths.webview_data_dir.clone()));
    let active_host_mode = Arc::new(AtomicU8::new(HOST_MAIN));
    let auth_flow_active = Arc::new(AtomicBool::new(false));
    let auth_header = AuthNavHeader::attach(&main_window, effective_dark, proxy.clone());
    let engine = WebViewEngine::new(
        &main_window,
        &mut web_context,
        theme_for_session,
        cfg.performance_mode,
        effective_dark,
        Arc::clone(&active_host_mode),
        Arc::clone(&auth_flow_active),
        startup_clock,
        proxy.clone(),
    )?;

    info!(
        elapsed_ms = startup_clock.elapsed().as_millis(),
        "Startup phase: WebView initialized"
    );

    // 5. Tray & Global Shortcuts
    let tray = match TrayController::new(&cfg) {
        Ok(t) => Some(t),
        Err(err) => {
            warn!(?err, "System tray unavailable");
            None
        }
    };

    let mut hotkeys = HotkeyController::new(proxy.clone());
    hotkeys.apply_from_config(&cfg);

    let updater_cfg = UpdaterConfig::for_current_build(paths.updates_dir.clone());
    let initial_update_status = UpdateStatus::Idle {
        configured: updater_cfg.is_signing_configured(),
        last_checked_unix: None,
    };

    let mut app = DesktopApp {
        paths,
        config: cfg,
        state,
        main_window,
        quick_chat_window,
        companion_window,
        web_context,
        engine,
        active_host: ActiveHost::Main,
        active_host_mode,
        auth_flow_active,
        auth_header,
        last_quick_chat_shown: None,
        snapping_companion: false,
        background_gen: Arc::new(AtomicU64::new(0)),
        idle_scheduler: IdleSuspendScheduler::new(),
        hotkeys,
        tray,
        settings_window: None,
        action_window: None,
        perf_sampler: PerformanceSampler::new(),
        auth: AuthController::new(),
        updater_cfg,
        update_status: initial_update_status,
        pending_offer: None,
        verified_download_path: None,
        effective_dark,
        last_main_geometry,
        last_companion_geometry,
        startup_clock,
        first_title_logged: false,
        proxy: proxy.clone(),
    };

    // Optional one-shot non-polling startup update check (only when Ed25519 signing is configured).
    if app.config.auto_check_updates && app.updater_cfg.is_signing_configured() {
        let ucfg = app.updater_cfg.clone();
        let uproxy = proxy.clone();
        std::thread::Builder::new()
            .name("geminidesk-startup-update".into())
            .spawn(move || {
                std::thread::sleep(Duration::from_secs(15));
                let result = updater::check_for_update(&ucfg);
                let _ = uproxy.send_event(AppEvent::UpdateCheckFinished {
                    result,
                    user_initiated: false,
                });
            })
            .ok();
    }

    // 6. Initial window presentation
    if options.start_minimized {
        app.enter_background_state();
        info!(
            init_ms = startup_clock.elapsed().as_millis(),
            "Started minimized in system tray"
        );
    } else if options.start_quick_chat {
        app.show_quick_chat();
        info!(
            init_ms = startup_clock.elapsed().as_millis(),
            "Started directly into Quick Chat"
        );
    } else if options.start_companion {
        app.show_companion();
        info!(
            init_ms = startup_clock.elapsed().as_millis(),
            "Started directly into Floating Companion"
        );
    } else {
        if restored_maximized {
            app.main_window.set_maximized(true);
        }
        app.main_window.set_visible(true);
        platform::focus_window(&app.main_window);
        app.engine.focus();
        info!(
            init_ms = startup_clock.elapsed().as_millis(),
            "Main window ready"
        );
    }

    if options.open_settings {
        let _ = proxy.send_event(AppEvent::OpenSettings(options.open_settings_tab));
    }
    if options.start_sign_in {
        app.start_auth_flow(SignInFlow::AccountChooser);
    }
    if options.open_clipboard_actions {
        let _ = proxy.send_event(AppEvent::InvokeClipboardActions);
    }
    if options.open_ask_selection {
        let _ = proxy.send_event(AppEvent::InvokeAskSelection);
    }
    if options.open_screenshot {
        let _ = proxy.send_event(AppEvent::InvokeScreenshot);
    }

    let mut guard = Some(_instance_guard);

    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(app_event) => match app_event {
                AppEvent::ShowMainWindow => app.show_main_window(),
                AppEvent::ToggleQuickChat => app.toggle_quick_chat(),
                AppEvent::HideQuickChat => app.hide_quick_chat(),
                AppEvent::ToggleCompanion => app.toggle_companion(),
                AppEvent::ToggleCompanionAlwaysOnTop => app.toggle_companion_always_on_top(),
                AppEvent::NewChat => {
                    if !app.any_gemini_window_active() {
                        app.show_main_window();
                    }
                    app.engine.load_new_chat();
                    app.engine.focus();
                }
                AppEvent::OpenSettings(tab) => app.open_or_focus_settings(target, tab),
                AppEvent::SettingsIpc(msg) => match msg {
                    SettingsIpcMessage::Ready => app.push_settings_state(None),
                    SettingsIpcMessage::SaveConfig { config: new_cfg } => {
                        app.apply_config_update(*new_cfg);
                    }
                    SettingsIpcMessage::ResetTemplates => {
                        let mut updated = app.config.clone();
                        updated.clipboard_templates = default_clipboard_templates();
                        app.apply_config_update(updated);
                    }
                    SettingsIpcMessage::RequestMetrics => {
                        if let Some(settings) = &app.settings_window {
                            let role_map = app.engine.process_role_map();
                            let metrics = app.perf_sampler.sample(
                                app.engine.is_low_memory(),
                                app.engine.is_suspended(),
                                runtime_version_string(),
                                &role_map,
                            );
                            settings.push_metrics(&metrics);
                        }
                    }
                    SettingsIpcMessage::ClearCache => {
                        let freed = clear_safe_cache(&app.paths.webview_data_dir);
                        let mb = freed as f64 / (1024.0 * 1024.0);
                        app.push_settings_state(Some(format!(
                            "Cleared {mb:.1} MB cache (Google sign-in preserved)"
                        )));
                    }
                    SettingsIpcMessage::OpenProfileFolder => {
                        if let Err(err) = open::that(&app.paths.webview_data_dir) {
                            warn!(?err, "Failed to open profile directory");
                        }
                    }
                    SettingsIpcMessage::StartSignIn => {
                        app.start_auth_flow(SignInFlow::AccountChooser);
                    }
                    SettingsIpcMessage::SwitchAccount => {
                        app.start_auth_flow(SignInFlow::AccountChooser);
                    }
                    SettingsIpcMessage::AddAccount => {
                        app.start_auth_flow(SignInFlow::AddAccount);
                    }
                    SettingsIpcMessage::SignOut => {
                        app.start_auth_flow(SignInFlow::SignOut);
                    }
                    SettingsIpcMessage::CancelSignIn => {
                        app.cancel_auth_flow();
                    }
                    SettingsIpcMessage::OpenGeminiInDefaultBrowser => {
                        if let Err(err) = open::that_detached(GEMINI_APP_URL) {
                            warn!(?err, "Failed to open Gemini in default system browser");
                            app.push_settings_state(Some(format!(
                                "Could not open default browser: {err}"
                            )));
                        } else {
                            app.push_settings_state(Some(
                                "Opened Gemini in your default system browser".into(),
                            ));
                        }
                    }
                    SettingsIpcMessage::CheckForUpdates => {
                        app.start_update_check();
                    }
                    SettingsIpcMessage::DownloadUpdate => {
                        app.start_update_download();
                    }
                    SettingsIpcMessage::InstallUpdate => {
                        if let (Some(offer), Some(path)) = (
                            app.pending_offer.clone(),
                            app.verified_download_path.clone(),
                        ) {
                            match updater::launch_verified_installer(
                                &path,
                                &offer.artifact.sha256,
                                offer.artifact.size,
                                offer.artifact.kind,
                            ) {
                                Ok(should_exit) => {
                                    if should_exit {
                                        let _ = app.proxy.send_event(AppEvent::Quit);
                                    }
                                }
                                Err(err) => {
                                    warn!(%err, "Verified update install handoff failed");
                                    app.set_update_status(UpdateStatus::Error { message: err });
                                }
                            }
                        }
                    }
                    SettingsIpcMessage::Close => {
                        app.settings_window = None;
                        app.perf_sampler.reset();
                        platform::trim_allocator();
                    }
                },
                AppEvent::InvokeAskSelection => {
                    let allow_fallback = app.config.ask_selection_clipboard_fallback;
                    let max_chars = app.config.clipboard_max_chars;
                    #[cfg(target_os = "linux")]
                    {
                        let outcome =
                            selection::capture_foreground_selection(allow_fallback, max_chars);
                        let _ = app.proxy.send_event(AppEvent::SelectionCaptured(outcome));
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        let p = app.proxy.clone();
                        std::thread::spawn(move || {
                            let outcome =
                                selection::capture_foreground_selection(allow_fallback, max_chars);
                            let _ = p.send_event(AppEvent::SelectionCaptured(outcome));
                        });
                    }
                }
                AppEvent::SelectionCaptured(outcome) => {
                    let (captured_text, source, truncated) = match outcome {
                        SelectionOutcome::Captured {
                            text,
                            source,
                            truncated,
                        } => (text, source, truncated),
                        SelectionOutcome::ProtectedField => {
                            (String::new(), CaptureSource::ProtectedField, false)
                        }
                        SelectionOutcome::Unavailable => {
                            (String::new(), CaptureSource::ManualFallback, false)
                        }
                    };
                    app.open_action_window(
                        target,
                        ActionPanelContent::Selection {
                            captured_text,
                            source,
                            truncated,
                            templates: app.config.clipboard_templates.clone(),
                        },
                    );
                }
                AppEvent::InvokeScreenshot => {
                    // Hide any active action sheet before capturing so it never blocks the screen.
                    app.action_window = None;
                    let p = app.proxy.clone();
                    std::thread::spawn(move || {
                        let outcome = screenshot::run_interactive_region_capture();
                        let _ = p.send_event(AppEvent::ScreenshotCaptured(outcome));
                    });
                }
                AppEvent::ScreenshotCaptured(outcome) => match outcome {
                    CaptureOutcome::Captured(data) => {
                        info!(
                            w = data.width,
                            h = data.height,
                            "Screenshot region captured"
                        );
                        app.open_action_window(target, ActionPanelContent::Screenshot(data));
                    }
                    CaptureOutcome::Cancelled => {
                        info!("Screenshot capture cancelled by user");
                    }
                    CaptureOutcome::Error(err) => {
                        warn!(%err, "Screenshot capture failed");
                        notifications::notify_if_enabled(
                            app.config.notifications_enabled,
                            "Screenshot capture unavailable",
                            &err,
                        );
                    }
                },
                AppEvent::InvokeClipboardActions => {
                    let (captured_text, source, truncated) =
                        match clipboard::read_unicode_text(app.config.clipboard_max_chars) {
                            ClipboardText::Text { text, truncated } => {
                                (text, CaptureSource::Clipboard, truncated)
                            }
                            ClipboardText::Empty => {
                                (String::new(), CaptureSource::ClipboardEmpty, false)
                            }
                            ClipboardText::UnsupportedFormat | ClipboardText::Unavailable(_) => {
                                (String::new(), CaptureSource::ManualFallback, false)
                            }
                        };
                    app.open_action_window(
                        target,
                        ActionPanelContent::Clipboard {
                            captured_text,
                            source,
                            truncated,
                            templates: app.config.clipboard_templates.clone(),
                        },
                    );
                }
                AppEvent::ActionIpc(msg) => match msg {
                    ActionIpcMessage::Ready => {
                        if let Some(win) = &app.action_window {
                            win.push_initial_content();
                        }
                    }
                    ActionIpcMessage::ConfirmText { prompt } => {
                        // Also place prompt on clipboard so Ctrl+V works as an instant fallback.
                        let _ = clipboard::write_unicode_text(&prompt);
                        app.action_window = None;
                        app.focus_active_gemini_for_handoff();
                        app.engine.insert_prompt_into_composer(&prompt);
                    }
                    ActionIpcMessage::CopyText { prompt } => {
                        if let Err(err) = clipboard::write_unicode_text(&prompt) {
                            warn!(%err, "Failed to copy prompt text");
                        }
                        app.action_window = None;
                    }
                    ActionIpcMessage::ConfirmScreenshot { prompt } => {
                        let shot = app
                            .action_window
                            .as_ref()
                            .and_then(|w| w.screenshot_data().cloned());
                        app.action_window = None;
                        if let Some(data) = shot {
                            if app.config.screenshot_copy_to_clipboard {
                                let _ = clipboard::write_image(
                                    &data.bgra_pixels,
                                    data.width,
                                    data.height,
                                    &data.png_bytes,
                                );
                            }
                            app.focus_active_gemini_for_handoff();
                            app.engine
                                .attach_screenshot_to_composer(&data.png_bytes, Some(&prompt));
                        }
                    }
                    ActionIpcMessage::CopyScreenshot => {
                        if let Some(data) = app
                            .action_window
                            .as_ref()
                            .and_then(|w| w.screenshot_data().cloned())
                            && let Err(err) = clipboard::write_image(
                                &data.bgra_pixels,
                                data.width,
                                data.height,
                                &data.png_bytes,
                            )
                        {
                            warn!(%err, "Failed to copy screenshot to clipboard");
                        }
                        app.action_window = None;
                    }
                    ActionIpcMessage::RetakeScreenshot => {
                        app.action_window = None;
                        let _ = app.proxy.send_event(AppEvent::InvokeScreenshot);
                    }
                    ActionIpcMessage::Cancel => {
                        app.action_window = None;
                        platform::trim_allocator();
                    }
                },
                AppEvent::EvaluateIdleSuspend(generation) => {
                    app.handle_evaluate_idle_suspend(generation);
                }
                AppEvent::SafeSuspendResult { generation, safe } => {
                    app.handle_safe_suspend_result(generation, safe);
                }
                AppEvent::HotkeyPressed(id) => {
                    if let Some(action) = app.hotkeys.action_for_id(id) {
                        app.dispatch_hotkey_action(action);
                    }
                }
                AppEvent::PortalHotkeyActivated(action) => {
                    app.dispatch_hotkey_action(action);
                }
                AppEvent::PortalShortcutsStatusUpdated {
                    status,
                    permission_denied,
                } => {
                    #[cfg(target_os = "linux")]
                    app.hotkeys.set_portal_status(status);
                    #[cfg(not(target_os = "linux"))]
                    let _ = status;
                    if permission_denied {
                        notifications::notify_if_enabled(
                            app.config.notifications_enabled,
                            "Global shortcuts disabled",
                            "Wayland shortcut permission was declined. You can re-enable shortcuts in System Settings -> Shortcuts.",
                        );
                    }
                    app.push_settings_state(None);
                }
                AppEvent::UpdateCheckFinished {
                    result,
                    user_initiated,
                } => match result {
                    Ok(Some(offer)) => {
                        let latest_ver = offer.version.clone();
                        let status = UpdateStatus::Available {
                            current_version: app.updater_cfg.current_version.clone(),
                            latest_version: offer.version.clone(),
                            pub_date: offer.pub_date.clone(),
                            release_notes: offer.release_notes.clone(),
                            artifact_size: offer.artifact.size,
                            artifact_kind: offer.artifact.kind,
                        };
                        app.pending_offer = Some(offer);
                        app.verified_download_path = None;
                        app.set_update_status(status);
                        notifications::notify_if_enabled(
                            app.config.notifications_enabled,
                            &format!("GeminiDesk v{latest_ver} available"),
                            "A verified update is available in Settings -> About.",
                        );
                        if app.config.auto_download_updates {
                            app.start_update_download();
                        }
                    }
                    Ok(None) => {
                        app.pending_offer = None;
                        app.set_update_status(UpdateStatus::UpToDate {
                            current_version: app.updater_cfg.current_version.clone(),
                            checked_at_unix: updater::now_unix(),
                        });
                    }
                    Err(message) => {
                        if user_initiated {
                            app.set_update_status(UpdateStatus::Error { message });
                        } else {
                            warn!(%message, "Background startup update check skipped");
                        }
                    }
                },
                AppEvent::UpdateDownloadFinished(result) => match result {
                    Ok(path) => {
                        app.verified_download_path = Some(path.clone());
                        if let Some(offer) = &app.pending_offer {
                            let ver = offer.version.clone();
                            let sha_prefix: String =
                                offer.artifact.sha256.chars().take(12).collect();
                            app.set_update_status(UpdateStatus::ReadyToInstall {
                                version: offer.version.clone(),
                                pub_date: offer.pub_date.clone(),
                                release_notes: offer.release_notes.clone(),
                                verified_path: path.display().to_string(),
                                sha256_prefix: sha_prefix,
                                artifact_kind: offer.artifact.kind,
                            });
                            notifications::notify_if_enabled(
                                app.config.notifications_enabled,
                                &format!("GeminiDesk v{ver} ready to install"),
                                "Signature and SHA-256 verified. Open Settings -> About to install.",
                            );
                        }
                    }
                    Err(message) => {
                        app.set_update_status(UpdateStatus::Error { message });
                    }
                },
                AppEvent::TrayMenu(id) => match id.as_str() {
                    MENU_OPEN_GEMINI => app.show_main_window(),
                    MENU_QUICK_CHAT => app.toggle_quick_chat(),
                    MENU_COMPANION => app.toggle_companion(),
                    MENU_NEW_CHAT => {
                        let _ = app.proxy.send_event(AppEvent::NewChat);
                    }
                    MENU_CAPTURE_SCREENSHOT => {
                        let _ = app.proxy.send_event(AppEvent::InvokeScreenshot);
                    }
                    MENU_CLIPBOARD_ACTIONS => {
                        let _ = app.proxy.send_event(AppEvent::InvokeClipboardActions);
                    }
                    MENU_AUTH_BACK => app.navigate_auth_back(),
                    MENU_AUTH_CANCEL => app.cancel_auth_flow(),
                    MENU_ACCOUNT => app.open_or_focus_settings(target, Some("account".into())),
                    MENU_SETTINGS => app.open_or_focus_settings(target, None),
                    MENU_QUIT => {
                        let _ = app.proxy.send_event(AppEvent::Quit);
                    }
                    _ => {}
                },
                AppEvent::TrayLeftClick => {
                    if app.main_window.is_visible() && !app.main_window.is_minimized() {
                        app.hide_main_to_tray();
                    } else if app.companion_window.is_visible()
                        && !app.companion_window.is_minimized()
                    {
                        app.hide_companion();
                    } else {
                        app.show_main_window();
                    }
                }
                AppEvent::PageLoadStateChanged { finished, uri } => {
                    let should_probe = app.auth.on_page_url(&uri, finished);
                    app.sync_auth_ui();
                    if should_probe {
                        app.engine
                            .probe_auth_state(app.auth.flow_generation(), app.proxy.clone());
                    }
                }
                AppEvent::PageLoadFailed { uri, error } => {
                    if app.auth.on_load_failed(&uri, &error) {
                        app.sync_auth_ui();
                    }
                }
                AppEvent::CancelAuthFlow => {
                    app.cancel_auth_flow();
                }
                AppEvent::RetryAuthFlow => {
                    app.retry_auth_flow();
                }
                AppEvent::AuthNavigateBack => {
                    app.navigate_auth_back();
                }
                AppEvent::AuthEscapePressed => {
                    if app.auth.is_account_flow_active() {
                        app.engine.check_escape_safe_to_cancel(app.proxy.clone());
                    }
                }
                AppEvent::VerifyAuthSession(generation) => {
                    if app.auth.flow_generation() == generation {
                        app.engine.probe_auth_state(generation, app.proxy.clone());
                    }
                }
                AppEvent::AuthProbeCompleted { generation, result } => {
                    if app.auth.apply_probe_result(generation, result).is_ok() {
                        app.sync_auth_ui();
                    }
                }
                AppEvent::DocumentTitleChanged(doc_title) => {
                    if !app.first_title_logged {
                        app.first_title_logged = true;
                        info!(
                            elapsed_ms = app.startup_clock.elapsed().as_millis(),
                            %doc_title,
                            "Startup phase: first visible Gemini document title"
                        );
                    } else {
                        info!(%doc_title, "Gemini document title updated");
                    }
                }
                AppEvent::Quit => {
                    app.shutdown();
                    guard.take();
                    *control_flow = ControlFlow::Exit;
                }
            },

            Event::WindowEvent {
                window_id,
                event: win_event,
                ..
            } => {
                if window_id == app.main_window.id() {
                    match win_event {
                        WindowEvent::CloseRequested => match app.config.close_behavior {
                            CloseBehavior::MinimizeToTray => app.hide_main_to_tray(),
                            CloseBehavior::Quit => {
                                app.shutdown();
                                guard.take();
                                *control_flow = ControlFlow::Exit;
                            }
                        },
                        WindowEvent::Resized(_) => {
                            if app.main_window.is_minimized() {
                                if app.config.minimize_to_tray && app.main_window.is_visible() {
                                    app.hide_main_to_tray();
                                } else {
                                    // Even when minimized to the Windows taskbar, throttle WebView2
                                    // so the hidden page does not burn ~7% idle GPU/renderer CPU.
                                    app.enter_background_state();
                                }
                            } else if app.main_window.is_visible() {
                                if let Some(header) = &app.auth_header {
                                    header.layout(&app.main_window);
                                }
                                if app.active_host == ActiveHost::Main {
                                    if app.engine.is_low_memory() || app.engine.is_suspended() {
                                        app.wake_engine();
                                    }
                                    app.engine.sync_bounds_to_window_with_inset(
                                        &app.main_window,
                                        app.main_top_inset(),
                                    );
                                }
                                app.capture_main_geometry();
                            }
                        }
                        WindowEvent::Moved(_) => {
                            if app.main_window.is_visible() {
                                app.capture_main_geometry();
                            }
                        }
                        WindowEvent::ThemeChanged(_)
                            if app.config.theme == config::ThemePreference::System =>
                        {
                            app.effective_dark = platform::is_system_dark_mode();
                            app.apply_theme_to_all_windows(app.config.theme);
                        }
                        _ => {}
                    }
                } else if window_id == app.quick_chat_window.id() {
                    match win_event {
                        WindowEvent::CloseRequested => app.hide_quick_chat(),
                        WindowEvent::KeyboardInput {
                            event:
                                KeyEvent {
                                    logical_key: Key::Escape,
                                    state: ElementState::Pressed,
                                    ..
                                },
                            ..
                        } => app.hide_quick_chat(),
                        WindowEvent::Focused(false)
                            if app.config.quick_chat_hide_on_blur
                                && app.quick_chat_window.is_visible()
                                && app
                                    .last_quick_chat_shown
                                    .is_none_or(|t| t.elapsed() >= BLUR_GRACE_PERIOD)
                                && !platform::is_window_still_active_or_owned(
                                    &app.quick_chat_window,
                                ) =>
                        {
                            app.hide_quick_chat();
                        }
                        _ => {}
                    }
                } else if window_id == app.companion_window.id() {
                    match win_event {
                        WindowEvent::CloseRequested => app.hide_companion(),
                        WindowEvent::Moved(_) => {
                            app.snap_companion_if_needed();
                            app.capture_companion_geometry();
                        }
                        WindowEvent::Resized(_) => {
                            if app.companion_window.is_minimized() {
                                app.enter_background_state();
                            } else if app.companion_window.is_visible() {
                                if app.active_host == ActiveHost::Companion
                                    && (app.engine.is_low_memory() || app.engine.is_suspended())
                                {
                                    app.wake_engine();
                                    app.engine.sync_bounds_to_window(&app.companion_window);
                                }
                                app.capture_companion_geometry();
                            }
                        }
                        _ => {}
                    }
                } else if app
                    .settings_window
                    .as_ref()
                    .is_some_and(|s| s.id() == window_id)
                    && matches!(win_event, WindowEvent::CloseRequested)
                {
                    app.settings_window = None;
                    app.perf_sampler.reset();
                    platform::trim_allocator();
                } else if app
                    .action_window
                    .as_ref()
                    .is_some_and(|a| a.id() == window_id)
                    && matches!(win_event, WindowEvent::CloseRequested)
                {
                    app.action_window = None;
                    platform::trim_allocator();
                }
            }
            Event::Reopen {
                has_visible_windows: false,
                ..
            } => {
                // macOS Dock icon click restores the Main Window cleanly.
                app.show_main_window();
            }
            _ => {}
        }
    });
}
