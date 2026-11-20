use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use tracing::warn;

/// Default global shortcut to toggle Quick Chat (matches official Gemini Desktop).
pub const DEFAULT_QUICK_CHAT_SHORTCUT: &str = "Alt+Space";
/// Default global shortcut to open/focus the full Gemini window.
pub const DEFAULT_FULL_WINDOW_SHORTCUT: &str = "Alt+Shift+Space";

/// Behavior when the user clicks the main window close button (`X`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehavior {
    /// Hide the window to the system tray and keep GeminiDesk running in the background.
    #[default]
    MinimizeToTray,
    /// Terminate the application completely.
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

/// User-configurable application settings persisted in `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub launch_at_startup: bool,
    pub minimize_to_tray: bool,
    pub close_behavior: CloseBehavior,
    pub quick_chat_shortcut: String,
    pub full_window_shortcut: String,
    pub quick_chat_hide_on_blur: bool,
    pub theme: ThemePreference,
    pub reduce_memory_when_hidden: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            launch_at_startup: false,
            minimize_to_tray: false,
            close_behavior: CloseBehavior::MinimizeToTray,
            quick_chat_shortcut: DEFAULT_QUICK_CHAT_SHORTCUT.to_string(),
            full_window_shortcut: DEFAULT_FULL_WINDOW_SHORTCUT.to_string(),
            quick_chat_hide_on_blur: true,
            theme: ThemePreference::System,
            reduce_memory_when_hidden: true,
        }
    }
}

/// Persisted physical geometry of the main application window.
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
}

/// Standard OS directory layout for GeminiDesk.
/// Preserves the Phase B WebView2 profile directory:
/// `%LOCALAPPDATA%\geminidesk\GeminiDesk\data\webview`
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub webview_data_dir: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> Self {
        if let Some(proj) = ProjectDirs::from("dev", "geminidesk", "GeminiDesk") {
            let config_dir = proj.config_dir().to_path_buf();
            let webview_data_dir = proj.data_local_dir().join("webview");
            let _ = fs::create_dir_all(&config_dir);
            let _ = fs::create_dir_all(&webview_data_dir);
            Self {
                config_dir,
                webview_data_dir,
            }
        } else {
            let base = PathBuf::from(".geminidesk-data");
            let config_dir = base.join("config");
            let webview_data_dir = base.join("webview");
            let _ = fs::create_dir_all(&config_dir);
            let _ = fs::create_dir_all(&webview_data_dir);
            Self {
                config_dir,
                webview_data_dir,
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
        // On Windows, rename over an existing file can fail in rare antivirus/lock cases;
        // fall back to direct overwrite so settings aren't lost.
        let _ = fs::remove_file(&tmp);
        fs::write(path, contents).map_err(|_| err)?;
    }
    Ok(())
}

pub fn load_config(path: &Path) -> AppConfig {
    match fs::read_to_string(path) {
        Ok(raw) => toml::from_str(&raw).unwrap_or_else(|err| {
            warn!(?err, ?path, "Failed to parse config.toml; using defaults");
            AppConfig::default()
        }),
        Err(_) => AppConfig::default(),
    }
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

/// Computes the approximate size in bytes of WebView2's safe-to-clear HTTP/GPU/Code cache
/// directories (excluding cookies, Local Storage, IndexedDB, and Google login sessions).
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

/// Safely removes only transient WebView cache files (HTTP/Code/GPU shader caches)
/// while strictly preserving Google authentication cookies, IndexedDB, and session data.
/// Files currently locked by the running WebView2 process are skipped cleanly.
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
    fn config_roundtrip_preserves_all_fields() {
        let dir = std::env::temp_dir().join(format!("geminidesk_cfg_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("config.toml");

        let cfg = AppConfig {
            launch_at_startup: true,
            minimize_to_tray: true,
            close_behavior: CloseBehavior::Quit,
            quick_chat_shortcut: "Ctrl+Alt+G".to_string(),
            full_window_shortcut: "Ctrl+Shift+G".to_string(),
            quick_chat_hide_on_blur: false,
            theme: ThemePreference::Dark,
            reduce_memory_when_hidden: false,
        };

        save_config(&path, &cfg).expect("save config");
        let loaded = load_config(&path);
        assert_eq!(cfg, loaded);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn state_roundtrip_preserves_window_geometry() {
        let dir =
            std::env::temp_dir().join(format!("geminidesk_state_test_{}", std::process::id()));
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
        };

        save_state(&path, &state).expect("save state");
        let loaded = load_state(&path);
        assert_eq!(state, loaded);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_config_falls_back_to_defaults() {
        let dir =
            std::env::temp_dir().join(format!("geminidesk_badcfg_test_{}", std::process::id()));
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
        let dir =
            std::env::temp_dir().join(format!("geminidesk_cache_test_{}", std::process::id()));
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
