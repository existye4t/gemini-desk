# GeminiDesk

An unofficial, ultra-lightweight, high-performance desktop client for Google Gemini written in Rust.

> **Disclaimer:** GeminiDesk is an independent, unofficial desktop shell for [Google Gemini](https://gemini.google.com). It is not affiliated with, endorsed by, or sponsored by Google. All Google authentication, models, subscriptions, conversations, and user data remain strictly managed by Google.

---

## Philosophy

**"Premium simplicity. Native speed. Genuine Gemini."**

GeminiDesk wraps Google's official Gemini web experience inside a thin native Rust shell without Electron, Node.js, or bundled Chromium runtimes:

- **Core:** Rust (`tao` + `wry` + `tray-icon` + `muda` + `global-hotkey` + native Windows 11 APIs)
- **Web Engine:** Native OS WebView (**Microsoft Edge WebView2** on Windows 11; WebKitGTK/WKWebView planned for Phase E validation on Linux and macOS)
- **Single-WebView Reparenting:** Main Window, Smart Mini Chat (Quick Chat), and Floating Companion all share **one single live WebView renderer** (`reparent` + synchronous bounds sync), preserving active conversations, streaming responses, and Google sign-in across ~17–20 ms transitions with zero duplicate Chromium renderers.
- **Security & Privacy:**
  - Zero persistent script injection into `gemini.google.com` or `accounts.google.com`.
  - No credential interception, cookie extraction, or custom backend servers.
  - Strict domain allowlist covering Google's official 187 `supported_domains` (for regional `SetSID` SSO), `accounts.youtube.com`, and Workspace pickers; external links open in your default browser.
  - Redacted logging strips URL query strings and fragments.
  - Local Settings and Prompt Review sheets run in lazy-created, strictly isolated WebViews with strict CSP and zero external network access.
  - Sensitive OS capture features (Selection Capture, Screenshot Capture, Clipboard Actions) are **opt-in by default**, never poll or monitor in the background, never log clipboard/selection text, refuse Windows UI Automation password fields (`CurrentIsPassword`), and **never auto-submit** prompts to Gemini.

---

## Features (`v0.3.0` — Windows 11)

### Desktop Shell & Settings
- **Redesigned Settings (`Ctrl + ,` or Tray → Settings):** Minimalist Apple/macOS-inspired two-column preferences window across 8 sections (*General*, *Appearance*, *Shortcuts*, *Quick Chat*, *Productivity*, *Performance*, *Privacy*, *About*) with a `#0B0B0C` monochrome dark theme, an independently crafted `#FFFFFF` / `#F5F5F7` light theme, keyboard navigation (`Alt+1..8`, `Esc`), and zero idle memory when closed.
- **Main Window:** Seamless Windows 11 DWM dark/light title bar, rounded corners, custom anti-aliased RGBA icon, multi-monitor DPI awareness, window geometry persistence, and single-instance wake.
- **Persistent System Tray:** Event-driven (0% idle CPU) tray menu with *Open Gemini*, *Quick Chat*, *Floating Companion*, *New Chat*, *Capture Screenshot*, *Clipboard Actions*, *Settings...*, and *Quit GeminiDesk*.

### Six Native Productivity Features
1. **Ask Gemini with Selection** (`Alt + Shift + A`, opt-in):
   - Reads selected text from the active application via **Windows UI Automation (`IUIAutomation`)**, explicitly refusing `CurrentIsPassword` fields.
   - Optional non-destructive clipboard snapshot/restore fallback for apps without UIA `TextPattern`, plus a clean manual-paste fallback.
   - Opens an editable review sheet so you inspect and confirm before inserting into Gemini's composer (`Ctrl + Enter`, never auto-submits).
2. **Screenshot to Gemini** (`Alt + Shift + S`, opt-in, or Tray → *Capture Screenshot*):
   - On-demand native Win32 multi-monitor, mixed-DPI virtual-screen region selector (zero continuous capture, zero temporary disk writes).
   - Dimmed scrim, crisp selection frame, live `W × H px` readout, `Esc` / right-click cancel, in-memory PNG encoding.
   - Preview sheet with optional prompt text, *Attach to Gemini* (verified native composer attachment + `CF_DIB`/`PNG` clipboard), *Copy Image*, and *Retake*.
3. **Smart Mini Chat (Quick Chat)** (`Alt + Space`):
   - Borderless, always-on-top Spotlight-style floating overlay on your active cursor monitor with configurable size (`Compact` / `Standard` / `Spacious` / custom W×H) and placement (`Top center`, `Screen center`, `Top right`, `Bottom right`).
   - Synchronous WebView2 bounds synchronization fixes narrow-viewport reflow cleanly on Main → Quick Chat reparenting and auto-focuses Gemini's composer.
   - `Esc` hides, `Ctrl + Shift + E` expands seamlessly into the Full Window.
4. **Floating Companion** (`Alt + Shift + G`, or Tray → *Floating Companion*):
   - Persistent, resizable compact side window (`490 × 700` default) sharing the single live WebView renderer.
   - Toggleable **Always on Top** (`Ctrl + Shift + T` or Settings), configurable window opacity (`60%–100%` via Win32 `WS_EX_LAYERED`), magnetic snap-to-screen-edge across monitors, and persisted position/size.
5. **Clipboard Actions** (`Alt + Shift + C`, opt-in, or Tray → *Clipboard Actions*):
   - Reads clipboard text strictly on invocation (zero continuous monitoring, zero stored history, never logged) with configurable character limit.
   - Includes 7 built-in actions (*Explain*, *Summarize*, *Translate*, *Fix grammar*, *Improve writing*, *Explain code*, *Debug error*) plus a live **Custom Prompt Template editor** in Settings → Productivity.
6. **Performance Mode** (Settings → Performance):
   - Three real resource profiles: **Efficiency** (`MemoryUsageLevel::Low` + conditional `ICoreWebView2_3::TrySuspend` after configurable idle delay in tray), **Balanced** (`MemoryUsageLevel::Low` in tray, instant wake), and **Performance** (`MemoryUsageLevel::Normal` always).
   - Live bounded process-tree telemetry (Rust shell RAM, full WebView2 process-tree RAM, CPU %, process count, WebView2 runtime version) using direct Win32 ToolHelp/PSAPI calls — active **only** while Settings → Performance is open.

---

## Project Structure

```text
src/
├── main.rs                    # Entry point, tracing setup, Windows GUI subsystem in release
├── app.rs                     # Event-driven coordinator (Main, Quick Chat, Companion, Settings, Productivity, Performance)
├── config.rs                  # Backwards-compatible TOML settings & window geometry persistence + safe cache cleanup
├── hotkeys.rs                 # Multi-action global shortcut controller with live conflict detection
├── icon.rs                    # 4x supersampled anti-aliased RGBA icon renderer
├── navigation.rs              # Official Google 187-domain allowlist, popup policy, URL log redaction
├── performance.rs             # On-demand Win32 ToolHelp + PSAPI process-tree telemetry sampler
├── settings_window.rs         # Lazy-created isolated Settings window & typed JSON IPC
├── tray.rs                    # Persistent system tray icon and context menu
├── webview.rs                 # Modular WebViewEngine (reparenting, bounds sync, TrySuspend/Resume, composer handoff)
├── productivity/
│   ├── mod.rs                 # Shared productivity types & RFC 4648 base64 encoder
│   ├── clipboard.rs           # On-demand Win32 Unicode text, CF_DIB+PNG image write, non-destructive snapshot/restore
│   ├── selection.rs           # Windows UI Automation (IUIAutomation) selection capture + IsPassword protection
│   ├── screenshot.rs          # Native Win32 multi-monitor GDI capture + region selection overlay + in-memory PNG
│   └── action_window.rs       # Lazy-created quiet review & preview utility window
├── ui/
│   ├── settings.html          # Apple/macOS-inspired 2-column Settings UI (Dark & Light)
│   └── action_panel.html      # Unified Selection Review, Clipboard Actions & Screenshot Preview UI
└── platform/
    ├── mod.rs                 # Cross-platform abstractions, Quick Chat & Companion monitor/snap math
    ├── windows.rs             # Windows 11 DWM caption/border/corners, opacity, single-instance, HKCU startup
    ├── linux.rs               # Linux XDG autostart & platform module (Phase E validation pending)
    └── macos.rs               # macOS platform module (Phase E validation pending)
```

---

## Build & Run (Windows 11)

### Prerequisites
- Windows 10/11 (x64)
- Rust toolchain (`rustup` / `cargo`) + MSVC Build Tools
- Microsoft Edge WebView2 Runtime (pre-installed on Windows 11)

### Commands

```powershell
# Verify formatting, lints, and unit tests
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test

# Build optimized release binary (~1.94 MB)
cargo build --release

# Launch GeminiDesk
.\target\release\gemini-desk.exe
```

### CLI Flags
- `--minimized` (or `--tray`): Starts quietly in the system tray.
- `--quick-chat`: Launches or wakes directly into Quick Chat.
- `--companion`: Launches or wakes directly into Floating Companion.
- `--settings` / `--settings=<tab>`: Opens Settings directly (e.g., `--settings=performance`).
- `--clipboard-actions`: Opens the Clipboard Actions sheet immediately.
- `--preview-theme=<dark|light|system>`: Previews a theme for the session without changing `config.toml`.

---

## Measured Windows 11 Performance (`v0.3.0` vs `v0.2.0`, Release Build)

Measured on Windows 11 Pro (Build 26300, x64, 6 logical cores, WebView2 Runtime `157.0.4341.60`) against an authenticated Google Gemini Pro session:

| Metric | `v0.2.0` Baseline | `v0.3.0` Measured |
| --- | --- | --- |
| Release executable size | `1.70 MB` | **`1.94 MB`** (+240 KB w/ 6 features + PNG + UIA) |
| Native shell windows ready | — | **`38–61 ms`** |
| WebView2 initialized (warm / cold) | `337–392 ms` / `818 ms` | **`369 ms`** / **`790 ms`** |
| First visible Gemini title / Interactive load | `~5.5–7.8 s` | **`2.11–2.43 s`** / **`4.19–4.46 s`** |
| Quick Chat reparent latency (1 renderer) | `24 ms` | **`17 ms`** |
| Floating Companion reparent latency (1 renderer) | — | **`20 ms`** |
| Rust host Private RAM (visible / tray) | `3.79 MB` / `3.75 MB` | **`3.75 MB`** / **`3.76–3.79 MB`** |
| Full process tree Private RAM (visible Main) | `557.15 MB` | **`601.91 MB`** (1 renderer, 9 procs) |
| Background Private RAM (Balanced tray, `Low`) | `333.02 MB` | **`335.27 MB`** (0.000% Rust CPU) |
| Background Private RAM (All 6 features + **Efficiency `TrySuspend`**) | — | **`288.77 MB`** (**−44.3 MB / −13.3% vs v0.2.0**) |
| Background process-tree Idle CPU (8s window) | `0.553%` | **`0.065%`** (**8.5× lower** in Efficiency mode; Rust `0.000%`) |

---

## Known Limitations

- **Google-hosted "Download app" button:** Google serves a "Download app" button inside `gemini.google.com` for standard Chromium/WebView2 user agents. Because there is no documented Google flag or header to hide it—and spoofing first-party headers or injecting persistent CSS targeting Google's internal DOM would risk breaking authentication or future Gemini updates—GeminiDesk intentionally leaves it untouched.
- **Proprietary official Gemini Desktop capabilities:** First-party OS-level features such as Google's "Solve It" live screen sharing, system-wide "Speak to Window", and macOS Computer Use rely on proprietary Google client integrations not exposed to third-party web clients.
- **Linux & macOS:** Clean platform abstractions exist in `src/platform/{linux,macos}.rs`, but compilation and runtime validation on Linux (KDE Plasma / GNOME / Wayland / X11 / Arch) and macOS (Apple Silicon) are scheduled for Phase E.

---

## Data & Profile Locations

- **Settings & Window State:** `%APPDATA%\geminidesk\GeminiDesk\config\config.toml` and `state.toml`
- **WebView2 Profile (Google Session):** `%LOCALAPPDATA%\geminidesk\GeminiDesk\data\webview`

---

## License

Dual-licensed under MIT or Apache-2.0.
