use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use muda::MenuEvent;
use tao::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder};
use tracing::{error, info, warn};
use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};
use wry::WebContext;

use crate::config::{
    self, AppConfig, AppPaths, AppState, CloseBehavior, WindowGeometry, clear_safe_cache,
    compute_cache_size_bytes, load_config, load_state, save_config, save_state,
};
use crate::hotkeys::{HotkeyAction, HotkeyController};
use crate::icon;
use crate::platform;
use crate::settings_window::{SettingsIpcMessage, SettingsWindow};
use crate::tray::{
    MENU_NEW_CHAT, MENU_OPEN_GEMINI, MENU_QUICK_CHAT, MENU_QUIT, MENU_SETTINGS, TrayController,
};
use crate::webview::WebViewEngine;

const MAIN_WINDOW_TITLE: &str = "GeminiDesk — Unofficial Gemini Client";
const QUICK_CHAT_TITLE: &str = "GeminiDesk Quick Chat";
const BLUR_GRACE_PERIOD: Duration = Duration::from_millis(300);

#[derive(Debug, Default, Clone, Copy)]
pub struct LaunchOptions {
    pub start_minimized: bool,
    pub start_quick_chat: bool,
    pub open_settings: bool,
}

impl LaunchOptions {
    pub fn from_env() -> Self {
        let mut opts = Self::default();
        for arg in std::env::args().skip(1) {
            match arg.as_str() {
                "--minimized" | "--tray" => opts.start_minimized = true,
                "--quick-chat" => opts.start_quick_chat = true,
                "--settings" => opts.open_settings = true,
                _ => {}
            }
        }
        opts
    }
}

/// Application-wide event bus driven entirely by OS callbacks (zero polling).
#[derive(Debug)]
pub enum AppEvent {
    ShowMainWindow,
    ToggleQuickChat,
    HideQuickChat,
    NewChat,
    OpenSettings,
    SettingsIpc(SettingsIpcMessage),
    HotkeyPressed(u32),
    TrayMenu(String),
    TrayLeftClick,
    DocumentTitleChanged(String),
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveHost {
    Main,
    QuickChat,
}

struct DesktopApp {
    paths: AppPaths,
    config: AppConfig,
    state: AppState,
    main_window: Window,
    quick_chat_window: Window,
    web_context: WebContext,
    engine: WebViewEngine,
    active_host: ActiveHost,
    in_quick_chat: Arc<AtomicBool>,
    last_quick_chat_shown: Option<Instant>,
    hotkeys: HotkeyController,
    tray: Option<TrayController>,
    settings_window: Option<SettingsWindow>,
    effective_dark: bool,
    last_normal_geometry: Option<WindowGeometry>,
    proxy: tao::event_loop::EventLoopProxy<AppEvent>,
}

impl DesktopApp {
    fn show_main_window(&mut self) {
        let t0 = Instant::now();
        if self.quick_chat_window.is_visible() {
            self.quick_chat_window.set_visible(false);
        }

        if self.active_host != ActiveHost::Main {
            if let Err(err) = self.engine.attach_to_window(&self.main_window) {
                error!(?err, "Failed to reparent WebView to main window");
            } else {
                self.active_host = ActiveHost::Main;
                self.in_quick_chat.store(false, Ordering::Relaxed);
            }
        }

        self.engine.set_low_memory(false);
        self.engine.set_visible(true);
        if self.main_window.is_minimized() {
            self.main_window.set_minimized(false);
        }
        self.main_window.set_visible(true);
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
            self.main_window.set_visible(false);
        }

        let work_area = platform::active_monitor_work_area(&self.main_window);
        let scale = platform::monitor_scale_for_work_area(&self.main_window, work_area);
        let (x, y, w, h) = platform::compute_quick_chat_rect(work_area, scale);

        self.quick_chat_window
            .set_outer_position(PhysicalPosition::new(x, y));
        self.quick_chat_window
            .set_inner_size(PhysicalSize::new(w, h));

        if self.active_host != ActiveHost::QuickChat {
            if let Err(err) = self.engine.attach_to_window(&self.quick_chat_window) {
                error!(?err, "Failed to reparent WebView to Quick Chat window");
            } else {
                self.active_host = ActiveHost::QuickChat;
                self.in_quick_chat.store(true, Ordering::Relaxed);
            }
        }

        self.engine.set_low_memory(false);
        self.engine.set_visible(true);
        self.quick_chat_window.set_visible(true);
        platform::focus_window(&self.quick_chat_window);
        self.engine.focus();

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
        self.quick_chat_window.set_visible(false);
        self.in_quick_chat.store(false, Ordering::Relaxed);
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

    fn hide_main_to_tray(&mut self) {
        self.capture_main_geometry();
        self.persist_state();
        self.main_window.set_visible(false);
        self.enter_background_state();
        info!("Main window hidden to system tray");
    }

    fn enter_background_state(&self) {
        if !self.main_window.is_visible() && !self.quick_chat_window.is_visible() {
            self.engine.set_visible(false);
            if self.config.reduce_memory_when_hidden {
                self.engine.set_low_memory(true);
            }
        }
    }

    fn open_or_focus_settings(&mut self, target: &EventLoopWindowTarget<AppEvent>) {
        if let Some(settings) = &self.settings_window {
            settings.focus();
            self.push_settings_state(None);
            return;
        }

        match SettingsWindow::open(
            target,
            &mut self.web_context,
            self.effective_dark,
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

    fn push_settings_state(&self, toast: Option<String>) {
        if let Some(settings) = &self.settings_window {
            let cache_bytes = compute_cache_size_bytes(&self.paths.webview_data_dir);
            settings.push_state(
                &self.config,
                self.hotkeys.status(),
                self.effective_dark,
                self.paths.webview_data_dir.display().to_string(),
                cache_bytes,
                toast,
            );
        }
    }

    fn apply_config_update(&mut self, mut new_cfg: AppConfig) {
        let mut toast = Some("Settings saved".to_string());

        // 1. OS startup registration
        if new_cfg.launch_at_startup != self.config.launch_at_startup
            && let Err(err) = platform::set_launch_at_startup(new_cfg.launch_at_startup)
        {
            warn!("{err}");
            new_cfg.launch_at_startup = self.config.launch_at_startup;
            toast = Some(err);
        }

        // 2. Global shortcuts (re-register if changed)
        if new_cfg.quick_chat_shortcut != self.config.quick_chat_shortcut
            || new_cfg.full_window_shortcut != self.config.full_window_shortcut
        {
            self.hotkeys
                .apply(&new_cfg.quick_chat_shortcut, &new_cfg.full_window_shortcut);
            if let Some(tray) = &self.tray {
                tray.update_quick_chat_shortcut(&new_cfg.quick_chat_shortcut);
            }
        }

        // 3. Theme updates
        if new_cfg.theme != self.config.theme {
            self.effective_dark = platform::resolve_is_dark(new_cfg.theme);
            platform::style_main_window(&self.main_window, self.effective_dark);
            platform::style_quick_chat_window(&self.quick_chat_window, self.effective_dark);
            self.engine.apply_theme(new_cfg.theme, self.effective_dark);
            if let Some(settings) = &self.settings_window {
                settings.apply_theme(self.effective_dark);
            }
        }

        // 4. Memory reduction preference
        if !new_cfg.reduce_memory_when_hidden {
            self.engine.set_low_memory(false);
        } else if !self.main_window.is_visible() && !self.quick_chat_window.is_visible() {
            self.engine.set_low_memory(true);
        }

        self.config = new_cfg;
        if let Err(err) = save_config(&self.paths.config_file(), &self.config) {
            warn!(?err, "Failed to persist config.toml");
            toast = Some(format!("Failed to save config: {err}"));
        }

        self.push_settings_state(toast);
    }

    fn capture_main_geometry(&mut self) {
        if self.main_window.is_minimized() {
            return;
        }
        let maximized = self.main_window.is_maximized();
        if !maximized && let Ok(pos) = self.main_window.outer_position() {
            let size = self.main_window.inner_size();
            if size.width >= 320 && size.height >= 320 {
                self.last_normal_geometry = Some(WindowGeometry {
                    x: pos.x,
                    y: pos.y,
                    width: size.width,
                    height: size.height,
                    maximized: false,
                });
            }
        }
        if let Some(mut geom) = self.last_normal_geometry {
            geom.maximized = maximized;
            self.state.main_window = Some(geom);
        }
    }

    fn persist_state(&self) {
        if let Err(err) = save_state(&self.paths.state_file(), &self.state) {
            warn!(?err, "Failed to save state.toml");
        }
    }

    fn shutdown(&mut self) {
        self.capture_main_geometry();
        self.persist_state();
        self.hotkeys.unregister_all();
        self.settings_window = None;
        self.tray = None;
        info!("GeminiDesk shutting down cleanly");
    }
}

pub fn run(options: LaunchOptions) -> Result<(), Box<dyn std::error::Error>> {
    let startup_clock = Instant::now();

    let Some(_instance_guard) = platform::acquire_single_instance(options.start_quick_chat) else {
        info!("Existing GeminiDesk instance signaled; exiting secondary process");
        return Ok(());
    };

    let paths = AppPaths::resolve();
    let mut cfg = load_config(&paths.config_file());
    // Keep startup checkbox in sync with the actual OS registry/autostart state.
    cfg.launch_at_startup = platform::is_launch_at_startup_enabled();
    let state = load_state(&paths.state_file());
    let effective_dark = platform::resolve_is_dark(cfg.theme);

    info!(
        version = env!("CARGO_PKG_VERSION"),
        data_dir = %paths.webview_data_dir.display(),
        dark = effective_dark,
        "Starting GeminiDesk"
    );

    let event_loop = EventLoopBuilder::<AppEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // Wire zero-polling push event handlers before creating tray/hotkeys.
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

    // 1. Main Window (created hidden until WebView2 is attached to prevent any white flash)
    let mut main_wb = WindowBuilder::new()
        .with_title(MAIN_WINDOW_TITLE)
        .with_inner_size(LogicalSize::new(1220.0, 840.0))
        .with_min_inner_size(LogicalSize::new(420.0, 520.0))
        .with_visible(false);
    if let Some(ic) = app_icon.clone() {
        main_wb = main_wb.with_window_icon(Some(ic));
    }
    let main_window = main_wb.build(&event_loop)?;

    // Restore persisted window geometry if still visible on a connected monitor.
    let mut restored_maximized = false;
    let mut last_normal_geometry = None;
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
        last_normal_geometry = Some(geom);
    }

    // 2. Quick Chat Spotlight Window (pre-created hidden HWND -> ~15-25ms instant summon)
    let mut quick_wb = WindowBuilder::new()
        .with_title(QUICK_CHAT_TITLE)
        .with_inner_size(LogicalSize::new(760.0, 580.0))
        .with_resizable(false)
        .with_decorations(false)
        .with_always_on_top(true)
        .with_visible(false);
    if let Some(ic) = app_icon {
        quick_wb = quick_wb.with_window_icon(Some(ic));
    }
    #[cfg(target_os = "windows")]
    {
        use tao::platform::windows::WindowBuilderExtWindows;
        quick_wb = quick_wb.with_skip_taskbar(true);
    }
    let quick_chat_window = quick_wb.build(&event_loop)?;

    // Style both native windows BEFORE displaying them.
    platform::style_main_window(&main_window, effective_dark);
    platform::style_quick_chat_window(&quick_chat_window, effective_dark);

    #[cfg(target_os = "windows")]
    platform::windows::install_activation_subclass(&main_window, proxy.clone());

    // 3. Shared WebContext preserving Phase B Google authentication profile
    let mut web_context = WebContext::new(Some(paths.webview_data_dir.clone()));
    let in_quick_chat = Arc::new(AtomicBool::new(false));
    let engine = WebViewEngine::new(
        &main_window,
        &mut web_context,
        cfg.theme,
        effective_dark,
        Arc::clone(&in_quick_chat),
        proxy.clone(),
    )?;

    // 4. Persistent system tray & global shortcuts
    let tray = match TrayController::new(&cfg.quick_chat_shortcut) {
        Ok(t) => Some(t),
        Err(err) => {
            warn!(?err, "System tray unavailable");
            None
        }
    };

    let mut hotkeys = HotkeyController::new();
    hotkeys.apply(&cfg.quick_chat_shortcut, &cfg.full_window_shortcut);

    let mut app = DesktopApp {
        paths,
        config: cfg,
        state,
        main_window,
        quick_chat_window,
        web_context,
        engine,
        active_host: ActiveHost::Main,
        in_quick_chat,
        last_quick_chat_shown: None,
        hotkeys,
        tray,
        settings_window: None,
        effective_dark,
        last_normal_geometry,
        proxy: proxy.clone(),
    };

    // 5. Initial window presentation
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
        let _ = proxy.send_event(AppEvent::OpenSettings);
    }

    // Keep single-instance guard alive until the event loop exits.
    let mut guard = Some(_instance_guard);

    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(app_event) => match app_event {
                AppEvent::ShowMainWindow => app.show_main_window(),
                AppEvent::ToggleQuickChat => app.toggle_quick_chat(),
                AppEvent::HideQuickChat => app.hide_quick_chat(),
                AppEvent::NewChat => {
                    if !app.main_window.is_visible() && !app.quick_chat_window.is_visible() {
                        app.show_main_window();
                    }
                    app.engine.load_new_chat();
                    app.engine.focus();
                }
                AppEvent::OpenSettings => app.open_or_focus_settings(target),
                AppEvent::SettingsIpc(msg) => match msg {
                    SettingsIpcMessage::Ready => app.push_settings_state(None),
                    SettingsIpcMessage::SaveConfig { config: new_cfg } => {
                        app.apply_config_update(new_cfg);
                    }
                    SettingsIpcMessage::ClearCache => {
                        let freed = clear_safe_cache(&app.paths.webview_data_dir);
                        let mb = freed as f64 / (1024.0 * 1024.0);
                        app.push_settings_state(Some(format!(
                            "Cleared {mb:.1} MB of cache (Google sign-in preserved)"
                        )));
                    }
                    SettingsIpcMessage::OpenProfileFolder => {
                        if let Err(err) = open::that(&app.paths.webview_data_dir) {
                            warn!(?err, "Failed to open profile directory");
                        }
                    }
                    SettingsIpcMessage::Close => {
                        app.settings_window = None;
                    }
                },
                AppEvent::HotkeyPressed(id) => match app.hotkeys.action_for_id(id) {
                    Some(HotkeyAction::ToggleQuickChat) => app.toggle_quick_chat(),
                    Some(HotkeyAction::ShowFullWindow) => app.show_main_window(),
                    None => {}
                },
                AppEvent::TrayMenu(id) => match id.as_str() {
                    MENU_OPEN_GEMINI => app.show_main_window(),
                    MENU_QUICK_CHAT => app.toggle_quick_chat(),
                    MENU_NEW_CHAT => {
                        let _ = app.proxy.send_event(AppEvent::NewChat);
                    }
                    MENU_SETTINGS => app.open_or_focus_settings(target),
                    MENU_QUIT => {
                        let _ = app.proxy.send_event(AppEvent::Quit);
                    }
                    _ => {}
                },
                AppEvent::TrayLeftClick => {
                    if app.main_window.is_visible() && !app.main_window.is_minimized() {
                        app.hide_main_to_tray();
                    } else {
                        app.show_main_window();
                    }
                }
                AppEvent::DocumentTitleChanged(doc_title) => {
                    // Keep the base title stable so single-instance discovery remains reliable,
                    // while still reflecting Google Gemini context cleanly in logs.
                    info!(%doc_title, "Gemini document title updated");
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
                            if app.main_window.is_minimized()
                                && app.config.minimize_to_tray
                                && app.main_window.is_visible()
                            {
                                app.hide_main_to_tray();
                            } else if app.main_window.is_visible() {
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
                            platform::style_main_window(&app.main_window, app.effective_dark);
                            platform::style_quick_chat_window(
                                &app.quick_chat_window,
                                app.effective_dark,
                            );
                            app.engine.apply_theme(app.config.theme, app.effective_dark);
                        }
                        _ => {}
                    }
                } else if window_id == app.quick_chat_window.id() {
                    match win_event {
                        WindowEvent::CloseRequested => app.hide_quick_chat(),
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
                } else if app
                    .settings_window
                    .as_ref()
                    .is_some_and(|s| s.id() == window_id)
                    && matches!(win_event, WindowEvent::CloseRequested)
                {
                    app.settings_window = None;
                }
            }
            _ => {}
        }
    });
}
