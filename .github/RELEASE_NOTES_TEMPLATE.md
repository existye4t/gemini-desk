# GeminiDesk v1.0.0

> GeminiDesk is an independent open-source desktop client for Google Gemini. It is not developed by, affiliated with, endorsed by, or sponsored by Google LLC.

## Highlights

- **Account Switching & Navigation UX:** Added a compact native navigation header (`Back to Gemini`, flow title, and `Cancel` / `Retry`) while Google sign-in, Account Chooser, or Google Account pages are active, with `Alt+Left` history back and non-interfering `Escape` cancellation. Returning preserves your previous Gemini conversation URL without clearing cookies or storage, and honestly re-verifies the active Google session afterward.
- **Linux (CachyOS / Arch · KDE Plasma 6 · Wayland) — Real-Hardware Verified:**
  - Verified Google sign-in, conversation loading, full-restart session persistence, and account switching on real CachyOS KDE Plasma 6 Wayland hardware (`WebKitGTK 4.1`).
  - Asynchronous XDG `GlobalShortcuts` portal lifecycle with duplicate-conflict detection, permission denial handling, and clean `Session::Close` teardown.
  - Native D-Bus desktop notifications (`org.freedesktop.Notifications` with `notify-send` fallback, respecting OS Do Not Disturb and never exposing chat contents).
  - Single-WebView reparenting across Main Window, Quick Chat, and Floating Companion, `ksni` StatusNotifierItem tray, native KDE Klipper D-Bus + GTK3 clipboard, multi-monitor GDK workarea detection, and `/proc` live telemetry.
- **Efficiency & Resource Lifecycle:**
  - Real Linux WebKitGTK background visibility hiding (`wk.hide()`), `CacheModel::DocumentViewer` in Efficiency mode, transient `MEMORY_CACHE` trimming, glibc `malloc_trim(0)` reclamation, and a single reusable condvar timer for idle suspension checks.
  - Measured `0.000%` settled background tray CPU (Host and Full Process Tree) in Balanced and Efficiency modes on CachyOS Linux.
- **Windows 11 — Preserved & CI-Verified:** Per-user Unicode NSIS installer (`GeminiDesk-1.0.0-Windows-x64-Setup.exe`) and Portable ZIP (`GeminiDesk-1.0.0-Windows-x64-Portable.zip`), preserving WebView2 profiles, settings, and `/S /AUTORESTART` signed updates.
- **macOS (Apple Silicon & Intel) — Experimental / Unsigned:** Native `WKWebView` builds and `.dmg` packages (`GeminiDesk-1.0.0-macOS-AppleSilicon.dmg`, `GeminiDesk-1.0.0-macOS-Intel.dmg`) compiled and packaged on `macos-15` CI. Not yet runtime-tested on physical macOS hardware and **not Apple Developer ID signed or notarized**.
- **Cryptographically Signed Automatic Updates:** Detached Ed25519 signatures (`latest.json` + `latest.json.sig`), SemVer anti-downgrade protection, HTTPS native-TLS downloads, streaming SHA-256 verification, and pre-install TOCTOU re-verification.

## Downloads & Verification

Verify package checksums against `SHA256SUMS.txt`:

```bash
sha256sum --ignore-missing -c SHA256SUMS.txt
```

Verify the detached Ed25519 release manifest signature against GeminiDesk's pinned public verifying key (`ce042ef3aa7286c10ae6fdd6a886cfee2bc7ec09cd24d1862ca878adc5b1b637`):

```bash
python3 scripts/release_manifest.py --verify latest.json latest.json.sig ce042ef3aa7286c10ae6fdd6a886cfee2bc7ec09cd24d1862ca878adc5b1b637
```

## Platform Notes

- **Windows 11:** Requires Microsoft Edge WebView2 Runtime (pre-installed on Windows 11).
- **Linux:** Requires system `webkit2gtk-4.1`, `gtk3`, `openssl`, and `xdg-desktop-portal` (plus `xdg-desktop-portal-kde` on KDE Plasma).
- **macOS (Experimental):** Unsigned / unnotarized CI `.dmg` bundles. macOS Gatekeeper will warn on first launch; macOS runtime behavior and `WKWebView` Google authentication have not yet been verified on real Mac hardware.
