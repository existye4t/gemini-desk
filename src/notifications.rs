//! Native desktop notifications for GeminiDesk.
//!
//! Privacy & UX guarantees:
//! - Strictly respects user opt-in (`notifications_enabled`) and OS Do Not Disturb settings.
//! - Used only for meaningful app-level events (verified updates, shortcut permission/conflict
//!   notices, or capture errors) — never spams and NEVER includes private chat messages,
//!   prompts, clipboard contents, or Google account emails.
//! - Uses native Linux D-Bus `org.freedesktop.Notifications` (via `zbus`) with `notify-send`
//!   fallback; degrades gracefully without errors if the notification daemon is unavailable.

use tracing::debug;

const MAX_TITLE_CHARS: usize = 80;
const MAX_BODY_CHARS: usize = 220;

/// Dispatches a concise, non-sensitive desktop notification asynchronously if `enabled` is true.
pub fn notify_if_enabled(enabled: bool, title: &str, body: &str) {
    if !enabled {
        return;
    }
    let (clean_title, clean_body) = sanitize_notification_text(title, body);
    if clean_title.is_empty() && clean_body.is_empty() {
        return;
    }

    #[cfg(target_os = "linux")]
    {
        std::thread::Builder::new()
            .name("geminidesk-notify".into())
            .spawn(move || {
                if let Err(err) = send_linux_dbus_notification(&clean_title, &clean_body) {
                    debug!(%err, "D-Bus notification unavailable; trying notify-send fallback");
                    let _ = send_linux_notify_send_fallback(&clean_title, &clean_body);
                }
            })
            .ok();
    }

    #[cfg(not(target_os = "linux"))]
    {
        debug!(title = %clean_title, body = %clean_body, "Desktop notification skipped on this target");
    }
}

/// Strips control characters and bounds notification length so OS daemons receive clean text.
pub fn sanitize_notification_text(title: &str, body: &str) -> (String, String) {
    let clean = |s: &str, limit: usize| -> String {
        let collapsed: String = s
            .chars()
            .map(|c| if c.is_control() && c != ' ' { ' ' } else { c })
            .collect();
        let trimmed = collapsed.split_whitespace().collect::<Vec<_>>().join(" ");
        trimmed.chars().take(limit).collect()
    };
    (clean(title, MAX_TITLE_CHARS), clean(body, MAX_BODY_CHARS))
}

#[cfg(target_os = "linux")]
fn send_linux_dbus_notification(title: &str, body: &str) -> Result<u32, String> {
    use std::collections::HashMap;
    use zbus::zvariant::Value;

    let conn =
        zbus::blocking::Connection::session().map_err(|e| format!("D-Bus session error: {e}"))?;
    let proxy = zbus::blocking::Proxy::new(
        &conn,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )
    .map_err(|e| format!("Notifications proxy error: {e}"))?;

    let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
    hints.insert(
        "desktop-entry",
        Value::from(crate::platform::linux::LINUX_APP_ID),
    );
    hints.insert("urgency", Value::from(1u8));

    let actions: Vec<&str> = Vec::new();
    let id: u32 = proxy
        .call(
            "Notify",
            &(
                "GeminiDesk",
                0u32,
                crate::platform::linux::LINUX_APP_ID,
                title,
                body,
                actions,
                hints,
                5000i32,
            ),
        )
        .map_err(|e| format!("Notify D-Bus call failed: {e}"))?;
    Ok(id)
}

#[cfg(target_os = "linux")]
fn send_linux_notify_send_fallback(title: &str, body: &str) -> Result<(), String> {
    let status = std::process::Command::new("notify-send")
        .args([
            "--app-name=GeminiDesk",
            &format!("--icon={}", crate::platform::linux::LINUX_APP_ID),
            "--expire-time=5000",
            title,
            body,
        ])
        .status()
        .map_err(|e| format!("notify-send unavailable: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("notify-send exited with {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_control_chars_and_bounds_notification_payloads() {
        let (title, body) = sanitize_notification_text(
            "  GeminiDesk\n\tUpdate  ",
            &format!("Ready\r\n{}", "x".repeat(500)),
        );
        assert_eq!(title, "GeminiDesk Update");
        assert!(body.starts_with("Ready "));
        assert!(body.chars().count() <= MAX_BODY_CHARS);
        assert!(!body.contains('\n'));
        assert!(!body.contains('\r'));
    }
}
