# Changelog

All notable changes to GeminiDesk are documented in this file.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and Semantic Versioning.

---

## [0.4.0] — 2026-11-20 (First Public Beta Candidate)

### Added
- **Cross-Platform Linux Implementation (Experimental — Real-Device Testing Pending):**
  - Native `WebKitGTK` integration via `WebViewBuilderExtUnix::build_gtk` + single-WebView `reparent` across Main, Quick Chat, and Floating Companion (Wayland & X11 compatible).
  - XDG Desktop Portal support via D-Bus (`zbus`): `Settings` color-scheme detection, `GlobalShortcuts` portal for KDE Plasma 6 / Wayland, and interactive `Screenshot` portal (`interactive = true`) with `spectacle` / `grim`+`slurp` fallback.
  - Pure-Rust `ksni` StatusNotifierItem system tray (no `libappindicator3` shared-library requirement on Arch / CachyOS KDE Plasma).
  - Wayland (`wl-clipboard`) & X11 (`xclip`/`xsel`) clipboard + primary selection support.
  - Unix domain socket single-instance activation (`$XDG_RUNTIME_DIR/io.geminidesk.GeminiDesk.sock`) for instant wake from CLI flags (`--quick-chat`, `--companion`, `--screenshot`, `--clipboard-actions`, `--ask-selection`).
  - `/proc` process-tree & role telemetry (`WebKitWebProcess`, `WebKitGPUProcess`, `WebKitNetworkProcess`).
  - Linux packages: Arch / CachyOS `PKGBUILD`, `.deb`, `.rpm`, lean `AppImage` builder, `.desktop` with KDE Kickoff actions, and AppStream `metainfo.xml`.
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
