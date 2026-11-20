# Security Policy

GeminiDesk is designed around strict isolation between Google's remote web application (`gemini.google.com` / `accounts.google.com`) and local desktop integrations.

## Security Boundaries & Guarantees

- **Zero credential interception:** Google authentication happens directly inside the native OS WebView. GeminiDesk never reads passwords, never extracts cookies from external browsers, and never runs a custom backend.
- **Strict domain allowlist (`src/navigation.rs`):** Only Google's official authentication domains (the 187 official regional SSO `supported_domains`), Gemini app origins, and Google Workspace file pickers are permitted in-app. External URLs open in your system default browser; unsafe schemes (`file:`, `javascript:`, `data:`) are blocked.
- **Redacted logs:** URL query strings and fragments are stripped before writing logs so OAuth/session parameters are never printed.
- **Isolated local UIs (`src/ui_assets.rs`):** Settings and Prompt Review sheets run in lazy-created local WebViews served from memory via `geminidesk://` with strict CSP (`default-src 'none'`), zero remote network access, and zero IPC exposed to `gemini.google.com`.
- **Safe file downloads:** Gemini file exports sanitize path separators, traversal sequences, and reserved Windows device names before saving to your OS `Downloads` directory.
- **Opt-in OS capture:** Selection Capture (`CurrentIsPassword` protected), Screenshot Capture (explicit interactive selection only), and Clipboard Actions never poll in the background, never store history, never log captured text, and never auto-submit prompts.
- **Cryptographically verified updates (`src/updater/mod.rs`):**
  - Detached **Ed25519** signature verification (`latest.json.sig`) against the pinned verifying key before parsing release metadata.
  - Strict Semantic Versioning anti-downgrade enforcement (`remote > current` and `current >= min_supported_version`).
  - HTTPS-only downloads verified against the OS native TLS certificate store and pinned to trusted GitHub Releases hosts (`github.com`, `objects.githubusercontent.com`, `release-assets.githubusercontent.com`).
  - Streaming `.part` download verification against exact byte length and **SHA-256** digest, plus pre-install on-disk SHA-256 re-verification before execution.
- **Hardened CI/CD:** GitHub Actions workflows use least-privilege `contents: read` permissions by default, pin third-party actions to reviewed commit SHAs, keep release signing secrets strictly isolated from pull-request builds, and prepare releases as drafts requiring explicit manual publication.

## Key Management & Rotation Strategy

- **Active Ed25519 Release Verifying Key:** `ce042ef3aa7286c10ae6fdd6a886cfee2bc7ec09cd24d1862ca878adc5b1b637`
- **Private Key Storage:** Stored strictly offline / inside encrypted GitHub Actions Repository Secrets (`UPDATER_SIGNING_PRIVATE_KEY`) — never committed to source control or printed in logs.
- **Planned Key Rotation Procedure:**
  1. Generate a replacement keypair offline (`python scripts/release_manifest.py --generate-keypair`) without overwriting the outgoing key yet.
  2. Publish a **bridge release** signed with the **outgoing** private key whose binary already trusts the **incoming** public key (set `GEMINIDESK_UPDATER_PUBKEY=<new_pubkey>` during CI build while signing `latest.json` with the outgoing key), setting `min_supported_version` cleanly.
  3. After users upgrade to the bridge release, replace `UPDATER_SIGNING_PRIVATE_KEY` in GitHub Secrets and `DEFAULT_UPDATER_PUBKEY_HEX` in `src/updater/mod.rs` with the incoming keypair.
- **Emergency Key Revocation / Recovery:** Immediately revoke the compromised secret in GitHub Settings → Secrets, rotate both `UPDATER_SIGNING_PRIVATE_KEY` and `GEMINIDESK_UPDATER_PUBKEY`, commit the updated `DEFAULT_UPDATER_PUBKEY_HEX`, and advise users in the next release notes to install the signed installer manually. Silent key swaps without a bridge release or explicit user release are intentionally unsupported.

## Reporting a Vulnerability

Please **do not** open a public GitHub issue for security vulnerabilities.

Instead, report security concerns privately through [GitHub Security Advisories](https://github.com/existye4t/gemini-desk/security/advisories/new) with:

1. Affected version, OS, and WebView runtime version.
2. Clear reproduction steps or proof-of-concept.
3. Expected security boundary vs. observed behavior.

We aim to acknowledge reports within 72 hours and coordinate a fix before public disclosure.
