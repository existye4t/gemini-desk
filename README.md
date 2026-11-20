# GeminiDesk

An unofficial, ultra-lightweight, high-performance desktop client for Google Gemini written in Rust.

> **Disclaimer:** GeminiDesk is an independent, unofficial desktop shell for [Google Gemini](https://gemini.google.com). It is not affiliated with, endorsed by, or sponsored by Google. All Google authentication, models, subscriptions, conversations, and user data remain strictly managed by Google.

---

## Philosophy

**"Premium simplicity. Native speed. Genuine Gemini."**

GeminiDesk wraps Google's official Gemini web experience inside a thin native Rust shell without Electron, Node.js, or bundled Chromium runtimes:

- **Core:** Rust (`tao` + `wry` + `tray-icon` + `muda` + `global-hotkey` + native Windows 11 APIs)
- **Web Engine:** Native OS WebView (**Microsoft Edge WebView2** on Windows 11; WebKitGTK/WKWebView planned for Phase E validation on Linux and macOS)
- **Single-WebView Reparenting:** Main Window, Smart Mini Chat (Quick Chat), and Floating Companion all share **one single live WebView renderer** (`reparent` + synchronous bounds sync), preserving active conversations, streaming responses, and Google sign-in across ~9–24 ms transitions with zero duplicate Chromium renderers.
- **Crafted Local UI & Typography:** Settings and all productivity sheets share a unified monochrome design system (`src/ui/design-system.css`) inspired by Apple System Settings, Linear, and Raycast, served over an isolated local custom protocol (`geminidesk://`) with locally bundled **Geist** & **Geist Mono** variable fonts (SIL OFL 1.1, full Latin + Turkish glyph coverage, zero external CDN calls).
- **Security & Privacy:**
  - Zero persistent script injection into `gemini.google.com` or `accounts.google.com`.
  - No credential interception, cookie extraction, or custom backend servers.
  - Strict domain allowlist covering Google's official 187 `supported_domains` (for regional `SetSID` SSO), `accounts.youtube.com`, and Workspace pickers; external links open in your default browser.
  - Redacted logging strips URL query strings and fragments.
  - Local Settings and Prompt Review sheets run in lazy-created, strictly isolated WebViews with strict CSP (`default-src 'none'`, local assets only) and zero external network access.
  - Sensitive OS capture features (Selection Capture, Screenshot Capture, Clipboard Actions) are **opt-in by default**, never poll or monitor in the background, never log clipboard/selection text, refuse Windows UI Automation password fields (`CurrentIsPassword`), and **never auto-submit** prompts to Gemini.

---

## Features (`v0.3.1` — Windows 11)

### Desktop Shell, Design System & Settings
- **Refined Settings (`Ctrl + ,` or Tray → Settings):** Two-column preferences window across 8 sections (*General*, *Appearance*, *Shortcuts*, *Quick Chat*, *Productivity*, *Performance*, *Privacy*, *About*) with a `#0B0B0C` / `#121213` monochrome dark palette, an independently tuned `#FFFFFF` / `#F5F5F7` light palette, locally bundled **Geist + Geist Mono**, custom popover dropdowns, sliding segmented pills, interactive click-to-record `<kbd>` shortcut recorders, inline confirmation dialogs, restrained GPU-composited motion (`prefers-reduced-motion` aware), keyboard navigation (`Alt+1..8`, `Esc`), and zero idle memory when closed.
- **Main Window:** Seamless Windows 11 DWM dark/light title bar, rounded corners, cached 4× supersampled RGBA icon, multi-monitor DPI awareness, window geometry persistence, automatic background throttling when minimized to the taskbar, and single-instance wake.
- **Persistent System Tray:** Event-driven (`0.000%` Rust idle CPU) tray menu with *Open Gemini*, *Quick Chat*, *Floating Companion*, *New Chat*, *Capture Screenshot*, *Clipboard Actions*, *Settings...*, and *Quit GeminiDesk*.

### Six Native Productivity Features
1. **Ask Gemini with Selection** (`Alt + Shift + A`, opt-in):
   - Reads selected text from the active application via **Windows UI Automation (`IUIAutomation`)**, explicitly refusing `CurrentIsPassword` fields.
   - Optional non-destructive clipboard snapshot/restore fallback for apps without UIA `TextPattern`, plus a clean manual-paste fallback.
   - Opens a focused writing sheet with a single-row quick-intent pill rail (*Raw text*, *Explain*, *Summarize*, *Translate*, *Fix grammar*, *Improve writing*, *Explain code*), live char/line counter, and `Ctrl + Enter` insertion (never auto-submits).
2. **Screenshot to Gemini** (`Alt + Shift + S`, opt-in, or Tray → *Capture Screenshot*):
   - On-demand native Win32 multi-monitor, mixed-DPI virtual-screen region selector (zero continuous capture, zero temporary disk writes).
   - Dimmed scrim, crisp selection frame, live `W × H px` readout, `Esc` / right-click cancel, in-memory PNG encoding.
   - Image-first matte dotted gallery stage with translucent `W × H · PNG · N KB` metadata badge, optional instruction input bar, *Attach to Gemini* (native composer attachment + `CF_DIB`/`PNG` clipboard), *Copy Image*, and *Retake*.
3. **Smart Mini Chat (Quick Chat)** (`Alt + Space`):
   - Borderless, always-on-top Spotlight-style floating overlay on your active cursor monitor with configurable size (`Compact` / `Standard` / `Spacious` / custom W×H) and placement (`Top center`, `Screen center`, `Top right`, `Bottom right`).
   - Synchronous WebView2 bounds synchronization fixes narrow-viewport reflow cleanly on Main → Quick Chat reparenting and auto-focuses Gemini's composer.
   - `Esc` hides, `Ctrl + Shift + E` expands seamlessly into the Full Window.
4. **Floating Companion** (`Alt + Shift + G`, or Tray → *Floating Companion*):
   - Persistent, resizable compact side window (`490 × 700` default) sharing the single live WebView renderer.
   - Toggleable **Always on Top** (`Ctrl + Shift + T` or Settings), configurable window opacity (`60%–100%` via Win32 `WS_EX_LAYERED`), magnetic snap-to-screen-edge across monitors, and persisted position/size.
5. **Clipboard Actions** (`Alt + Shift + C`, opt-in, or Tray → *Clipboard Actions*):
   - Reads clipboard text strictly on invocation (zero continuous monitoring, zero stored history, never logged) with configurable character limit.
   - Raycast-style searchable two-pane command palette with keyboard filtering, `↑`/`↓` navigation, `Alt+1..9` instant selection, live editable prompt preview, 7 built-in actions, and a **Custom Prompt Template editor** in Settings → Productivity.
6. **Performance Mode & Accurate Process-Tree Telemetry** (Settings → Performance):
   - Three resource profiles:
     - **Efficiency:** Immediately sets `IsVisible = false` + `MemoryUsageLevel::Low` when hidden or taskbar-minimized, then conditionally calls `ICoreWebView2_3::TrySuspend` after a configurable idle delay **only** when no navigation, document audio (`IsDocumentPlayingAudio`), or active Gemini response stream is detected (deferring safely if work is in progress).
     - **Balanced:** Applies `IsVisible = false` + `MemoryUsageLevel::Low` when hidden or taskbar-minimized for low RAM/CPU with instant wake.
     - **Performance:** Keeps `MemoryUsageLevel::Normal` always.
   - Lean WebView2 configuration disables non-essential browser autofill, password autosave, pinch-zoom, and swipe-navigation overhead without weakening TLS, sandboxing, or Google authentication.
   - Live bounded telemetry in Settings → Performance combines Win32 ToolHelp/PSAPI with Microsoft WebView2's official `ICoreWebView2Environment8::GetProcessInfos()` to display Rust host RAM, total process-tree Private Commit, per-role subprocess memory bars (*Renderer*, *GPU Process*, *Browser*, *Utility / Net*, *Rust Host*, *Crashpad / Aux*), and both rolling 10-second averaged and instantaneous 2-second CPU % — stopping immediately when leaving the tab.

---

## Project Structure

```text
assets/
└── fonts/
    ├── Geist-Variable.woff2       # Locally bundled Geist Sans variable font (SIL OFL 1.1)
    ├── GeistMono-Variable.woff2   # Locally bundled Geist Mono variable font (SIL OFL 1.1)
    └── OFL-Geist.txt              # Full SIL Open Font License 1.1 text
src/
├── main.rs                        # Entry point, tracing setup, Windows GUI subsystem in release
├── app.rs                         # Event-driven coordinator (Main, Quick Chat, Companion, Settings, Productivity, Performance)
├── config.rs                      # Backwards-compatible TOML settings & window geometry persistence + safe cache cleanup
├── hotkeys.rs                     # Multi-action global shortcut controller with live conflict detection
├── icon.rs                        # Cached 4x supersampled anti-aliased RGBA icon renderer
├── navigation.rs                  # Official Google 187-domain allowlist, popup policy, URL log redaction
├── performance.rs                 # On-demand Win32 ToolHelp/PSAPI + WebView2 GetProcessInfos() role telemetry
├── settings_window.rs             # Lazy-created isolated Settings window & typed JSON IPC
├── tray.rs                        # Persistent system tray icon and context menu
├── ui_assets.rs                   # Isolated local `geminidesk://` asset protocol serving embedded CSS/HTML/WOFF2
├── webview.rs                     # WebViewEngine (reparenting, bounds sync, lean settings, safe TrySuspend/Resume, handoff)
├── productivity/
│   ├── mod.rs                     # Shared productivity types & RFC 4648 base64 encoder
│   ├── clipboard.rs               # On-demand Win32 Unicode text, CF_DIB+PNG image write, non-destructive snapshot/restore
│   ├── selection.rs               # Windows UI Automation (IUIAutomation) selection capture + IsPassword protection
│   ├── screenshot.rs              # Native Win32 multi-monitor GDI capture + region selection overlay + in-memory PNG
│   └── action_window.rs           # Lazy-created quiet review, command palette & screenshot gallery window
├── ui/
│   ├── design-system.css          # Shared Apple/Linear/Raycast monochrome Dark & Light tokens, controls & motion
│   ├── settings.html              # 8-section Settings UI with custom controls & per-role telemetry
│   └── action_panel.html          # Selection Review, Clipboard Command Palette & Screenshot Gallery UI
└── platform/
    ├── mod.rs                     # Cross-platform abstractions, Quick Chat & Companion monitor/snap math
    ├── windows.rs                 # Windows 11 DWM caption/border/corners, opacity, single-instance, HKCU startup
    ├── linux.rs                   # Linux XDG autostart & platform module (Phase E validation pending)
    └── macos.rs                   # macOS platform module (Phase E validation pending)
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

# Build optimized release binary (~2.13 MB including bundled Geist & Geist Mono fonts)
cargo build --release

# Launch GeminiDesk
.\target\release\gemini-desk.exe
```

### CLI Flags
- `--minimized` (or `--tray`): Starts quietly in the system tray.
- `--quick-chat`: Launches or wakes directly into Quick Chat.
- `--companion`: Launches or wakes directly into Floating Companion.
- `--settings` / `--settings=<tab>`: Opens Settings directly (e.g., `--settings=performance`).
- `--clipboard-actions`: Opens the Clipboard Actions command palette immediately.
- `--ask-selection`: Opens the Ask Gemini with Selection sheet immediately.
- `--preview-theme=<dark|light|system>`: Previews a theme for the session without changing `config.toml`.

---

## Measured Windows 11 Performance (`v0.3.1` vs `v0.3.0`, Controlled 60s Settled Benchmarks)

Measured on Windows 11 Pro (Build 26300, x64, 6 logical cores, WebView2 Runtime `157.0.4341.60`) against a live authenticated Google Gemini Pro session using an identical release-build harness (`15s` post-load settle followed by `60s` continuous sampling across the full process tree, normalized by logical cores):

| Scenario / Metric | `v0.3.0` Baseline | `v0.3.1` Measured | Delta / Notes |
| --- | --- | --- | --- |
| Release executable size | `1.94 MB` | **`2.13 MB`** | Includes `140 KB` bundled Geist + Geist Mono `.woff2` |
| Native windows ready (Run 1 / Warm) | `29 ms` / `50 ms` | **`28 ms` / `39 ms`** | Cached RGBA icon rendering |
| WebView2 initialized (Run 1 / Warm) | `428 ms` / `589 ms` | **`360 ms` / `406 ms`** | **−68 ms to −183 ms** |
| First visible Gemini title (Run 1 / Warm) | `2,203 ms` / `2,794 ms` | **`2,050 ms` / `2,434 ms`** | Includes network round-trip |
| Rust host Private RAM (idle) | `3.58–3.68 MB` | **`3.63–3.67 MB`** | **`0.000%` CPU** (`0.000s` over 60s) |
| Visible Gemini Home (60s settled) | `711.3 MB` Priv · `926.4 MB` WS · `7.44%` CPU | **`714.4 MB` Priv · `932.5 MB` WS · `7.22%` CPU** | 87.8% CPU is Google's Gemini web SPA renderer; 10.6% GPU |
| Floating Companion idle (12s, 1 renderer) | `731.9 MB` Priv · `961.4 MB` WS (9 procs) | **`717.3 MB` Priv · `939.3 MB` WS (8 procs)** | **−14.6 MB Priv**, `17 ms` reparent |
| Settings open — Performance tab (12s) | `753.7 MB` Priv · `1,012.6 MB` WS · `7.79%` CPU | **`745.5 MB` Priv · `1,004.0 MB` WS · `7.10%` CPU** | **−8.2 MB Priv**; local renderer freed on close |
| **Taskbar-Minimized** (`SW_MINIMIZE`, 30s) | Same as visible (~`711 MB` Priv / `926 MB` WS / `~7.4%` CPU) | **`436.9 MB` Priv · `93.5 MB` WS · `0.078%` CPU** | **NEW in v0.3.1: −89.1% WS, −98.5% CPU** vs visible |
| Background Tray — **Balanced** (60s settled) | `289.9 MB` Priv · `241.5 MB` WS · `0.430%` CPU | **`284.2 MB` Priv · `241.6 MB` WS · `0.417%` CPU** | **−60.2% Priv, −74.1% WS, −94.2% CPU** vs visible |
| Background Tray — **Efficiency Safe-Suspended** (60s) | `282.8 MB` Priv · `62.2 MB` WS · `0.030%` CPU | **`282.9 MB` Priv · `64.2 MB` WS · `0.061%` CPU** | **−93.1% WS (`3.65 MB` renderer WS), −99.2% CPU** vs visible |
| Efficiency suspend → Quick Chat wake (5× cycles) | `29 ms` (single) | **`9–24 ms` (median `18 ms`)** | 5/5 clean safe suspends & wakes |

> **Why visible Gemini uses ~7% CPU and ~710 MB Private RAM:** Controlled per-subprocess profiling confirms the Rust host itself uses **`0.000%` CPU** and **`3.6 MB` Private RAM**. While the Gemini window is visible, `87.8%` of process-tree CPU (`~468 MB` Private) is consumed by Google's `gemini.google.com/app` web SPA renderer and `10.6%` (`~175 MB` Private) by WebView2's GPU compositor. Because GeminiDesk preserves genuine unmodified Google Gemini without mutating its DOM or disabling hardware acceleration, the effective way to reclaim that budget is native lifecycle throttling whenever the window is minimized to the taskbar or hidden in the system tray (where Working Set drops to `64–241 MB` and CPU drops to `0.06%–0.42%`).

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

## License & Third-Party Notices

Dual-licensed under MIT or Apache-2.0.
Bundled Geist & Geist Mono fonts are licensed under the **SIL Open Font License 1.1** — see [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) and [assets/fonts/OFL-Geist.txt](assets/fonts/OFL-Geist.txt).
