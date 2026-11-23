# GeminiDesk v1.0.2

> GeminiDesk is an independent open-source desktop client for Google Gemini. It is not developed by, affiliated with, endorsed by, or sponsored by Google LLC.

## Highlights

- **Fixes Signed-In Gemini Response Hangs & Stalls on Linux (`libsoup3` HTTP/2 Workaround):**
  - **Root cause:** Upstream `libsoup3` (3.6.x–3.8.x) HTTP/2 stream multiplexing can stall indefinitely on Google Gemini's signed-in `geminiweb-pa.clients6.google.com/v1/processSession` WebChannel streams (reproduced on real CachyOS Linux with `libsoup3 3.8.1`: **5/8 signed-in prompts hung at 366 bytes** under HTTP/2, while **`SOUP_FORCE_HTTP1=1` achieved 6/6 (100%) reliable streaming** with faster warm ack times `254–272 ms` vs `398–443 ms`).
  - GeminiDesk now enables `SOUP_FORCE_HTTP1=1` automatically on Linux unless explicitly overridden by the user (`SOUP_FORCE_HTTP1=0`).
- **Fixes Disabled Disk Cache in Efficiency Mode on Linux:**
  - Replaced `CacheModel::DocumentViewer` (which completely disabled WebKitGTK's HTTP disk cache) with `CacheModel::DocumentBrowser` in Efficiency mode so Gemini's lazy-loaded JS chunks and UI assets stay cached locally instead of re-downloading.
- **Preserves `v1.0.1` Linux Smoothness & Rendering Fixes:**
  - Retains runtime version-aware `WEBKIT_DMABUF_RENDERER_FORCE_SHM=1` on WebKitGTK ≥ 2.44 (`87–111 FPS` on live Gemini, `1–2 ms` Main ↔ Quick Chat ↔ Companion reparenting with zero black surfaces).
- **Windows 11 — Preserved & CI-Verified:** Per-user Unicode NSIS installer (`GeminiDesk-1.0.2-Windows-x64-Setup.exe`) and Portable ZIP (`GeminiDesk-1.0.2-Windows-x64-Portable.zip`), preserving WebView2 profiles, settings, and `/S /AUTORESTART` signed updates.
- **macOS (Apple Silicon & Intel) — Experimental / Unsigned:** Native `WKWebView` builds and `.dmg` packages (`GeminiDesk-1.0.2-macOS-AppleSilicon.dmg`, `GeminiDesk-1.0.2-macOS-Intel.dmg`) compiled and packaged on `macos-15` CI. Not yet runtime-tested on physical macOS hardware and **not Apple Developer ID signed or notarized**.
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
- **Linux:** Requires system `webkit2gtk-4.1`, `gtk3`, `openssl`, and `xdg-desktop-portal` (plus `xdg-desktop-portal-kde` on KDE Plasma). Users upgrading from `v1.0.0`/`v1.0.1` AppImage can use **Settings → About → Check for Updates** to download and verify `v1.0.2` automatically.
- **macOS (Experimental):** Unsigned / unnotarized CI `.dmg` bundles. macOS Gatekeeper will warn on first launch; macOS runtime behavior and `WKWebView` Google authentication have not yet been verified on real Mac hardware.
