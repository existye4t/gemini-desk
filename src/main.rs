mod navigation;

use std::path::PathBuf;

use directories::ProjectDirs;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use tracing::{info, warn};
use wry::{NewWindowResponse, PageLoadEvent, PermissionResponse, WebContext, WebViewBuilder};

use crate::navigation::{
    NavigationDecision, NewWindowDecision, classify_navigation, classify_new_window,
    redact_url_for_log,
};

const GEMINI_URL: &str = "https://gemini.google.com/app";

fn webview_data_dir() -> PathBuf {
    let base = ProjectDirs::from("dev", "geminidesk", "GeminiDesk")
        .map(|d| d.data_local_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".geminidesk-data"));
    let dir = base.join("webview");
    let _ = std::fs::create_dir_all(&dir);
    dir
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
        .with_title("GeminiDesk — Unofficial Gemini Client (Phase B Auth Verification)")
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
        .with_on_page_load_handler(|event, uri| {
            let safe_url = redact_url_for_log(&uri);
            let state = match event {
                PageLoadEvent::Started => "started",
                PageLoadEvent::Finished => "finished",
            };
            info!(state, url = %safe_url, "Page load state changed");
        })
        .with_navigation_handler(|uri| {
            let safe_url = redact_url_for_log(&uri);
            match classify_navigation(&uri) {
                NavigationDecision::AllowInApp => {
                    info!(url = %safe_url, "Allowing in-app navigation");
                    true
                }
                NavigationDecision::OpenExternal => {
                    info!(url = %safe_url, "Opening external URL in system browser");
                    if let Err(err) = open::that(&uri) {
                        warn!(url = %safe_url, ?err, "Failed to open external URL");
                    }
                    false
                }
                NavigationDecision::Block => {
                    warn!(url = %safe_url, "Blocked disallowed navigation");
                    false
                }
            }
        })
        .with_new_window_req_handler(|uri, _features| {
            let safe_url = redact_url_for_log(&uri);
            match classify_new_window(&uri) {
                NewWindowDecision::AllowInAppPopup => {
                    info!(url = %safe_url, "Allowing in-app popup");
                    NewWindowResponse::Allow
                }
                NewWindowDecision::OpenExternal => {
                    info!(url = %safe_url, "Opening external popup URL in system browser");
                    if let Err(err) = open::that(&uri) {
                        warn!(url = %safe_url, ?err, "Failed to open external popup URL");
                    }
                    NewWindowResponse::Deny
                }
                NewWindowDecision::Deny => {
                    warn!(url = %safe_url, "Denied disallowed popup");
                    NewWindowResponse::Deny
                }
            }
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
