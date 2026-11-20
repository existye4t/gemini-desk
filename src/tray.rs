use muda::{Menu, MenuItem, PredefinedMenuItem};
use tracing::warn;
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::config::AppConfig;
use crate::icon;

pub const MENU_OPEN_GEMINI: &str = "open_gemini";
pub const MENU_QUICK_CHAT: &str = "quick_chat";
pub const MENU_COMPANION: &str = "companion";
pub const MENU_NEW_CHAT: &str = "new_chat";
pub const MENU_CAPTURE_SCREENSHOT: &str = "capture_screenshot";
pub const MENU_CLIPBOARD_ACTIONS: &str = "clipboard_actions";
pub const MENU_ACCOUNT: &str = "account";
#[allow(dead_code)]
pub const MENU_AUTH_BACK: &str = "auth_back";
#[allow(dead_code)]
pub const MENU_AUTH_CANCEL: &str = "auth_cancel";
pub const MENU_SETTINGS: &str = "settings";
pub const MENU_QUIT: &str = "quit";

/// Persistent system tray icon and native context menu.
pub struct TrayController {
    _tray_icon: TrayIcon,
    quick_chat_item: MenuItem,
    companion_item: MenuItem,
    screenshot_item: MenuItem,
    clipboard_item: MenuItem,
}

impl TrayController {
    pub fn new(cfg: &AppConfig) -> Result<Self, tray_icon::Error> {
        let menu = Menu::new();

        let open_item = MenuItem::with_id(MENU_OPEN_GEMINI, "Open Gemini", true, None);
        let quick_chat_item = MenuItem::with_id(
            MENU_QUICK_CHAT,
            format_shortcut_label("Quick Chat", &cfg.quick_chat_shortcut),
            true,
            None,
        );
        let companion_item = MenuItem::with_id(
            MENU_COMPANION,
            format_shortcut_label("Floating Companion", &cfg.companion_shortcut),
            true,
            None,
        );
        let new_chat_item = MenuItem::with_id(MENU_NEW_CHAT, "New Chat", true, None);

        let screenshot_item = MenuItem::with_id(
            MENU_CAPTURE_SCREENSHOT,
            format_optional_shortcut_label(
                "Capture Screenshot",
                &cfg.screenshot_shortcut,
                cfg.screenshot_enabled,
            ),
            true,
            None,
        );
        let clipboard_item = MenuItem::with_id(
            MENU_CLIPBOARD_ACTIONS,
            format_optional_shortcut_label(
                "Clipboard Actions",
                &cfg.clipboard_actions_shortcut,
                cfg.clipboard_actions_enabled,
            ),
            true,
            None,
        );

        let account_item =
            MenuItem::with_id(MENU_ACCOUNT, "Google Account & Sign-In...", true, None);
        let settings_item = MenuItem::with_id(MENU_SETTINGS, "Settings...", true, None);
        let quit_item = MenuItem::with_id(MENU_QUIT, "Quit GeminiDesk", true, None);

        let _ = menu.append(&open_item);
        let _ = menu.append(&quick_chat_item);
        let _ = menu.append(&companion_item);
        let _ = menu.append(&new_chat_item);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&screenshot_item);
        let _ = menu.append(&clipboard_item);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&account_item);
        let _ = menu.append(&settings_item);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&quit_item);

        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("GeminiDesk")
            .with_menu_on_left_click(false);

        if let Some(icon) = icon::tray_icon() {
            builder = builder.with_icon(icon);
        } else {
            warn!("Failed to build RGBA tray icon");
        }

        let tray_icon = builder.build()?;
        Ok(Self {
            _tray_icon: tray_icon,
            quick_chat_item,
            companion_item,
            screenshot_item,
            clipboard_item,
        })
    }

    pub fn sync_labels(&self, cfg: &AppConfig) {
        self.quick_chat_item.set_text(format_shortcut_label(
            "Quick Chat",
            &cfg.quick_chat_shortcut,
        ));
        self.companion_item.set_text(format_shortcut_label(
            "Floating Companion",
            &cfg.companion_shortcut,
        ));
        self.screenshot_item
            .set_text(format_optional_shortcut_label(
                "Capture Screenshot",
                &cfg.screenshot_shortcut,
                cfg.screenshot_enabled,
            ));
        self.clipboard_item.set_text(format_optional_shortcut_label(
            "Clipboard Actions",
            &cfg.clipboard_actions_shortcut,
            cfg.clipboard_actions_enabled,
        ));
    }
}

fn format_shortcut_label(title: &str, shortcut: &str) -> String {
    let trimmed = shortcut.trim();
    if trimmed.is_empty() {
        title.to_string()
    } else {
        format!("{title}  ({trimmed})")
    }
}

fn format_optional_shortcut_label(title: &str, shortcut: &str, enabled: bool) -> String {
    if enabled {
        format_shortcut_label(title, shortcut)
    } else {
        title.to_string()
    }
}
