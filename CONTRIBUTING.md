# Contributing to GeminiDesk

Thank you for your interest in contributing to **GeminiDesk** — an independent, unofficial lightweight Rust desktop client for Google Gemini.

## Guiding Principles

1. **Native-first & lightweight:** Keep the Rust host shell minimal, avoid Electron or redundant Chromium engines, and preserve the single-WebView architecture.
2. **Genuine Gemini:** Never intercept Google credentials, extract external browser cookies, spoof browsers to evade Google security checks, or inject persistent scripts into `gemini.google.com`.
3. **Privacy by default:** Sensitive OS capture capabilities (Selection Capture, Screenshot Capture, Clipboard Actions) must remain opt-in, on-demand (never polling), and never auto-submit prompts.
4. **Honest platform claims:** Only label a platform or feature as supported after real-hardware verification. Compilation in CI alone is not runtime verification.
5. **Cohesive design:** Local Settings and Productivity sheets share `src/ui/design-system.css` using bundled Geist & Geist Mono fonts in monochrome charcoal/light palettes.

---

## Development Setup

### Windows 11 (x64)
- Rust stable toolchain (`rustup`) + MSVC Build Tools
- Microsoft Edge WebView2 Runtime (pre-installed on Windows 11)
- Optional: NSIS 3 (`makensis`) to compile the per-user installer

### Linux (Arch / CachyOS — Fish shell)
```fish
sudo pacman -S --needed rust pkgconf webkit2gtk-4.1 gtk3 openssl xdg-desktop-portal xdg-desktop-portal-kde wl-clipboard
```

### Linux (Ubuntu 24.04 / Debian)
```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libssl-dev pkg-config libxdo-dev xdg-desktop-portal
```

### macOS (Apple Silicon / Intel)
- Xcode Command Line Tools (`xcode-select --install`)
- Rust stable toolchain (`rustup`)

---

## Before Opening a Pull Request

Run all checks locally and ensure zero warnings:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Keep commits focused and atomic, update documentation when adding features or changing behavior, and clearly state which OS / desktop environment you actually tested on.
