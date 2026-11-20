# GeminiDesk v1.0.1

> GeminiDesk is an independent open-source desktop client for Google Gemini. It is not developed by, affiliated with, endorsed by, or sponsored by Google LLC.

## Highlights

- **Linux UI Smoothness & Frame Pacing Fix (CachyOS / KDE Plasma 6 / Wayland Verified):**
  - **Root cause fixed:** In `v1.0.0`, `WEBKIT_DISABLE_DMABUF_RENDERER=1` disabled WebKitGTK's accelerated compositing pipeline entirely, causing live `gemini.google.com/app` rendering to drop to **13–20 FPS** (36–78 ms median frame times).
  - On WebKitGTK ≥ 2.44, GeminiDesk now uses `WEBKIT_DMABUF_RENDERER_FORCE_SHM=1` automatically (falling back to `WEBKIT_DISABLE_DMABUF_RENDERER=1` only on WebKitGTK < 2.44 or when `GEMINIDESK_SOFTWARE_RENDER=1` is set, and never overriding user-specified `WEBKIT_*` variables).
  - **Measured improvement on real CachyOS + KDE Plasma 6 Wayland + AMD Radeon RX 6600 XT (`239.76 Hz`, `webkit2gtk-4.1 2.54.3`):**
    - Typing: **`19.9 FPS → 87.0 FPS`** (input-to-paint latency **`36 ms → 15 ms`** P50, **`87 ms → 25 ms`** P95)
    - Scrolling: **`16.7 FPS → 110.6 FPS`** (frames > 16.7 ms: **`100% → 1.4%`**)
    - Sidebar transitions: **`13.3 FPS → 105.2 FPS`** (frames > 16.7 ms: **`100% → 0.9%`**)
    - Multi-window reparenting (`Main ↔ Quick Chat ↔ Floating Companion`): **`1–2 ms`** with zero black/blank surfaces, and background tray CPU remains **`0.03%–0.67%`**.
  - Preserves WebKit's decoded `MEMORY_CACHE` across transient Balanced-mode hides / Quick Chat toggles (reserving `MEMORY_CACHE` trimming for delayed Efficiency idle suspension) and skips redundant GTK `show()`/`hide()` redraws.
- **Windows 11 — Preserved & CI-Verified:** Per-user Unicode NSIS installer (`GeminiDesk-1.0.1-Windows-x64-Setup.exe`) and Portable ZIP (`GeminiDesk-1.0.1-Windows-x64-Portable.zip`), preserving WebView2 profiles, settings, and `/S /AUTORESTART` signed updates.
- **macOS (Apple Silicon & Intel) — Experimental / Unsigned:** Native `WKWebView` builds and `.dmg` packages (`GeminiDesk-1.0.1-macOS-AppleSilicon.dmg`, `GeminiDesk-1.0.1-macOS-Intel.dmg`) compiled and packaged on `macos-15` CI. Not yet runtime-tested on physical macOS hardware and **not Apple Developer ID signed or notarized**.
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
- **Linux:** Requires system `webkit2gtk-4.1`, `gtk3`, `openssl`, and `xdg-desktop-portal` (plus `xdg-desktop-portal-kde` on KDE Plasma). Users upgrading from `v1.0.0` AppImage can use **Settings → About → Check for Updates** to download and verify `v1.0.1` automatically.
- **macOS (Experimental):** Unsigned / unnotarized CI `.dmg` bundles. macOS Gatekeeper will warn on first launch; macOS runtime behavior and `WKWebView` Google authentication have not yet been verified on real Mac hardware.
