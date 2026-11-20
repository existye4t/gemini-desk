## GeminiDesk v0.4.0 (Public Beta Candidate)

> **Disclaimer:** GeminiDesk is an independent, unofficial desktop client for Google Gemini. It is not affiliated with, endorsed by, or sponsored by Google LLC.

### Highlights

- **Windows 11 (Verified):** Per-user Unicode NSIS installer (`GeminiDesk-0.4.0-Windows-x64-Setup.exe`, zero UAC admin prompt, WebView2 detection, silent `/S /AUTORESTART` update support, upgrades preserve Google sign-in & settings), embedded monochrome icon & `VERSIONINFO`, and portable ZIP (`GeminiDesk-0.4.0-Windows-x64-Portable.zip`).
- **Linux — CachyOS / Arch / KDE Plasma Wayland (Experimental — Real-Device Testing Requested):** Native `WebKitGTK 4.1` single-WebView reparenting, XDG Desktop Portals (`GlobalShortcuts`, interactive `Screenshot`, `Settings` color-scheme), pure-Rust `ksni` StatusNotifierItem tray, Wayland/X11 clipboard + primary selection, `/proc` process telemetry, Arch `PKGBUILD`, `GeminiDesk-0.4.0-Linux-x86_64.deb`, `GeminiDesk-0.4.0-Linux-x86_64.rpm`, `GeminiDesk-0.4.0-Linux-x86_64.AppImage`, and `GeminiDesk-0.4.0-Linux-x86_64.tar.gz`.
- **macOS — Apple Silicon & Intel (Experimental — Real-Device Testing Requested):** Native `WKWebView` reparenting, macOS App Menu Bar, Dock reopen handling, LaunchAgent startup, interactive `screencapture -i`, `.app` + `.dmg` bundle pipeline (`GeminiDesk-0.4.0-macOS-AppleSilicon.dmg`, `GeminiDesk-0.4.0-macOS-Intel.dmg`) with Hardened Runtime & conditional Developer ID signing/notarization.
- **Secure Automatic Updates:** Ed25519-signed release manifests (`latest.json` + `latest.json.sig`), strict SemVer anti-downgrade protection, trusted GitHub Releases HTTPS host allowlist, SHA-256 verification, and Settings → About update controls.

### Verification & Checksums

- Verify artifact integrity against `SHA256SUMS.txt`:
  ```bash
  sha256sum -c SHA256SUMS.txt
  ```
- Verify `latest.json` and `latest.json.sig` against GeminiDesk's pinned Ed25519 public key (`ce042ef3aa7286c10ae6fdd6a886cfee2bc7ec09cd24d1862ca878adc5b1b637`):
  ```bash
  python3 scripts/release_manifest.py --verify latest.json latest.json.sig ce042ef3aa7286c10ae6fdd6a886cfee2bc7ec09cd24d1862ca878adc5b1b637
  ```
- **macOS Gatekeeper note:** Builds created without Apple Developer ID signing and notarization secrets are unsigned development/beta bundles and are not Gatekeeper-notarized.
- **Linux / macOS Google Sign-In note:** Google authentication is verified on Windows 11 (WebView2). WebKitGTK and WKWebView authentication compatibility must be verified on real hardware before removing the *Experimental* label.
