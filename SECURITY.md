# Security Policy

GeminiDesk is designed around strict isolation between Google's remote web application (`gemini.google.com` / `accounts.google.com`) and local desktop integrations.

## Security Boundaries & Guarantees

- **Zero credential interception:** Google authentication happens directly inside the native OS WebView. GeminiDesk never reads passwords, never extracts cookies from external browsers, and never runs a custom backend.
- **Strict domain allowlist (`src/navigation.rs`):** Only Google's official authentication domains (the 187 official regional SSO `supported_domains`), Gemini app origins, and Google Workspace file pickers are permitted in-app. External URLs open in your system default browser; unsafe schemes (`file:`, `javascript:`, `data:`) are blocked.
- **Redacted logs:** URL query strings and fragments are stripped before writing logs so OAuth/session parameters are never printed.
- **Isolated local UIs (`src/ui_assets.rs`):** Settings and Prompt Review sheets run in lazy-created local WebViews served from memory via `geminidesk://` with strict CSP (`default-src 'none'`), zero remote network access, and zero IPC exposed to `gemini.google.com`.
- **Opt-in OS capture:** Selection Capture (`CurrentIsPassword` protected), Screenshot Capture (explicit interactive selection only), and Clipboard Actions never poll in the background, never store history, never log captured text, and never auto-submit prompts.
- **Cryptographically verified updates (`src/updater/mod.rs`):** Release metadata (`latest.json`) is verified via a detached **Ed25519** signature (`latest.json.sig`) against a compile-time pinned public key before parsing or downloading over HTTPS. Artifacts are streamed to `.part` files, verified against exact byte size and **SHA-256** digest, and re-verified before installer handoff. Unsigned development builds fail closed.

## Reporting a Vulnerability

Please **do not** open a public GitHub issue for security vulnerabilities.

Instead, report security concerns privately through GitHub Security Advisories (once hosted) or contact the project maintainers directly with:

1. Affected version, OS, and WebView runtime version.
2. Clear reproduction steps or proof-of-concept.
3. Expected security boundary vs. observed behavior.

We aim to acknowledge reports within 72 hours and coordinate a fix before public disclosure.
