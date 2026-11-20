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

use gtk::prelude::{GtkSettingsExt, GtkWindowExt, WidgetExt};
use tao::event_loop::EventLoopProxy;
use tao::platform::unix::WindowExtUnix;
use tao::window::Window;
use tracing::{info, warn};

use crate::app::AppEvent;
use crate::config::AppConfig;
use crate::hotkeys::{HotkeyAction, HotkeyStatus, SingleHotkeyStatus};
use crate::platform::ActivationTarget;

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

pub fn style_main_window(window: &Window, dark: bool) {
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(dark);
    }
    let gtk_win = window.gtk_window();
    gtk_win.set_role("geminidesk-main");
}

pub fn style_quick_chat_window(window: &Window, dark: bool) {
    style_main_window(window, dark);
    let gtk_win = window.gtk_window();
    gtk_win.set_role("geminidesk-quick-chat");
    gtk_win.set_skip_taskbar_hint(true);
    gtk_win.set_skip_pager_hint(true);
    gtk_win.set_keep_above(true);
}

pub fn set_window_opacity(window: &Window, opacity_pct: u8) {
    let alpha = (opacity_pct.clamp(55, 100) as f64) / 100.0;
    window.gtk_window().set_opacity(alpha);
}

pub fn focus_window(window: &Window) {
    let gtk_win = window.gtk_window();
    gtk_win.deiconify();
    gtk_win.present();
    window.set_focus();
}

pub fn is_window_still_active_or_owned(window: &Window) -> bool {
    window.gtk_window().is_active()
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
             Comment=Unofficial Desktop Client for Google Gemini\n\
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

fn encode_target(target: ActivationTarget) -> &'static [u8] {
    match target {
        ActivationTarget::MainWindow => b"main\n",
        ActivationTarget::QuickChat => b"quick_chat\n",
        ActivationTarget::Companion => b"companion\n",
        ActivationTarget::Settings => b"settings\n",
        ActivationTarget::ClipboardActions => b"clipboard_actions\n",
        ActivationTarget::AskSelection => b"ask_selection\n",
        ActivationTarget::Screenshot => b"screenshot\n",
    }
}

fn decode_target(line: &str) -> Option<AppEvent> {
    match line.trim() {
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
            let _ = stream.write_all(encode_target(target));
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
}

impl PortalShortcutsSession {
    /// Attempts to open an XDG GlobalShortcuts portal session and bind configured actions.
    /// Returns `(Some(session), status)` when the portal is available (e.g., KDE Plasma 6 Wayland),
    /// or `(None, fallback_status)` when the compositor does not expose the portal.
    pub fn try_start(
        cfg: &AppConfig,
        proxy: EventLoopProxy<AppEvent>,
    ) -> (Option<Self>, HotkeyStatus) {
        let specs = build_portal_specs(cfg);
        match connect_and_bind_portal(&specs) {
            Ok((conn, session_path)) => {
                info!(
                    session = %session_path,
                    desktop = %desktop_environment(),
                    "Bound Wayland global shortcuts via XDG GlobalShortcuts portal"
                );
                let stop_flag = Arc::new(AtomicBool::new(false));
                let stop_clone = Arc::clone(&stop_flag);

                thread::Builder::new()
                    .name("geminidesk-xdg-shortcuts".into())
                    .spawn(move || {
                        run_portal_signal_loop(conn, session_path, proxy, stop_clone);
                    })
                    .ok();

                let status = portal_status_from_cfg(cfg, true, None);
                (Some(Self { stop_flag }), status)
            }
            Err(reason) => {
                warn!(
                    desktop = %desktop_environment(),
                    %reason,
                    "XDG GlobalShortcuts portal unavailable; bind desktop shortcuts to CLI flags"
                );
                let hint = format!(
                    "Wayland compositor portal unavailable ({reason}). Bind desktop shortcuts to `gemini-desk --quick-chat` / `--companion`."
                );
                let status = portal_status_from_cfg(cfg, false, Some(hint));
                (None, status)
            }
        }
    }
}

impl Drop for PortalShortcutsSession {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

struct PortalShortcutSpec {
    id: &'static str,
    description: &'static str,
    trigger: String,
}

fn build_portal_specs(cfg: &AppConfig) -> Vec<PortalShortcutSpec> {
    let mut out = Vec::new();
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
        if enabled && !spec.trim().is_empty() {
            out.push(PortalShortcutSpec {
                id,
                description,
                trigger: to_xdg_trigger(spec),
            });
        }
    }
    out
}

fn portal_status_from_cfg(
    cfg: &AppConfig,
    registered: bool,
    error: Option<String>,
) -> HotkeyStatus {
    let make = |enabled: bool, spec: &str| {
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
        } else {
            SingleHotkeyStatus {
                registered,
                enabled: true,
                error: error.clone(),
            }
        }
    };
    HotkeyStatus {
        quick_chat: make(true, &cfg.quick_chat_shortcut),
        full_window: make(true, &cfg.full_window_shortcut),
        companion: make(true, &cfg.companion_shortcut),
        ask_selection: make(cfg.ask_selection_enabled, &cfg.ask_selection_shortcut),
        screenshot: make(cfg.screenshot_enabled, &cfg.screenshot_shortcut),
        clipboard_actions: make(
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

fn connect_and_bind_portal(
    specs: &[PortalShortcutSpec],
) -> Result<(zbus::blocking::Connection, String), String> {
    use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

    let conn = zbus::blocking::Connection::session()
        .map_err(|e| format!("D-Bus session unavailable: {e}"))?;

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

        let _bind_req: OwnedObjectPath = portal
            .call(
                "BindShortcuts",
                &(sess_obj, shortcuts_payload, "", bind_opts),
            )
            .map_err(|e| format!("BindShortcuts failed: {e}"))?;
    }

    Ok((conn, session_handle))
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
