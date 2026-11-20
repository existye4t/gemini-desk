## Summary

- What changed and why?

## Platform Verification Status

Check only environments where you **actually executed and verified** the change:

- [ ] Windows 11 (x64 · WebView2)
- [ ] Linux — KDE Plasma Wayland (CachyOS / Arch · WebKitGTK)
- [ ] Linux — X11 / GNOME
- [ ] macOS — Apple Silicon (WKWebView)
- [ ] macOS — Intel
- [ ] Automated CI / unit tests only

## Security & Quality Checklist

- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --all-targets -- -D warnings` passes
- [ ] `cargo test` passes
- [ ] No credentials, cookies, or private keys touched or logged
- [ ] No continuous background polling or duplicate Gemini renderers introduced
- [ ] Existing user settings and Google WebView session profiles remain preserved
