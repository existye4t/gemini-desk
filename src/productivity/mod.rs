pub mod action_window;
pub mod clipboard;
pub mod screenshot;
pub mod selection;

use serde::Serialize;

/// Encoded in-memory screenshot capture (never written to disk automatically).
#[derive(Debug, Clone)]
pub struct ScreenshotData {
    pub width: u32,
    pub height: u32,
    /// Lossless PNG bytes.
    pub png_bytes: Vec<u8>,
    /// Raw top-down 32-bit BGRA pixels for direct native `CF_DIB` Windows clipboard handoff.
    pub bgra_pixels: Vec<u8>,
}

/// Source description shown transparently in the Prompt Review UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum CaptureSource {
    UiAutomation,
    ClipboardFallback,
    ManualFallback,
    ProtectedField,
    Clipboard,
    ClipboardEmpty,
}

/// Minimal zero-dependency RFC 4648 base64 encoder (used strictly for local preview data URIs).
pub fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Applies a clipboard action template by replacing `{{text}}` safely.
#[cfg(test)]
pub fn apply_template(template: &str, text: &str) -> String {
    if template.contains("{{text}}") {
        template.replace("{{text}}", text)
    } else if text.is_empty() {
        template.to_string()
    } else {
        format!("{template}\n\n{text}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_encodes_rfc4648_vectors() {
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
        assert_eq!(encode_base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn template_substitution_works_with_or_without_placeholder() {
        assert_eq!(
            apply_template("Explain:\n{{text}}", "let x = 1;"),
            "Explain:\nlet x = 1;"
        );
        assert_eq!(
            apply_template("Summarize this", "Hello world"),
            "Summarize this\n\nHello world"
        );
    }
}
