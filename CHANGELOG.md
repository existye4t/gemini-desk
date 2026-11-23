# Changelog

All notable changes to GeminiDesk are documented in this file.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and Semantic Versioning.

---

## [1.0.2] — 2026-11-23 (Linux Signed-In Streaming Hang & Disk Cache Fix)

### Fixed
- **Signed-In Gemini Streaming Hangs on Linux (`src/main.rs`, `src/webview.rs`):**
  - Diagnosed a real upstream `libsoup3` (3.6.x–3.8.x) HTTP/2 stream multiplexing/flow-control hang affecting Google Gemini's signed-in `geminiweb-pa.clients6.google.com/v1/processSession` WebChannel streams: in controlled testing on CachyOS Linux (`libsoup3 3.8.1`), **5 of 8 signed-in prompts (62.5%) hung indefinitely at 366 bytes** under HTTP/2, whereas **`SOUP_FORCE_HTTP1=1` succeeded on 6 of 6 prompts (100%)** with zero hangs and faster warm stream ack (`254–272 ms` vs `398–443 ms`).
  - GeminiDesk now sets `SOUP_FORCE_HTTP1=1` automatically on Linux at startup unless the user explicitly overrides `SOUP_FORCE_HTTP1` in their environment, and logs the active networking/rendering configuration at startup.
- **Efficiency Mode HTTP Disk Cache (`src/webview.rs`):**
  - Switched Linux Efficiency mode from `CacheModel::DocumentViewer` (which completely disables WebKitGTK's HTTP disk cache) to **`CacheModel::DocumentBrowser`**, preserving HTTP disk caching for Gemini's lazy-loaded JS bundles, fonts, and UI assets while keeping in-memory cache usage lean.

### Tests
- Added unit test `linux_soup_http_forces_http1_unless_user_overridden` covering default `SOUP_FORCE_HTTP1=1` selection and explicit user override preservation (**50 unit tests passing**).

---

## [1.0.1] — 2026-11-20 (Linux UI Smoothness & WebKitGTK Acceleration Fix)

### Fixed
- **Linux WebKitGTK UI Stuttering & Frame Pacing (`src/main.rs`, `src/webview.rs`):**
  - Replaced the unconditional `WEBKIT_DISABLE_DMABUF_RENDERER=1` startup workaround with runtime version-aware `WEBKIT_DMABUF_RENDERER_FORCE_SHM=1` on WebKitGTK ≥ 2.44 (retaining `WEBKIT_DISABLE_DMABUF_RENDERER=1` only as a fallback for WebKitGTK < 2.44 or explicit `GEMINIDESK_SOFTWARE_RENDER=1`, while always respecting explicit user environment overrides).
  - **Root cause resolved:** In WebKitGTK 2.44–2.54, `WEBKIT_DISABLE_DMABUF_RENDERER=1` completely disabled accelerated compositing (equivalent to `HardwareAccelerationPolicy::Never`), dropping live `gemini.google.com/app` rendering to **13–20 FPS (36–78 ms median frame times, 100% dropped 60 Hz frames)**. Switching to `WEBKIT_DMABUF_RENDERER_FORCE_SHM=1` restores GPU-accelerated compositing, threaded Skia rasterization, and async overflow scrolling while preserving seamless zero-blank single-WebView reparenting across Main Window, Quick Chat, and Floating Companion on Wayland.
  - Measured on real CachyOS + KDE Plasma 6 Wayland + AMD Radeon RX 6600 XT (`239.76 Hz` display, `webkit2gtk-4.1 2.54.3`): typing improved from **`19.9 → 87.0 FPS`** (input-to-paint latency **`36 ms → 15 ms`** median, **`87 ms → 25 ms`** P95), scrolling from **`16.7 → 110.6 FPS`** (`100% → 1.4%` frames > 16.7 ms), and sidebar transitions from **`13.3 → 105.2 FPS`** (`100% → 0.9%` frames > 16.7 ms).
- **Foreground Restore & Reparent Smoothness (`src/webview.rs`):**
  - Stopped evicting WebKitGTK's decoded `MEMORY_CACHE` on every transient Balanced-mode hide / Quick Chat toggle (eliminating asset re-decode hitches on reopen; `MEMORY_CACHE` trimming is now strictly reserved for delayed Efficiency-mode idle suspension while Balanced mode trims host glibc arenas and hides the GTK widget to keep background tray CPU near `0%`).
  - Explicitly enabled `enable_webgl(true)` and `enable_page_cache(true)` on Linux WebKitGTK, guarded `set_visible()` against redundant GTK widget `show()`/`hide()` calls, and queued an immediate resize/draw after reparenting or resuming from Efficiency idle.
  - Added startup diagnostics logging the active WebKitGTK version, `HardwareAccelerationPolicy`, `PerformanceMode`, and DMABuf/SHM renderer selection.

### Tests
- Added unit tests for `select_linux_webkit_renderer_env` covering WebKitGTK ≥ 2.44 (`FORCE_SHM`), legacy < 2.44 fallback, `GEMINIDESK_SOFTWARE_RENDER` escape hatch, and preservation of explicit user environment overrides (49 unit tests passing).

---

## [1.0.0] — 2026-11-20 (First Public Stable Release)

### Added
- **In-App Account Switching Navigation & Cancellation (`src/auth.rs`, `src/platform/*`, `src/webview.rs`, `src/ui/settings.html`):**
  - Compact native navigation header (`Back to Gemini`, active flow title, and `Cancel` / `Retry`) mounted at the top of the Main Window strictly while Google sign-in, Account Chooser, or Google Account pages are active (GTK3 monochrome bar on Linux, native Win32 child bar on Windows, native Window menu & title sync on macOS — zero duplicate WebViews).
  - Keyboard navigation: `Alt+Left` steps back in WebView history or returns cleanly to Gemini; `Escape` cancels only after a read-only DOM focus/dialog check confirms no Google input field or modal dialog is active.
  - Preserves the exact previous Gemini conversation URL (using `history.back()` when 1 step deep to avoid unnecessary full reloads, or direct validated navigation when deeper in a multi-step flow) without ever clearing cookies, sessions, or browser storage.
  - Honest post-return verification (`AuthState::Returning` followed by live DOM probe) so canceling never pretends to undo an already-completed Google account switch.
  - Added **Add Account** and **Back to Gemini** controls in **Settings → Account** and switched **Switch Account** to open Google's `AccountChooser` directly.
- **Native Desktop Notifications (`src/notifications.rs`, `src/config.rs`, Settings → General):**
  - Native Linux D-Bus `org.freedesktop.Notifications` integration (with `notify-send` fallback) respecting OS Do Not Disturb settings and user opt-in (`notifications_enabled`).
  - Notifies strictly on non-sensitive app events (verified updates available/ready, Wayland shortcut permission declined, capture errors) and never exposes private chat content, prompts, clipboard data, or account emails.
- **Enhanced KDE Plasma 6 Wayland Integration (`src/platform/linux.rs`, `src/hotkeys.rs`):**
  - Non-blocking background XDG `GlobalShortcuts` portal lifecycle with duplicate shortcut conflict detection, asynchronous `BindShortcuts` permission response monitoring, and clean `org.freedesktop.portal.Session::Close` teardown on exit.
  - Multi-monitor cursor-aware GDK `Monitor::workarea()` detection excluding panels/docks.
  - Added KDE desktop quick actions (`QuickChat`, `Companion`, `Settings`) to user-local `.desktop` launcher generation and removed "Unofficial" from autostart metadata.
- **Linux WebKitGTK Performance & Memory Lifecycle (`src/webview.rs`, `src/app.rs`, `src/platform/linux.rs`):**
  - Implemented real Linux `PerformanceMode` behavior: `CacheModel::DocumentViewer` in Efficiency mode, explicit `WebKitWebView::hide()` on tray hide for native WebKit visibility/timer throttling, transient `WebsiteDataTypes::MEMORY_CACHE` trimming (strictly preserving cookies, LocalStorage, IndexedDB, and live conversations), glibc `malloc_trim(0)` reclamation on tray hide and utility window teardown, and native `WebViewExt::is_playing_audio()` guard.
  - Replaced per-hide sleeping threads with a single reusable condvar `IdleSuspendScheduler`.
- **Updater Polish (`src/updater/mod.rs`):**
  - Automatically marks verified downloaded Linux `.AppImage` updates executable (`0o755`) before opening the updates folder.
  - Quietly skips offline background startup update errors while preserving full error reporting for manual **Check for Updates**.
  - Added regression tests covering `min_supported_version`, interrupted/truncated download `.part` cleanup, and pre-install TOCTOU SHA-256 verification.

### Changed
- Promoted **Linux (CachyOS / Arch · KDE Plasma 6 · Wayland)** to real-hardware verified status following user-confirmed Google sign-in, conversation loading, full-restart session persistence, and account switching.
- Updated `.github/workflows/release.yml` so stable semantic versions (`v1.0.0`) publish with `prerelease: false`, ensuring GitHub's `/releases/latest/download/latest.json` endpoint resolves automatically for the signed updater.

---

## [0.4.0] — 2026-11-20 (First Public Beta Candidate)

### Added
- **Google Account & Authentication Lifecycle (`src/auth.rs`, Settings → Account, Tray → *Google Account & Sign-In...*):**
  - Typed authentication state machine (`Checking`, `Authenticated`, `SignedOut`, `SigningIn`, `AuthError`) with flow-generation correlation, cancellation handling, strict HTTPS host & `continue` redirect validation, and event-driven non-invasive session detection.
  - Native in-app **Sign in with Google** (`AccountChooser`), **Switch / Add Account** (`AddSession`), **Sign Out** (`Logout` with confirmation dialog), and an explicitly labeled **Open Gemini in Default Browser** fallback (honestly documenting that Google does not support transferring external-browser OAuth logins into consumer `gemini.google.com` WebView cookies).
  - CLI flags `--account` and `--sign-in`, plus Unix `0700` permission hardening for local config/profile directories and WebKitGTK ITP/cookie-policy configuration for reliable cross-subdomain Google SSO persistence on Linux.
- **Cross-Platform Linux Implementation (Desktop Verified on CachyOS · KDE Plasma 6 · Wayland):**
  - Native `WebKitGTK 4.1` (`2.54.3`) integration via `WebViewBuilderExtUnix::build_gtk` + single-WebView `reparent` across Main, Quick Chat (`780 × 580`), and Floating Companion (`490 × 700`) with automatic `WEBKIT_DISABLE_DMABUF_RENDERER=1` multi-toplevel Wayland surface compatibility and native GTK keyboard accelerators (`Esc`, `Ctrl+,`, `Ctrl+Shift+E`, `Ctrl+Shift+T`).
  - XDG Desktop Portal support via D-Bus (`zbus`): `org.freedesktop.host.portal.Registry` host `app_id` registration (`io.geminidesk.GeminiDesk`), `Settings` color-scheme detection, `GlobalShortcuts` portal for KDE Plasma 6 / Wayland, and interactive `Screenshot` portal (`interactive = true`) with `spectacle` / `grim`+`slurp` fallback.
  - Pure-Rust `ksni` StatusNotifierItem system tray (no `libappindicator3` shared-library requirement on Arch / CachyOS KDE Plasma).
  - Out-of-the-box Linux clipboard & selection integration supporting KDE Plasma's `org.kde.klipper` D-Bus service, in-process GTK3 `gtk::Clipboard` (text, primary selection, and `gdk-pixbuf` PNG images), `wl-clipboard`, and `xclip`/`xsel`.
  - Unix domain socket single-instance activation (`$XDG_RUNTIME_DIR/io.geminidesk.GeminiDesk.sock`) for instant wake from CLI flags (`--quick-chat`, `--companion`, `--settings=<tab>`, `--screenshot`, `--clipboard-actions`, `--ask-selection`).
  - `/proc` process-tree & role telemetry (`WebKitWebProcess`, `WebKitGPUProcess`, `WebKitNetworkProcess`) and safe WebKitGTK cache cleanup (`WebKitCache`, `CacheStorage`) preserving cookies and local storage.
  - Linux packages: Arch / CachyOS `PKGBUILD` (supporting both local checkout and release archive builds), `.deb`, `.rpm`, lean `AppImage` builder, `.desktop` with KDE Kickoff actions, and AppStream `metainfo.xml`.
- **Cross-Platform macOS Implementation (Experimental — Real-Device Testing Pending):**
  - Native `WKWebView` single-WebView reparenting, `NSUserDefaults` dark-mode detection, `NSWindow` floating Spaces behavior & Companion opacity, native `muda` macOS App Menu Bar (`Cmd+C/V/X/A/Z`, `Cmd+,`, `Cmd+N`, `Cmd+Q`), Dock `Event::Reopen` handling, LaunchAgent startup plist, `pbcopy`/`pbpaste` + `NSPasteboardTypePNG` clipboard, interactive `/usr/sbin/screencapture -i`, and `ps` process-tree telemetry.
  - `.app` & `.dmg` packaging pipeline with `Info.plist`, Hardened Runtime `entitlements.plist`, and conditional Developer ID signing + Apple `notarytool` notarization.
- **Windows Native Installer & Metadata:**
  - Embedded monochrome multi-resolution `.ico` and Windows `VERSIONINFO` resource via `build.rs` (`winresource`).
  - Per-user Unicode NSIS installer (`GeminiDesk-0.4.0-Windows-x64-Setup.exe`, `%LOCALAPPDATA%\Programs\GeminiDesk`, zero UAC admin prompt), WebView2 Runtime detection, Start Menu & optional Desktop shortcuts, `/S /AUTORESTART` post-update relaunch, clean upgrades preserving Google sign-in & config, and opt-in user-data cleanup on uninstall.
  - Portable Windows ZIP packaging script (`GeminiDesk-0.4.0-Windows-x64-Portable.zip`).
- **Cryptographically Authenticated Updater (`src/updater/mod.rs`):**
  - Ed25519 signature verification over `latest.json` (`latest.json.sig`) with embedded production public key (`ce042ef3aa7286c10ae6fdd6a886cfee2bc7ec09cd24d1862ca878adc5b1b637`), strict SemVer anti-downgrade checks, trusted GitHub Releases HTTPS host allowlist, OS native TLS (`TlsProvider::NativeTls`), streaming `.part` SHA-256 verification, pre-install TOCTOU re-verification, stale `.part` recovery, and integrated **Settings → About** update controls.
- **Cross-Platform GitHub Actions CI/CD:**
  - `.github/workflows/ci.yml` (Windows x64, Ubuntu 24.04 Linux x64, macOS 15 Apple Silicon + Intel compile & packaging verification) with third-party Actions pinned to immutable commit SHAs and least-privilege `contents: read` permissions.
  - `.github/workflows/release.yml` (tag/dispatch builds, packaging, `SHA256SUMS.txt`, Ed25519-signed `latest.json` + `--verify` self-check, Draft GitHub Release).
  - `.github/dependabot.yml` for weekly Cargo and GitHub Actions dependency updates.
- **Project & Community Documentation:** `LICENSE-MIT`, `LICENSE-APACHE`, `CONTRIBUTING.md`, `SECURITY.md`, `CHANGELOG.md`, issue templates, PR template, and release notes template.

### Changed
- Unified product identity across all windows, menus, tray tooltips, shortcuts, and installer metadata to **`GeminiDesk`** (with non-affiliation disclosures kept in `README.md`, `Settings → About`, and legal documentation).
- Refined application icon (`src/icon.rs` + `assets/icons/*`) into a cohesive monochrome obsidian squircle + silver-white prism matching the `v0.3.1` design system.
- Isolated local utility WebViews (`SettingsWindow`, `ActionWindow`) into incognito ephemeral contexts on Linux/macOS to eliminate WebKitGTK custom-protocol collisions.
- Hardened cross-platform download filename sanitization (`sanitize_download_filename`) against path traversal, control characters, and Windows reserved device names (`CON`, `NUL`, `COM1`, `LPT1`, etc.).

---

## [0.3.1] — 2026-11-20

### Added
- Shared monochrome design system (`src/ui/design-system.css`) with locally bundled **Geist** & **Geist Mono** variable fonts (`SIL OFL 1.1`, zero CDN) served via isolated `geminidesk://` protocol.
- Rebuilt Settings UI (custom popover dropdowns, sliding segmented pills, click-to-record `<kbd>` shortcut recorders, inline confirmation dialogs, per-role Private Commit telemetry bars) and rebuilt Productivity sheets (Raycast-style 2-pane Clipboard Command Palette, Selection Intent Rail, matte dotted Screenshot Gallery).
- Taskbar-minimized WebView2 throttling (`SW_MINIMIZE`: −89.1% Working Set, −98.5% CPU vs visible idle).
- Safe Efficiency `TrySuspend` guards checking in-flight navigation, `IsDocumentPlayingAudio`, and active Gemini response streaming.
- Official `ICoreWebView2Environment8::GetProcessInfos()` per-role telemetry with rolling 10s averaged CPU %.
- Cached RGBA icon rendering via `OnceLock`.

---

## [0.3.0] — 2026-11-20

### Added
- Six native Windows 11 productivity features: *Ask Gemini with Selection* (Windows UI Automation + `CurrentIsPassword` guard), *Screenshot to Gemini* (native multi-monitor GDI region overlay + in-memory PNG), *Smart Mini Chat* synchronous bounds sync, *Floating Companion* (always-on-top, opacity, edge snap), *Clipboard Actions* + custom templates, and *Performance Mode* (*Efficiency*, *Balanced*, *Performance*).

---

## [0.2.0] — 2026-11-20

### Added
- Production-quality Windows 11 native desktop shell, single-WebView reparenting across Main Window and Quick Chat, persistent system tray, global shortcuts, Settings window, geometry persistence, and single-instance wake.
