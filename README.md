# GeminiDesk

An unofficial, ultra-lightweight, high-performance desktop client for Google Gemini written in Rust.

> **Disclaimer:** GeminiDesk is an independent, unofficial desktop shell for [Google Gemini](https://gemini.google.com). It is not affiliated with, endorsed by, or sponsored by Google. All Google authentication, models, subscriptions, conversations, and user data are handled directly by Google.

---

## Architecture

GeminiDesk wraps Google's official Gemini web experience inside a thin native Rust desktop shell without Electron, Node.js, or bundled Chromium runtimes:

- **Core:** Rust (`tao` + `wry` + `tray-icon` + `muda` + `global-hotkey`)
- **Web Engine:** Native OS WebView (**Microsoft Edge WebView2** on Windows 11; WebKitGTK/WKWebView planned for Phase E validation on Linux/macOS)
- **Single-WebView Reparenting:** Both the Main Window and the Spotlight-style Quick Chat overlay share a **single live WebView instance** (`WebViewExtWindows::reparent`), preserving active conversations, streaming responses, and authentication with zero duplicate renderer processes.
- **Security & Privacy:**
  - Zero JavaScript injection into `gemini.google.com` or `accounts.google.com`.
  - No credential interception, cookie extraction, or custom backend servers.
  - Strict domain allowlist covering Google's official 187 `supported_domains` (for regional `SetSID` SSO), `accounts.youtube.com`, and Google Workspace pickers; all external links open in your default browser.
  - Redacted logging strips all URL query strings and fragments.
  - Settings UI runs in a lazy-created, strictly isolated local WebView with strict CSP and zero external network access.

### Project Structure

```text
src/
├── main.rs              # Entry point, tracing initialization, Windows GUI subsystem in release
├── app.rs               # Event-driven coordinator (zero polling), window lifecycle, Quick Chat reparenting
├── config.rs            # TOML settings (config.toml) & window geometry (state.toml) persistence + safe cache cleanup
├── hotkeys.rs           # Global shortcut parser & conflict-safe registration
├── icon.rs              # Anti-aliased RGBA icon renderer (window, taskbar, tray)
├── navigation.rs        # Google domain/SSO allowlist, popup routing, URL log redaction
├── settings_window.rs   # Lazy-created isolated Settings window + typed JSON IPC
├── tray.rs              # Persistent system tray icon and context menu
├── webview.rs           # Modular WebView engine wrapper (reparenting, theme, MemoryUsageLevel, Win32 accelerators)
├── ui/settings.html     # Native-styled dark/light Settings UI
└── platform/
    ├── mod.rs           # Cross-platform shell abstractions & Quick Chat monitor geometry
    ├── windows.rs       # Windows 11 DWM styling, cursor-monitor work area, single-instance mutex, HKCU startup
    ├── linux.rs         # Linux XDG autostart & Wayland/X11 platform module (Phase E runtime verification pending)
    └── macos.rs         # macOS platform module (Phase E runtime verification pending)
```

---

## Features (Windows 11)

- **Main Window:** Seamless Windows 11 dark/light DWM title bar (`#131314` in dark mode), rounded corners, custom icon, multi-monitor DPI awareness, window size/position persistence, and single-instance wake.
- **Quick Chat Overlay (`Alt + Space`):** Borderless, always-on-top floating Gemini overlay centered horizontally near the upper portion of your active display:
  - Press `Alt + Space` from any app to toggle Quick Chat.
  - Press `Esc` inside Quick Chat to hide immediately.
  - Press `Ctrl + Shift + E` or `Alt + Shift + Space` to seamlessly expand your active Quick Chat conversation into the Full Window.
  - Optional auto-hide when clicking outside (safely ignores owned dialogs/WebView surfaces).
- **System Tray:** Persistent tray icon with quick actions (*Open Gemini*, *Quick Chat*, *New Chat*, *Settings*, *Quit GeminiDesk*) and configurable close/minimize-to-tray behavior.
- **Settings (`Ctrl + ,` or Tray → Settings):** Launch at Windows startup (`--minimized`), close/minimize behavior, configurable global shortcuts with live conflict detection, theme (`System`, `Dark`, `Light`), background memory reduction (`MemoryUsageLevel::Low`), and safe cache cleanup that preserves Google sign-in.

---

## Build & Run (Windows 11)

### Prerequisites

- Windows 10/11 (x64)
- Rust toolchain (`rustup` / `cargo`) + MSVC Build Tools
- Microsoft Edge WebView2 Runtime (pre-installed on Windows 11)

### Commands

```powershell
# Run checks and unit tests
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test

# Build optimized release binary (~1.7 MB)
cargo build --release

# Launch GeminiDesk
.\target\release\gemini-desk.exe
```

### CLI Flags

- `--minimized` (or `--tray`): Starts silently in the system tray with `MemoryUsageLevel::Low`.
- `--quick-chat`: Launches or wakes directly into the Quick Chat overlay.
- `--settings`: Opens the Settings window on launch.

---

## Measured Windows 11 Performance (`v0.2.0`, Release Build)

Measured on Windows 11 Pro (Build 26300, x64, 6 logical cores, WebView2 Runtime `157.0.4341.60`) against an authenticated Google Gemini Pro account:

| Metric | Measured Result |
| --- | --- |
| Release executable size (`target\release\gemini-desk.exe`) | **1.70 MB** (`1,783,808 bytes`) |
| Native shell initialization (`init_ms`) | **337–392 ms** warm (`818 ms` cold) |
| Quick Chat reparent latency (Main → Quick Chat) | **24 ms** (single renderer, zero extra processes) |
| Expand latency (Quick Chat → Main) | **20 ms** |
| Rust host process Private RAM (visible / tray) | **3.79 MB** / **3.75 MB** |
| Full process tree Private RAM (visible Main) | **557.15 MB** (`705.94 MB` Working Set, 9 procs) |
| Full process tree Private RAM (minimized tray, `MemoryUsageLevel::Low`) | **333.02 MB** (`612.31 MB` Working Set, **-40.2% Private RAM**) |
| Idle CPU — Rust host (minimized tray, 8s window) | **0.000%** (`0.000s`) |
| Idle CPU — Full process tree (minimized tray, 8s window) | **0.553%** (`0.266s` across 6 cores) |

---

## Data & Profile Locations

- **Settings & Window State:** `%APPDATA%\geminidesk\GeminiDesk\config\config.toml` and `state.toml`
- **WebView2 Profile (Google Session):** `%LOCALAPPDATA%\geminidesk\GeminiDesk\data\webview`

---

## Platform Status

| Platform | Status |
| --- | --- |
| Windows 11 (x64) | Built, unit-tested, benchmarked, visually verified, live auth verified |
| Linux (KDE Plasma / GNOME / Wayland / X11) | Platform module scaffolded; compilation & runtime testing planned for Phase E |
| macOS (Apple Silicon / Intel) | Platform module scaffolded; compilation & runtime testing planned for Phase E |

---

## License

Dual-licensed under MIT or Apache-2.0.
