# GeminiDesk `v1.0.0` — Architecture & Maintenance Reference

> **Purpose:** Concise engineering reference covering **GeminiDesk**'s native cross-platform architecture, platform verification status, build pipelines, and cryptographic release workflow.

---

## 1. Project Architecture & Directory Structure

### Core Architectural Model
- **Product Name:** **`GeminiDesk`** (App ID: `io.geminidesk.GeminiDesk`).
- **Mission:** Ultra-lightweight, native Rust desktop client wrapping Google's official web experience (`https://gemini.google.com/app`) with zero Electron, zero Node.js, zero bundled Chromium runtime, and zero custom AI backend. Google handles all authentication, accounts, subscriptions, models, and chat history.
- **UI & Windowing Stack:**
  - **Rust (`edition = "2024"`)** + **`tao` (`0.37`)** + **`wry` (`0.57`)** + **`tray-icon` (`0.26`, `ksni` on Linux)** + **`muda` (`0.21`)** + **`global-hotkey` (`0.10`)**.
  - **Native OS WebView Engines:**
    - **Windows 11:** Microsoft Edge **WebView2** (`webview2-com 0.39` + `windows 0.62`).
    - **Linux:** **WebKitGTK 4.1** (`webkit2gtk 2.0`, `gtk 0.18`, `zbus 5`).
    - **macOS:** **WKWebView** (`objc2 0.6`, `objc2-app-kit 0.3.2`, `objc2-foundation 0.3.2`, `objc2-web-kit 0.3.2`).
- **Single-WebView Reparenting Architecture (`src/webview.rs` + `src/app.rs`):**
  - The Main Window, Smart Mini Chat (Quick Chat), and Floating Companion all share **one single live `GeminiWebView` instance**.
  - Transitioning between windows reparents the active WebView container (`WebViewExtWindows::reparent` on Windows, GTK `ContainerExt::remove`/`add` on Linux `vbox`, `WebViewExtMacOS::reparent` on macOS) in ~15–21 ms, preserving active streaming responses, composer drafts, and authenticated sessions without spawning extra renderer processes.
- **Lazy Utility Windows (`src/settings_window.rs`, `src/productivity/action_window.rs`):**
  - Settings (`GeminiDesk Settings`) and the Productivity Action Sheet (`Ask Gemini with Selection`, `GeminiDesk Clipboard Actions`, `Screenshot to Gemini`) are created on demand and destroyed immediately when closed (`0 MB` idle RAM).
  - They load embedded HTML/CSS/WOFF2 assets over a custom `geminidesk://` protocol (`src/ui_assets.rs`) and communicate via typed JSON IPC (`SettingsIpcMessage`, `ActionIpcMessage`). The main `gemini.google.com` WebView never has an IPC handler attached.

### Repository Directory Tree
```text
gemini-desk/
├── .github/
│   ├── ISSUE_TEMPLATE/
│   │   ├── bug_report.yml
│   │   └── feature_request.yml
│   ├── PULL_REQUEST_TEMPLATE.md
│   ├── RELEASE_NOTES_TEMPLATE.md
│   ├── dependabot.yml
│   └── workflows/
│       ├── ci.yml                     # Cross-platform fmt, clippy (-D warnings), test, build & packaging CI
│       └── release.yml                # Multi-OS packaging, Ed25519 signing & Draft GitHub Release pipeline
├── assets/
│   ├── fonts/
│   │   ├── Geist-Variable.woff2       # Locally bundled UI font (SIL OFL 1.1, Latin + Turkish glyphs)
│   │   ├── GeistMono-Variable.woff2   # Locally bundled monospace font (SIL OFL 1.1)
│   │   └── OFL-Geist.txt
│   └── icons/
│       ├── geminidesk.ico             # Multi-resolution Windows icon (16/24/32/48/64/128/256)
│       ├── geminidesk.svg             # Scalable Linux hicolor vector icon
│       └── geminidesk-{64,128,256,512}.png
├── docs/
│   ├── DEVELOPMENT_HANDOFF.md         # This technical handoff document
│   └── screenshots/                   # Real Windows 11 v0.4.0 Dark & Light UI screenshots
├── packaging/
│   ├── linux/
│   │   ├── PKGBUILD                   # Arch Linux / CachyOS package build definition
│   │   ├── build-appimage.sh          # Builds lean AppImage + portable .tar.gz tarball
│   │   ├── io.geminidesk.GeminiDesk.desktop
│   │   └── io.geminidesk.GeminiDesk.metainfo.xml
│   ├── macos/
│   │   ├── Info.plist.template
│   │   ├── build-bundle.sh            # Builds .app bundle + verified .dmg (optional codesign/notarytool)
│   │   └── entitlements.plist
│   └── windows/
│       ├── build-installer.ps1        # Builds NSIS Setup.exe + Portable.zip
│       └── geminidesk.nsi             # Per-user Unicode NSIS script (/S and /S /AUTORESTART support)
├── scripts/
│   └── release_manifest.py            # Ed25519 keygen, SHA256SUMS.txt, latest.json signing & verification
├── src/
│   ├── main.rs                        # Entry point, tracing init, single-instance guard
│   ├── app.rs                         # EventLoop controller, window state machine, reparenting, CLI flags
│   ├── config.rs                      # Atomic TOML config & JSON window state persistence + safe cache cleanup
│   ├── hotkeys.rs                     # Global shortcut parser, registration, and Linux portal integration
│   ├── icon.rs                        # Procedural fallback + cached OnceLock RGBA icon renderer
│   ├── navigation.rs                  # Strict Google/Gemini domain allowlist, popup routing & log redaction
│   ├── performance.rs                 # Live process-tree & subprocess role telemetry (Win32 / Linux /proc / macOS ps)
│   ├── settings_window.rs             # Lazy two-column Settings window & typed IPC handler
│   ├── tray.rs                        # Cross-platform system tray icon & context menu
│   ├── ui_assets.rs                   # Embedded geminidesk:// custom protocol server (HTML/CSS/WOFF2)
│   ├── webview.rs                     # Single-WebView lifecycle, composer handoff, Efficiency suspend, download guard
│   ├── platform/
│   │   ├── mod.rs                     # Cross-platform trait/function dispatch & geometry math
│   │   ├── windows.rs                 # Win32 dark mode, DWM corner styling, startup registry, single-instance HWND wake
│   │   ├── linux.rs                   # Wayland/X11 detection, XDG Portals (Settings, GlobalShortcuts), autostart, Unix socket IPC
│   │   └── macos.rs                   # NSWindow styling, muda App Menu Bar, LaunchAgent plist, Unix socket IPC
│   ├── productivity/
│   │   ├── mod.rs                     # Shared types (CaptureSource, ScreenshotData), base64, template substitution
│   │   ├── action_window.rs           # Lazy utility window for Selection, Clipboard Actions & Screenshot review
│   │   ├── clipboard.rs               # On-demand clipboard text/PNG/CF_DIB access + snapshot/restore
│   │   ├── screenshot.rs              # Native region capture (Win32 GDI / Linux XDG Portal+spectacle+grim / macOS screencapture)
│   │   └── selection.rs               # Selected text capture (Win32 UIA / Linux Primary Selection / macOS manual sheet)
│   ├── ui/
│   │   ├── design-system.css          # Monochrome Obsidian/Alabaster design system (#0B0B0C dark / #FFFFFF light)
│   │   ├── settings.html              # 8-tab Settings UI (General, Appearance, Shortcuts, Quick Chat, Productivity, Performance, Privacy, About)
│   │   └── action_panel.html          # Raycast-inspired Clipboard Palette, Selection Intent Rail & Screenshot Gallery
│   └── updater/
│       └── mod.rs                     # Ed25519 signature verification, SemVer guard, HTTPS native-tls download, SHA-256 & installer launch
├── build.rs                           # Windows PE icon & VERSIONINFO embedding (winresource)
├── Cargo.toml
├── Cargo.lock
├── CHANGELOG.md
├── CONTRIBUTING.md
├── LICENSE-APACHE
├── LICENSE-MIT
├── README.md
├── SECURITY.md
└── THIRD_PARTY_LICENSES.md
```

---

## 2. Current Implementation Status (`v1.0.0`)

- **Version:** `1.0.0` (Public Stable Release)
- **Windows 11 (`x86_64-pc-windows-msvc`):** **Previously Real-Hardware Tested & CI-Verified.** Compiles cleanly with `-D warnings`, passes all 46 unit tests on CI, and packages into per-user NSIS installer (`GeminiDesk-1.0.0-Windows-x64-Setup.exe`) and Portable ZIP (`GeminiDesk-1.0.0-Windows-x64-Portable.zip`).
- **Linux (`x86_64-unknown-linux-gnu`):** **Real-Hardware Verified on CachyOS (`Linux 7.3.4-1-cachyos` · KDE Plasma 6 · Wayland · WebKitGTK `2.54.3`).** User-verified Google sign-in, authenticated conversations, full-restart session persistence, and account switching; native account-switching navigation bar (`Back to Gemini` / `Cancel` / `Alt+Left` / safe `Escape`); non-blocking XDG `GlobalShortcuts` portal with conflict detection and `Session::Close`; native D-Bus desktop notifications; WebKitGTK background visibility hiding + `MEMORY_CACHE` trimming + glibc `malloc_trim(0)` (`0.000%` settled tray CPU); and verified Arch `makepkg` `.pkg.tar.zst` (`2.6 MB`), `.deb`, `.rpm`, `.AppImage`, and `.tar.gz` packages.
- **macOS (`aarch64-apple-darwin` & `x86_64-apple-darwin`):** **Experimental / Unsigned (CI-Built Only).** Compiles cleanly for Apple Silicon and Intel with `-D warnings`, passes all unit tests on `macos-15`, and builds verified unsigned `.dmg` bundles. Real-hardware macOS runtime testing and Apple Developer ID signing/notarization remain pending.

---

## 3. All Completed Features

1. **Authenticated Google Gemini Workspace (`src/webview.rs`, `src/navigation.rs`):**
   - Loads `https://gemini.google.com/app` inside a persistent OS WebView profile while allowing official Google OAuth / SSO domains (`accounts.google.com`, `myaccount.google.com`, regional Google accounts domains) in-app and routing external third-party links to the user's default browser (`open::that_detached`).
   - Query strings and fragments are redacted from all navigation logs.
2. **Single-WebView Window Modes (`src/app.rs`, `src/webview.rs`):**
   - **Main Window:** Resizable desktop workspace with persisted geometry and maximized state.
   - **Smart Mini Chat (Quick Chat) (`Alt + Space`):** Borderless always-on-top overlay centered on the cursor's active monitor, with size presets (`Compact` / `Standard` / `Spacious` / custom W×H), placement options (*Top-Center Spotlight*, *Center*, *Bottom-Right*, *Remember Last Position*), `Esc` to hide, and `Ctrl + Shift + E` to expand into the Main Window.
   - **Floating Companion (`Alt + Shift + G`):** Persistent side window (`490 × 700` default) with Always-on-Top toggle (`Ctrl + Shift + T`), adjustable opacity (`55%–100%`), screen-edge snapping, and persisted geometry.
3. **Six Native Productivity Integrations (`src/productivity/*`):**
   - **Ask Gemini with Selection (`Alt + Shift + A`, opt-in):** Captures selected text via Windows UI Automation (refusing `CurrentIsPassword` fields) or Linux Primary Selection (`wl-paste --primary` / `xclip -o -selection primary`), opens an editable review sheet with a single-row intent rail (*Raw text*, *Explain*, *Summarize*, *Translate*, *Fix grammar*, *Improve writing*, *Explain code*), and inserts into Gemini's ARIA composer (`[role="textbox"][contenteditable="true"]`) without auto-submitting.
   - **Screenshot to Gemini (`Alt + Shift + S`, opt-in):** Native multi-monitor GDI region selector on Windows, XDG Screenshot Portal (`org.freedesktop.portal.Screenshot`, `interactive = true`) with `spectacle`/`grim`+`slurp` fallback on Linux, and `/usr/sbin/screencapture -i` on macOS. Opens an image-first preview stage (`W × H · PNG · N KB`), copies PNG (+ `CF_DIB` on Windows) to clipboard on confirm, and focuses the Gemini composer for paste.
   - **Clipboard Actions (`Alt + Shift + C`, opt-in):** Raycast-style 2-pane searchable command palette with keyboard navigation (`↑`/`↓`, `Alt+1..9`), 7 built-in actions, and custom user templates (`{text}` placeholder).
   - **Smart Mini Chat** & **Floating Companion** (described above).
   - **Performance Mode & Live Per-Role Telemetry (`src/performance.rs`):** *Efficiency*, *Balanced*, and *Performance* resource profiles; taskbar-minimized throttling (`SW_MINIMIZE`); safe Efficiency idle suspension (`TrySuspend` guarded by DOM probe checking active Gemini streaming, in-flight navigation, and `IsDocumentPlayingAudio`); and live 2-second per-role memory/CPU telemetry in `Settings → Performance` (active only while viewing the Performance tab).
4. **Refined Monochrome Design System (`src/ui/design-system.css`, `src/ui/settings.html`, `src/ui/action_panel.html`):**
   - Bundled **Geist** and **Geist Mono** variable `.woff2` fonts (`SIL OFL 1.1`, zero external network calls).
   - Independently calibrated Dark (`#0B0B0C`) and Light (`#FFFFFF`) themes, custom popover selects, sliding segmented pills, click-to-record `<kbd>` shortcut recorders, and inline confirmation dialogs.
5. **Cryptographically Authenticated Updater (`src/updater/mod.rs`, `scripts/release_manifest.py`):**
   - Detached Ed25519 signature verification (`latest.json.sig`), SemVer anti-downgrade enforcement, trusted GitHub Releases HTTPS host allowlist, OS native TLS (`TlsProvider::NativeTls`), streaming `.part` SHA-256 verification, pre-install TOCTOU re-verification, and `Settings → About` update controls.

---

## 4. Important Architectural Decisions & Their Reasoning

1. **Why `WebViewBuilderExtUnix::build_gtk` on Linux (`src/webview.rs`, `src/settings_window.rs`, `src/productivity/action_window.rs`):**
   - `wry::WebViewBuilder::build` fails on Wayland compositors (including KDE Plasma 6 Wayland on CachyOS) if treated as an X11 window handle. Using `tao::platform::unix::WindowExtUnix::default_vbox()` with `wry::WebViewBuilderExtUnix::build_gtk(vbox)` embeds `WebKitWebView` directly inside the GTK3 container on both Wayland and X11, and enables single-WebView reparenting via GTK `ContainerExt::remove` / `add`.
2. **Why Ephemeral Incognito `WebContext` for Linux/macOS Utility Windows (`src/settings_window.rs`, `src/productivity/action_window.rs`):**
   - WebKitGTK panics if `register_uri_scheme("geminidesk")` is registered twice on the same default `WebContext`. Utility windows (`SettingsWindow`, `ActionWindow`) use a fresh ephemeral `WebContext` so custom protocol registration never collides with the main persistent Gemini `WebContext` or across repeated open/close cycles.
3. **Why `ksni` StatusNotifierItem on Linux (`Cargo.toml`, `src/tray.rs`):**
   - Enabling `tray-icon = { version = "0.26", default-features = false, features = ["ksni"] }` speaks KDE's `org.kde.StatusNotifierItem` protocol directly over D-Bus in pure Rust without requiring `libappindicator3.so` / `libayatana-appindicator` C libraries at runtime on Arch/CachyOS.
4. **Why Hybrid XDG `GlobalShortcuts` Portal + Unix Socket CLI Wake on Linux (`src/platform/linux.rs`, `src/hotkeys.rs`):**
   - X11 `XGrabKey` (`global-hotkey`) does not capture global keys under Wayland windows. On Wayland (`XDG_SESSION_TYPE=wayland`), GeminiDesk spawns a background thread connecting to `org.freedesktop.portal.GlobalShortcuts` (`CreateSession` -> `BindShortcuts` -> `Activated` signal) AND listens on `$XDG_RUNTIME_DIR/io.geminidesk.GeminiDesk.sock` so users or compositor keybindings can always invoke `gemini-desk --quick-chat`, `--companion`, `--clipboard-actions`, `--ask-selection`, or `--screenshot` with instant single-instance wake.
5. **Why `ureq` Must Explicitly Configure `TlsProvider::NativeTls` (`src/updater/mod.rs`):**
   - In `ureq 3.5.2`, `TlsConfig::default()` defaults to `TlsProvider::Rustls` even when compiled with `default-features = false, features = ["native-tls", "json"]`. `build_http_agent()` in `src/updater/mod.rs` explicitly sets `TlsConfig::builder().provider(TlsProvider::NativeTls).build()` so HTTPS requests use OS native TLS (Schannel on Windows, OpenSSL on Linux, Security.framework on macOS) without panicking.
6. **Why Never Spoof User-Agent or Inject Google Auth Scripts (`src/webview.rs`, `src/navigation.rs`):**
   - Modifying User-Agent strings, harvesting external browser cookies, or injecting scripts into `accounts.google.com` violates Google's security policies and causes account lockouts or fragility. GeminiDesk uses the genuine native WebView runtime profile on each OS.

---

## 5. Windows, Linux, and macOS Compatibility Status

| Platform | Target Triple | WebView Engine | Current Label | What Is Verified vs. Pending |
| --- | --- | --- | --- | --- |
| **Windows 11** | `x86_64-pc-windows-msvc` | Microsoft Edge WebView2 | **Verified (Supported)** | Real-hardware verified: Google sign-in, session persistence, Quick Chat, Companion, tray, global hotkeys, all 6 productivity tools, NSIS installer/upgrade/uninstall, E2E auto-update, and 30s settled benchmarks. |
| **Linux (CachyOS / Arch · KDE Plasma Wayland)** | `x86_64-unknown-linux-gnu` | WebKitGTK 4.1 (`webkit2gtk-4.1`) | **Beta (Desktop Verified)** | Real-hardware verified on CachyOS (`Linux 7.3.4-1-cachyos`, KDE Plasma 6 Wayland, WebKitGTK `2.54.3`): single-WebView reparenting (`0–2 ms`), `780 × 580` Quick Chat & `490 × 700` Companion, `ksni` tray, XDG `GlobalShortcuts` (`org.freedesktop.host.portal.Registry` `io.geminidesk.GeminiDesk`), XDG `Screenshot` & `Settings` portals, native KDE Klipper D-Bus + GTK3 clipboard, `/proc` performance benchmarks, and native `makepkg` `.pkg.tar.zst` package (`2.49 MB`). Interactive user Google sign-in + restart persistence test ready. |
| **macOS (Apple Silicon & Intel)** | `aarch64-apple-darwin`, `x86_64-apple-darwin` | WKWebView | **Experimental** | Compiled & unit-tested on `macos-15` CI; unsigned `.app` and `.dmg` bundles verified via `file`, `otool -L`, and `hdiutil verify`. **Pending:** real-hardware WKWebView Google sign-in test and Apple Developer ID signing + `notarytool` notarization secrets. |

---

## 6. GitHub Repository & CI/CD Information

- **Private Repository:** `https://github.com/existye4t/gemini-desk` (`"isPrivate": true`)
- **Workflows:**
  1. **`.github/workflows/ci.yml`** (`CI — Cross-Platform Build, Lint, Test & Packaging`):
     - Runs on every push and pull request to `main`.
     - Matrix: `windows-latest` (`x86_64-pc-windows-msvc`), `ubuntu-24.04` (`x86_64-unknown-linux-gnu`), `macos-15` (`aarch64-apple-darwin` + `x86_64-apple-darwin`).
     - Enforces `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked` (31 tests), release builds, metadata validation, and package generation.
     - All third-party actions are pinned to immutable 40-character commit SHAs with top-level `permissions: contents: read`.
  2. **`.github/workflows/release.yml`** (`Release — Cross-Platform Packages & Signed Update Metadata`):
     - Triggered via `workflow_dispatch` (`version`, `dry_run`) or `v*` tag push.
     - Builds all Windows, Linux, and macOS packages, generates `SHA256SUMS.txt`, signs `latest.json` → `latest.json.sig` using `secrets.UPDATER_SIGNING_PRIVATE_KEY`, self-verifies the signature against `vars.GEMINIDESK_UPDATER_PUBKEY`, and creates a **Draft** GitHub Release (`draft: true`, `prerelease: true`).
- **Configured Repository Secret & Variable (in `existye4t/gemini-desk`):**
  - Secret: `UPDATER_SIGNING_PRIVATE_KEY` (32-byte hex Ed25519 private seed — never exposed).
  - Variable: `GEMINIDESK_UPDATER_PUBKEY` = `ce042ef3aa7286c10ae6fdd6a886cfee2bc7ec09cd24d1862ca878adc5b1b637`.
- **Current Draft Release:** `GeminiDesk v0.4.0` (`tagName: "v0.4.0"`, `"isDraft": true`, `"isPrerelease": true`) containing all 12 release assets (`Setup.exe`, `Portable.zip`, `.AppImage`, `.deb`, `.rpm`, `.tar.gz`, `PKGBUILD`, `AppleSilicon.dmg`, `Intel.dmg`, `SHA256SUMS.txt`, `latest.json`, `latest.json.sig`).

---

## 7. Automatic Update Architecture

- **Implementation:** `src/updater/mod.rs` and `scripts/release_manifest.py`.
- **Compile-Time Public Key:** `DEFAULT_UPDATER_PUBKEY_HEX = Some("ce042ef3aa7286c10ae6fdd6a886cfee2bc7ec09cd24d1862ca878adc5b1b637")` (can be overridden at compile time via `GEMINIDESK_UPDATER_PUBKEY`).
- **Default Manifest URL:** `https://github.com/existye4t/gemini-desk/releases/latest/download/latest.json` (can be overridden at compile time via `GEMINIDESK_UPDATE_MANIFEST_URL`).
- **Platform Keys in `latest.json` (`artifacts` map):**
  - `windows-x86_64` → `GeminiDesk-<ver>-Windows-x64-Setup.exe` (`nsis_installer`, launched with `["/S", "/AUTORESTART"]`)
  - `linux-x86_64` → `GeminiDesk-<ver>-Linux-x86_64.AppImage` (`linux_app_image`, marked `0o755` and revealed in file manager)
  - `macos-aarch64` → `GeminiDesk-<ver>-macOS-AppleSilicon.dmg` (`macos_dmg`, opened via `/usr/bin/open -R`)
  - `macos-x86_64` → `GeminiDesk-<ver>-macOS-Intel.dmg` (`macos_dmg`, opened via `/usr/bin/open -R`)
- **Note on Private Repository Updates:** While `existye4t/gemini-desk` remains private and `v0.4.0` remains a Draft release, `https://github.com/existye4t/gemini-desk/releases/latest/download/latest.json` returns HTTP 404 to unauthenticated clients. The startup background check (`15s` after launch) logs/ignores network 404s quietly without disrupting the user. Once the repository is made public and a release is published, the endpoint resolves automatically.

---

## 8. Existing Test Results & Performance Benchmarks

### Automated Unit Tests (`cargo test --locked`)
- **31 passed; 0 failed** across Windows, Linux, and macOS CI:
  - `config::tests` (4 tests): v0.2→v0.4 migration, config roundtrip, state geometry roundtrip, corrupt config fallback, safe cache cleanup preserving cookies/sessions.
  - `hotkeys::tests` (2 tests): default & custom shortcut parsing, invalid spec rejection.
  - `icon::tests` (1 test): RGBA rendering dimensions & center pixel verification.
  - `navigation::tests` (5 tests): Google core/regional SSO allowlist, sorted domain table invariant, popup policy, unsafe scheme blocking, query/fragment log redaction.
  - `performance::tests` (1 test): current process & subprocess role sampler.
  - `platform::tests` (3 tests): Quick Chat monitor placement & small-display clamping, Companion screen-edge snapping.
  - `productivity::tests` & `productivity::screenshot::tests` (4 tests): RFC 4648 base64, template substitution, BGRA→PNG in-memory encoding, DIB section blit & crop.
  - `ui_assets::tests` (2 tests): embedded font/stylesheet serving, non-local URL blocking.
  - `updater::tests` (7 tests): valid Ed25519 manifest verification, tampered signature rejection, downgrade/equal version rejection, wrong platform rejection, non-HTTPS URL rejection, streaming SHA-256 download + tampered binary cleanup, `TlsProvider::NativeTls` HTTPS agent check.

### Measured Windows 11 Pro Benchmarks (`v0.4.0 Final`, Release Binary `2.68 MB`)
- **Startup Timings (Run 1 / Run 2 Warm):**
  - Native windows ready: `42 ms` / `49 ms`
  - WebView2 initialized: `342 ms` / `365 ms`
  - First navigation dispatched: `351 ms` / `373 ms`
  - First visible Gemini document title: `1,997 ms` / `1,960 ms`
  - Quick Chat / Companion reparent latency: `16 ms` / `20 ms` (`21 ms` waking from Efficiency suspension)
- **Resource Consumption (6 logical cores, WebView2 `157.0.4341.60`):**
  - **Gemini Home Idle (`30s` settled):** 8 processes (`1` renderer), Rust Host `5.31 MB` Priv / `0.035%` CPU, Full Tree `719.27 MB` Priv / `990.18 MB` WS / `6.684%` CPU.
  - **Quick Chat Idle (`12s` settled):** 8 processes (`1` renderer), Rust Host `5.22 MB` Priv / `0.000%` CPU, Full Tree `722.21 MB` Priv / `983.05 MB` WS / `5.664%` CPU.
  - **Floating Companion Idle (`12s` settled):** 8 processes (`1` renderer), Rust Host `5.22 MB` Priv / `0.000%` CPU, Full Tree `727.28 MB` Priv / `988.28 MB` WS / `5.816%` CPU.
  - **Background Tray — Balanced (`30s` settled):** 8 processes, Rust Host `4.89 MB` Priv / `0.017%` CPU, Full Tree `291.36 MB` Priv / `224.24 MB` WS / `0.321%` CPU.
  - **Background Tray — All 6 Features + Efficiency Safe-Suspended (`30s` settled):** 8 processes, Rust Host **`4.73 MB` Priv / `0.000%` CPU**, Full Tree `285.34 MB` Priv / **`78.02 MB` WS** (Renderer WS: **`0.92 MB`**) / **`0.052%` CPU**.

### Measured CachyOS Linux Benchmarks (`v0.4.0`, Release Binary `5.21 MB`, Arch Package `2.49 MB`)
- **Environment:** `Linux 7.3.4-1-cachyos x86_64`, 6 logical cores, AMD Radeon RX 6600, KDE Plasma 6 Wayland, `webkit2gtk-4.1` `2.54.3-1`, `gtk3` `3.24.52`.
- **Startup Timings:**
  - Native windows ready: `134–149 ms`
  - WebKitGTK initialized: `186–208 ms`
  - First navigation dispatched: `249–267 ms`
  - First visible Gemini document title: `723–815 ms`
  - Quick Chat / Companion reparent latency: `0–2 ms`
- **Resource Consumption (`/proc/<pid>/smaps_rollup` `Private_Clean + Private_Dirty` & `Rss`):**
  - **Gemini Home Idle:** 3 processes (`1` `WebKitWebProcess` renderer), Rust Host `49.57 MB` Priv, Full Tree `522.74 MB` Priv / `740.96 MB` RSS.
  - **Floating Companion Idle:** 3 processes (`1` renderer), Full Tree `530.16 MB` Priv / `747.52 MB` RSS.
  - **Background System Tray (`--minimized`):** 3 processes, Rust Host **`47.11 MB` Priv / `0.067%` CPU**, Full Tree `500.76 MB` Priv / `720.14 MB` RSS / **`0.533%` CPU**.

---

## 9. Known Bugs, Limitations & Technical Risks

1. **WebKitGTK Google Authentication Compatibility (Primary Risk on Linux):**
   - Google's sign-in flows (`accounts.google.com`) occasionally challenge or restrict embedded WebKit browsers compared to Chromium/WebView2.
   - Do **not** spoof Chromium User-Agent headers or import external browser SQLite cookie databases if challenged. First test standard WebKitGTK 4.1 sign-in with persistent `WebContext` storage (`~/.local/share/geminidesk/webview`).
2. **Wayland Global Window Positioning Limitations:**
   - By design of the Wayland protocol, top-level `xdg_toplevel` windows cannot set arbitrary global `(x, y)` screen coordinates from client code (`window.set_outer_position` is ignored by KWin Wayland).
   - Verify how KWin places Quick Chat and Floating Companion on KDE Plasma 6 Wayland, and if needed use GTK3 window hints (`gtk_window_set_position(GTK_WIN_POS_CENTER)` / `set_type_hint` / KWin window rules) cleanly inside `src/platform/linux.rs`.
3. **WebKitGTK GTK Box Reparenting Lifecycle (`src/webview.rs`):**
   - On Linux, `GeminiWebView::reparent_to` moves the child widget from the old window's `default_vbox()` to the new window's `default_vbox()`. On real KDE Wayland hardware, verify that GTK does not unrealize/destroy the underlying `WebKitWebView` when removed from the previous `vbox` (if `container.remove(&child)` drops the refcount to zero without an extra GObject ref, `child` must be held via `glib::ObjectExt` / `WidgetExt` clone — `children` in `gtk-rs` returns `Vec<gtk::Widget>` where each `gtk::Widget` holds a `GObject` strong reference, which prevents destruction during remove/add, but verify visually that the webview remains visible and focused after Main ↔ Quick Chat ↔ Companion transitions!).
4. **WebKitGTK Hardware Acceleration / DMABUF on NVIDIA Wayland:**
   - Some Arch/CachyOS systems with NVIDIA drivers experience blank or flickering WebKitGTK surfaces due to DMABUF renderer issues. `src/webview.rs` respects `GEMINIDESK_SOFTWARE_RENDER=1`, and users can also set `WEBKIT_DISABLE_DMABUF_RENDERER=1`.
5. **XDG `GlobalShortcuts` Portal Session Payload Format on KDE Plasma 6:**
   - `run_wayland_global_shortcuts_portal` in `src/platform/linux.rs` binds shortcuts via `org.freedesktop.portal.GlobalShortcuts`. On KDE Plasma 6 (`xdg-desktop-portal-kde`), `CreateSession` returns a Request object path and completes asynchronously via `org.freedesktop.portal.Request::Response` signal before `BindShortcuts` can be called on `session_handle`. Verify on real KDE Plasma 6 whether `BindShortcuts` succeeds immediately or needs to await the `Response` signal on the request path before binding.

---

## 10. Exact Next Steps for Real CachyOS + KDE Plasma + Wayland Testing

When resuming on **CachyOS (KDE Plasma · Wayland · Fish shell)**, execute this verification plan in order:

1. **Environment & Build Verification:**
   - Confirm `$XDG_CURRENT_DESKTOP` (`KDE`), `$XDG_SESSION_TYPE` (`wayland`), `webkit2gtk-4.1`, `xdg-desktop-portal-kde`, and `wl-clipboard`.
   - Clone `https://github.com/existye4t/gemini-desk` (or pull `main`), run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked`, and `cargo build --release --locked`.
2. **Main Window & Google Authentication Test:**
   - Launch `./target/release/gemini-desk`.
   - Verify the Main Window opens cleanly with title `"GeminiDesk"`, signs into a real Google account, loads existing Gemini conversations, sends a prompt, streams a response, and persists the session across a full application restart (`~/.local/share/geminidesk/webview`).
3. **Single-WebView Reparenting Test (Main ↔ Quick Chat ↔ Floating Companion):**
   - Trigger Quick Chat (`gemini-desk --quick-chat`, tray menu, or global shortcut) and Floating Companion (`gemini-desk --companion` or tray menu).
   - Verify the GTK `default_vbox()` reparenting in `src/webview.rs` (`reparent_to`) moves the live `WebKitWebView` cleanly without blanking, losing focus, or reloading the active conversation. Fix any GTK realization/sizing or KWin positioning quirks observed on Wayland.
4. **KDE StatusNotifierItem System Tray Test (`ksni`):**
   - Verify the GeminiDesk monochrome prism icon appears in the KDE Plasma System Tray and all menu actions (*Open Gemini*, *Quick Chat*, *Floating Companion*, *New Chat*, *Capture Screenshot*, *Clipboard Actions*, *Settings*, *Quit GeminiDesk*) work.
5. **Global Shortcuts & Unix Socket CLI Wake Test:**
   - Inspect tracing logs (`RUST_LOG=info ./target/release/gemini-desk`) for `org.freedesktop.portal.GlobalShortcuts` registration on KDE Plasma 6. If `CreateSession` / `BindShortcuts` requires awaiting the XDG Portal `Request::Response` signal on KDE Plasma 6, refine `run_wayland_global_shortcuts_portal` in `src/platform/linux.rs`.
   - Test secondary CLI invocations (`gemini-desk --quick-chat`, `--companion`, `--clipboard-actions`, `--ask-selection`, `--screenshot`, `--settings`) while an instance is already running to confirm instant wake over `$XDG_RUNTIME_DIR/io.geminidesk.GeminiDesk.sock`.
6. **Productivity Features Test on Wayland:**
   - **Ask Gemini with Selection:** Select text in another app (e.g. Kate/Konsole/Firefox), invoke `gemini-desk --ask-selection`, verify `wl-paste --primary` captures the selection into the review sheet, and confirm *Insert into Gemini* populates the Gemini composer.
   - **Clipboard Actions:** Copy text, invoke `gemini-desk --clipboard-actions`, select a template, and confirm composer insertion.
   - **Screenshot to Gemini:** Invoke `gemini-desk --screenshot` (or Tray → *Capture Screenshot*), verify the KDE XDG Screenshot Portal interactive UI (or `spectacle` fallback) captures a PNG region, displays it in the preview gallery, copies PNG via `wl-copy --type image/png`, and focuses the Gemini composer.
7. **Settings Window & Live `/proc` Performance Telemetry Test:**
   - Open `Settings` (`Ctrl + ,` or `gemini-desk --settings=performance`), verify Dark/Light theme switching, Geist font rendering, and live `/proc` process-tree memory/CPU breakdown (`WebKitWebProcess`, `WebKitGPUProcess`, `WebKitNetworkProcess`, `rust-host`).
8. **Native Arch `PKGBUILD` & `.AppImage` Packaging Test:**
   - Build and install the Arch package via `makepkg -si` inside `packaging/linux/`, and test the CI-built `GeminiDesk-0.4.0-Linux-x86_64.AppImage`.
9. **Record Real CachyOS Linux Benchmarks & Update Platform Matrix:**
   - Measure cold/warm startup latency, idle RAM/CPU (Main Window, Quick Chat, Companion, Tray), and binary/package sizes on CachyOS.
   - Once all checks pass, promote **Linux — CachyOS / Arch (KDE Plasma · Wayland)** from **Experimental** to **Verified (Supported)** in `README.md`.

---

## 11. Required Dependencies & Fish-Compatible Commands

All shell snippets below are written for the **Fish shell** (`fish`) on **CachyOS / Arch Linux**:

### Install System & Build Dependencies
```fish
sudo pacman -S --needed base-devel git github-cli rust pkgconf webkit2gtk-4.1 gtk3 openssl xdg-desktop-portal xdg-desktop-portal-kde wl-clipboard spectacle
```

### Verify Wayland & KDE Portal Environment
```fish
echo "Desktop: $XDG_CURRENT_DESKTOP | Session: $XDG_SESSION_TYPE | Runtime: $XDG_RUNTIME_DIR"
systemctl --user status xdg-desktop-portal xdg-desktop-portal-kde --no-pager
```

### Clone Repository & Verify Build / Tests
```fish
git clone https://github.com/existye4t/gemini-desk.git ~/gemini-desk
cd ~/gemini-desk
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

### Run with Live Diagnostic Logging
```fish
env RUST_LOG=info ./target/release/gemini-desk
```

### Test Single-Instance Unix-Socket CLI Wake Commands (in a second Fish terminal)
```fish
./target/release/gemini-desk --quick-chat
./target/release/gemini-desk --companion
./target/release/gemini-desk --clipboard-actions
./target/release/gemini-desk --ask-selection
./target/release/gemini-desk --screenshot
./target/release/gemini-desk --settings=performance
```

### Download Prebuilt Linux Artifacts from the `v0.4.0` Draft Release
```fish
mkdir -p /tmp/geminidesk-artifacts; and cd /tmp/geminidesk-artifacts
gh release download v0.4.0 --repo existye4t/gemini-desk -p "GeminiDesk-0.4.0-Linux-x86_64.*" -p "PKGBUILD" -p "SHA256SUMS.txt"
sha256sum --ignore-missing -c SHA256SUMS.txt
chmod +x GeminiDesk-0.4.0-Linux-x86_64.AppImage
./GeminiDesk-0.4.0-Linux-x86_64.AppImage
```

---

## 12. Important Coding Conventions & Design Preferences

- **Product Identity:** Always use **`GeminiDesk`**. Never put `"Unofficial"`, `"Phase B/C/D"`, or version strings in window titles, tray tooltips, Start Menu/desktop launchers, or installer product names. Keep the concise non-affiliation disclaimer strictly in `README.md`, `Settings → About`, and legal/release docs.
- **Visual Design Language (`src/ui/design-system.css`):**
  - Quiet monochrome aesthetic inspired by Raycast, Linear, and Apple macOS Sonoma Settings.
  - No neon gradients, no rainbow badges, no multi-color charts, no emojis in native UI controls.
  - Strict 8px/12px spatial grid, `1px` hairline borders, locally bundled **Geist** & **Geist Mono** `.woff2` fonts (`tabular-nums` on all metrics/counters).
- **Zero Background Polling:**
  - Never poll the clipboard, selection, or CPU/RAM in the background.
  - Clipboard and selection are read strictly on explicit user shortcut/menu invocation.
  - Performance telemetry runs its 2-second timer **only** while the Settings window is open on the *Performance* tab.
- **Compiler Hygiene:**
  - Every commit must pass `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --locked` across Windows, Linux, and macOS CI.

---

## 13. Current Git Branch & Commit Hash

- **Repository:** `https://github.com/existye4t/gemini-desk` (Private)
- **Branch:** `main`
- **Verified Baseline Commit (prior to adding this handoff file):** `b57ddb44e9404d5fff8b216cad0c1aded4a9a19c` (`b57ddb4` — `docs(v0.4.0): add final Windows 11 benchmarks, screenshots, CachyOS commands, and release notes`, CI Run `44736505358` 100% GREEN).
- **Active `HEAD` Commit:** The commit on `main` directly following `b57ddb4` that adds `docs/DEVELOPMENT_HANDOFF.md` (`git log -n 2 --oneline`).

---

## 14. Unfinished Work & Pending Decisions

1. **Real-Device CachyOS Linux Verification & Benchmarking (Immediate Priority):**
   - Execute Section 10 on CachyOS (KDE Plasma + Wayland), fix any runtime WebKitGTK / GTK reparenting / XDG Portal issues discovered on real hardware, record Linux performance benchmarks, and update `README.md` to mark Linux as **Verified (Supported)**.
2. **Real-Device macOS Verification & Apple Notarization (Future Priority):**
   - Requires a physical Mac for `WKWebView` Google authentication testing and Apple Developer ID credentials (`MACOS_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD`) in GitHub Actions secrets for Gatekeeper notarization.
3. **Repository Visibility & Public Release Publication (User Decision):**
   - The repository `existye4t/gemini-desk` is currently **Private** and the `GeminiDesk v0.4.0` release is a **Draft** (`isDraft: true`).
   - Do **not** make the repository public or publish the draft release without explicit user instruction.
