use std::collections::HashMap;
use std::str::FromStr;

use global_hotkey::{GlobalHotKeyManager, hotkey::HotKey};
use serde::Serialize;
use tao::event_loop::EventLoopProxy;
use tracing::{info, warn};

use crate::app::AppEvent;
use crate::config::AppConfig;

/// Actions triggered by registered global hotkeys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyAction {
    ToggleQuickChat,
    ShowFullWindow,
    ToggleCompanion,
    AskSelection,
    CaptureScreenshot,
    ClipboardActions,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SingleHotkeyStatus {
    pub registered: bool,
    pub enabled: bool,
    pub error: Option<String>,
}

/// Live registration status for all configurable global shortcuts, surfaced to Settings UI.
#[derive(Debug, Clone, Default, Serialize)]
pub struct HotkeyStatus {
    pub quick_chat: SingleHotkeyStatus,
    pub full_window: SingleHotkeyStatus,
    pub companion: SingleHotkeyStatus,
    pub ask_selection: SingleHotkeyStatus,
    pub screenshot: SingleHotkeyStatus,
    pub clipboard_actions: SingleHotkeyStatus,
}

/// Manages global keyboard shortcuts with graceful OS conflict handling and live updates.
///
/// - **Windows / macOS / Linux X11:** Uses native OS global hotkeys via `global-hotkey`.
/// - **Linux Wayland (KDE Plasma 6 / Hyprland):** Uses the XDG `GlobalShortcuts` D-Bus portal,
///   with clear fallback instructions for compositors without portal support (e.g., GNOME Wayland).
pub struct HotkeyController {
    manager: Option<GlobalHotKeyManager>,
    registered: HashMap<u32, (HotkeyAction, HotKey)>,
    status: HotkeyStatus,
    #[cfg(target_os = "linux")]
    portal_session: Option<crate::platform::linux::PortalShortcutsSession>,
    #[cfg(target_os = "linux")]
    wayland_mode: bool,
    #[cfg(target_os = "linux")]
    proxy: Option<EventLoopProxy<AppEvent>>,
}

impl HotkeyController {
    pub fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        #[cfg(target_os = "linux")]
        let wayland_mode = crate::platform::linux::is_wayland_session();
        #[cfg(not(target_os = "linux"))]
        let _ = &proxy;

        let manager = {
            #[cfg(target_os = "linux")]
            if wayland_mode {
                None
            } else {
                match GlobalHotKeyManager::new() {
                    Ok(mgr) => Some(mgr),
                    Err(err) => {
                        warn!(?err, "GlobalHotKeyManager unavailable in this X11 session");
                        None
                    }
                }
            }
            #[cfg(not(target_os = "linux"))]
            match GlobalHotKeyManager::new() {
                Ok(mgr) => Some(mgr),
                Err(err) => {
                    warn!(?err, "GlobalHotKeyManager unavailable in this session");
                    None
                }
            }
        };

        Self {
            manager,
            registered: HashMap::new(),
            status: HotkeyStatus::default(),
            #[cfg(target_os = "linux")]
            portal_session: None,
            #[cfg(target_os = "linux")]
            wayland_mode,
            #[cfg(target_os = "linux")]
            proxy: Some(proxy),
        }
    }

    pub fn status(&self) -> &HotkeyStatus {
        &self.status
    }

    #[cfg(target_os = "linux")]
    pub fn set_portal_status(&mut self, status: HotkeyStatus) {
        self.status = status;
    }

    /// Parses a user-facing shortcut string (e.g. `"Alt+Space"`, `"Ctrl+Alt+G"`).
    /// Returns `Ok(None)` when the string is empty (shortcut disabled).
    pub fn parse_shortcut(spec: &str) -> Result<Option<HotKey>, String> {
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        let normalized = trimmed
            .split('+')
            .map(|part| match part.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "control" => "Control".to_string(),
                "alt" | "option" => "Alt".to_string(),
                "shift" => "Shift".to_string(),
                "win" | "super" | "meta" | "cmd" | "command" => "Super".to_string(),
                "space" => "Space".to_string(),
                other => other.to_ascii_uppercase(),
            })
            .collect::<Vec<_>>()
            .join("+");

        HotKey::from_str(&normalized)
            .map(Some)
            .map_err(|err| format!("Invalid shortcut '{trimmed}': {err}"))
    }

    /// Applies all shortcut bindings from `AppConfig`, unregistering previous bindings first.
    /// Sensitive capture shortcuts (Selection, Screenshot, Clipboard Actions) are ONLY
    /// registered when their corresponding opt-in feature toggle is enabled.
    pub fn apply_from_config(&mut self, cfg: &AppConfig) -> HotkeyStatus {
        self.unregister_all();

        #[cfg(target_os = "linux")]
        if self.wayland_mode {
            self.portal_session = None;
            if let Some(proxy) = self.proxy.clone() {
                let (session, status) =
                    crate::platform::linux::PortalShortcutsSession::try_start(cfg, proxy);
                self.portal_session = session;
                self.status = status.clone();
                return status;
            }
        }

        let quick_chat = self.register_one(
            HotkeyAction::ToggleQuickChat,
            &cfg.quick_chat_shortcut,
            true,
        );
        let full_window = self.register_one(
            HotkeyAction::ShowFullWindow,
            &cfg.full_window_shortcut,
            true,
        );
        let companion =
            self.register_one(HotkeyAction::ToggleCompanion, &cfg.companion_shortcut, true);
        let ask_selection = self.register_one(
            HotkeyAction::AskSelection,
            &cfg.ask_selection_shortcut,
            cfg.ask_selection_enabled,
        );
        let screenshot = self.register_one(
            HotkeyAction::CaptureScreenshot,
            &cfg.screenshot_shortcut,
            cfg.screenshot_enabled,
        );
        let clipboard_actions = self.register_one(
            HotkeyAction::ClipboardActions,
            &cfg.clipboard_actions_shortcut,
            cfg.clipboard_actions_enabled,
        );

        let status = HotkeyStatus {
            quick_chat,
            full_window,
            companion,
            ask_selection,
            screenshot,
            clipboard_actions,
        };
        self.status = status.clone();
        status
    }

    fn register_one(
        &mut self,
        action: HotkeyAction,
        spec: &str,
        enabled: bool,
    ) -> SingleHotkeyStatus {
        if !enabled {
            return SingleHotkeyStatus {
                registered: false,
                enabled: false,
                error: None,
            };
        }

        let Some(manager) = self.manager.as_ref() else {
            return SingleHotkeyStatus {
                registered: false,
                enabled: true,
                error: Some("Global hotkeys are unavailable in this desktop session".into()),
            };
        };

        match Self::parse_shortcut(spec) {
            Ok(Some(hk)) => {
                if self.registered.contains_key(&hk.id()) {
                    let msg = format!("'{spec}' is already assigned to another GeminiDesk action");
                    warn!(?action, "{msg}");
                    return SingleHotkeyStatus {
                        registered: false,
                        enabled: true,
                        error: Some(msg),
                    };
                }
                match manager.register(hk) {
                    Ok(()) => {
                        info!(?action, shortcut = %spec, id = hk.id(), "Registered global hotkey");
                        self.registered.insert(hk.id(), (action, hk));
                        SingleHotkeyStatus {
                            registered: true,
                            enabled: true,
                            error: None,
                        }
                    }
                    Err(err) => {
                        let msg =
                            format!("Conflict on '{spec}' (likely in use by another app): {err}");
                        warn!(?action, "{msg}");
                        SingleHotkeyStatus {
                            registered: false,
                            enabled: true,
                            error: Some(msg),
                        }
                    }
                }
            }
            Ok(None) => SingleHotkeyStatus {
                registered: false,
                enabled: true,
                error: None,
            },
            Err(err) => {
                warn!(?action, "{err}");
                SingleHotkeyStatus {
                    registered: false,
                    enabled: true,
                    error: Some(err),
                }
            }
        }
    }

    pub fn action_for_id(&self, id: u32) -> Option<HotkeyAction> {
        self.registered.get(&id).map(|(action, _)| *action)
    }

    pub fn unregister_all(&mut self) {
        if let Some(manager) = self.manager.as_ref() {
            for (_, (_, hk)) in self.registered.drain() {
                let _ = manager.unregister(hk);
            }
        } else {
            self.registered.clear();
        }
    }
}

impl Drop for HotkeyController {
    fn drop(&mut self) {
        self.unregister_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_and_custom_shortcuts() {
        let q = HotkeyController::parse_shortcut("Alt+Space")
            .expect("Alt+Space valid")
            .expect("Some hotkey");
        let f = HotkeyController::parse_shortcut("Alt+Shift+Space")
            .expect("Alt+Shift+Space valid")
            .expect("Some hotkey");
        let s = HotkeyController::parse_shortcut("Alt+Shift+S")
            .expect("Alt+Shift+S valid")
            .expect("Some hotkey");
        assert_ne!(q.id(), f.id());
        assert_ne!(f.id(), s.id());

        assert!(
            HotkeyController::parse_shortcut("Ctrl+Alt+G")
                .expect("Ctrl+Alt+G valid")
                .is_some()
        );
        assert!(
            HotkeyController::parse_shortcut("   ")
                .expect("empty disables")
                .is_none()
        );
    }

    #[test]
    fn rejects_invalid_shortcut_specs() {
        assert!(HotkeyController::parse_shortcut("NotAModifier+InvalidKey999").is_err());
    }
}
