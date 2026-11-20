use std::path::PathBuf;

use directories::ProjectDirs;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use tracing::{info, warn};
use url::Url;
use wry::{NewWindowResponse, PermissionResponse, WebContext, WebViewBuilder};

const GEMINI_URL: &str = "https://gemini.google.com/app";

fn webview_data_dir() -> PathBuf {
    let base = ProjectDirs::from("dev", "geminidesk", "GeminiDesk")
        .map(|d| d.data_local_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".geminidesk-data"));
    let dir = base.join("webview");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn is_allowed_in_app_host(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    h == "gemini.google.com"
        || h == "accounts.google.com"
        || h == "accounts.youtube.com"
        || h == "myaccount.google.com"
        || h == "gds.google.com"
        || h == "consent.google.com"
        || h == "ogs.google.com"
        || h == "docs.google.com"
        || h == "drive.google.com"
        || h == "workspace.google.com"
        || h == "one.google.com"
        || h == "aistudio.google.com"
        || h == "bard.google.com"
        || h.ends_with(".google.com")
        || h.ends_with(".gstatic.com")
        || h.ends_with(".googleusercontent.com")
}

fn main() -> wry::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let data_dir = webview_data_dir();
    info!(?data_dir, "Starting GeminiDesk Phase B compatibility proof");

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("GeminiDesk — Unofficial Gemini Client (Phase B Proof)")
        .with_inner_size(LogicalSize::new(1200.0, 820.0))
        .with_min_inner_size(LogicalSize::new(420.0, 520.0))
        .build(&event_loop)
        .expect("failed to build main window");

    let mut web_context = WebContext::new(Some(data_dir));

    let _webview = WebViewBuilder::new_with_web_context(&mut web_context)
        .with_url(GEMINI_URL)
        .with_clipboard(true)
        .with_hotkeys_zoom(true)
        .with_permission_handler(|kind| {
            info!(?kind, "Permission requested by web content");
            PermissionResponse::Default
        })
        .with_navigation_handler(|uri| {
            match Url::parse(&uri) {
                Ok(parsed) => {
                    let scheme = parsed.scheme();
                    if scheme == "about" || scheme == "blob" || scheme == "data" {
                        return true;
                    }
                    if scheme != "https" {
                        warn!(%uri, "Blocked non-HTTPS navigation");
                        return false;
                    }
                    if let Some(host) = parsed.host_str() {
                        if is_allowed_in_app_host(host) {
                            info!(%host, "Allowing in-app navigation");
                            return true;
                        }
                        info!(%host, "Opening external URL in system browser");
                        let _ = open::that(&uri);
                        return false;
                    }
                    false
                }
                Err(err) => {
                    warn!(%uri, ?err, "Invalid navigation URL");
                    false
                }
            }
        })
        .with_new_window_req_handler(|uri, _features| {
            info!(%uri, "New window requested");
            if let Ok(parsed) = Url::parse(&uri)
                && parsed.scheme() == "https"
                && let Some(host) = parsed.host_str()
            {
                if host == "accounts.google.com"
                    || host == "docs.google.com"
                    || host == "drive.google.com"
                {
                    return NewWindowResponse::Allow;
                }
                let _ = open::that(&uri);
            }
            NewWindowResponse::Deny
        })
        .with_document_title_changed_handler(|title| {
            info!(%title, "Document title changed");
        })
        .build(&window)?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}
