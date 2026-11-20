use std::str::FromStr;

use global_hotkey::{GlobalHotKeyManager, hotkey::HotKey};
use tracing::{info, warn};

/// Actions triggered by registered global hotkeys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    ToggleQuickChat,
    ShowFullWindow,
}

/// Live registration status for both global shortcuts, surfaced to Settings UI.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct HotkeyStatus {
    pub quick_chat_registered: bool,
    pub quick_chat_error: Option<String>,
    pub full_window_registered: bool,
    pub full_window_error: Option<String>,
}

/// Manages global keyboard shortcuts with graceful conflict handling and live updates.
pub struct HotkeyController {
    manager: Option<GlobalHotKeyManager>,
    quick_chat_hotkey: Option<HotKey>,
    full_window_hotkey: Option<HotKey>,
    status: HotkeyStatus,
}

impl HotkeyController {
    pub fn new() -> Self {
        let manager = match GlobalHotKeyManager::new() {
            Ok(mgr) => Some(mgr),
            Err(err) => {
                warn!(
                    ?err,
                    "GlobalHotKeyManager unavailable on this platform/session"
                );
                None
            }
        };

        Self {
            manager,
            quick_chat_hotkey: None,
            full_window_hotkey: None,
            status: HotkeyStatus::default(),
        }
    }

    pub fn status(&self) -> &HotkeyStatus {
        &self.status
    }

    /// Parses a user-facing shortcut string (e.g. `"Alt+Space"`, `"Ctrl+Alt+G"`).
    /// Returns `Ok(None)` when the string is empty (shortcut disabled).
    pub fn parse_shortcut(spec: &str) -> Result<Option<HotKey>, String> {
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        // Normalize common aliases so users can type `Ctrl+Alt+G` or `Control+Alt+G`.
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

    /// Applies updated shortcut strings, unregistering previous bindings first.
    /// Conflicts with other OS apps (e.g. PowerToys Run on `Alt+Space`) are handled
    /// gracefully and reported in `HotkeyStatus` without crashing.
    pub fn apply(&mut self, quick_chat_spec: &str, full_window_spec: &str) -> HotkeyStatus {
        self.unregister_all();

        let mut status = HotkeyStatus::default();
        let Some(manager) = self.manager.as_ref() else {
            let msg = "Global hotkeys are unavailable in this desktop session".to_string();
            status.quick_chat_error = Some(msg.clone());
            status.full_window_error = Some(msg);
            self.status = status.clone();
            return status;
        };

        // 1. Quick Chat shortcut
        match Self::parse_shortcut(quick_chat_spec) {
            Ok(Some(hk)) => match manager.register(hk) {
                Ok(()) => {
                    info!(shortcut = %quick_chat_spec, id = hk.id(), "Registered Quick Chat hotkey");
                    self.quick_chat_hotkey = Some(hk);
                    status.quick_chat_registered = true;
                }
                Err(err) => {
                    let msg = format!(
                        "Could not register '{quick_chat_spec}' (likely used by another app): {err}"
                    );
                    warn!("{msg}");
                    status.quick_chat_error = Some(msg);
                }
            },
            Ok(None) => {
                info!("Quick Chat global shortcut disabled");
            }
            Err(err) => {
                warn!("{err}");
                status.quick_chat_error = Some(err);
            }
        }

        // 2. Full Window shortcut
        match Self::parse_shortcut(full_window_spec) {
            Ok(Some(hk)) => {
                if self.quick_chat_hotkey.is_some_and(|q| q.id() == hk.id()) {
                    let msg = "Full Window shortcut cannot be identical to Quick Chat shortcut"
                        .to_string();
                    warn!("{msg}");
                    status.full_window_error = Some(msg);
                } else {
                    match manager.register(hk) {
                        Ok(()) => {
                            info!(
                                shortcut = %full_window_spec,
                                id = hk.id(),
                                "Registered Full Window hotkey"
                            );
                            self.full_window_hotkey = Some(hk);
                            status.full_window_registered = true;
                        }
                        Err(err) => {
                            let msg = format!(
                                "Could not register '{full_window_spec}' (likely used by another app): {err}"
                            );
                            warn!("{msg}");
                            status.full_window_error = Some(msg);
                        }
                    }
                }
            }
            Ok(None) => {
                info!("Full Window global shortcut disabled");
            }
            Err(err) => {
                warn!("{err}");
                status.full_window_error = Some(err);
            }
        }

        self.status = status.clone();
        status
    }

    pub fn action_for_id(&self, id: u32) -> Option<HotkeyAction> {
        if self.quick_chat_hotkey.is_some_and(|hk| hk.id() == id) {
            return Some(HotkeyAction::ToggleQuickChat);
        }
        if self.full_window_hotkey.is_some_and(|hk| hk.id() == id) {
            return Some(HotkeyAction::ShowFullWindow);
        }
        None
    }

    pub fn unregister_all(&mut self) {
        if let Some(manager) = self.manager.as_ref() {
            if let Some(hk) = self.quick_chat_hotkey.take() {
                let _ = manager.unregister(hk);
            }
            if let Some(hk) = self.full_window_hotkey.take() {
                let _ = manager.unregister(hk);
            }
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
        assert_ne!(q.id(), f.id());

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
