//! Embedded local UI asset server for strictly isolated GeminiDesk utility windows
//! (Settings and Productivity Action Panels).
//!
//! Security boundary:
//! - Registered ONLY on local utility WebViews (`SettingsWindow` and `ActionWindow`).
//! - NEVER registered on the remote `gemini.google.com` WebView.
//! - Zero third-party network requests; all stylesheets and SIL OFL 1.1 Geist variable
//!   WOFF2 fonts are served synchronously from memory (<0.2 ms, zero font flash).

use wry::http::{Request, Response, StatusCode, header::CONTENT_TYPE};

const SETTINGS_HTML: &[u8] = include_bytes!("ui/settings.html");
const ACTION_PANEL_HTML: &[u8] = include_bytes!("ui/action_panel.html");
const DESIGN_SYSTEM_CSS: &[u8] = include_bytes!("ui/design-system.css");
const GEIST_SANS_WOFF2: &[u8] = include_bytes!("../assets/fonts/Geist-Variable.woff2");
const GEIST_MONO_WOFF2: &[u8] = include_bytes!("../assets/fonts/GeistMono-Variable.woff2");

/// Custom URI scheme registered exclusively on local GeminiDesk utility WebViews.
pub const UI_PROTOCOL: &str = "geminidesk";

/// Returns the entry URL for a local utility page on the current platform.
pub fn page_url(path: &str) -> String {
    let trimmed = path.trim_start_matches('/');
    #[cfg(target_os = "windows")]
    {
        // On WebView2, `wry` maps custom protocol `<scheme>` to `http://<scheme>.localhost/`.
        format!("http://{UI_PROTOCOL}.localhost/{trimmed}")
    }
    #[cfg(not(target_os = "windows"))]
    {
        format!("{UI_PROTOCOL}://localhost/{trimmed}")
    }
}

/// Returns true only for navigation targets strictly belonging to our local UI protocol.
pub fn is_local_ui_url(uri: &str) -> bool {
    uri == "about:blank"
        || uri.starts_with(&format!("http://{UI_PROTOCOL}.localhost/"))
        || uri.starts_with(&format!("https://{UI_PROTOCOL}.localhost/"))
        || uri.starts_with(&format!("{UI_PROTOCOL}://localhost/"))
}

/// Resolves an embedded asset request for the local `geminidesk` protocol.
pub fn handle_protocol_request(req: Request<Vec<u8>>) -> Response<std::borrow::Cow<'static, [u8]>> {
    let path = req.uri().path();
    let (status, mime, body): (StatusCode, &str, &'static [u8]) = match path {
        "/settings.html" => (StatusCode::OK, "text/html; charset=utf-8", SETTINGS_HTML),
        "/action_panel.html" => (
            StatusCode::OK,
            "text/html; charset=utf-8",
            ACTION_PANEL_HTML,
        ),
        "/design-system.css" => (StatusCode::OK, "text/css; charset=utf-8", DESIGN_SYSTEM_CSS),
        "/fonts/Geist-Variable.woff2" => (StatusCode::OK, "font/woff2", GEIST_SANS_WOFF2),
        "/fonts/GeistMono-Variable.woff2" => (StatusCode::OK, "font/woff2", GEIST_MONO_WOFF2),
        _ => (
            StatusCode::NOT_FOUND,
            "text/plain; charset=utf-8",
            b"Not Found",
        ),
    };

    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, mime)
        .header("Cache-Control", "public, max-age=31536000, immutable")
        .body(std::borrow::Cow::Borrowed(body))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(std::borrow::Cow::Borrowed(&b"Internal Error"[..]))
                .expect("fallback response")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_embedded_fonts_and_stylesheets() {
        for (path, expected_mime) in [
            ("/settings.html", "text/html; charset=utf-8"),
            ("/action_panel.html", "text/html; charset=utf-8"),
            ("/design-system.css", "text/css; charset=utf-8"),
            ("/fonts/Geist-Variable.woff2", "font/woff2"),
            ("/fonts/GeistMono-Variable.woff2", "font/woff2"),
        ] {
            let req = Request::builder().uri(path).body(Vec::new()).unwrap();
            let resp = handle_protocol_request(req);
            assert_eq!(resp.status(), StatusCode::OK, "path {path}");
            assert_eq!(resp.headers().get(CONTENT_TYPE).unwrap(), expected_mime);
            assert!(!resp.body().is_empty(), "empty body for {path}");
        }

        // Verify WOFF2 magic bytes (`wOF2`)
        assert_eq!(&GEIST_SANS_WOFF2[..4], b"wOF2");
        assert_eq!(&GEIST_MONO_WOFF2[..4], b"wOF2");
    }

    #[test]
    fn blocks_non_local_urls() {
        assert!(is_local_ui_url("http://geminidesk.localhost/settings.html"));
        assert!(is_local_ui_url("geminidesk://localhost/action_panel.html"));
        assert!(!is_local_ui_url("https://gemini.google.com/app"));
        assert!(!is_local_ui_url("https://evil.example/"));
    }
}
