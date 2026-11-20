use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use tracing::warn;

/// Default global shortcuts.
pub const DEFAULT_QUICK_CHAT_SHORTCUT: &str = "Alt+Space";
pub const DEFAULT_FULL_WINDOW_SHORTCUT: &str = "Alt+Shift+Space";
pub const DEFAULT_COMPANION_SHORTCUT: &str = "Alt+Shift+G";
pub const DEFAULT_ASK_SELECTION_SHORTCUT: &str = "Alt+Shift+A";
pub const DEFAULT_SCREENSHOT_SHORTCUT: &str = "Alt+Shift+S";
pub const DEFAULT_CLIPBOARD_ACTIONS_SHORTCUT: &str = "Alt+Shift+C";

/// Behavior when the user clicks the main window close button (`X`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehavior {
    #[default]
    MinimizeToTray,
    Quit,
}

/// Visual theme preference for the native shell and embedded WebView.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    System,
    Dark,
    Light,
}

/// Display placement preset for the Quick Chat overlay on the active monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum QuickChatPosition {
    #[default]
    TopCenter,
    Center,
    TopRight,
    BottomRight,
}

/// Real resource-management mode controlling WebView2 memory target & background suspension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceMode {
    Efficiency,
    #[default]
    Balanced,
    Performance,
}

/// User-editable prompt template for Clipboard Actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipboardActionTemplate {
    pub id: String,
    pub label: String,
    /// Prompt template containing `{{text}}` where the clipboard text will be substituted.
    pub prompt: String,
}

pub fn default_clipboard_templates() -> Vec<ClipboardActionTemplate> {
    vec![
        ClipboardActionTemplate {
            id: "explain".into(),
            label: "Explain".into(),
            prompt: "Explain this clearly and concisely:\n\n{{text}}".into(),
        },
        ClipboardActionTemplate {
            id: "summarize".into(),
            label: "Summarize".into(),
            prompt: "Summarize the key points from the following text:\n\n{{text}}".into(),
        },
        ClipboardActionTemplate {
            id: "translate".into(),
            label: "Translate".into(),
            prompt: "Translate the following text into clear, natural English (or if already English, clarify its meaning):\n\n{{text}}".into(),
        },
        ClipboardActionTemplate {
            id: "fix_grammar".into(),
            label: "Fix grammar".into(),
            prompt: "Fix grammar, spelling, and punctuation while preserving the original meaning and tone:\n\n{{text}}".into(),
        },
        ClipboardActionTemplate {
            id: "improve_writing".into(),
            label: "Improve writing".into(),
            prompt: "Rewrite the following text so it is clearer, more concise, and better structured:\n\n{{text}}".into(),
        },
        ClipboardActionTemplate {
            id: "explain_code".into(),
            label: "Explain code".into(),
            prompt: "Explain what this code does, step by step, and highlight any subtle edge cases:\n\n```\n{{text}}\n```".into(),
        },
        ClipboardActionTemplate {
            id: "debug_error".into(),
            label: "Debug error".into(),
            prompt: "Diagnose this error, explain the likely root cause, and suggest a concrete fix:\n\n```\n{{text}}\n```".into(),
        },
    ]
}

fn default_quick_chat_width() -> u32 {
    780
}
fn default_quick_chat_height() -> u32 {
    580
}
fn default_companion_opacity() -> u8 {
    100
}
fn default_true() -> bool {
    true
}
fn default_idle_suspend_secs() -> u32 {
    60
}
fn default_clipboard_max_chars() -> usize {
    24_000
}
fn default_companion_shortcut() -> String {
    DEFAULT_COMPANION_SHORTCUT.to_string()
}
fn default_selection_shortcut() -> String {
    DEFAULT_ASK_SELECTION_SHORTCUT.to_string()
}
fn default_screenshot_shortcut() -> String {
    DEFAULT_SCREENSHOT_SHORTCUT.to_string()
}
fn default_clipboard_shortcut() -> String {
    DEFAULT_CLIPBOARD_ACTIONS_SHORTCUT.to_string()
}

/// User-configurable application settings persisted in `config.toml`.
/// Every field uses `#[serde(default)]` to preserve 100% compatibility with v0.2.0 configs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    // --- General & Appearance (preserved from v0.2.0) ---
    pub launch_at_startup: bool,
    pub minimize_to_tray: bool,
    pub close_behavior: CloseBehavior,
    pub theme: ThemePreference,

    // --- Shortcuts ---
    pub quick_chat_shortcut: String,
    pub full_window_shortcut: String,
    #[serde(default = "default_companion_shortcut")]
    pub companion_shortcut: String,
    #[serde(default = "default_selection_shortcut")]
    pub ask_selection_shortcut: String,
    #[serde(default = "default_screenshot_shortcut")]
    pub screenshot_shortcut: String,
    #[serde(default = "default_clipboard_shortcut")]
    pub clipboard_actions_shortcut: String,

    // --- Quick Chat ---
    pub quick_chat_hide_on_blur: bool,
    #[serde(default = "default_quick_chat_width")]
    pub quick_chat_width: u32,
    #[serde(default = "default_quick_chat_height")]
    pub quick_chat_height: u32,
    pub quick_chat_position: QuickChatPosition,

    // --- Floating Companion ---
    #[serde(default = "default_true")]
    pub companion_always_on_top: bool,
    #[serde(default = "default_companion_opacity")]
    pub companion_opacity: u8,
    #[serde(default = "default_true")]
    pub companion_snap_to_edges: bool,

    // --- Productivity features (sensitive OS capture hotkeys default OFF for least privilege) ---
    pub ask_selection_enabled: bool,
    #[serde(default = "default_true")]
    pub ask_selection_clipboard_fallback: bool,
    pub screenshot_enabled: bool,
    #[serde(default = "default_true")]
    pub screenshot_copy_to_clipboard: bool,
    pub clipboard_actions_enabled: bool,
    #[serde(default = "default_clipboard_max_chars")]
    pub clipboard_max_chars: usize,
    #[serde(default = "default_clipboard_templates")]
    pub clipboard_templates: Vec<ClipboardActionTemplate>,

    // --- Performance ---
    pub performance_mode: PerformanceMode,
    /// Legacy v0.2.0 field retained for backwards compatibility; synchronized with `performance_mode`.
    pub reduce_memory_when_hidden: bool,
    #[serde(default = "default_idle_suspend_secs")]
    pub efficiency_suspend_delay_secs: u32,

    // --- Software Updates (v0.4.0, user-controlled — never silently installs major upgrades) ---
    #[serde(default = "default_true")]
    pub auto_check_updates: bool,
    pub auto_download_updates: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            launch_at_startup: false,
            minimize_to_tray: false,
            close_behavior: CloseBehavior::MinimizeToTray,
            theme: ThemePreference::System,

            quick_chat_shortcut: DEFAULT_QUICK_CHAT_SHORTCUT.to_string(),
            full_window_shortcut: DEFAULT_FULL_WINDOW_SHORTCUT.to_string(),
            companion_shortcut: DEFAULT_COMPANION_SHORTCUT.to_string(),
            ask_selection_shortcut: DEFAULT_ASK_SELECTION_SHORTCUT.to_string(),
            screenshot_shortcut: DEFAULT_SCREENSHOT_SHORTCUT.to_string(),
            clipboard_actions_shortcut: DEFAULT_CLIPBOARD_ACTIONS_SHORTCUT.to_string(),

            quick_chat_hide_on_blur: true,
            quick_chat_width: default_quick_chat_width(),
            quick_chat_height: default_quick_chat_height(),
            quick_chat_position: QuickChatPosition::TopCenter,

            companion_always_on_top: true,
            companion_opacity: default_companion_opacity(),
            companion_snap_to_edges: true,

            // Opt-in by default since these read foreground selection / screen pixels / clipboard.
            ask_selection_enabled: false,
            ask_selection_clipboard_fallback: true,
            screenshot_enabled: false,
            screenshot_copy_to_clipboard: true,
            clipboard_actions_enabled: false,
            clipboard_max_chars: default_clipboard_max_chars(),
            clipboard_templates: default_clipboard_templates(),

            performance_mode: PerformanceMode::Balanced,
            reduce_memory_when_hidden: true,
            efficiency_suspend_delay_secs: default_idle_suspend_secs(),

            auto_check_updates: true,
            auto_download_updates: false,
        }
    }
}

impl AppConfig {
    /// Clamps numeric fields to safe UI/OS bounds.
    pub fn sanitize(&mut self) {
        self.quick_chat_width = self.quick_chat_width.clamp(520, 1400);
        self.quick_chat_height = self.quick_chat_height.clamp(420, 1000);
        self.companion_opacity = self.companion_opacity.clamp(55, 100);
        self.clipboard_max_chars = self.clipboard_max_chars.clamp(1_000, 100_000);
        self.efficiency_suspend_delay_secs = self.efficiency_suspend_delay_secs.clamp(15, 600);
        self.reduce_memory_when_hidden = self.performance_mode != PerformanceMode::Performance;
        if self.clipboard_templates.is_empty() {
            self.clipboard_templates = default_clipboard_templates();
        }
    }
}

/// Persisted physical geometry of a desktop window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

/// Persisted UI state stored separately from user settings (`state.toml`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    pub main_window: Option<WindowGeometry>,
    pub companion_window: Option<WindowGeometry>,
}

/// Standard OS directory layout for GeminiDesk.
/// Preserves the Phase B/C WebView2 profile directory:
/// `%LOCALAPPDATA%\geminidesk\GeminiDesk\data\webview`
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub webview_data_dir: PathBuf,
    pub updates_dir: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> Self {
        if let Some(proj) = ProjectDirs::from("dev", "geminidesk", "GeminiDesk") {
            let config_dir = proj.config_dir().to_path_buf();
            let webview_data_dir = proj.data_local_dir().join("webview");
            let updates_dir = proj.data_local_dir().join("updates");
            let _ = fs::create_dir_all(&config_dir);
            let _ = fs::create_dir_all(&webview_data_dir);
            Self {
                config_dir,
                webview_data_dir,
                updates_dir,
            }
        } else {
            let base = PathBuf::from(".geminidesk-data");
            let config_dir = base.join("config");
            let webview_data_dir = base.join("webview");
            let updates_dir = base.join("updates");
            let _ = fs::create_dir_all(&config_dir);
            let _ = fs::create_dir_all(&webview_data_dir);
            Self {
                config_dir,
                webview_data_dir,
                updates_dir,
            }
        }
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn state_file(&self) -> PathBuf {
        self.config_dir.join("state.toml")
    }
}

fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, contents)?;
    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        fs::write(path, contents).map_err(|_| err)?;
    }
    Ok(())
}

pub fn load_config(path: &Path) -> AppConfig {
    let mut cfg = match fs::read_to_string(path) {
        Ok(raw) => toml::from_str::<AppConfig>(&raw).unwrap_or_else(|err| {
            warn!(?err, ?path, "Failed to parse config.toml; using defaults");
            AppConfig::default()
        }),
        Err(_) => AppConfig::default(),
    };
    // Preserve v0.2.0 `reduce_memory_when_hidden = false` intent if migrating.
    if !cfg.reduce_memory_when_hidden && cfg.performance_mode == PerformanceMode::Balanced {
        cfg.performance_mode = PerformanceMode::Performance;
    }
    cfg.sanitize();
    cfg
}

pub fn save_config(path: &Path, config: &AppConfig) -> std::io::Result<()> {
    let serialized = toml::to_string_pretty(config)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    write_atomic(path, &serialized)
}

pub fn load_state(path: &Path) -> AppState {
    match fs::read_to_string(path) {
        Ok(raw) => toml::from_str(&raw).unwrap_or_else(|err| {
            warn!(?err, ?path, "Failed to parse state.toml; using defaults");
            AppState::default()
        }),
        Err(_) => AppState::default(),
    }
}

pub fn save_state(path: &Path, state: &AppState) -> std::io::Result<()> {
    let serialized = toml::to_string_pretty(state)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    write_atomic(path, &serialized)
}

/// Safe-to-clear transient cache directories inside WebView2's profile.
/// Strictly excludes Cookies, Network, Local Storage, IndexedDB, and Google login state.
pub fn webview_cache_dirs(webview_data_dir: &Path) -> Vec<PathBuf> {
    let default_profile = webview_data_dir.join("EBWebView").join("Default");
    vec![
        default_profile.join("Cache"),
        default_profile.join("Code Cache"),
        default_profile.join("GPUCache"),
        default_profile.join("DawnGraphiteCache"),
        default_profile.join("DawnWebGPUCache"),
        webview_data_dir.join("EBWebView").join("GrShaderCache"),
        webview_data_dir.join("EBWebView").join("ShaderCache"),
    ]
}

pub fn directory_size_bytes(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in entries.flatten() {
        let p = entry.path();
        if let Ok(meta) = entry.metadata() {
            if meta.is_file() {
                total = total.saturating_add(meta.len());
            } else if meta.is_dir() {
                total = total.saturating_add(directory_size_bytes(&p));
            }
        }
    }
    total
}

pub fn compute_cache_size_bytes(webview_data_dir: &Path) -> u64 {
    webview_cache_dirs(webview_data_dir)
        .into_iter()
        .map(|d| directory_size_bytes(&d))
        .sum()
}

/// Removes transient WebView cache files without affecting Google authentication cookies.
pub fn clear_safe_cache(webview_data_dir: &Path) -> u64 {
    let mut freed = 0u64;
    for dir in webview_cache_dirs(webview_data_dir) {
        freed = freed.saturating_add(remove_files_best_effort(&dir));
    }
    freed
}

fn remove_files_best_effort(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut freed = 0u64;
    for entry in entries.flatten() {
        let p = entry.path();
        if let Ok(meta) = entry.metadata() {
            if meta.is_file() {
                let len = meta.len();
                if fs::remove_file(&p).is_ok() {
                    freed = freed.saturating_add(len);
                }
            } else if meta.is_dir() {
                freed = freed.saturating_add(remove_files_best_effort(&p));
                let _ = fs::remove_dir(&p);
            }
        }
    }
    freed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip_preserves_all_v03_fields() {
        let dir = std::env::temp_dir().join(format!("geminidesk_cfg_v3_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("config.toml");

        let mut cfg = AppConfig {
            launch_at_startup: true,
            minimize_to_tray: true,
            close_behavior: CloseBehavior::Quit,
            theme: ThemePreference::Dark,
            quick_chat_shortcut: "Ctrl+Alt+G".into(),
            full_window_shortcut: "Ctrl+Shift+G".into(),
            companion_shortcut: "Alt+Shift+P".into(),
            ask_selection_shortcut: "Alt+Shift+Q".into(),
            screenshot_shortcut: "Alt+Shift+W".into(),
            clipboard_actions_shortcut: "Alt+Shift+X".into(),
            quick_chat_hide_on_blur: false,
            quick_chat_width: 860,
            quick_chat_height: 620,
            quick_chat_position: QuickChatPosition::TopRight,
            companion_always_on_top: false,
            companion_opacity: 85,
            companion_snap_to_edges: false,
            ask_selection_enabled: true,
            ask_selection_clipboard_fallback: false,
            screenshot_enabled: true,
            screenshot_copy_to_clipboard: false,
            clipboard_actions_enabled: true,
            clipboard_max_chars: 12_000,
            clipboard_templates: vec![ClipboardActionTemplate {
                id: "custom".into(),
                label: "Code Review".into(),
                prompt: "Review this:\n{{text}}".into(),
            }],
            performance_mode: PerformanceMode::Efficiency,
            reduce_memory_when_hidden: true,
            efficiency_suspend_delay_secs: 45,
            auto_check_updates: false,
            auto_download_updates: true,
        };
        cfg.sanitize();

        save_config(&path, &cfg).expect("save config");
        let loaded = load_config(&path);
        assert_eq!(cfg, loaded);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrates_v02_config_cleanly_without_losing_preferences() {
        let dir = std::env::temp_dir().join(format!("geminidesk_mig_v2_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");

        let v02 = r#"
launch_at_startup = true
minimize_to_tray = true
close_behavior = "minimize_to_tray"
quick_chat_shortcut = "Alt+G"
full_window_shortcut = "Alt+Shift+G"
quick_chat_hide_on_blur = false
theme = "light"
reduce_memory_when_hidden = false
"#;
        fs::write(&path, v02).unwrap();

        let loaded = load_config(&path);
        assert!(loaded.launch_at_startup);
        assert_eq!(loaded.quick_chat_shortcut, "Alt+G");
        assert_eq!(loaded.theme, ThemePreference::Light);
        assert_eq!(loaded.performance_mode, PerformanceMode::Performance);
        assert!(!loaded.ask_selection_enabled);
        assert!(!loaded.screenshot_enabled);
        assert!(!loaded.clipboard_actions_enabled);
        assert_eq!(loaded.clipboard_templates.len(), 7);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn state_roundtrip_preserves_main_and_companion_geometry() {
        let dir = std::env::temp_dir().join(format!("geminidesk_state_v3_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("state.toml");

        let state = AppState {
            main_window: Some(WindowGeometry {
                x: 120,
                y: 80,
                width: 1280,
                height: 840,
                maximized: true,
            }),
            companion_window: Some(WindowGeometry {
                x: 1460,
                y: 100,
                width: 440,
                height: 700,
                maximized: false,
            }),
        };

        save_state(&path, &state).expect("save state");
        let loaded = load_state(&path);
        assert_eq!(state, loaded);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_config_falls_back_to_defaults() {
        let dir = std::env::temp_dir().join(format!("geminidesk_badcfg_v3_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        fs::write(&path, "not valid toml === [[[").unwrap();

        let loaded = load_config(&path);
        assert_eq!(loaded, AppConfig::default());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn safe_cache_cleanup_never_touches_cookies_or_sessions() {
        let dir = std::env::temp_dir().join(format!("geminidesk_cache_v3_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let default_dir = dir.join("EBWebView").join("Default");
        let cache_dir = default_dir.join("Cache");
        let network_dir = default_dir.join("Network");
        fs::create_dir_all(&cache_dir).unwrap();
        fs::create_dir_all(&network_dir).unwrap();

        let cache_file = cache_dir.join("data_0");
        let cookies_file = network_dir.join("Cookies");
        fs::write(&cache_file, vec![0u8; 4096]).unwrap();
        fs::write(&cookies_file, b"protected-session-data").unwrap();

        assert_eq!(compute_cache_size_bytes(&dir), 4096);
        let freed = clear_safe_cache(&dir);
        assert_eq!(freed, 4096);
        assert!(!cache_file.exists());
        assert!(cookies_file.exists(), "Cookies file MUST NEVER be deleted");

        let _ = fs::remove_dir_all(&dir);
    }
}
