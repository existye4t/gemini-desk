## GeminiDesk v0.4.0 (Public Beta Candidate)

> **Disclaimer:** GeminiDesk is an independent, unofficial desktop client for Google Gemini. It is not affiliated with, endorsed by, or sponsored by Google LLC.

### Highlights

- **Windows 11 (Verified):** Per-user Unicode NSIS installer (zero UAC admin prompt, WebView2 detection, upgrades preserve Google sign-in & settings), embedded monochrome icon & `VERSIONINFO`, and portable ZIP.
- **Linux — CachyOS / Arch / KDE Plasma Wayland (Experimental — Real-Device Testing Requested):** Native `WebKitGTK` single-WebView reparenting, XDG Desktop Portals (`GlobalShortcuts`, interactive `Screenshot`, `Settings` color-scheme), pure-Rust `ksni` StatusNotifierItem tray, Wayland/X11 clipboard + primary selection, `/proc` process telemetry, Arch `PKGBUILD`, `.deb`, `.rpm`, and lean `AppImage`.
- **macOS — Apple Silicon & Intel (Experimental — Real-Device Testing Requested):** Native `WKWebView` reparenting, macOS App Menu Bar, Dock reopen handling, LaunchAgent startup, interactive `screencapture -i`, `.app` + `.dmg` bundle pipeline with Hardened Runtime & conditional Developer ID signing/notarization.
- **Secure Automatic Updates:** Ed25519-signed release manifests (`latest.json` + `latest.json.sig`), strict SemVer anti-downgrade protection, HTTPS-only downloads, SHA-256 verification, and Settings → About update controls.

### Verification & Checksums

- Verify artifact integrity against `SHA256SUMS.txt`.
- Official signed releases verify `latest.json.sig` with GeminiDesk's pinned Ed25519 public key.
- **macOS Gatekeeper note:** Builds created without Apple Developer ID signing and notarization secrets are unsigned development/beta bundles and are not Gatekeeper-notarized.
- **Linux / macOS Google Sign-In note:** Google authentication is verified on Windows 11 (WebView2). WebKitGTK and WKWebView authentication compatibility must be verified on real hardware before removing the *Experimental* label.
