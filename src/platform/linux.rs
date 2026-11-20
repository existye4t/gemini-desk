//! Linux platform integration — Wayland-first (CachyOS / Arch Linux / KDE Plasma 6)
//! with full X11 and GNOME compatibility.
//!
//! Capabilities implemented:
//! - Dark/Light theme detection via XDG Settings Portal (`org.freedesktop.appearance.color-scheme`)
//!   with KDE `kdeglobals` and `GTK_THEME` fallbacks.
//! - GTK3 window styling, opacity, focus activation, and Quick Chat floating hints.
//! - Unix domain socket single-instance activation (`$XDG_RUNTIME_DIR/io.geminidesk.GeminiDesk.sock`)
//!   allowing instant wake from CLI flags or KDE/GNOME/Hyprland custom shortcuts.
//! - XDG GlobalShortcuts portal (`org.freedesktop.portal.GlobalShortcuts`) over D-Bus on Wayland
//!   for native KDE Plasma 6 / Hyprland global shortcut approval & activation.
//! - XDG Autostart (`~/.config/autostart/io.geminidesk.GeminiDesk.desktop`).

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use gtk::prelude::{
    BoxExt, ButtonExt, ContainerExt, CssProviderExt, GtkSettingsExt, GtkWindowExt, LabelExt,
    StyleContextExt, WidgetExt,
};
use tao::event_loop::EventLoopProxy;
use tao::platform::unix::WindowExtUnix;
use tao::window::Window;
use tracing::{info, warn};

use crate::app::AppEvent;
use crate::auth::AuthNavBarState;
use crate::config::AppConfig;
use crate::hotkeys::{HotkeyAction, HotkeyStatus, SingleHotkeyStatus};
use crate::platform::{ActivationTarget, WorkArea};

/// Desktop application ID matching our `.desktop` and AppStream metadata files.
pub const LINUX_APP_ID: &str = "io.geminidesk.GeminiDesk";

/// Returns true when running inside a Wayland session.
pub fn is_wayland_session() -> bool {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return true;
    }
    std::env::var("XDG_SESSION_TYPE")
        .map(|v| v.eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
}

/// Returns the current desktop environment identifier (e.g. `"KDE"`, `"GNOME"`).
pub fn desktop_environment() -> String {
    std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "Linux".to_string())
}

/// Queries the XDG Desktop Settings Portal (`org.freedesktop.appearance.color-scheme`)
/// for native KDE Plasma 6 / GNOME dark-mode preference, with `kdeglobals` and `GTK_THEME` fallback.
pub fn is_system_dark_mode() -> bool {
    if let Some(dark) = query_portal_color_scheme() {
        return dark;
    }
    if let Some(dark) = query_kdeglobals_dark_mode() {
        return dark;
    }
    if let Ok(theme) = std::env::var("GTK_THEME") {
        let lower = theme.to_ascii_lowercase();
        if lower.contains("light") {
            return false;
        }
        if lower.contains("dark") {
            return true;
        }
    }
    true
}

fn query_portal_color_scheme() -> Option<bool> {
    let conn = zbus::blocking::Connection::session().ok()?;
    let proxy = zbus::blocking::Proxy::new(
        &conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
    )
    .ok()?;

    // XDG Portal spec: ReadOne returns variant(u32); legacy Read returns variant(variant(u32)):
    // 0 = No preference, 1 = Prefer dark, 2 = Prefer light.
    let val: zbus::zvariant::OwnedValue = proxy
        .call("ReadOne", &("org.freedesktop.appearance", "color-scheme"))
        .or_else(|_| proxy.call("Read", &("org.freedesktop.appearance", "color-scheme")))
        .ok()?;
    let mut cur: &zbus::zvariant::Value<'_> = &val;
    while let zbus::zvariant::Value::Value(inner) = cur {
        cur = inner;
    }
    let scheme = match cur {
        zbus::zvariant::Value::U32(n) => *n,
        _ => return None,
    };
    match scheme {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

fn query_kdeglobals_dark_mode() -> Option<bool> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    let contents = fs::read_to_string(config_home.join("kdeglobals")).ok()?;
    for line in contents.lines() {
        let trimmed = line.trim();
        if let Some(val) = trimmed.strip_prefix("ColorScheme=") {
            let lower = val.to_ascii_lowercase();
            if lower.contains("dark") || lower.contains("obsidian") || lower.contains("noir") {
                return Some(true);
            }
            if lower.contains("light") || lower.contains("breeze") {
                return Some(false);
            }
        }
    }
    None
}

/// Initializes Linux/Wayland application identity (`prgname`, default icon name, and user-level
/// XDG hicolor icons + `.desktop` launcher when running uninstalled/portable builds) so Wayland
/// compositors (KWin, Mutter, Hyprland) and XDG Desktop Portals associate windows and global
/// shortcuts with `io.geminidesk.GeminiDesk` and display the GeminiDesk prism icon.
pub fn init_desktop_identity() {
    gtk::glib::set_prgname(Some(LINUX_APP_ID));
    gtk::glib::set_application_name("GeminiDesk");
    gtk::Window::set_default_icon_name(LINUX_APP_ID);
    ensure_user_desktop_assets();
}

fn ensure_user_desktop_assets() {
    const ICON_SVG: &[u8] = include_bytes!("../../assets/icons/geminidesk.svg");
    const ICON_PNG_256: &[u8] = include_bytes!("../../assets/icons/geminidesk-256.png");

    let Some(data_home) = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    else {
        return;
    };

    let svg_path = data_home
        .join("icons/hicolor/scalable/apps")
        .join(format!("{LINUX_APP_ID}.svg"));
    if !svg_path.exists() {
        if let Some(parent) = svg_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&svg_path, ICON_SVG);
    }

    let png_path = data_home
        .join("icons/hicolor/256x256/apps")
        .join(format!("{LINUX_APP_ID}.png"));
    if !png_path.exists() {
        if let Some(parent) = png_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&png_path, ICON_PNG_256);
    }

    let system_desktop = PathBuf::from(format!("/usr/share/applications/{LINUX_APP_ID}.desktop"));
    let user_desktop = data_home
        .join("applications")
        .join(format!("{LINUX_APP_ID}.desktop"));
    if !system_desktop.exists()
        && !user_desktop.exists()
        && let Ok(exe) = std::env::current_exe()
    {
        if let Some(parent) = user_desktop.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let exe_str = exe.display();
        let desktop_entry = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Version=1.5\n\
             Name=GeminiDesk\n\
             GenericName=Gemini Desktop Client\n\
             Comment=Lightweight native Rust desktop client for Google Gemini\n\
             Exec=\"{exe_str}\" %U\n\
             Icon={LINUX_APP_ID}\n\
             Terminal=false\n\
             Categories=Network;Chat;\n\
             Keywords=Gemini;AI;Google;Chat;Assistant;\n\
             StartupNotify=true\n\
             StartupWMClass={LINUX_APP_ID}\n\
             Actions=QuickChat;Companion;Settings;\n\
             \n\
             [Desktop Action QuickChat]\n\
             Name=Open Quick Chat\n\
             Exec=\"{exe_str}\" --quick-chat\n\
             \n\
             [Desktop Action Companion]\n\
             Name=Toggle Floating Companion\n\
             Exec=\"{exe_str}\" --companion\n\
             \n\
             [Desktop Action Settings]\n\
             Name=Settings\n\
             Exec=\"{exe_str}\" --settings\n"
        );
        let _ = fs::write(&user_desktop, desktop_entry);
    }
}

pub fn style_main_window(window: &Window, dark: bool) {
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(dark);
    }
    let gtk_win = window.gtk_window();
    gtk_win.set_role("geminidesk-main");
    gtk_win.set_icon_name(Some(LINUX_APP_ID));
}

pub fn style_quick_chat_window(window: &Window, dark: bool) {
    style_main_window(window, dark);
    let gtk_win = window.gtk_window();
    gtk_win.set_role("geminidesk-quick-chat");
    gtk_win.set_skip_taskbar_hint(true);
    gtk_win.set_skip_pager_hint(true);
    gtk_win.set_keep_above(true);
    gtk_win.set_position(gtk::WindowPosition::CenterAlways);

    // Give the borderless Quick Chat overlay card a crisp 1px monochrome frame on Wayland/X11.
    if let Some(vbox) = window.default_vbox() {
        let border_hex = if dark { "#29292B" } else { "#D8D8DC" };
        let bg_hex = if dark { "#131314" } else { "#FFFFFF" };
        let css = format!("box {{ border: 1px solid {border_hex}; background-color: {bg_hex}; }}");
        let provider = gtk::CssProvider::new();
        if provider.load_from_data(css.as_bytes()).is_ok() {
            vbox.style_context()
                .add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
    }
}

/// Configures Quick Chat dimensions on GTK3/Wayland.
/// In GTK3, `set_resizable(false)` forces a `GtkWindow` to expand to its child's natural size
/// (which `WebKitWebView` sets to its last `1220x840` Main Window allocation) and ignores
/// `gtk_window_resize(w, h)`. Keeping the borderless Quick Chat window resizable at the GTK level
/// causes GTK3 to size it to `default_size` / `resize(w, h)` bounded by `child_min = (0, 0)`.
pub fn apply_quick_chat_geometry(window: &Window, logical_w: u32, logical_h: u32) {
    let w = (logical_w.clamp(520, 1400)) as i32;
    let h = (logical_h.clamp(420, 1000)) as i32;
    let gtk_win = window.gtk_window();
    gtk_win.set_resizable(true);
    if let Some(vbox) = window.default_vbox() {
        vbox.set_size_request(-1, -1);
        for child in vbox.children() {
            child.set_size_request(-1, -1);
        }
    }
    gtk_win.set_size_request(-1, -1);
    gtk_win.set_default_size(w, h);
    gtk_win.resize(w, h);
}

pub fn set_window_opacity(window: &Window, opacity_pct: u8) {
    let alpha = (opacity_pct.clamp(55, 100) as f64) / 100.0;
    window.gtk_window().set_opacity(alpha);
}

pub fn focus_window(window: &Window) {
    let gtk_win = window.gtk_window();
    gtk_win.set_accept_focus(true);
    if let Some(vbox) = window.default_vbox() {
        vbox.show_all();
    }
    gtk_win.show_all();
    gtk_win.deiconify();
    gtk_win.present();
    if !window.is_always_on_top() {
        gtk_win.set_keep_above(true);
        let win_clone = gtk_win.clone();
        gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(180), move || {
            win_clone.set_keep_above(false);
        });
    }
    // Also present on the next GLib idle tick after tao's asynchronous `window_requests_rx`
    // finishes hiding any previous window, preventing Wayland focus loss during reparenting.
    let win_idle = gtk_win.clone();
    gtk::glib::idle_add_local_once(move || {
        if win_idle.is_visible() {
            win_idle.present();
        }
    });
    window.set_focus();
}

pub fn is_window_still_active_or_owned(window: &Window) -> bool {
    window.gtk_window().is_active()
}

/// Queries the usable work area (excluding panels/docks) of the monitor currently containing
/// the pointer, falling back to the primary monitor or `window`'s monitor.
pub fn active_monitor_work_area(window: &Window) -> Option<WorkArea> {
    use gtk::gdk::prelude::{DeviceExt, MonitorExt, SeatExt};

    let display = gtk::gdk::Display::default()?;
    let monitor = display
        .default_seat()
        .and_then(|seat| seat.pointer())
        .and_then(|ptr| {
            let (_screen, x, y) = ptr.position();
            display.monitor_at_point(x, y)
        })
        .or_else(|| {
            window
                .gtk_window()
                .window()
                .and_then(|gdk_win| display.monitor_at_window(&gdk_win))
        })
        .or_else(|| display.primary_monitor())
        .or_else(|| display.monitor(0))?;

    let rect = monitor.workarea();
    let scale = monitor.scale_factor().max(1);
    Some(WorkArea {
        x: rect.x() * scale,
        y: rect.y() * scale,
        width: (rect.width() * scale).max(320),
        height: (rect.height() * scale).max(240),
    })
}

/// Hints glibc to release freed heap arenas back to the Linux kernel after closing
/// transient utility windows or entering background tray mode.
pub fn trim_allocator() {
    #[cfg(target_env = "gnu")]
    {
        unsafe extern "C" {
            fn malloc_trim(pad: usize) -> i32;
        }
        unsafe {
            let _ = malloc_trim(0);
        }
    }
}

/// Subtle, compact (`38px`) native GTK3 navigation header mounted at the top of the Main Window
/// strictly while a Google account-management or sign-in flow is active.
/// Zero duplicate WebViews, zero JS injection into `accounts.google.com`.
pub struct AuthNavHeader {
    container: gtk::Box,
    back_btn: gtk::Button,
    title_lbl: gtk::Label,
    subtitle_lbl: gtk::Label,
    retry_btn: gtk::Button,
    cancel_btn: gtk::Button,
    css_provider: gtk::CssProvider,
    visible: bool,
}

impl AuthNavHeader {
    pub fn attach(window: &Window, dark: bool, proxy: EventLoopProxy<AppEvent>) -> Option<Self> {
        let vbox = window.default_vbox()?;

        let container = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        container.set_widget_name("gd-auth-bar");
        container.set_size_request(-1, 38);
        container.set_hexpand(true);
        container.set_vexpand(false);
        container.set_margin_start(0);
        container.set_margin_end(0);

        let back_btn = gtk::Button::with_label("←  Back to Gemini");
        back_btn.set_widget_name("gd-auth-back");
        back_btn.set_tooltip_text(Some("Return to Gemini (Alt+Left)"));
        back_btn.set_relief(gtk::ReliefStyle::None);
        {
            let p = proxy.clone();
            back_btn.connect_clicked(move |_| {
                let _ = p.send_event(AppEvent::CancelAuthFlow);
            });
        }

        let text_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        text_box.set_hexpand(true);

        let title_lbl = gtk::Label::new(Some("Switch Google Account"));
        title_lbl.set_widget_name("gd-auth-title");
        title_lbl.set_xalign(0.0);

        let subtitle_lbl = gtk::Label::new(None);
        subtitle_lbl.set_widget_name("gd-auth-subtitle");
        subtitle_lbl.set_xalign(0.0);
        subtitle_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
        subtitle_lbl.set_hexpand(true);

        text_box.pack_start(&title_lbl, false, false, 0);
        text_box.pack_start(&subtitle_lbl, true, true, 0);

        let retry_btn = gtk::Button::with_label("Retry");
        retry_btn.set_widget_name("gd-auth-retry");
        retry_btn.set_relief(gtk::ReliefStyle::None);
        {
            let p = proxy.clone();
            retry_btn.connect_clicked(move |_| {
                let _ = p.send_event(AppEvent::RetryAuthFlow);
            });
        }

        let cancel_btn = gtk::Button::with_label("Cancel");
        cancel_btn.set_widget_name("gd-auth-cancel");
        cancel_btn.set_tooltip_text(Some("Cancel and return to Gemini (Esc)"));
        cancel_btn.set_relief(gtk::ReliefStyle::None);
        {
            let p = proxy.clone();
            cancel_btn.connect_clicked(move |_| {
                let _ = p.send_event(AppEvent::CancelAuthFlow);
            });
        }

        // Pressing Escape while keyboard focus is inside the native header bar safely cancels.
        {
            let p = proxy;
            container.connect_key_press_event(move |_, ev| {
                if ev.keyval() == gtk::gdk::keys::constants::Escape {
                    let _ = p.send_event(AppEvent::CancelAuthFlow);
                    return gtk::glib::Propagation::Stop;
                }
                gtk::glib::Propagation::Proceed
            });
        }

        container.pack_start(&back_btn, false, false, 0);
        container.pack_start(&text_box, true, true, 0);
        container.pack_end(&cancel_btn, false, false, 0);
        container.pack_end(&retry_btn, false, false, 0);

        vbox.pack_start(&container, false, true, 0);
        vbox.reorder_child(&container, 0);

        let css_provider = gtk::CssProvider::new();
        if let Some(screen) = gtk::gdk::Screen::default() {
            gtk::StyleContext::add_provider_for_screen(
                &screen,
                &css_provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        let header = Self {
            container,
            back_btn,
            title_lbl,
            subtitle_lbl,
            retry_btn,
            cancel_btn,
            css_provider,
            visible: false,
        };
        header.apply_theme(dark);
        header.container.set_no_show_all(true);
        header.container.hide();
        Some(header)
    }

    pub fn apply_theme(&self, dark: bool) {
        let (bg, border, text, muted, btn_bg, btn_hover, btn_border) = if dark {
            (
                "#0B0B0C", "#242426", "#F2F2F4", "#8E8E93", "#18181A", "#262629", "#2E2E32",
            )
        } else {
            (
                "#FFFFFF", "#E5E5E7", "#111113", "#6E6E73", "#F4F4F6", "#EAEAEF", "#DCDCE0",
            )
        };
        let css = format!(
            "#gd-auth-bar {{\n\
               background-color: {bg};\n\
               border-bottom: 1px solid {border};\n\
               padding: 4px 10px;\n\
             }}\n\
             #gd-auth-title {{\n\
               color: {text};\n\
               font-weight: 600;\n\
               font-size: 12px;\n\
               padding-left: 4px;\n\
             }}\n\
             #gd-auth-subtitle {{\n\
               color: {muted};\n\
               font-size: 11px;\n\
               padding-left: 4px;\n\
             }}\n\
             #gd-auth-back, #gd-auth-cancel, #gd-auth-retry {{\n\
               color: {text};\n\
               background-color: {btn_bg};\n\
               border: 1px solid {btn_border};\n\
               border-radius: 6px;\n\
               padding: 3px 10px;\n\
               min-height: 24px;\n\
               font-size: 12px;\n\
               font-weight: 500;\n\
             }}\n\
             #gd-auth-back:hover, #gd-auth-cancel:hover, #gd-auth-retry:hover {{\n\
               background-color: {btn_hover};\n\
             }}\n"
        );
        let _ = self.css_provider.load_from_data(css.as_bytes());
    }

    pub fn apply_state(&mut self, state: &AuthNavBarState) {
        if !state.visible {
            if self.visible {
                self.container.hide();
                self.container.set_no_show_all(true);
                self.visible = false;
            }
            return;
        }

        self.title_lbl.set_text(&state.title);
        self.subtitle_lbl.set_text(&state.subtitle);
        self.cancel_btn.set_label(&state.cancel_label);

        self.container.set_no_show_all(false);
        self.container.show_all();
        self.back_btn.show();
        if state.is_error {
            self.retry_btn.show();
            self.cancel_btn.hide();
        } else {
            self.retry_btn.hide();
            self.cancel_btn.show();
        }
        self.visible = true;
    }

    pub fn top_inset_px(&self) -> i32 {
        // GTK Box packs the header bar and WebView vertically without manual pixel insets.
        0
    }
}

fn autostart_desktop_file() -> Option<PathBuf> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(
        config_home
            .join("autostart")
            .join(format!("{LINUX_APP_ID}.desktop")),
    )
}

pub fn set_launch_at_startup(enabled: bool) -> Result<(), String> {
    let path = autostart_desktop_file()
        .ok_or_else(|| "Could not resolve XDG autostart directory".to_string())?;
    if enabled {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create autostart directory: {e}"))?;
        }
        let exe = std::env::current_exe()
            .map_err(|e| format!("Failed to resolve executable path: {e}"))?;
        let contents = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=GeminiDesk\n\
             Comment=Lightweight native Rust desktop client for Google Gemini\n\
             Exec=\"{}\" --minimized\n\
             Icon={LINUX_APP_ID}\n\
             StartupWMClass={LINUX_APP_ID}\n\
             Terminal=false\n\
             X-GNOME-Autostart-enabled=true\n",
            exe.display()
        );
        fs::write(&path, contents).map_err(|e| format!("Failed to write .desktop file: {e}"))
    } else if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("Failed to remove .desktop file: {e}"))
    } else {
        Ok(())
    }
}

pub fn is_launch_at_startup_enabled() -> bool {
    autostart_desktop_file().is_some_and(|p| p.exists())
}

// ---------------------------------------------------------------------------
// Single-instance Unix domain socket activation (instant CLI/compositor wake)
// ---------------------------------------------------------------------------

fn socket_path() -> PathBuf {
    if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime_dir).join(format!("{LINUX_APP_ID}.sock"))
    } else {
        let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
        std::env::temp_dir().join(format!("{LINUX_APP_ID}-{user}.sock"))
    }
}

fn encode_target(target: ActivationTarget) -> Vec<u8> {
    match target {
        ActivationTarget::MainWindow => b"main\n".to_vec(),
        ActivationTarget::QuickChat => b"quick_chat\n".to_vec(),
        ActivationTarget::Companion => b"companion\n".to_vec(),
        ActivationTarget::Settings => {
            if std::env::args().skip(1).any(|a| a == "--account") {
                b"settings:account\n".to_vec()
            } else if let Some(tab) = std::env::args()
                .skip(1)
                .find_map(|a| a.strip_prefix("--settings=").map(str::to_string))
                .filter(|s| !s.trim().is_empty())
            {
                format!("settings:{}\n", tab.trim()).into_bytes()
            } else {
                b"settings\n".to_vec()
            }
        }
        ActivationTarget::ClipboardActions => b"clipboard_actions\n".to_vec(),
        ActivationTarget::AskSelection => b"ask_selection\n".to_vec(),
        ActivationTarget::Screenshot => b"screenshot\n".to_vec(),
    }
}

fn decode_target(line: &str) -> Option<AppEvent> {
    let trimmed = line.trim();
    if let Some(tab) = trimmed.strip_prefix("settings:") {
        let tab_opt = (!tab.trim().is_empty()).then(|| tab.trim().to_string());
        return Some(AppEvent::OpenSettings(tab_opt));
    }
    match trimmed {
        "main" => Some(AppEvent::ShowMainWindow),
        "quick_chat" => Some(AppEvent::ToggleQuickChat),
        "companion" => Some(AppEvent::ToggleCompanion),
        "settings" => Some(AppEvent::OpenSettings(None)),
        "clipboard_actions" => Some(AppEvent::InvokeClipboardActions),
        "ask_selection" => Some(AppEvent::InvokeAskSelection),
        "screenshot" => Some(AppEvent::InvokeScreenshot),
        _ => None,
    }
}

pub struct SingleInstanceHandle {
    path: PathBuf,
    listener: Option<UnixListener>,
}

impl Drop for SingleInstanceHandle {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn acquire_single_instance(target: ActivationTarget) -> Option<SingleInstanceHandle> {
    let path = socket_path();
    if path.exists() {
        // Attempt to connect to an active primary instance first.
        if let Ok(mut stream) = UnixStream::connect(&path) {
            let _ = stream.write_all(&encode_target(target));
            let _ = stream.flush();
            return None;
        }
        // Stale socket from a previous crash; clean up safely.
        let _ = fs::remove_file(&path);
    }

    match UnixListener::bind(&path) {
        Ok(listener) => Some(SingleInstanceHandle {
            path,
            listener: Some(listener),
        }),
        Err(err) => {
            warn!(
                ?err,
                "Could not bind Linux single-instance socket; continuing"
            );
            Some(SingleInstanceHandle {
                path,
                listener: None,
            })
        }
    }
}

pub fn start_activation_listener(
    handle: &mut SingleInstanceHandle,
    proxy: EventLoopProxy<AppEvent>,
) {
    let Some(listener) = handle.listener.take() else {
        return;
    };
    thread::Builder::new()
        .name("geminidesk-ipc-wake".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut buf = [0u8; 64];
                if let Ok(n) = stream.read(&mut buf)
                    && let Ok(text) = std::str::from_utf8(&buf[..n])
                    && let Some(event) = decode_target(text)
                {
                    let _ = proxy.send_event(event);
                }
            }
        })
        .ok();
}

// ---------------------------------------------------------------------------
// XDG GlobalShortcuts Portal (KDE Plasma 6 / Hyprland / Wayland native)
// ---------------------------------------------------------------------------

/// Converts a user shortcut spec (e.g. `"Alt+Shift+Space"`) into XDG GlobalShortcuts
/// `preferred_trigger` notation (e.g. `"ALT+SHIFT+space"`).
pub fn to_xdg_trigger(spec: &str) -> String {
    spec.split('+')
        .map(|part| match part.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "control" => "CTRL".to_string(),
            "alt" | "option" => "ALT".to_string(),
            "shift" => "SHIFT".to_string(),
            "super" | "meta" | "win" | "cmd" => "LOGO".to_string(),
            "space" => "space".to_string(),
            other => other.to_ascii_lowercase(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Manages a background D-Bus XDG `org.freedesktop.portal.GlobalShortcuts` session on Wayland.
pub struct PortalShortcutsSession {
    stop_flag: Arc<AtomicBool>,
    session_path: Arc<std::sync::Mutex<Option<String>>>,
}

impl PortalShortcutsSession {
    /// Starts an XDG GlobalShortcuts portal session asynchronously on a background thread so
    /// desktop startup and KDE permission dialogs never block the UI event loop.
    /// Returns `(Some(session), initial_status)` immediately; live approval/denial updates arrive
    /// via `AppEvent::PortalShortcutsStatusUpdated`.
    pub fn try_start(
        cfg: &AppConfig,
        proxy: EventLoopProxy<AppEvent>,
    ) -> (Option<Self>, HotkeyStatus) {
        let (specs, conflicts) = build_portal_specs_checked(cfg);
        let initial_status = portal_status_from_cfg(cfg, true, None, &conflicts);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop_flag);
        let session_path = Arc::new(std::sync::Mutex::new(None));
        let session_slot = Arc::clone(&session_path);
        let cfg_clone = cfg.clone();

        thread::Builder::new()
            .name("geminidesk-xdg-shortcuts".into())
            .spawn(move || {
                run_portal_lifecycle(cfg_clone, specs, conflicts, proxy, stop_clone, session_slot);
            })
            .ok();

        (
            Some(Self {
                stop_flag,
                session_path,
            }),
            initial_status,
        )
    }
}

impl Drop for PortalShortcutsSession {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Ok(mut guard) = self.session_path.lock()
            && let Some(path) = guard.take()
        {
            close_portal_session_best_effort(&path);
        }
    }
}

fn close_portal_session_best_effort(session_path: &str) {
    if let Ok(conn) = zbus::blocking::Connection::session()
        && let Ok(sess_proxy) = zbus::blocking::Proxy::new(
            &conn,
            "org.freedesktop.portal.Desktop",
            session_path,
            "org.freedesktop.portal.Session",
        )
    {
        let _: Result<(), _> = sess_proxy.call("Close", &());
    }
}

#[derive(Debug, Clone)]
struct PortalShortcutSpec {
    id: &'static str,
    description: &'static str,
    trigger: String,
}

/// Builds XDG GlobalShortcuts binding specs while detecting duplicate trigger assignments
/// across enabled actions so conflicts are reported clearly in Settings -> Shortcuts.
fn build_portal_specs_checked(
    cfg: &AppConfig,
) -> (Vec<PortalShortcutSpec>, HashMap<&'static str, String>) {
    let mut out = Vec::new();
    let mut seen_triggers: HashMap<String, &'static str> = HashMap::new();
    let mut conflicts: HashMap<&'static str, String> = HashMap::new();

    let entries = [
        (
            "toggle-quick-chat",
            "Toggle GeminiDesk Quick Chat",
            &cfg.quick_chat_shortcut,
            true,
        ),
        (
            "show-full-window",
            "Show GeminiDesk Main Window",
            &cfg.full_window_shortcut,
            true,
        ),
        (
            "toggle-companion",
            "Toggle GeminiDesk Floating Companion",
            &cfg.companion_shortcut,
            true,
        ),
        (
            "ask-selection",
            "Ask Gemini with Selection",
            &cfg.ask_selection_shortcut,
            cfg.ask_selection_enabled,
        ),
        (
            "capture-screenshot",
            "Screenshot to Gemini",
            &cfg.screenshot_shortcut,
            cfg.screenshot_enabled,
        ),
        (
            "clipboard-actions",
            "GeminiDesk Clipboard Actions",
            &cfg.clipboard_actions_shortcut,
            cfg.clipboard_actions_enabled,
        ),
    ];

    for (id, description, spec, enabled) in entries {
        if !enabled || spec.trim().is_empty() {
            continue;
        }
        let trigger = to_xdg_trigger(spec);
        let norm_key = trigger.to_ascii_lowercase();
        if let Some(prev_id) = seen_triggers.get(&norm_key) {
            conflicts.insert(
                id,
                format!("'{spec}' conflicts with another GeminiDesk shortcut ({prev_id})"),
            );
            continue;
        }
        seen_triggers.insert(norm_key, id);
        out.push(PortalShortcutSpec {
            id,
            description,
            trigger,
        });
    }
    (out, conflicts)
}

fn portal_status_from_cfg(
    cfg: &AppConfig,
    registered: bool,
    global_error: Option<String>,
    conflicts: &HashMap<&'static str, String>,
) -> HotkeyStatus {
    let make = |id: &'static str, enabled: bool, spec: &str| {
        if !enabled {
            SingleHotkeyStatus {
                registered: false,
                enabled: false,
                error: None,
            }
        } else if spec.trim().is_empty() {
            SingleHotkeyStatus {
                registered: false,
                enabled: true,
                error: None,
            }
        } else if let Some(conflict_msg) = conflicts.get(id) {
            SingleHotkeyStatus {
                registered: false,
                enabled: true,
                error: Some(conflict_msg.clone()),
            }
        } else {
            SingleHotkeyStatus {
                registered,
                enabled: true,
                error: global_error.clone(),
            }
        }
    };
    HotkeyStatus {
        quick_chat: make("toggle-quick-chat", true, &cfg.quick_chat_shortcut),
        full_window: make("show-full-window", true, &cfg.full_window_shortcut),
        companion: make("toggle-companion", true, &cfg.companion_shortcut),
        ask_selection: make(
            "ask-selection",
            cfg.ask_selection_enabled,
            &cfg.ask_selection_shortcut,
        ),
        screenshot: make(
            "capture-screenshot",
            cfg.screenshot_enabled,
            &cfg.screenshot_shortcut,
        ),
        clipboard_actions: make(
            "clipboard-actions",
            cfg.clipboard_actions_enabled,
            &cfg.clipboard_actions_shortcut,
        ),
    }
}

fn portal_sender_token(conn: &zbus::blocking::Connection) -> String {
    conn.unique_name()
        .map(|n| n.as_str().trim_start_matches(':').replace('.', "_"))
        .unwrap_or_else(|| "geminidesk".to_string())
}

fn unique_token(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}_{}_{nanos}", std::process::id())
}

fn run_portal_lifecycle(
    cfg: AppConfig,
    specs: Vec<PortalShortcutSpec>,
    conflicts: HashMap<&'static str, String>,
    proxy: EventLoopProxy<AppEvent>,
    stop_flag: Arc<AtomicBool>,
    session_slot: Arc<std::sync::Mutex<Option<String>>>,
) {
    if stop_flag.load(Ordering::Relaxed) {
        return;
    }

    let (conn, session_path, bind_req_path) = match connect_and_bind_portal(&specs) {
        Ok(res) => res,
        Err(reason) => {
            warn!(
                desktop = %desktop_environment(),
                %reason,
                "XDG GlobalShortcuts portal unavailable; bind desktop shortcuts to CLI flags"
            );
            let hint = format!(
                "Wayland compositor portal unavailable ({reason}). Bind desktop shortcuts to `gemini-desk --quick-chat` / `--companion`."
            );
            let status = portal_status_from_cfg(&cfg, false, Some(hint), &conflicts);
            let _ = proxy.send_event(AppEvent::PortalShortcutsStatusUpdated {
                status,
                permission_denied: false,
            });
            return;
        }
    };

    if stop_flag.load(Ordering::Relaxed) {
        close_portal_session_best_effort(&session_path);
        return;
    }

    if let Ok(mut guard) = session_slot.lock() {
        *guard = Some(session_path.clone());
    }

    info!(
        session = %session_path,
        desktop = %desktop_environment(),
        "Opened Wayland XDG GlobalShortcuts portal session"
    );

    // Monitor asynchronous BindShortcuts permission dialog response on a helper thread so
    // Activation signals are never delayed while waiting for user consent in KDE Plasma.
    if let Some(req_path) = bind_req_path {
        let cfg_resp = cfg.clone();
        let conflicts_resp = conflicts.clone();
        let proxy_resp = proxy.clone();
        let stop_resp = Arc::clone(&stop_flag);
        thread::Builder::new()
            .name("geminidesk-xdg-bind-resp".into())
            .spawn(move || {
                monitor_bind_response(&req_path, &cfg_resp, &conflicts_resp, proxy_resp, stop_resp);
            })
            .ok();
    }

    run_portal_signal_loop(conn, session_path, proxy, stop_flag);
}

fn monitor_bind_response(
    req_path: &str,
    cfg: &AppConfig,
    conflicts: &HashMap<&'static str, String>,
    proxy: EventLoopProxy<AppEvent>,
    stop_flag: Arc<AtomicBool>,
) {
    let Ok(conn) = zbus::blocking::Connection::session() else {
        return;
    };
    let Ok(req_proxy) = zbus::blocking::Proxy::new(
        &conn,
        "org.freedesktop.portal.Desktop",
        req_path,
        "org.freedesktop.portal.Request",
    ) else {
        return;
    };
    let Ok(mut responses) = req_proxy.receive_signal("Response") else {
        return;
    };

    if let Some(resp_msg) = responses.next() {
        if stop_flag.load(Ordering::Relaxed) {
            return;
        }
        if let Ok((code, _results)) = resp_msg
            .body()
            .deserialize::<(u32, HashMap<String, zbus::zvariant::OwnedValue>)>()
        {
            match code {
                0 => {
                    info!("XDG GlobalShortcuts permission approved by desktop environment");
                    let status = portal_status_from_cfg(cfg, true, None, conflicts);
                    let _ = proxy.send_event(AppEvent::PortalShortcutsStatusUpdated {
                        status,
                        permission_denied: false,
                    });
                }
                1 => {
                    warn!("XDG GlobalShortcuts permission denied/cancelled by user");
                    let hint = "Global shortcut permission was declined in KDE/desktop prompt. You can re-enable shortcuts in System Settings -> Shortcuts or bind `gemini-desk --quick-chat`.".to_string();
                    let status = portal_status_from_cfg(cfg, false, Some(hint), conflicts);
                    let _ = proxy.send_event(AppEvent::PortalShortcutsStatusUpdated {
                        status,
                        permission_denied: true,
                    });
                }
                other => {
                    warn!(
                        code = other,
                        "XDG GlobalShortcuts BindShortcuts returned error"
                    );
                    let hint = format!(
                        "Global shortcut registration returned portal code {other}. Configure shortcuts in System Settings -> Shortcuts."
                    );
                    let status = portal_status_from_cfg(cfg, false, Some(hint), conflicts);
                    let _ = proxy.send_event(AppEvent::PortalShortcutsStatusUpdated {
                        status,
                        permission_denied: false,
                    });
                }
            }
        }
    }
}

fn connect_and_bind_portal(
    specs: &[PortalShortcutSpec],
) -> Result<(zbus::blocking::Connection, String, Option<String>), String> {
    use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

    let conn = zbus::blocking::Connection::session()
        .map_err(|e| format!("D-Bus session unavailable: {e}"))?;

    // Register host app_id (`io.geminidesk.GeminiDesk`) with xdg-desktop-portal 1.20+
    // so KDE Plasma / KGlobalAccel attributes shortcuts to GeminiDesk rather than the parent shell.
    if let Ok(registry) = zbus::blocking::Proxy::new(
        &conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.host.portal.Registry",
    ) {
        let empty_opts: HashMap<&str, Value<'_>> = HashMap::new();
        let _: Result<(), _> = registry.call("Register", &(LINUX_APP_ID, empty_opts));
    }

    let portal = zbus::blocking::Proxy::new(
        &conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.GlobalShortcuts",
    )
    .map_err(|e| format!("GlobalShortcuts proxy failed: {e}"))?;

    let sender = portal_sender_token(&conn);
    let req_tok = unique_token("gd_cs");
    let sess_tok = unique_token("gd_sess");
    let expected_req_path = format!("/org/freedesktop/portal/desktop/request/{sender}/{req_tok}");

    let req_proxy = zbus::blocking::Proxy::new(
        &conn,
        "org.freedesktop.portal.Desktop",
        expected_req_path.as_str(),
        "org.freedesktop.portal.Request",
    )
    .map_err(|e| format!("Portal Request proxy failed: {e}"))?;
    let mut responses = req_proxy
        .receive_signal("Response")
        .map_err(|e| format!("Portal Response subscribe failed: {e}"))?;

    let mut create_opts: HashMap<&str, Value<'_>> = HashMap::new();
    create_opts.insert("handle_token", Value::from(req_tok.as_str()));
    create_opts.insert("session_handle_token", Value::from(sess_tok.as_str()));

    let _req_handle: OwnedObjectPath = portal
        .call("CreateSession", &(create_opts,))
        .map_err(|e| format!("CreateSession failed: {e}"))?;

    let resp_msg = responses
        .next()
        .ok_or_else(|| "No Response from GlobalShortcuts CreateSession".to_string())?;
    let (code, results): (u32, HashMap<String, OwnedValue>) = resp_msg
        .body()
        .deserialize()
        .map_err(|e| format!("Failed decoding CreateSession response: {e}"))?;
    if code != 0 {
        return Err(format!("CreateSession rejected with code {code}"));
    }

    let session_handle: String = results
        .get("session_handle")
        .and_then(|v| String::try_from(v.clone()).ok())
        .ok_or_else(|| "Missing session_handle in portal response".to_string())?;

    let mut bind_req_path = None;
    if !specs.is_empty() {
        let mut shortcuts_payload: Vec<(&str, HashMap<&str, Value<'_>>)> =
            Vec::with_capacity(specs.len());
        for s in specs {
            let mut props: HashMap<&str, Value<'_>> = HashMap::new();
            props.insert("description", Value::from(s.description));
            if !s.trigger.is_empty() {
                props.insert("preferred_trigger", Value::from(s.trigger.as_str()));
            }
            shortcuts_payload.push((s.id, props));
        }

        let bind_tok = unique_token("gd_bind");
        let mut bind_opts: HashMap<&str, Value<'_>> = HashMap::new();
        bind_opts.insert("handle_token", Value::from(bind_tok.as_str()));

        let sess_obj = ObjectPath::try_from(session_handle.as_str())
            .map_err(|e| format!("Invalid session ObjectPath: {e}"))?;

        let bind_handle: OwnedObjectPath = portal
            .call(
                "BindShortcuts",
                &(sess_obj, shortcuts_payload, "", bind_opts),
            )
            .map_err(|e| format!("BindShortcuts failed: {e}"))?;
        bind_req_path = Some(bind_handle.as_str().to_string());
    }

    Ok((conn, session_handle, bind_req_path))
}

fn run_portal_signal_loop(
    conn: zbus::blocking::Connection,
    session_path: String,
    proxy: EventLoopProxy<AppEvent>,
    stop_flag: Arc<AtomicBool>,
) {
    let Ok(portal) = zbus::blocking::Proxy::new(
        &conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.GlobalShortcuts",
    ) else {
        return;
    };

    let Ok(signals) = portal.receive_signal("Activated") else {
        return;
    };

    for msg in signals {
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }
        if let Ok((sig_session, shortcut_id, _timestamp, _options)) = msg.body().deserialize::<(
            zbus::zvariant::OwnedObjectPath,
            String,
            u64,
            HashMap<String, zbus::zvariant::OwnedValue>,
        )>() && sig_session.as_str() == session_path
        {
            let action = match shortcut_id.as_str() {
                "toggle-quick-chat" => Some(HotkeyAction::ToggleQuickChat),
                "show-full-window" => Some(HotkeyAction::ShowFullWindow),
                "toggle-companion" => Some(HotkeyAction::ToggleCompanion),
                "ask-selection" => Some(HotkeyAction::AskSelection),
                "capture-screenshot" => Some(HotkeyAction::CaptureScreenshot),
                "clipboard-actions" => Some(HotkeyAction::ClipboardActions),
                _ => None,
            };
            if let Some(act) = action {
                let _ = proxy.send_event(AppEvent::PortalHotkeyActivated(act));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_shortcut_specs_to_xdg_trigger_format() {
        assert_eq!(to_xdg_trigger("Alt+Space"), "ALT+space");
        assert_eq!(to_xdg_trigger("Alt+Shift+Space"), "ALT+SHIFT+space");
        assert_eq!(to_xdg_trigger("Ctrl+Super+G"), "CTRL+LOGO+g");
    }

    #[test]
    fn detects_duplicate_portal_shortcut_conflicts() {
        let cfg = AppConfig {
            quick_chat_shortcut: "Alt+Space".into(),
            full_window_shortcut: "Alt+Space".into(),
            ..Default::default()
        };

        let (specs, conflicts) = build_portal_specs_checked(&cfg);
        assert!(conflicts.contains_key("show-full-window"));
        assert!(!specs.iter().any(|s| s.id == "show-full-window"));

        let status = portal_status_from_cfg(&cfg, true, None, &conflicts);
        assert!(status.quick_chat.registered);
        assert!(!status.full_window.registered);
        assert!(status.full_window.error.is_some());
    }
}
