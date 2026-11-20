use url::Url;

/// Decision for a top-level navigation inside the embedded WebView.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationDecision {
    /// Allow the WebView to proceed with the navigation.
    AllowInApp,
    /// Cancel in-app navigation and open the URL in the user's default system browser.
    OpenExternal,
    /// Cancel navigation without opening externally (e.g., unsafe or malformed URL).
    Block,
}

/// Decision for a `window.open` / `target="_blank"` request from the WebView.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewWindowDecision {
    /// Allow WebView2 / WKWebView / WebKitGTK to open a child popup sharing the same session.
    AllowInAppPopup,
    /// Deny the popup and open the URL in the user's default system browser.
    OpenExternal,
    /// Deny without opening externally.
    Deny,
}

/// Official Google regional/ccTLD domains from <https://www.google.com/supported_domains>.
/// Kept strictly sorted for binary search.
const GOOGLE_SUPPORTED_DOMAINS: &[&str] = &[
    "google.ad",
    "google.ae",
    "google.al",
    "google.am",
    "google.as",
    "google.at",
    "google.az",
    "google.ba",
    "google.be",
    "google.bf",
    "google.bg",
    "google.bi",
    "google.bj",
    "google.bs",
    "google.bt",
    "google.by",
    "google.ca",
    "google.cat",
    "google.cd",
    "google.cf",
    "google.cg",
    "google.ch",
    "google.ci",
    "google.cl",
    "google.cm",
    "google.cn",
    "google.co.ao",
    "google.co.bw",
    "google.co.ck",
    "google.co.cr",
    "google.co.id",
    "google.co.il",
    "google.co.in",
    "google.co.jp",
    "google.co.ke",
    "google.co.kr",
    "google.co.ls",
    "google.co.ma",
    "google.co.mz",
    "google.co.nz",
    "google.co.th",
    "google.co.tz",
    "google.co.ug",
    "google.co.uk",
    "google.co.uz",
    "google.co.ve",
    "google.co.vi",
    "google.co.za",
    "google.co.zm",
    "google.co.zw",
    "google.com",
    "google.com.af",
    "google.com.ag",
    "google.com.ar",
    "google.com.au",
    "google.com.bd",
    "google.com.bh",
    "google.com.bn",
    "google.com.bo",
    "google.com.br",
    "google.com.bz",
    "google.com.co",
    "google.com.cu",
    "google.com.cy",
    "google.com.do",
    "google.com.ec",
    "google.com.eg",
    "google.com.et",
    "google.com.fj",
    "google.com.gh",
    "google.com.gi",
    "google.com.gt",
    "google.com.hk",
    "google.com.jm",
    "google.com.kh",
    "google.com.kw",
    "google.com.lb",
    "google.com.ly",
    "google.com.mm",
    "google.com.mt",
    "google.com.mx",
    "google.com.my",
    "google.com.na",
    "google.com.ng",
    "google.com.ni",
    "google.com.np",
    "google.com.om",
    "google.com.pa",
    "google.com.pe",
    "google.com.pg",
    "google.com.ph",
    "google.com.pk",
    "google.com.pr",
    "google.com.py",
    "google.com.qa",
    "google.com.sa",
    "google.com.sb",
    "google.com.sg",
    "google.com.sl",
    "google.com.sv",
    "google.com.tj",
    "google.com.tr",
    "google.com.tw",
    "google.com.ua",
    "google.com.uy",
    "google.com.vc",
    "google.com.vn",
    "google.cv",
    "google.cz",
    "google.de",
    "google.dj",
    "google.dk",
    "google.dm",
    "google.dz",
    "google.ee",
    "google.es",
    "google.fi",
    "google.fm",
    "google.fr",
    "google.ga",
    "google.ge",
    "google.gg",
    "google.gl",
    "google.gm",
    "google.gr",
    "google.gy",
    "google.hn",
    "google.hr",
    "google.ht",
    "google.hu",
    "google.ie",
    "google.im",
    "google.iq",
    "google.is",
    "google.it",
    "google.je",
    "google.jo",
    "google.kg",
    "google.ki",
    "google.kz",
    "google.la",
    "google.li",
    "google.lk",
    "google.lt",
    "google.lu",
    "google.lv",
    "google.md",
    "google.me",
    "google.mg",
    "google.mk",
    "google.ml",
    "google.mn",
    "google.mu",
    "google.mv",
    "google.mw",
    "google.ne",
    "google.nl",
    "google.no",
    "google.nr",
    "google.nu",
    "google.pl",
    "google.pn",
    "google.ps",
    "google.pt",
    "google.ro",
    "google.rs",
    "google.ru",
    "google.rw",
    "google.sc",
    "google.se",
    "google.sh",
    "google.si",
    "google.sk",
    "google.sm",
    "google.sn",
    "google.so",
    "google.sr",
    "google.st",
    "google.td",
    "google.tg",
    "google.tl",
    "google.tm",
    "google.tn",
    "google.to",
    "google.tt",
    "google.vu",
    "google.ws",
];

fn normalize_host(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}

fn matches_domain(host: &str, domain: &str) -> bool {
    host == domain
        || (host.len() > domain.len()
            && host.ends_with(domain)
            && host.as_bytes()[host.len() - domain.len() - 1] == b'.')
}

/// Returns `Some(subdomain_prefix)` if `host` is an official Google domain or subdomain.
/// For bare `google.com`, returns `Some("")`.
/// For `accounts.google.com.tr`, returns `Some("accounts")`.
fn match_google_supported_domain(normalized_host: &str) -> Option<&str> {
    if normalized_host.starts_with("google.")
        && GOOGLE_SUPPORTED_DOMAINS
            .binary_search(&normalized_host)
            .is_ok()
    {
        return Some("");
    }

    let mut offset = 0;
    while let Some(rel_idx) = normalized_host[offset..].find('.') {
        let dot_idx = offset + rel_idx;
        let candidate = &normalized_host[dot_idx + 1..];
        if candidate.starts_with("google.")
            && GOOGLE_SUPPORTED_DOMAINS.binary_search(&candidate).is_ok()
        {
            return Some(&normalized_host[..dot_idx]);
        }
        offset = dot_idx + 1;
    }

    None
}

/// Returns true if `host` belongs to Google's Gemini, authentication, SSO (`SetSID`),
/// or Google Workspace infrastructure and should stay inside the app WebView.
pub fn is_allowed_in_app_host(host: &str) -> bool {
    let h = normalize_host(host);
    if h.is_empty() {
        return false;
    }

    if match_google_supported_domain(&h).is_some() {
        return true;
    }

    // Cross-domain Google SSO endpoints & first-party Google assets/domains.
    h == "accounts.youtube.com"
        || h == "g.co"
        || matches_domain(&h, "gemini.google")
        || matches_domain(&h, "gstatic.com")
        || matches_domain(&h, "googleusercontent.com")
        || matches_domain(&h, "googleapis.com")
        || matches_domain(&h, "gvt1.com")
        || matches_domain(&h, "gvt2.com")
}

/// Returns true if a popup (`window.open` / `target="_blank"`) to `host` should open
/// inside the shared WebView2/WebKit session (for Google Sign-In, Account Switcher,
/// consent flows, Gemini links, or Drive/Docs pickers).
pub fn is_allowed_in_app_popup_host(host: &str) -> bool {
    let h = normalize_host(host);
    if h.is_empty() {
        return false;
    }

    if h == "accounts.youtube.com" || matches_domain(&h, "gemini.google") {
        return true;
    }

    if let Some(sub) = match_google_supported_domain(&h) {
        return matches!(
            sub,
            "accounts"
                | "gemini"
                | "myaccount"
                | "gds"
                | "consent"
                | "ogs"
                | "docs"
                | "drive"
                | "workspace"
                | "one"
                | "aistudio"
                | "colab.research"
        );
    }

    false
}

/// Classifies a top-level navigation URI.
pub fn classify_navigation(uri: &str) -> NavigationDecision {
    let Ok(parsed) = Url::parse(uri) else {
        return NavigationDecision::Block;
    };

    match parsed.scheme() {
        "about" | "blob" | "data" => NavigationDecision::AllowInApp,
        "https" => match parsed.host_str() {
            Some(host) if is_allowed_in_app_host(host) => NavigationDecision::AllowInApp,
            Some(_) => NavigationDecision::OpenExternal,
            None => NavigationDecision::Block,
        },
        "http" => {
            if parsed.host_str().is_some() {
                NavigationDecision::OpenExternal
            } else {
                NavigationDecision::Block
            }
        }
        _ => NavigationDecision::Block,
    }
}

/// Classifies a new-window / popup request URI.
pub fn classify_new_window(uri: &str) -> NewWindowDecision {
    if uri.is_empty() || uri.eq_ignore_ascii_case("about:blank") {
        return NewWindowDecision::AllowInAppPopup;
    }

    let Ok(parsed) = Url::parse(uri) else {
        return NewWindowDecision::Deny;
    };

    match parsed.scheme() {
        "https" => match parsed.host_str() {
            Some(host) if is_allowed_in_app_popup_host(host) => NewWindowDecision::AllowInAppPopup,
            Some(_) => NewWindowDecision::OpenExternal,
            None => NewWindowDecision::Deny,
        },
        "http" => {
            if parsed.host_str().is_some() {
                NewWindowDecision::OpenExternal
            } else {
                NewWindowDecision::Deny
            }
        }
        _ => NewWindowDecision::Deny,
    }
}

/// Formats a URL safely for logs by stripping query parameters, credentials, and fragments
/// so tokens (`sidt`, `code`, `state`, `authuser`, etc.) are never written to logs.
pub fn redact_url_for_log(uri: &str) -> String {
    match Url::parse(uri) {
        Ok(parsed) => {
            let scheme = parsed.scheme();
            if matches!(scheme, "about" | "blob" | "data") {
                return format!("{scheme}:<redacted>");
            }
            let host = parsed.host_str().unwrap_or("<no-host>");
            let path = parsed.path();
            format!("{scheme}://{host}{path}")
        }
        Err(_) => "<invalid-url>".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_domains_array_is_strictly_sorted() {
        for pair in GOOGLE_SUPPORTED_DOMAINS.windows(2) {
            assert!(
                pair[0] < pair[1],
                "GOOGLE_SUPPORTED_DOMAINS not strictly sorted: {} >= {}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn allows_google_core_and_regional_sso_domains() {
        let allowed = [
            "https://gemini.google.com/app",
            "https://accounts.google.com/v3/signin/identifier",
            "https://accounts.google.com/CheckCookie?continue=https://gemini.google.com/app",
            "https://accounts.google.com.tr/accounts/SetSID?sidt=SECRET_TOKEN&continue=https://gemini.google.com/app",
            "https://accounts.google.co.uk/accounts/SetSID?sidt=SECRET",
            "https://accounts.google.de/Logout",
            "https://accounts.youtube.com/accounts/SetSID?sidt=SECRET",
            "https://google.com/",
            "https://google.com.tr/",
            "https://gemini.google/desktop/",
            "https://myaccount.google.com/",
            "https://docs.google.com/document/d/123",
            "https://drive.google.com/drive/my-drive",
            "https://lh3.googleusercontent.com/a/avatar",
            "https://fonts.gstatic.com/s/font.woff2",
            "about:blank",
            "blob:https://gemini.google.com/1234",
        ];

        for uri in allowed {
            assert_eq!(
                classify_navigation(uri),
                NavigationDecision::AllowInApp,
                "expected AllowInApp for {uri}"
            );
        }
    }

    #[test]
    fn rejects_spoofed_google_domains_to_external() {
        let external = [
            "https://google.com.evil.com/phish",
            "https://accounts.google.com.tr.attacker.org/login",
            "https://evil-google.com/signin",
            "https://notgoogle.com/",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://github.com/rust-lang/rust",
            "https://en.wikipedia.org/wiki/Rust",
        ];

        for uri in external {
            assert_eq!(
                classify_navigation(uri),
                NavigationDecision::OpenExternal,
                "expected OpenExternal for {uri}"
            );
        }
    }

    #[test]
    fn blocks_unsafe_schemes() {
        for uri in [
            "file:///C:/Windows/System32/cmd.exe",
            "javascript:alert(1)",
            "ms-msdt:/id",
            "ftp://example.com/file",
            "not a url",
        ] {
            assert_eq!(
                classify_navigation(uri),
                NavigationDecision::Block,
                "expected Block for {uri}"
            );
            assert_eq!(
                classify_new_window(uri),
                NewWindowDecision::Deny,
                "expected Deny for popup {uri}"
            );
        }
    }

    #[test]
    fn popup_policy_keeps_auth_and_workspace_in_app_and_opens_links_externally() {
        for uri in [
            "about:blank",
            "",
            "https://accounts.google.com/o/oauth2/auth",
            "https://accounts.google.com.tr/accounts/SetSID",
            "https://accounts.youtube.com/accounts/SetSID",
            "https://gemini.google.com/app/abc",
            "https://drive.google.com/picker",
            "https://docs.google.com/picker",
            "https://myaccount.google.com/",
        ] {
            assert_eq!(
                classify_new_window(uri),
                NewWindowDecision::AllowInAppPopup,
                "expected AllowInAppPopup for {uri}"
            );
        }

        for uri in [
            "https://github.com/openai",
            "https://policies.google.com/privacy",
            "https://support.google.com/gemini",
            "https://www.youtube.com/watch?v=123",
            "https://example.com/article",
        ] {
            assert_eq!(
                classify_new_window(uri),
                NewWindowDecision::OpenExternal,
                "expected OpenExternal for popup {uri}"
            );
        }
    }

    #[test]
    fn redacts_query_strings_and_fragments_from_logs() {
        let sensitive = "https://accounts.google.com.tr/accounts/SetSID?sidt=SUPER_SECRET_TOKEN&continue=https%3A%2F%2Fgemini.google.com%2Fapp#frag=secret";
        let redacted = redact_url_for_log(sensitive);
        assert_eq!(redacted, "https://accounts.google.com.tr/accounts/SetSID");
        assert!(!redacted.contains("SUPER_SECRET_TOKEN"));
        assert!(!redacted.contains("frag"));
    }
}
