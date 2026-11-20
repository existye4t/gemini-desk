//! Cryptographically authenticated automatic update system for GeminiDesk.
//!
//! Security model (fail-closed):
//! 1. **Ed25519 Signature Verification:** Update metadata (`latest.json`) is authenticated
//!    using a detached Ed25519 signature (`latest.json.sig`) verified against a compile-time
//!    pinned public key BEFORE trusting any version number, URL, or SHA-256 hash.
//! 2. **HTTPS Enforcement:** Both manifest URLs and artifact download URLs must use `https://`
//!    (verified via the OS native TLS certificate store — never disabling certificate checks).
//! 3. **Anti-Downgrade Protection:** Strict `semver::Version` comparison rejects any release
//!    whose version is less than or equal to the running version, and enforces `min_supported_version`.
//! 4. **Verified Streaming Download & Recovery:** Downloads write into `<updates_dir>/<file>.part`
//!    with bounded maximum size and incremental SHA-256 verification. Corrupted or interrupted
//!    `.part` files are discarded and cleaned up; an unverified binary is NEVER executed.
//! 5. **User Consent:** Updates are never silently installed without explicit user confirmation.
//!    User settings, custom templates, and WebView authentication profiles are always preserved.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

/// Default release manifest endpoint (overridable at build time via `GEMINIDESK_UPDATE_MANIFEST_URL`).
pub const DEFAULT_MANIFEST_URL: &str =
    "https://github.com/existye4t/gemini-desk/releases/latest/download/latest.json";

/// Compile-time pinned Ed25519 public verifying key (64 hex chars = 32 bytes).
/// Overridable at compile time via `GEMINIDESK_UPDATER_PUBKEY` for key rotation.
const EMBEDDED_PUBKEY_HEX: Option<&str> = option_env!("GEMINIDESK_UPDATER_PUBKEY");
const DEFAULT_UPDATER_PUBKEY_HEX: Option<&str> =
    Some("ce042ef3aa7286c10ae6fdd6a886cfee2bc7ec09cd24d1862ca878adc5b1b637");

const MAX_MANIFEST_BYTES: usize = 256 * 1024;
const MAX_SIG_BYTES: usize = 512;
const MAX_ARTIFACT_BYTES: u64 = 256 * 1024 * 1024; // 256 MiB hard safety cap
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

/// Artifact packaging kind determining how GeminiDesk hands off a verified update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    NsisInstaller,
    PortableZip,
    MacosDmg,
    LinuxAppImage,
    LinuxDeb,
    LinuxRpm,
    LinuxTarball,
}

/// Platform-specific downloadable release artifact inside a signed `UpdateManifest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactEntry {
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub kind: ArtifactKind,
    #[serde(default)]
    pub filename: Option<String>,
}

/// Signed release metadata manifest (`latest.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateManifest {
    pub version: String,
    pub pub_date: String,
    pub release_notes: String,
    #[serde(default)]
    pub min_supported_version: Option<String>,
    pub artifacts: HashMap<String, ArtifactEntry>,
}

/// Serializable update status pushed to `Settings -> About`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum UpdateStatus {
    Idle {
        configured: bool,
        last_checked_unix: Option<u64>,
    },
    NotConfigured {
        message: String,
    },
    Checking,
    UpToDate {
        current_version: String,
        checked_at_unix: u64,
    },
    Available {
        current_version: String,
        latest_version: String,
        pub_date: String,
        release_notes: String,
        artifact_size: u64,
        artifact_kind: ArtifactKind,
    },
    Downloading {
        version: String,
        downloaded_bytes: u64,
        total_bytes: u64,
    },
    ReadyToInstall {
        version: String,
        pub_date: String,
        release_notes: String,
        verified_path: String,
        sha256_prefix: String,
        artifact_kind: ArtifactKind,
    },
    Error {
        message: String,
    },
}

/// Verified update candidate held in memory until the user confirms Download / Install.
#[derive(Debug, Clone)]
pub struct VerifiedUpdateOffer {
    pub version: String,
    pub pub_date: String,
    pub release_notes: String,
    pub artifact: ArtifactEntry,
}

/// Runtime updater configuration.
#[derive(Debug, Clone)]
pub struct UpdaterConfig {
    pub manifest_url: String,
    pub verifying_key_bytes: Option<[u8; 32]>,
    pub current_version: String,
    pub platform_key: String,
    pub updates_dir: PathBuf,
    #[cfg(test)]
    pub allow_insecure_http_for_tests: bool,
}

impl UpdaterConfig {
    pub fn for_current_build(updates_dir: PathBuf) -> Self {
        let manifest_url = option_env!("GEMINIDESK_UPDATE_MANIFEST_URL")
            .unwrap_or(DEFAULT_MANIFEST_URL)
            .to_string();
        let verifying_key_bytes = EMBEDDED_PUBKEY_HEX
            .filter(|s| !s.trim().is_empty())
            .or(DEFAULT_UPDATER_PUBKEY_HEX)
            .and_then(parse_hex_32);

        Self {
            manifest_url,
            verifying_key_bytes,
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            platform_key: current_platform_key().to_string(),
            updates_dir,
            #[cfg(test)]
            allow_insecure_http_for_tests: false,
        }
    }

    pub fn is_signing_configured(&self) -> bool {
        self.verifying_key_bytes.is_some()
    }
}

/// Canonical `(os-arch)` artifact lookup key matching release CI output.
pub const fn current_platform_key() -> &'static str {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "windows-x86_64"
    }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        "windows-aarch64"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "linux-x86_64"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "linux-aarch64"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "macos-aarch64"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "macos-x86_64"
    }
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "aarch64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
    )))]
    {
        "unsupported"
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Removes stale `.part` partial downloads left behind by crashed or interrupted updates.
pub fn cleanup_stale_partial_downloads(updates_dir: &Path) {
    let Ok(entries) = fs::read_dir(updates_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("part") {
            let _ = fs::remove_file(&path);
        }
    }
}

/// Checks for an authenticated update over HTTPS.
/// Returns `Ok(Some(offer))` if a verified newer release exists for this platform,
/// or `Ok(None)` if already up to date.
pub fn check_for_update(cfg: &UpdaterConfig) -> Result<Option<VerifiedUpdateOffer>, String> {
    let Some(pubkey_bytes) = cfg.verifying_key_bytes else {
        return Err(
            "Update signing key not configured in this build (unsigned development build).".into(),
        );
    };

    validate_download_url(&cfg.manifest_url, cfg)?;
    let sig_url = format!("{}.sig", cfg.manifest_url);
    validate_download_url(&sig_url, cfg)?;

    let agent = build_http_agent();
    let manifest_bytes = fetch_bounded_bytes(&agent, &cfg.manifest_url, MAX_MANIFEST_BYTES)?;
    let sig_bytes = fetch_bounded_bytes(&agent, &sig_url, MAX_SIG_BYTES)?;

    evaluate_signed_manifest(&manifest_bytes, &sig_bytes, &pubkey_bytes, cfg)
}

/// Verifies a raw `latest.json` payload against its detached Ed25519 signature, enforces
/// SemVer anti-downgrade rules, validates HTTPS artifact URLs, and selects the current platform.
pub fn evaluate_signed_manifest(
    manifest_bytes: &[u8],
    sig_raw: &[u8],
    pubkey_bytes: &[u8; 32],
    cfg: &UpdaterConfig,
) -> Result<Option<VerifiedUpdateOffer>, String> {
    let verifying_key = VerifyingKey::from_bytes(pubkey_bytes)
        .map_err(|e| format!("Invalid embedded Ed25519 verifying key: {e}"))?;
    let signature = parse_signature(sig_raw)?;

    // Step 1: Cryptographic authenticity check BEFORE deserializing untrusted JSON.
    verifying_key
        .verify(manifest_bytes, &signature)
        .map_err(|_| {
            "Update manifest signature verification failed (possible tampering).".to_string()
        })?;

    // Step 2: Parse authenticated JSON manifest.
    let manifest: UpdateManifest = serde_json::from_slice(manifest_bytes)
        .map_err(|e| format!("Authenticated manifest JSON invalid: {e}"))?;

    // Step 3: Strict SemVer comparison & anti-downgrade check.
    let current = Version::parse(&cfg.current_version)
        .map_err(|e| format!("Invalid running version '{}': {e}", cfg.current_version))?;
    let remote = Version::parse(manifest.version.trim_start_matches('v'))
        .map_err(|e| format!("Invalid remote version '{}': {e}", manifest.version))?;

    if remote <= current {
        return Ok(None);
    }

    if let Some(min_str) = &manifest.min_supported_version {
        let min_ver = Version::parse(min_str.trim_start_matches('v'))
            .map_err(|e| format!("Invalid min_supported_version '{min_str}': {e}"))?;
        if current < min_ver {
            return Err(format!(
                "Automatic upgrade to v{remote} requires v{min_ver} or newer; please download manually."
            ));
        }
    }

    // Step 4: Platform binding & HTTPS + hash format validation.
    let artifact = manifest
        .artifacts
        .get(&cfg.platform_key)
        .cloned()
        .ok_or_else(|| {
            format!(
                "Release v{remote} has no verified package for platform '{}'.",
                cfg.platform_key
            )
        })?;

    validate_download_url(&artifact.url, cfg)?;
    if artifact.size == 0 || artifact.size > MAX_ARTIFACT_BYTES {
        return Err(format!(
            "Update artifact size ({} bytes) is outside safe bounds.",
            artifact.size
        ));
    }
    if parse_hex_32(&artifact.sha256).is_none() {
        return Err("Update manifest contains malformed SHA-256 digest.".into());
    }

    Ok(Some(VerifiedUpdateOffer {
        version: remote.to_string(),
        pub_date: manifest.pub_date,
        release_notes: manifest.release_notes,
        artifact,
    }))
}

/// Downloads a verified update artifact into `<updates_dir>`, verifies exact byte length and
/// SHA-256 digest before atomically promoting from `.part` to final path.
/// Never leaves an unverified file at the final path.
pub fn download_and_verify_artifact(
    offer: &VerifiedUpdateOffer,
    cfg: &UpdaterConfig,
) -> Result<PathBuf, String> {
    validate_download_url(&offer.artifact.url, cfg)?;
    let expected_sha = parse_hex_32(&offer.artifact.sha256)
        .ok_or_else(|| "Malformed expected SHA-256 digest".to_string())?;

    fs::create_dir_all(&cfg.updates_dir)
        .map_err(|e| format!("Failed creating updates staging directory: {e}"))?;
    cleanup_stale_partial_downloads(&cfg.updates_dir);

    let safe_name = sanitize_artifact_filename(
        offer
            .artifact
            .filename
            .as_deref()
            .unwrap_or_else(|| default_artifact_name(&offer.version, offer.artifact.kind)),
    );
    let final_path = cfg.updates_dir.join(&safe_name);
    let part_path = cfg.updates_dir.join(format!("{safe_name}.part"));

    // If a previously downloaded file already exists and matches exact SHA-256, reuse it!
    if final_path.exists()
        && verify_file_sha256(&final_path, &expected_sha, offer.artifact.size).is_ok()
    {
        info!(path = %final_path.display(), "Reusing already-verified cached update artifact");
        return Ok(final_path);
    }

    let agent = build_http_agent();
    let resp = agent
        .get(&offer.artifact.url)
        .call()
        .map_err(|e| format!("Update download request failed: {e}"))?;

    let mut reader = resp.into_body().into_reader();
    let mut part_file = File::create(&part_path)
        .map_err(|e| format!("Failed creating partial update file: {e}"))?;

    let mut hasher = Sha256::new();
    let mut written: u64 = 0;
    let mut buf = [0u8; 32 * 1024];

    let write_res = (|| -> Result<(), String> {
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| format!("Interrupted while downloading update: {e}"))?;
            if n == 0 {
                break;
            }
            written = written.saturating_add(n as u64);
            if written > offer.artifact.size || written > MAX_ARTIFACT_BYTES {
                return Err("Downloaded artifact exceeded declared manifest size.".into());
            }
            hasher.update(&buf[..n]);
            part_file
                .write_all(&buf[..n])
                .map_err(|e| format!("Failed writing update payload: {e}"))?;
        }
        part_file
            .flush()
            .map_err(|e| format!("Failed flushing update payload: {e}"))?;
        Ok(())
    })();

    drop(part_file);

    if let Err(err) = write_res {
        let _ = fs::remove_file(&part_path);
        return Err(err);
    }

    if written != offer.artifact.size {
        let _ = fs::remove_file(&part_path);
        return Err(format!(
            "Incomplete download: expected {} bytes, received {written} bytes.",
            offer.artifact.size
        ));
    }

    let digest: [u8; 32] = hasher.finalize().into();
    if digest != expected_sha {
        let _ = fs::remove_file(&part_path);
        return Err("SHA-256 verification failed: downloaded artifact was rejected.".into());
    }

    // Atomic replacement (on Windows, remove stale destination first).
    if final_path.exists() {
        let _ = fs::remove_file(&final_path);
    }
    fs::rename(&part_path, &final_path)
        .map_err(|e| format!("Failed promoting verified update file: {e}"))?;

    info!(
        version = %offer.version,
        path = %final_path.display(),
        bytes = written,
        "Update artifact downloaded and cryptographically verified"
    );
    Ok(final_path)
}

/// Re-verifies the on-disk file's exact size and SHA-256 immediately prior to installation
/// (mitigating TOCTOU tampering between download and install), then hands off safely to the OS.
/// Returns `Ok(true)` when the application should exit so the installer can replace files cleanly.
pub fn launch_verified_installer(
    verified_path: &Path,
    expected_sha256_hex: &str,
    expected_size: u64,
    kind: ArtifactKind,
) -> Result<bool, String> {
    let expected_sha = parse_hex_32(expected_sha256_hex)
        .ok_or_else(|| "Invalid SHA-256 digest for installer verification".to_string())?;
    verify_file_sha256(verified_path, &expected_sha, expected_size)?;

    match kind {
        ArtifactKind::NsisInstaller => {
            #[cfg(target_os = "windows")]
            {
                std::process::Command::new(verified_path)
                    .args(["/S", "/AUTORESTART"])
                    .spawn()
                    .map_err(|e| format!("Failed launching verified Windows installer: {e}"))?;
                Ok(true)
            }
            #[cfg(not(target_os = "windows"))]
            {
                Err("NSIS installer is Windows-only.".into())
            }
        }
        ArtifactKind::MacosDmg => {
            open::that(verified_path)
                .map_err(|e| format!("Failed opening verified macOS DMG: {e}"))?;
            Ok(false)
        }
        ArtifactKind::LinuxAppImage => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Err(err) =
                    fs::set_permissions(verified_path, fs::Permissions::from_mode(0o755))
                {
                    warn!(?err, "Could not mark verified AppImage executable");
                }
            }
            let target = verified_path.parent().unwrap_or(verified_path);
            if let Err(err) = open::that(target) {
                warn!(?err, "Could not open updates folder automatically");
            }
            Ok(false)
        }
        ArtifactKind::PortableZip
        | ArtifactKind::LinuxDeb
        | ArtifactKind::LinuxRpm
        | ArtifactKind::LinuxTarball => {
            // Reveal the verified package/folder cleanly for user-controlled installation.
            let target = verified_path.parent().unwrap_or(verified_path);
            if let Err(err) = open::that(target) {
                warn!(?err, "Could not open updates folder automatically");
            }
            Ok(false)
        }
    }
}

pub fn verify_file_sha256(
    path: &Path,
    expected: &[u8; 32],
    expected_size: u64,
) -> Result<(), String> {
    let meta = fs::metadata(path).map_err(|e| format!("Cannot read update file metadata: {e}"))?;
    if meta.len() != expected_size {
        return Err("Update file size mismatch during pre-install re-verification.".into());
    }
    let mut file = File::open(path).map_err(|e| format!("Cannot open update file: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 32 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Failed reading update file: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual: [u8; 32] = hasher.finalize().into();
    if &actual != expected {
        return Err("Pre-install SHA-256 re-verification failed; refusing to execute.".into());
    }
    Ok(())
}

fn build_http_agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::NativeTls)
        .build();
    let config = ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(HTTP_TIMEOUT))
        .user_agent(format!("GeminiDesk/{}", env!("CARGO_PKG_VERSION")))
        .build();
    ureq::Agent::new_with_config(config)
}

fn fetch_bounded_bytes(
    agent: &ureq::Agent,
    url: &str,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let resp = agent
        .get(url)
        .call()
        .map_err(|e| format!("HTTPS request failed: {e}"))?;
    let mut out = Vec::new();
    let mut limited = resp.into_body().into_reader().take((max_bytes + 1) as u64);
    limited
        .read_to_end(&mut out)
        .map_err(|e| format!("Failed reading response body: {e}"))?;
    if out.len() > max_bytes {
        return Err(format!(
            "Remote payload exceeded {max_bytes} byte safety limit."
        ));
    }
    Ok(out)
}

fn validate_download_url(raw: &str, cfg: &UpdaterConfig) -> Result<(), String> {
    let parsed = url::Url::parse(raw).map_err(|e| format!("Invalid update URL: {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "Update URL missing host.".to_string())?;
    let is_https = parsed.scheme() == "https";

    #[cfg(test)]
    if cfg.allow_insecure_http_for_tests
        && parsed.scheme() == "http"
        && matches!(host, "127.0.0.1" | "localhost")
    {
        return Ok(());
    }

    if !is_https {
        return Err(format!(
            "Rejected non-HTTPS update URL scheme '{}'.",
            parsed.scheme()
        ));
    }

    let manifest_host = url::Url::parse(&cfg.manifest_url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()));
    let lower_host = host.to_ascii_lowercase();
    let trusted_host = matches!(
        lower_host.as_str(),
        "github.com" | "objects.githubusercontent.com" | "release-assets.githubusercontent.com"
    ) || manifest_host.as_deref() == Some(lower_host.as_str());

    if !trusted_host {
        return Err(format!("Rejected untrusted update download host '{host}'."));
    }

    Ok(())
}

fn parse_signature(raw: &[u8]) -> Result<Signature, String> {
    if raw.len() == 64 {
        let arr: [u8; 64] = raw.try_into().map_err(|_| "Invalid signature length")?;
        return Ok(Signature::from_bytes(&arr));
    }
    let text = std::str::from_utf8(raw)
        .map_err(|_| "Update signature is neither raw 64-byte Ed25519 nor UTF-8 hex")?
        .trim();
    if text.len() == 128 {
        let bytes =
            parse_hex_vec(text).ok_or_else(|| "Invalid hex Ed25519 signature".to_string())?;
        let arr: [u8; 64] = bytes
            .try_into()
            .map_err(|_| "Invalid hex signature length")?;
        return Ok(Signature::from_bytes(&arr));
    }
    Err("Unsupported Ed25519 signature encoding (expected 64 raw bytes or 128 hex chars).".into())
}

fn parse_hex_32(hex: &str) -> Option<[u8; 32]> {
    let trimmed = hex.trim();
    if trimmed.len() != 64 {
        return None;
    }
    let bytes = parse_hex_vec(trimmed)?;
    bytes.try_into().ok()
}

fn parse_hex_vec(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    let bytes = hex.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = hex_nibble(bytes[i])?;
        let lo = hex_nibble(bytes[i + 1])?;
        out.push((hi << 4) | lo);
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn default_artifact_name(version: &str, kind: ArtifactKind) -> &'static str {
    let _ = version;
    match kind {
        ArtifactKind::NsisInstaller => "GeminiDesk-Setup.exe",
        ArtifactKind::PortableZip => "GeminiDesk-Portable.zip",
        ArtifactKind::MacosDmg => "GeminiDesk.dmg",
        ArtifactKind::LinuxAppImage => "GeminiDesk.AppImage",
        ArtifactKind::LinuxDeb => "gemini-desk.deb",
        ArtifactKind::LinuxRpm => "gemini-desk.rpm",
        ArtifactKind::LinuxTarball => "gemini-desk.tar.gz",
    }
}

fn sanitize_artifact_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    if cleaned.is_empty() || cleaned.starts_with('.') {
        "geminidesk-update.bin".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::net::TcpListener;
    use std::thread;

    fn test_keypair() -> (SigningKey, [u8; 32]) {
        let seed = [0x42u8; 32];
        let sk = SigningKey::from_bytes(&seed);
        let pk = sk.verifying_key().to_bytes();
        (sk, pk)
    }

    fn sha256_hex(data: &[u8]) -> String {
        let digest = Sha256::digest(data);
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn sample_manifest(version: &str, url: &str, payload: &[u8]) -> Vec<u8> {
        let mut artifacts = HashMap::new();
        artifacts.insert(
            "windows-x86_64".to_string(),
            ArtifactEntry {
                url: url.to_string(),
                sha256: sha256_hex(payload),
                size: payload.len() as u64,
                kind: ArtifactKind::NsisInstaller,
                filename: Some(format!("GeminiDesk-Setup-{version}.exe")),
            },
        );
        let manifest = UpdateManifest {
            version: version.to_string(),
            pub_date: "2026-11-20T12:00:00Z".to_string(),
            release_notes: "Cross-platform beta with secure updates.".to_string(),
            min_supported_version: Some("0.3.0".to_string()),
            artifacts,
        };
        serde_json::to_vec(&manifest).unwrap()
    }

    fn test_cfg(
        pk: [u8; 32],
        current: &str,
        platform: &str,
        updates_dir: PathBuf,
    ) -> UpdaterConfig {
        UpdaterConfig {
            manifest_url: "https://example.invalid/latest.json".to_string(),
            verifying_key_bytes: Some(pk),
            current_version: current.to_string(),
            platform_key: platform.to_string(),
            updates_dir,
            allow_insecure_http_for_tests: false,
        }
    }

    #[test]
    fn verifies_valid_signed_manifest_and_detects_upgrade() {
        let (sk, pk) = test_keypair();
        let payload = b"verified-installer-bytes";
        let manifest_bytes = sample_manifest(
            "0.5.0",
            "https://github.com/existye4t/gemini-desk/releases/download/v0.5.0/setup.exe",
            payload,
        );
        let sig = sk.sign(&manifest_bytes).to_bytes();
        let cfg = test_cfg(pk, "0.4.0", "windows-x86_64", std::env::temp_dir());

        let offer = evaluate_signed_manifest(&manifest_bytes, &sig, &pk, &cfg)
            .expect("valid manifest")
            .expect("upgrade available");
        assert_eq!(offer.version, "0.5.0");
        assert_eq!(offer.artifact.size, payload.len() as u64);
    }

    #[test]
    fn rejects_tampered_manifest_signature() {
        let (sk, pk) = test_keypair();
        let manifest_bytes = sample_manifest(
            "0.5.0",
            "https://github.com/existye4t/gemini-desk/releases/download/v0.5.0/setup.exe",
            b"legit",
        );
        let sig = sk.sign(&manifest_bytes).to_bytes();
        let mut tampered = manifest_bytes.clone();
        tampered[5] ^= 0x01;
        let cfg = test_cfg(pk, "0.4.0", "windows-x86_64", std::env::temp_dir());

        let err = evaluate_signed_manifest(&tampered, &sig, &pk, &cfg).unwrap_err();
        assert!(err.contains("signature verification failed"));
    }

    #[test]
    fn rejects_downgrade_and_equal_versions() {
        let (sk, pk) = test_keypair();
        let cfg = test_cfg(pk, "0.4.0", "windows-x86_64", std::env::temp_dir());

        for older in ["0.4.0", "0.3.1", "0.4.0-beta.1"] {
            let bytes = sample_manifest(
                older,
                "https://github.com/existye4t/gemini-desk/releases/download/v0.4.0/setup.exe",
                b"old",
            );
            let sig = sk.sign(&bytes).to_bytes();
            let res = evaluate_signed_manifest(&bytes, &sig, &pk, &cfg).unwrap();
            assert!(res.is_none(), "version {older} must not trigger upgrade");
        }
    }

    #[test]
    fn rejects_insecure_http_artifact_url() {
        let (sk, pk) = test_keypair();
        let bytes = sample_manifest("0.5.0", "http://evil.example/setup.exe", b"payload");
        let sig = sk.sign(&bytes).to_bytes();
        let cfg = test_cfg(pk, "0.4.0", "windows-x86_64", std::env::temp_dir());

        let err = evaluate_signed_manifest(&bytes, &sig, &pk, &cfg).unwrap_err();
        assert!(err.contains("non-HTTPS"));
    }

    #[test]
    fn rejects_wrong_platform_artifact() {
        let (sk, pk) = test_keypair();
        let bytes = sample_manifest(
            "0.5.0",
            "https://github.com/existye4t/gemini-desk/releases/download/v0.5.0/setup.exe",
            b"payload",
        );
        let sig = sk.sign(&bytes).to_bytes();
        let cfg = test_cfg(pk, "0.4.0", "macos-aarch64", std::env::temp_dir());

        let err = evaluate_signed_manifest(&bytes, &sig, &pk, &cfg).unwrap_err();
        assert!(err.contains("no verified package"));
    }

    #[test]
    fn downloads_verifies_sha256_and_rejects_tampered_binary() {
        let (sk, pk) = test_keypair();
        let good_payload = b"GeminiDesk-v0.5.0-verified-payload-bytes".to_vec();
        let bad_payload = b"GeminiDesk-v0.5.0-TAMPERED-payload-bytes".to_vec();

        let tmp_dir =
            std::env::temp_dir().join(format!("geminidesk-updater-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp_dir);
        fs::create_dir_all(&tmp_dir).unwrap();
        // Plant a stale `.part` file to verify interrupted download cleanup.
        fs::write(tmp_dir.join("stale.part"), b"partial").unwrap();

        // Spawn a tiny loopback HTTP test server serving good and tampered endpoints.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let good_clone = good_payload.clone();
        let bad_clone = bad_payload.clone();
        let handle = thread::spawn(move || {
            for _ in 0..2 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut req_buf = [0u8; 512];
                    let n = stream.read(&mut req_buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&req_buf[..n]);
                    let body = if req.contains("/bad") {
                        &bad_clone
                    } else {
                        &good_clone
                    };
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(header.as_bytes());
                    let _ = stream.write_all(body);
                }
            }
        });

        let mut cfg = test_cfg(pk, "0.4.0", "windows-x86_64", tmp_dir.clone());
        cfg.allow_insecure_http_for_tests = true;

        // 1. Valid download passes SHA-256 & cleans stale `.part`
        let good_url = format!("http://{addr}/good");
        let manifest_good = sample_manifest("0.5.0", &good_url, &good_payload);
        let sig_good = sk.sign(&manifest_good).to_bytes();
        let offer_good = evaluate_signed_manifest(&manifest_good, &sig_good, &pk, &cfg)
            .unwrap()
            .unwrap();
        let saved =
            download_and_verify_artifact(&offer_good, &cfg).expect("good download verified");
        assert!(saved.exists());
        assert!(!tmp_dir.join("stale.part").exists());

        // 2. Tampered payload fails SHA-256 & leaves ZERO unverified binary behind
        let bad_url = format!("http://{addr}/bad");
        let mut offer_bad = offer_good.clone();
        offer_bad.artifact.url = bad_url;
        offer_bad.artifact.filename = Some("tampered.exe".into());
        let err = download_and_verify_artifact(&offer_bad, &cfg).unwrap_err();
        assert!(err.contains("SHA-256 verification failed"));
        assert!(!tmp_dir.join("tampered.exe").exists());
        assert!(!tmp_dir.join("tampered.exe.part").exists());

        let _ = handle.join();
        let _ = fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn http_agent_uses_native_tls_provider_without_panicking_on_https() {
        let agent = build_http_agent();
        let res = fetch_bounded_bytes(&agent, "https://127.0.0.1:1/latest.json", 1024);
        assert!(res.is_err());
    }

    #[test]
    fn enforces_min_supported_version_and_pre_install_toctou_check() {
        let (sk, pk) = test_keypair();
        let payload = b"verified-release-binary-v1.0.0";
        let bytes = sample_manifest(
            "1.0.0",
            "https://github.com/existye4t/gemini-desk/releases/download/v1.0.0/setup.exe",
            payload,
        );
        let sig = sk.sign(&bytes).to_bytes();

        // Running v0.2.0 is below min_supported_version 0.3.0 -> must reject with clear guidance.
        let old_cfg = test_cfg(pk, "0.2.0", "windows-x86_64", std::env::temp_dir());
        let err = evaluate_signed_manifest(&bytes, &sig, &pk, &old_cfg).unwrap_err();
        assert!(err.contains("requires v0.3.0 or newer"));

        // Pre-install TOCTOU check rejects a modified file on disk.
        let tmp_file =
            std::env::temp_dir().join(format!("geminidesk-toctou-{}.bin", std::process::id()));
        fs::write(&tmp_file, b"tampered-after-download-bytes!").unwrap();
        let expected_sha = sha256_hex(payload);
        let res = launch_verified_installer(
            &tmp_file,
            &expected_sha,
            payload.len() as u64,
            ArtifactKind::LinuxAppImage,
        );
        assert!(res.is_err());
        let _ = fs::remove_file(&tmp_file);
    }

    #[test]
    fn rejects_truncated_interrupted_download_and_cleans_part_file() {
        let (sk, pk) = test_keypair();
        let full_payload = b"complete-update-payload-that-will-be-truncated-mid-stream".to_vec();
        let truncated = full_payload[..12].to_vec();

        let tmp_dir =
            std::env::temp_dir().join(format!("geminidesk-trunc-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp_dir);
        fs::create_dir_all(&tmp_dir).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut req_buf = [0u8; 256];
                let _ = stream.read(&mut req_buf);
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    truncated.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&truncated);
            }
        });

        let mut cfg = test_cfg(pk, "0.4.0", "windows-x86_64", tmp_dir.clone());
        cfg.allow_insecure_http_for_tests = true;
        let url = format!("http://{addr}/trunc");
        let manifest = sample_manifest("1.0.0", &url, &full_payload);
        let sig = sk.sign(&manifest).to_bytes();
        let offer = evaluate_signed_manifest(&manifest, &sig, &pk, &cfg)
            .unwrap()
            .unwrap();

        let err = download_and_verify_artifact(&offer, &cfg).unwrap_err();
        assert!(err.contains("Incomplete download"));
        assert!(!tmp_dir.join("GeminiDesk-Setup-1.0.0.exe").exists());
        assert!(!tmp_dir.join("GeminiDesk-Setup-1.0.0.exe.part").exists());

        let _ = handle.join();
        let _ = fs::remove_dir_all(&tmp_dir);
    }
}
