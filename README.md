# GeminiDesk

An unofficial, ultra-lightweight, high-performance desktop client for Google Gemini written in Rust.

> **Unofficial Client Disclaimer:** GeminiDesk is an independent open-source desktop shell for [Google Gemini](https://gemini.google.com). It is **not** affiliated with, endorsed by, or sponsored by Google LLC. All Google accounts, authentication, subscriptions, AI models, and conversation history remain entirely managed by Google.

---

## Philosophy

**"Premium simplicity. Native speed. Genuine Gemini."**

GeminiDesk wraps Google's official Gemini web application inside a thin native Rust desktop shell without Electron, Node.js, or bundled Chromium runtimes:

- **Core Stack:** Rust (`tao` + `wry` + `tray-icon` + `muda` + `global-hotkey` + native OS APIs)
- **Native OS WebViews:** Microsoft Edge **WebView2** (Windows), **WebKitGTK 4.1** (Linux), **WKWebView** (macOS)
- **Single-WebView Reparenting:** Main Window, Smart Mini Chat (Quick Chat), and Floating Companion all share **one single live WebView renderer**, preserving active conversations, streaming responses, and Google sign-in across ~15–21 ms transitions with zero duplicate browser processes.
- **Refined Local Design System:** Settings and all productivity dialogs share a quiet monochrome design system (`src/ui/design-system.css`) with locally bundled **Geist** & **Geist Mono** variable fonts (`SIL OFL 1.1`, full Latin + Turkish glyph coverage, zero external CDN calls) served over an isolated `geminidesk://` local protocol.
- **Cryptographically Authenticated Updates:** Release metadata (`latest.json`) is verified via a detached **Ed25519** signature (`latest.json.sig`) and **SHA-256** artifact verification over HTTPS before any download or installer handoff.

---

## Platform Support Matrix

GeminiDesk only labels a platform **Supported** after real-hardware verification of installation, rendering, Google sign-in, session persistence, and desktop integrations. Compilation alone does not prove authentication or runtime compatibility.

| Platform | Target / WebView | Status | Packages | Verification Notes |
| --- | --- | --- | --- | --- |
| **Windows 11 (x64)** | `x86_64-pc-windows-msvc` · **WebView2** | **Verified (Supported)** | NSIS Per-User Installer (`1.19 MB`), Portable ZIP (`1.37 MB`) | Real-hardware verified: Google auth, session persistence, Quick Chat, Companion, tray, shortcuts, productivity, installer/upgrade/uninstall, and controlled benchmarks. |
| **Linux — CachyOS / Arch (KDE Plasma · Wayland)** | `x86_64-unknown-linux-gnu` · **WebKitGTK 4.1** | **Experimental** *(Real-Device Testing Pending)* | Arch `PKGBUILD`, `.deb`, `.rpm`, lean `AppImage` | Implemented: `build_gtk` Wayland WebView + reparent, XDG Portals (`GlobalShortcuts`, interactive `Screenshot`, `Settings`), `ksni` StatusNotifierItem tray, `wl-clipboard`, `/proc` telemetry, Unix-socket CLI wake. Real-device WebKitGTK Google auth & portal verification pending. |
| **Linux — X11 / GNOME** | `x86_64-unknown-linux-gnu` · **WebKitGTK 4.1** | **Experimental** *(Real-Device Testing Pending)* | `.deb`, `.rpm`, lean `AppImage` | Implemented: X11 `global-hotkey`, `xclip`/`xsel`, XDG `Screenshot` & `Settings` portals. Real-device verification pending. |
| **macOS — Apple Silicon & Intel** | `aarch64` / `x86_64-apple-darwin` · **WKWebView** | **Experimental** *(Real-Device Testing Pending)* | `.app` bundle, `.dmg` *(unsigned until Apple Developer secrets configured)* | Implemented: `WKWebView` reparenting, native App Menu Bar, Dock reopen, LaunchAgent startup, `NSPasteboard`, interactive `screencapture -i`, `ps` telemetry. Real-device WKWebView Google auth & Gatekeeper notarization pending. |

---

## Features (`v0.4.0`)

### Desktop Shell & Settings
- **Two-Column Settings (`Ctrl + ,` / `Cmd + ,` or Tray → Settings):** 8 sections (*General*, *Appearance*, *Shortcuts*, *Quick Chat*, *Productivity*, *Performance*, *Privacy*, *About*) with monochrome Dark (`#0B0B0C`) and independently calibrated Light (`#FFFFFF`) themes, bundled Geist + Geist Mono typography, custom popover dropdowns, sliding segmented controls, click-to-record `<kbd>` shortcut recorders, inline confirmation dialogs, `prefers-reduced-motion` support, and zero idle RAM when closed.
- **Persistent System Tray:** Event-driven (`0.000%` Rust idle CPU) native tray on Windows (Win32), Linux (`ksni` StatusNotifierItem over D-Bus — native KDE Plasma support with zero `libappindicator3` dependency), and macOS (Menu Bar + Status Item).
- **Single-Instance Activation:** Secondary invocations with CLI flags (`--quick-chat`, `--companion`, `--settings`, `--clipboard-actions`, `--ask-selection`, `--screenshot`) wake the running instance instantaneously via Win32 messages (Windows) or Unix domain sockets (Linux/macOS).

### Six Native Productivity Features
1. **Ask Gemini with Selection** (`Alt + Shift + A`, opt-in):
   - Windows: reads selected text via **Windows UI Automation (`IUIAutomation`)** (explicitly refusing `CurrentIsPassword` fields), with optional non-destructive clipboard snapshot/restore fallback.
   - Linux: reads Wayland/X11 Primary Selection (`wl-paste --primary` / `xclip`) strictly on invocation.
   - Opens an editable writing sheet with a single-row quick-intent rail (*Raw text*, *Explain*, *Summarize*, *Translate*, *Fix grammar*, *Improve writing*, *Explain code*) and `Ctrl + Enter` insertion (never auto-submits).
2. **Screenshot to Gemini** (`Alt + Shift + S`, opt-in, or Tray → *Capture Screenshot*):
   - Windows: native Win32 multi-monitor, mixed-DPI virtual-screen region selector with pure in-memory PNG encoding.
   - Linux: **XDG Screenshot Portal** (`org.freedesktop.portal.Screenshot`, `interactive = true`) for explicit user-approved KDE Plasma / GNOME region capture, with `spectacle` / `grim`+`slurp` fallback.
   - macOS: native interactive `/usr/sbin/screencapture -i -x -t png` honoring macOS Screen Recording permissions.
   - Opens an image-first matte dotted gallery stage with `W × H · PNG · N KB` badge, optional instruction bar, *Attach to Gemini*, *Copy Image*, and *Retake*.
3. **Smart Mini Chat (Quick Chat)** (`Alt + Space`):
   - Borderless always-on-top Spotlight-style overlay on the active cursor monitor with size presets (`Compact` / `Standard` / `Spacious` / custom W×H) and placement options. `Esc` hides, `Ctrl + Shift + E` expands to the Full Window.
4. **Floating Companion** (`Alt + Shift + G`):
   - Resizable persistent side window (`490 × 700` default) sharing the single live WebView renderer, with **Always on Top** (`Ctrl + Shift + T`), window opacity (`55%–100%`), edge snapping, and persisted geometry.
5. **Clipboard Actions** (`Alt + Shift + C`, opt-in):
   - Raycast-style searchable two-pane command palette with `↑`/`↓` navigation, `Alt+1..9` instant selection, 7 built-in actions, and a custom prompt template editor in Settings → Productivity. Reads clipboard strictly on invocation; never stores history or logs contents.
6. **Performance Mode & Live Per-Role Telemetry** (Settings → Performance):
   - **Efficiency**, **Balanced**, and **Performance** resource profiles with taskbar-minimized throttling (`SW_MINIMIZE`) and safe Efficiency `TrySuspend` guards (checks in-flight navigation, `IsDocumentPlayingAudio`, and active Gemini streaming before suspending).
   - On-demand per-subprocess memory breakdown (*Renderer*, *GPU Process*, *Browser*, *Utility / Net*, *Rust Host*) and rolling 10s averaged + instantaneous 2s CPU % — stops immediately when leaving the tab.

### Secure Automatic Updates (`Settings → About`)
- Verifies detached **Ed25519** signatures (`latest.json.sig`) against a compile-time pinned public key before parsing update metadata.
- Enforces HTTPS-only transfers (OS native TLS store), strict SemVer anti-downgrade protection, streaming `.part` downloads with exact byte-size + **SHA-256** verification, pre-install TOCTOU re-verification, and interrupted-download cleanup.
- Never installs major updates without user consent; unsigned local builds fail closed cleanly.

---

## Installation

### Windows 11 (x64)
1. **Recommended — Per-User Installer (no admin prompt required):**
   Download and run `GeminiDesk-0.4.0-windows-x86_64-setup.exe`.
   - Installs to `%LOCALAPPDATA%\Programs\GeminiDesk`, creates Start Menu & optional Desktop shortcuts, detects Microsoft Edge WebView2 Runtime, and preserves your Google sign-in & settings across all upgrades.
   - Uninstalling asks explicitly whether to remove saved settings/session data (defaults to **No / Keep data**).
2. **Portable ZIP:**
   Extract `GeminiDesk-0.4.0-windows-x86_64-portable.zip` and run `gemini-desk.exe`.

### Linux — CachyOS / Arch Linux (KDE Plasma · Wayland · Fish Shell)
Install runtime and build dependencies:
```fish
sudo pacman -S --needed webkit2gtk-4.1 gtk3 openssl xdg-desktop-portal xdg-desktop-portal-kde wl-clipboard rust pkgconf
```

Build and install from the included Arch `PKGBUILD` (from a local checkout or release tarball):
```fish
cd packaging/linux
makepkg -si
```

Or build directly with Cargo:
```fish
cargo build --release --locked
./target/release/gemini-desk
```

**Wayland & KDE Plasma Notes:**
- Verify your portal environment variables in Fish:
  ```fish
  echo $XDG_CURRENT_DESKTOP  # Expected: KDE
  echo $XDG_SESSION_TYPE     # Expected: wayland
  ```
- On KDE Plasma 6 Wayland, GeminiDesk registers shortcuts through `org.freedesktop.portal.GlobalShortcuts` (manageable in *System Settings → Keyboard → Shortcuts*). On compositors without the `GlobalShortcuts` portal (such as GNOME Wayland), bind desktop custom shortcuts directly to GeminiDesk's instant Unix-socket CLI commands:
  - `gemini-desk --quick-chat`
  - `gemini-desk --companion`
  - `gemini-desk --clipboard-actions`
  - `gemini-desk --ask-selection`
  - `gemini-desk --screenshot`
- If you encounter WebKitGTK GPU/DMABUF rendering glitches on NVIDIA + Wayland, launch with:
  ```fish
  set -x GEMINIDESK_SOFTWARE_RENDER 1
  set -x WEBKIT_DISABLE_DMABUF_RENDERER 1
  gemini-desk
  ```

### Linux — Debian / Ubuntu / Fedora / AppImage
- **Debian / Ubuntu (`.deb`):** `sudo apt install ./gemini-desk_0.4.0-1_amd64.deb` (requires `libwebkit2gtk-4.1-0`, `libgtk-3-0`, `xdg-desktop-portal`).
- **Fedora (`.rpm`):** `sudo dnf install ./gemini-desk-0.4.0-1.x86_64.rpm` (requires `webkit2gtk4.1`, `gtk3`, `xdg-desktop-portal`).
- **Lean AppImage:** `./packaging/linux/build-appimage.sh 0.4.0 x86_64` — intentionally keeps host `WebKitGTK 4.1` and `GTK3` external rather than bundling a 200+ MB runtime.

### macOS (Apple Silicon & Intel — Experimental)
```bash
cargo build --release --locked
./packaging/macos/build-bundle.sh 0.4.0 aarch64
```
> **macOS Signing & Gatekeeper Notice:** Production macOS distribution requires Apple Developer ID signing (`MACOS_SIGNING_IDENTITY`) and Apple notarization (`APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD`) configured in CI secrets. Unsigned local/CI `.dmg` builds are development previews only — GeminiDesk never instructs users to disable Gatekeeper.

---

## Keyboard Shortcuts

| Action | Default Shortcut | Notes |
| --- | --- | --- |
| Toggle Quick Chat | `Alt + Space` | Always active; customizable in Settings → Shortcuts |
| Open Full Window | `Alt + Shift + Space` | Always active |
| Toggle Floating Companion | `Alt + Shift + G` | Always active |
| Ask Gemini with Selection | `Alt + Shift + A` | Opt-in (Settings → Productivity) |
| Screenshot to Gemini | `Alt + Shift + S` | Opt-in (Settings → Productivity) |
| Clipboard Actions | `Alt + Shift + C` | Opt-in (Settings → Productivity) |
| Settings | `Ctrl + ,` / `Cmd + ,` | In-app shortcut |
| Expand Quick Chat / Companion | `Ctrl + Shift + E` | Expands into Main Window |
| Toggle Companion Always-on-Top | `Ctrl + Shift + T` | While Companion is focused |
| Hide Quick Chat | `Esc` | Preserves conversation |

---

## CLI Flags

- `--minimized` / `--tray`: Start quietly in the system tray.
- `--quick-chat`: Launch or wake directly into Quick Chat.
- `--companion`: Launch or wake directly into Floating Companion.
- `--settings` / `--settings=<tab>`: Open Settings (e.g., `--settings=about`, `--settings=performance`).
- `--clipboard-actions`: Open Clipboard Actions command palette.
- `--ask-selection`: Open Ask Gemini with Selection sheet.
- `--screenshot`: Trigger interactive Screenshot to Gemini capture.
- `--preview-theme=<dark|light|system>`: Preview theme without modifying `config.toml`.

---

## Measured Windows 11 Performance (`v0.4.0` vs `v0.3.1`, Release Build)

Measured on Windows 11 Pro (Build 26300, x64, 6 logical cores, WebView2 Runtime `157.0.4341.60`) against a live authenticated Google Gemini Pro session (`15s` settle + sustained continuous sampling across the full process tree, normalized by 6 logical cores):

| Scenario / Metric | `v0.3.1` (60s settled) | `v0.4.0` (30s settled) | Notes |
| --- | --- | --- | --- |
| Release executable size | `2.13 MB` | **`2.68 MB`** | Adds embedded multi-res `.ico`, `VERSIONINFO`, Ed25519 + SHA-256 + HTTPS updater |
| Windows NSIS Installer / Portable ZIP | — | **`1.19 MB`** / **`1.37 MB`** | LZMA-compressed per-user installer |
| Native windows ready (Run 1 / Warm) | `28 ms` / `39 ms` | **`36 ms` / `48 ms`** | — |
| WebView2 initialized (Run 1 / Warm) | `360 ms` / `406 ms` | **`307 ms` / `334 ms`** | — |
| First visible Gemini title (Run 1 / Warm) | `2,050 ms` / `2,434 ms` | **`1,848 ms` / `1,902 ms`** | Includes network round-trip |
| Quick Chat / Companion reparent latency | `17 ms` | **`16 ms` / `15 ms`** | Single live renderer |
| Rust host Private RAM / CPU | `3.63–3.73 MB` / `0.000%` | **`3.78–3.99 MB` / `0.000%`** | `0.000s` Rust CPU time |
| Visible Gemini Home (settled) | `714.4 MB` Priv · `932.5 MB` WS · `7.22%` CPU | **`555.6 MB` Priv · `763.0 MB` WS · `5.96%` CPU** | ~88% of visible CPU is Google's web SPA renderer |
| Taskbar-Minimized (`SW_MINIMIZE`, 30s) | `436.9 MB` Priv · `93.5 MB` WS · `0.078%` CPU | Same engine path | **−89% WS, −98% CPU** vs visible |
| Background Tray — **Balanced** | `284.2 MB` Priv · `241.6 MB` WS · `0.417%` CPU | **`287.6 MB` Priv · `184.1 MB` WS · `0.347%` CPU** | Instant wake |
| Background Tray — **Efficiency Safe-Suspended** | `282.9 MB` Priv · `64.2 MB` WS · `0.061%` CPU | **`288.3 MB` Priv · `56.6 MB` WS · `0.052%` CPU** | Renderer WS: **`0.45 MB`**; Quick Chat wake **`21 ms`** |

*Linux (`WebKitGTK`) and macOS (`WKWebView`) benchmarks will be added only after real-hardware measurement; Windows WebView2 measurements must not be assumed to generalize to other OSes.*

---

## Privacy, Security & Data Locations

See [SECURITY.md](SECURITY.md) for the complete threat model and isolation boundaries.

- **Windows:**
  - Config & Window State: `%APPDATA%\geminidesk\GeminiDesk\config\`
  - WebView2 Profile & Updates: `%LOCALAPPDATA%\geminidesk\GeminiDesk\data\`
- **Linux:**
  - Config & Window State: `~/.config/geminidesk/`
  - WebKitGTK Data & Updates: `~/.local/share/geminidesk/`
- **macOS:**
  - Config & Window State: `~/Library/Application Support/dev.geminidesk.GeminiDesk/`
  - WKWebView Data: macOS default persistent `WKWebsiteDataStore` (`~/Library/WebKit/io.geminidesk.GeminiDesk/`)

---

## Known Limitations

- **Linux (`WebKitGTK`) & macOS (`WKWebView`) Google Authentication:** Google sometimes challenges or blocks embedded WebKit sign-in (`accounts.google.com`) compared to WebView2 (Chromium). GeminiDesk refuses to spoof browsers or harvest external browser cookies. Both platforms remain labeled **Experimental** until real-device sign-in verification completes.
- **macOS Signing & Notarization:** Automated signing and notarization are wired into `packaging/macos/build-bundle.sh` and `.github/workflows/release.yml`, but require Apple Developer credentials in repository secrets before distributing publicly.
- **Google-hosted "Download app" button:** Intentionally left unmodified inside `gemini.google.com` to avoid DOM injection or header spoofing.

---

## Contributing, Changelog & License

- [CONTRIBUTING.md](CONTRIBUTING.md) — Development setup & PR checklist
- [SECURITY.md](SECURITY.md) — Security architecture & vulnerability disclosure
- [CHANGELOG.md](CHANGELOG.md) — Version history
- **License:** Dual-licensed under MIT or Apache-2.0.
- **Third-Party Typography:** Bundled Geist & Geist Mono variable fonts are licensed under the **SIL Open Font License 1.1** — see [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) and [assets/fonts/OFL-Geist.txt](assets/fonts/OFL-Geist.txt).
