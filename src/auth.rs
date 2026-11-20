//! Google Account & Authentication lifecycle management for GeminiDesk.
//!
//! ## Architectural Reality & Security Policy
//! Consumer Google Gemini (`https://gemini.google.com/app`) authenticates strictly via
//! first-party Google web session cookies (`accounts.google.com` -> `CheckCookie` -> `SetSID`)
//! stored inside the native OS WebView profile (`WebView2` on Windows, `WebKitGTK 4.1` on Linux,
//! `WKWebView` on macOS).
//!
//! Google does **not** expose any public, documented API to exchange an external-browser OAuth 2.0
//! authorization code, access token, or OIDC ID token for first-party `gemini.google.com` web
//! session cookies. Internal endpoints such as `accounts.google.com/MergeSession` are undocumented
//! and restricted exclusively to first-party Google clients.
//!
//! Therefore, GeminiDesk:
//! - Never fakes an OAuth sign-in that leaves `gemini.google.com/app` unauthenticated.
//! - Never extracts cookies or reads SQLite databases from Zen, Firefox, Chrome, Edge, or Safari.
//! - Never spoofs browser User-Agents, injects credential-capture scripts, or uses undocumented APIs.
//! - Manages Google's official in-WebView sign-in, account chooser, account switching, and sign-out
//!   URLs with strict HTTPS host/redirect validation, state/generation correlation, and cancellation.
//! - Offers an explicitly labeled fallback to open `https://gemini.google.com/app` in the OS
//!   default browser if Google ever challenges an embedded WebView.

use std::path::Path;

use serde::Serialize;
use url::Url;

use crate::navigation::{NavigationDecision, classify_navigation, is_allowed_in_app_host};

/// Primary Gemini consumer workspace URL.
pub const GEMINI_APP_URL: &str = "https://gemini.google.com/app";

/// Official Google Account Chooser URL returning directly to Gemini.
pub const GOOGLE_ACCOUNT_CHOOSER_URL: &str =
    "https://accounts.google.com/AccountChooser?continue=https%3A%2F%2Fgemini.google.com%2Fapp";

/// Official Google Add/Switch Account URL returning directly to Gemini.
pub const GOOGLE_ADD_ACCOUNT_URL: &str =
    "https://accounts.google.com/AddSession?continue=https%3A%2F%2Fgemini.google.com%2Fapp";

/// Official Google Sign-In entry URL returning directly to Gemini.
pub const GOOGLE_SIGN_IN_URL: &str = "https://accounts.google.com/ServiceLogin?passive=false&continue=https%3A%2F%2Fgemini.google.com%2Fapp";

/// Official Google Sign-Out URL returning to Gemini.
pub const GOOGLE_SIGN_OUT_URL: &str =
    "https://accounts.google.com/Logout?continue=https%3A%2F%2Fgemini.google.com%2Fapp";

/// Supported in-app Google authentication actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SignInFlow {
    SignIn,
    AccountChooser,
    AddAccount,
    SignOut,
}

impl SignInFlow {
    /// Returns the official Google HTTPS URL for this flow.
    pub fn entry_url(self) -> &'static str {
        match self {
            Self::SignIn => GOOGLE_SIGN_IN_URL,
            Self::AccountChooser => GOOGLE_ACCOUNT_CHOOSER_URL,
            Self::AddAccount => GOOGLE_ADD_ACCOUNT_URL,
            Self::SignOut => GOOGLE_SIGN_OUT_URL,
        }
    }

    pub fn display_stage(self) -> &'static str {
        match self {
            Self::SignIn => "Waiting for Google sign-in...",
            Self::AccountChooser => "Choose a Google account...",
            Self::AddAccount => "Add or switch Google account...",
            Self::SignOut => "Signing out of Google...",
        }
    }
}

/// Structured error codes for authentication validation and embedded browser challenges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthErrorCode {
    /// Google challenged or rejected the embedded WebView (e.g., `disallowed_useragent`).
    EmbeddedBrowserBlocked,
    /// A navigation or `continue` redirect target failed strict Google HTTPS validation.
    InvalidRedirect,
    /// A callback or asynchronous probe arrived with a stale/mismatched flow generation.
    CallbackStateMismatch,
    /// Network or page-load error while loading Google authentication.
    NetworkOrLoadError,
}

/// Live authentication state surfaced to Settings -> Account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AuthState {
    Checking,
    Authenticated {
        account_hint: Option<String>,
        multi_account_index: Option<u8>,
    },
    SignedOut,
    SigningIn {
        flow: SignInFlow,
        stage: String,
    },
    AuthError {
        code: AuthErrorCode,
        message: String,
    },
}

/// Classification of a URL encountered during authentication or Gemini workspace navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthUrlKind {
    GeminiWorkspace { account_index: Option<u8> },
    GoogleSignIn,
    GoogleAccountChooser,
    GoogleSsoHandshake,
    GoogleSignOut,
    GoogleAuthBlocked,
    OtherAllowed,
    Disallowed,
}

/// Read-only status of the platform's persistent WebView session storage directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfilePersistenceStatus {
    pub profile_dir_exists: bool,
    pub session_storage_present: bool,
    pub engine_storage_note: String,
}

/// Result returned by the event-driven, non-invasive DOM session probe (no cookies/passwords read).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthDomProbeResult {
    Authenticated { account_hint: Option<String> },
    SignedOut,
    EmbeddedBrowserBlocked,
    InProgress,
}

/// Strictly validates a URL intended for authentication navigation or `continue` redirect.
///
/// Enforces:
/// - `https` scheme only (blocks `http`, `file`, `javascript`, `data`, custom schemes)
/// - No embedded credentials (`user:pass@host`)
/// - Host strictly on the official Google domain allowlist
/// - If a `continue` query parameter is present, recursively validates that redirect target too
pub fn validate_auth_navigation_url(raw_url: &str) -> Result<Url, AuthErrorCode> {
    let parsed = Url::parse(raw_url).map_err(|_| AuthErrorCode::InvalidRedirect)?;
    if parsed.scheme() != "https" {
        return Err(AuthErrorCode::InvalidRedirect);
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(AuthErrorCode::InvalidRedirect);
    }
    let host = parsed.host_str().ok_or(AuthErrorCode::InvalidRedirect)?;
    if !is_allowed_in_app_host(host) {
        return Err(AuthErrorCode::InvalidRedirect);
    }
    if classify_navigation(raw_url) != NavigationDecision::AllowInApp {
        return Err(AuthErrorCode::InvalidRedirect);
    }

    // Validate any `continue` redirect parameter to prevent open-redirect abuse.
    for (k, v) in parsed.query_pairs() {
        if k.eq_ignore_ascii_case("continue") && !v.is_empty() {
            validate_continue_target(&v)?;
        }
    }

    Ok(parsed)
}

/// Validates a `continue=` redirect target inside a Google auth URL.
pub fn validate_continue_target(target: &str) -> Result<(), AuthErrorCode> {
    let parsed = Url::parse(target).map_err(|_| AuthErrorCode::InvalidRedirect)?;
    if parsed.scheme() != "https" {
        return Err(AuthErrorCode::InvalidRedirect);
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(AuthErrorCode::InvalidRedirect);
    }
    let host = parsed.host_str().ok_or(AuthErrorCode::InvalidRedirect)?;
    if !is_allowed_in_app_host(host) {
        return Err(AuthErrorCode::InvalidRedirect);
    }
    Ok(())
}

/// Classifies an observed navigation URL into a structured authentication lifecycle category.
pub fn classify_auth_url(uri: &str) -> AuthUrlKind {
    let Ok(parsed) = Url::parse(uri) else {
        return AuthUrlKind::Disallowed;
    };

    if parsed.scheme() != "https" {
        return AuthUrlKind::Disallowed;
    }

    let Some(host_raw) = parsed.host_str() else {
        return AuthUrlKind::Disallowed;
    };
    let host = host_raw.trim_end_matches('.').to_ascii_lowercase();
    if !is_allowed_in_app_host(&host) {
        return AuthUrlKind::Disallowed;
    }

    let path = parsed.path().to_ascii_lowercase();
    let query = parsed.query().unwrap_or_default().to_ascii_lowercase();

    // Detect explicit Google embedded-webview rejection or error URLs.
    if host.starts_with("accounts.google.")
        && (path.contains("signin/rejected")
            || query.contains("disallowed_useragent")
            || query.contains("error=access_denied"))
    {
        return AuthUrlKind::GoogleAuthBlocked;
    }

    if host == "gemini.google.com" {
        let account_index = parse_multi_account_index(&path);
        return AuthUrlKind::GeminiWorkspace { account_index };
    }

    if host == "accounts.youtube.com"
        || (host.starts_with("accounts.google.") && path.contains("setsid"))
        || (host.starts_with("accounts.google.") && path.contains("checkcookie"))
    {
        return AuthUrlKind::GoogleSsoHandshake;
    }

    if host.starts_with("accounts.google.") {
        if path.contains("logout") {
            return AuthUrlKind::GoogleSignOut;
        }
        if path.contains("accountchooser") || path.contains("addsession") {
            return AuthUrlKind::GoogleAccountChooser;
        }
        return AuthUrlKind::GoogleSignIn;
    }

    AuthUrlKind::OtherAllowed
}

fn parse_multi_account_index(lower_path: &str) -> Option<u8> {
    let rest = lower_path.strip_prefix("/u/")?;
    let digits = rest.split('/').next()?;
    digits.parse::<u8>().ok()
}

/// Non-destructively inspects the WebView profile directory to report whether persistent
/// session storage has been initialized on disk. Never modifies or reads sensitive cookie values.
pub fn inspect_profile_persistence(webview_data_dir: &Path) -> ProfilePersistenceStatus {
    let profile_dir_exists = webview_data_dir.is_dir();

    #[cfg(target_os = "windows")]
    let (session_storage_present, engine_storage_note) = {
        let default_dir = webview_data_dir.join("EBWebView").join("Default");
        let present = default_dir.join("Network").join("Cookies").exists()
            || default_dir.join("Cookies").exists()
            || default_dir.join("Local Storage").is_dir();
        (
            present,
            "Persistent WebView2 profile stored locally under your Windows user profile."
                .to_string(),
        )
    };

    #[cfg(target_os = "linux")]
    let (session_storage_present, engine_storage_note) = {
        let present = webview_data_dir.join("cookies").exists()
            || webview_data_dir.join("localstorage").is_dir();
        (
            present,
            "Persistent WebKitGTK WebsiteDataManager profile stored locally with ITP disabled for Google SSO."
                .to_string(),
        )
    };

    #[cfg(target_os = "macos")]
    let (session_storage_present, engine_storage_note) = {
        let present = profile_dir_exists;
        (
            present,
            "Persistent macOS WKWebsiteDataStore managed natively by WebKit.".to_string(),
        )
    };

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    let (session_storage_present, engine_storage_note) = (
        profile_dir_exists,
        "Native WebView persistent profile.".to_string(),
    );

    ProfilePersistenceStatus {
        profile_dir_exists,
        session_storage_present,
        engine_storage_note,
    }
}

/// Stateful controller coordinating in-app Google authentication state, flow generations,
/// cancellation, and probe correlation.
#[derive(Debug)]
pub struct AuthController {
    state: AuthState,
    previous_stable_state: AuthState,
    flow_generation: u64,
    active_flow: Option<SignInFlow>,
    last_account_index: Option<u8>,
}

impl AuthController {
    pub fn new() -> Self {
        Self {
            state: AuthState::Checking,
            previous_stable_state: AuthState::SignedOut,
            flow_generation: 1,
            active_flow: None,
            last_account_index: None,
        }
    }

    pub fn state(&self) -> &AuthState {
        &self.state
    }

    pub fn flow_generation(&self) -> u64 {
        self.flow_generation
    }

    /// Starts a validated official Google authentication flow and returns `(url, new_generation)`.
    pub fn begin_flow(&mut self, flow: SignInFlow) -> Result<(&'static str, u64), AuthErrorCode> {
        let url = flow.entry_url();
        validate_auth_navigation_url(url)?;

        if matches!(
            self.state,
            AuthState::Authenticated { .. } | AuthState::SignedOut
        ) {
            self.previous_stable_state = self.state.clone();
        }

        self.flow_generation = self.flow_generation.saturating_add(1);
        self.active_flow = Some(flow);
        self.state = AuthState::SigningIn {
            flow,
            stage: flow.display_stage().to_string(),
        };
        Ok((url, self.flow_generation))
    }

    /// Cancels any active sign-in flow, invalidates pending async probe callbacks,
    /// and returns to the previous stable state (or `SignedOut`).
    pub fn cancel_flow(&mut self) -> u64 {
        self.flow_generation = self.flow_generation.saturating_add(1);
        self.active_flow = None;
        self.state = match &self.previous_stable_state {
            AuthState::Authenticated { .. } => self.previous_stable_state.clone(),
            _ => AuthState::SignedOut,
        };
        self.flow_generation
    }

    /// Updates state based on a top-level page load URL observed in the WebView.
    /// Returns `true` if a non-invasive DOM auth probe should run after this page finishes loading.
    pub fn on_page_url(&mut self, uri: &str, finished: bool) -> bool {
        match classify_auth_url(uri) {
            AuthUrlKind::GoogleAuthBlocked => {
                self.active_flow = None;
                self.state = AuthState::AuthError {
                    code: AuthErrorCode::EmbeddedBrowserBlocked,
                    message: "Google challenged this embedded WebView session. You can retry sign-in or open Gemini in your default system browser."
                        .to_string(),
                };
                false
            }
            AuthUrlKind::GoogleAccountChooser => {
                let flow = self.active_flow.unwrap_or(SignInFlow::AccountChooser);
                self.active_flow = Some(flow);
                self.state = AuthState::SigningIn {
                    flow,
                    stage: flow.display_stage().to_string(),
                };
                finished
            }
            AuthUrlKind::GoogleSignIn => {
                let flow = self.active_flow.unwrap_or(SignInFlow::SignIn);
                self.active_flow = Some(flow);
                self.state = AuthState::SigningIn {
                    flow,
                    stage: flow.display_stage().to_string(),
                };
                finished
            }
            AuthUrlKind::GoogleSsoHandshake => {
                let flow = self.active_flow.unwrap_or(SignInFlow::SignIn);
                self.active_flow = Some(flow);
                self.state = AuthState::SigningIn {
                    flow,
                    stage: "Completing Google session handoff...".to_string(),
                };
                false
            }
            AuthUrlKind::GoogleSignOut => {
                self.active_flow = Some(SignInFlow::SignOut);
                self.state = AuthState::SigningIn {
                    flow: SignInFlow::SignOut,
                    stage: SignInFlow::SignOut.display_stage().to_string(),
                };
                false
            }
            AuthUrlKind::GeminiWorkspace { account_index } => {
                self.last_account_index = account_index;
                if finished {
                    self.flow_generation = self.flow_generation.saturating_add(1);
                    true
                } else {
                    false
                }
            }
            AuthUrlKind::OtherAllowed => false,
            AuthUrlKind::Disallowed => false,
        }
    }

    /// Applies an asynchronous DOM session probe result only if `generation` matches the current
    /// flow generation (preventing stale or cancelled callback races).
    pub fn apply_probe_result(
        &mut self,
        generation: u64,
        result: AuthDomProbeResult,
    ) -> Result<(), AuthErrorCode> {
        if generation != self.flow_generation {
            return Err(AuthErrorCode::CallbackStateMismatch);
        }

        match result {
            AuthDomProbeResult::Authenticated { account_hint } => {
                self.active_flow = None;
                self.state = AuthState::Authenticated {
                    account_hint,
                    multi_account_index: self.last_account_index,
                };
                self.previous_stable_state = self.state.clone();
            }
            AuthDomProbeResult::SignedOut => {
                self.active_flow = None;
                self.state = AuthState::SignedOut;
                self.previous_stable_state = AuthState::SignedOut;
            }
            AuthDomProbeResult::EmbeddedBrowserBlocked => {
                self.active_flow = None;
                self.state = AuthState::AuthError {
                    code: AuthErrorCode::EmbeddedBrowserBlocked,
                    message: "Google reported that this embedded browser may not be secure. Retry sign-in or open Gemini in your default system browser."
                        .to_string(),
                };
            }
            AuthDomProbeResult::InProgress => {}
        }

        Ok(())
    }

    /// Records a structured load or navigation error during authentication.
    pub fn record_error(&mut self, code: AuthErrorCode, message: impl Into<String>) {
        self.flow_generation = self.flow_generation.saturating_add(1);
        self.active_flow = None;
        self.state = AuthState::AuthError {
            code,
            message: message.into(),
        };
    }
}

/// Parses the JSON/string payload returned by the non-invasive DOM session probe.
pub fn parse_dom_probe_payload(raw: &str) -> AuthDomProbeResult {
    // `evaluate_script_with_callback` returns a JSON-encoded string (e.g. `"signed_out"`).
    let unwrapped =
        serde_json::from_str::<String>(raw.trim()).unwrap_or_else(|_| raw.trim().to_string());
    if unwrapped == "blocked" {
        return AuthDomProbeResult::EmbeddedBrowserBlocked;
    }
    if unwrapped == "signed_out" {
        return AuthDomProbeResult::SignedOut;
    }
    if unwrapped == "in_progress" {
        return AuthDomProbeResult::InProgress;
    }
    if let Some(hint) = unwrapped.strip_prefix("authenticated:") {
        let trimmed = hint.trim();
        let account_hint = (!trimmed.is_empty()).then(|| trimmed.chars().take(120).collect());
        return AuthDomProbeResult::Authenticated { account_hint };
    }
    if unwrapped == "authenticated" {
        return AuthDomProbeResult::Authenticated { account_hint: None };
    }
    AuthDomProbeResult::InProgress
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn official_auth_entry_urls_pass_strict_validation() {
        for flow in [
            SignInFlow::SignIn,
            SignInFlow::AccountChooser,
            SignInFlow::AddAccount,
            SignInFlow::SignOut,
        ] {
            let url = flow.entry_url();
            assert!(
                validate_auth_navigation_url(url).is_ok(),
                "official flow URL must validate: {url}"
            );
        }
    }

    #[test]
    fn rejects_unsafe_schemes_spoofed_hosts_and_open_redirects() {
        let unsafe_urls = [
            "http://accounts.google.com/ServiceLogin",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,<h1>phish</h1>",
            "https://user:pass@accounts.google.com/ServiceLogin",
            "https://accounts.google.com.evil.com/ServiceLogin",
            "https://evil.example/login",
            "https://accounts.google.com/ServiceLogin?continue=https%3A%2F%2Fevil.example%2Fsteal",
            "https://accounts.google.com/ServiceLogin?continue=http%3A%2F%2Fgemini.google.com%2Fapp",
            "https://accounts.google.com/ServiceLogin?continue=javascript%3Aalert(1)",
        ];

        for bad in unsafe_urls {
            assert_eq!(
                validate_auth_navigation_url(bad),
                Err(AuthErrorCode::InvalidRedirect),
                "expected InvalidRedirect for {bad}"
            );
        }
    }

    #[test]
    fn classifies_google_auth_sso_workspace_and_blocked_urls() {
        assert_eq!(
            classify_auth_url("https://gemini.google.com/app"),
            AuthUrlKind::GeminiWorkspace {
                account_index: None
            }
        );
        assert_eq!(
            classify_auth_url("https://gemini.google.com/u/2/app"),
            AuthUrlKind::GeminiWorkspace {
                account_index: Some(2)
            }
        );
        assert_eq!(
            classify_auth_url(GOOGLE_ACCOUNT_CHOOSER_URL),
            AuthUrlKind::GoogleAccountChooser
        );
        assert_eq!(
            classify_auth_url(GOOGLE_ADD_ACCOUNT_URL),
            AuthUrlKind::GoogleAccountChooser
        );
        assert_eq!(
            classify_auth_url("https://accounts.google.com/v3/signin/identifier"),
            AuthUrlKind::GoogleSignIn
        );
        assert_eq!(
            classify_auth_url("https://accounts.google.com.tr/accounts/SetSID?sidt=SECRET"),
            AuthUrlKind::GoogleSsoHandshake
        );
        assert_eq!(
            classify_auth_url("https://accounts.youtube.com/accounts/SetSID?sidt=SECRET"),
            AuthUrlKind::GoogleSsoHandshake
        );
        assert_eq!(
            classify_auth_url(
                "https://accounts.google.com/CheckCookie?continue=https://gemini.google.com/app"
            ),
            AuthUrlKind::GoogleSsoHandshake
        );
        assert_eq!(
            classify_auth_url(GOOGLE_SIGN_OUT_URL),
            AuthUrlKind::GoogleSignOut
        );
        assert_eq!(
            classify_auth_url("https://accounts.google.com/signin/rejected?rhlk=no"),
            AuthUrlKind::GoogleAuthBlocked
        );
        assert_eq!(
            classify_auth_url(
                "https://accounts.google.com/o/oauth2/v2/auth?error=disallowed_useragent"
            ),
            AuthUrlKind::GoogleAuthBlocked
        );
        assert_eq!(
            classify_auth_url("https://evil.example/accounts"),
            AuthUrlKind::Disallowed
        );
    }

    #[test]
    fn state_machine_handles_sign_in_handoff_and_account_switching() {
        let mut ctrl = AuthController::new();
        assert_eq!(ctrl.state(), &AuthState::Checking);

        // Initial load lands on signed-out Gemini workspace.
        assert!(ctrl.on_page_url("https://gemini.google.com/app", true));
        let flow_gen = ctrl.flow_generation();
        ctrl.apply_probe_result(flow_gen, AuthDomProbeResult::SignedOut)
            .expect("matching generation");
        assert_eq!(ctrl.state(), &AuthState::SignedOut);

        // Begin official account chooser flow.
        let (url, _) = ctrl
            .begin_flow(SignInFlow::AccountChooser)
            .expect("valid flow");
        assert_eq!(url, GOOGLE_ACCOUNT_CHOOSER_URL);
        assert!(matches!(
            ctrl.state(),
            AuthState::SigningIn {
                flow: SignInFlow::AccountChooser,
                ..
            }
        ));

        // Regional SSO SetSID handshake stays in SigningIn state.
        assert!(!ctrl.on_page_url(
            "https://accounts.google.com.tr/accounts/SetSID?continue=https://gemini.google.com/app",
            false
        ));
        assert!(matches!(ctrl.state(), AuthState::SigningIn { .. }));

        // Lands back on multi-account Gemini workspace `/u/1/app`.
        assert!(ctrl.on_page_url("https://gemini.google.com/u/1/app", true));
        let gen_after = ctrl.flow_generation();
        ctrl.apply_probe_result(
            gen_after,
            AuthDomProbeResult::Authenticated {
                account_hint: Some("Google Account: Alex".into()),
            },
        )
        .expect("matching generation");

        assert_eq!(
            ctrl.state(),
            &AuthState::Authenticated {
                account_hint: Some("Google Account: Alex".into()),
                multi_account_index: Some(1),
            }
        );
    }

    #[test]
    fn cancellation_invalidates_stale_callbacks_and_restores_prior_state() {
        let mut ctrl = AuthController::new();
        assert!(ctrl.on_page_url("https://gemini.google.com/app", true));
        let g1 = ctrl.flow_generation();
        ctrl.apply_probe_result(
            g1,
            AuthDomProbeResult::Authenticated {
                account_hint: Some("Primary Account".into()),
            },
        )
        .unwrap();

        // Start adding a second account, capture generation, then cancel.
        let (_, add_gen) = ctrl.begin_flow(SignInFlow::AddAccount).unwrap();
        let cancel_gen = ctrl.cancel_flow();
        assert_ne!(add_gen, cancel_gen);

        // Prior authenticated state is restored cleanly.
        assert_eq!(
            ctrl.state(),
            &AuthState::Authenticated {
                account_hint: Some("Primary Account".into()),
                multi_account_index: None,
            }
        );

        // Any stale probe callback from the cancelled generation is rejected.
        assert_eq!(
            ctrl.apply_probe_result(add_gen, AuthDomProbeResult::SignedOut),
            Err(AuthErrorCode::CallbackStateMismatch)
        );
        assert!(matches!(ctrl.state(), AuthState::Authenticated { .. }));
    }

    #[test]
    fn error_handling_detects_blocked_webview_and_allows_retry() {
        let mut ctrl = AuthController::new();
        ctrl.begin_flow(SignInFlow::SignIn).unwrap();

        assert!(!ctrl.on_page_url("https://accounts.google.com/signin/rejected", true));
        assert!(matches!(
            ctrl.state(),
            AuthState::AuthError {
                code: AuthErrorCode::EmbeddedBrowserBlocked,
                ..
            }
        ));

        // Retry resets state cleanly into SigningIn.
        assert!(ctrl.begin_flow(SignInFlow::AccountChooser).is_ok());
        assert!(matches!(
            ctrl.state(),
            AuthState::SigningIn {
                flow: SignInFlow::AccountChooser,
                ..
            }
        ));
    }

    #[test]
    fn dom_probe_payload_parser_handles_all_states() {
        assert_eq!(
            parse_dom_probe_payload("\"authenticated:Google Account: Ada\""),
            AuthDomProbeResult::Authenticated {
                account_hint: Some("Google Account: Ada".into()),
            }
        );
        assert_eq!(
            parse_dom_probe_payload("\"signed_out\""),
            AuthDomProbeResult::SignedOut
        );
        assert_eq!(
            parse_dom_probe_payload("\"blocked\""),
            AuthDomProbeResult::EmbeddedBrowserBlocked
        );
        assert_eq!(
            parse_dom_probe_payload("\"in_progress\""),
            AuthDomProbeResult::InProgress
        );
    }

    #[test]
    fn profile_persistence_inspector_is_non_destructive() {
        let dir =
            std::env::temp_dir().join(format!("geminidesk_auth_persist_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let status_empty = inspect_profile_persistence(&dir);
        assert!(status_empty.profile_dir_exists);

        // Simulate an existing session cookie artifact and confirm inspection never modifies it.
        #[cfg(target_os = "linux")]
        let marker = dir.join("cookies");
        #[cfg(target_os = "windows")]
        let marker = {
            let p = dir.join("EBWebView").join("Default").join("Network");
            fs::create_dir_all(&p).unwrap();
            p.join("Cookies")
        };
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        let marker = dir.join("session_marker");

        fs::write(&marker, b"preserved-session-marker").unwrap();
        let status_seeded = inspect_profile_persistence(&dir);
        assert!(status_seeded.profile_dir_exists);
        assert!(status_seeded.session_storage_present);
        assert_eq!(fs::read(&marker).unwrap(), b"preserved-session-marker");

        let _ = fs::remove_dir_all(&dir);
    }
}
