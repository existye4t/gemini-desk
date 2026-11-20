//! On-demand native OS clipboard access with strict privacy guarantees:
//! - Never polls or monitors the clipboard in the background.
//! - Never logs clipboard contents.
//! - Provides safe multi-format snapshot & restoration for selection fallback.
//! - Writes both native `CF_DIB` and `"PNG"` formats for crisp image pasting into Gemini.

/// Result of reading plain text from the OS clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ClipboardText {
    Text { text: String, truncated: bool },
    Empty,
    UnsupportedFormat,
    Unavailable(String),
}

/// Captures a bounded Unicode text snapshot from the system clipboard on demand.
pub fn read_unicode_text(max_chars: usize) -> ClipboardText {
    #[cfg(target_os = "windows")]
    {
        windows_impl::read_unicode_text(max_chars)
    }
    #[cfg(target_os = "linux")]
    {
        linux_impl::read_unicode_text(max_chars)
    }
    #[cfg(target_os = "macos")]
    {
        macos_impl::read_unicode_text(max_chars)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let _ = max_chars;
        ClipboardText::Unavailable("Clipboard reading not yet implemented on this OS".into())
    }
}

/// Writes UTF-16/UTF-8 Unicode text to the system clipboard.
pub fn write_unicode_text(text: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        windows_impl::write_unicode_text(text)
    }
    #[cfg(target_os = "linux")]
    {
        linux_impl::write_unicode_text(text)
    }
    #[cfg(target_os = "macos")]
    {
        macos_impl::write_unicode_text(text)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let _ = text;
        Err("Clipboard writing not yet implemented on this OS".into())
    }
}

/// Writes an in-memory screenshot to the system clipboard (`CF_DIB`+`PNG` on Windows,
/// `image/png` on Linux Wayland/X11, `NSPasteboardTypePNG` on macOS).
pub fn write_image(
    bgra_pixels: &[u8],
    width: u32,
    height: u32,
    png_bytes: &[u8],
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        windows_impl::write_image(bgra_pixels, width, height, png_bytes)
    }
    #[cfg(target_os = "linux")]
    {
        let _ = (bgra_pixels, width, height);
        linux_impl::write_png_image(png_bytes)
    }
    #[cfg(target_os = "macos")]
    {
        let _ = (bgra_pixels, width, height);
        macos_impl::write_png_image(png_bytes)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let _ = (bgra_pixels, width, height, png_bytes);
        Err("Image clipboard writing not yet implemented on this OS".into())
    }
}

/// Linux-only helper reading the X11/Wayland Primary Selection strictly on explicit user invocation.
#[cfg(target_os = "linux")]
pub fn read_primary_selection(max_chars: usize) -> ClipboardText {
    linux_impl::read_primary_selection(max_chars)
}

/// Non-destructive clipboard snapshot preserving common memory-backed formats (text, HTML, RTF, DIB, PNG).
#[cfg(target_os = "windows")]
pub use windows_impl::{ClipboardSnapshot, sequence_number};

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::ffi::OsStr;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::copy_nonoverlapping;
    use std::thread;
    use std::time::Duration;

    use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
    use windows::Win32::Graphics::Gdi::{BI_RGB, BITMAPINFOHEADER};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
        GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard,
        RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
    };
    use windows::Win32::System::Ole::{CF_DIB, CF_OEMTEXT, CF_TEXT, CF_UNICODETEXT};
    use windows::core::PCWSTR;

    use super::ClipboardText;

    /// Maximum total bytes we will snapshot per format to protect against huge clipboard objects.
    const MAX_SNAPSHOT_FORMAT_BYTES: usize = 16 * 1024 * 1024;

    struct OpenClipboardGuard;

    impl OpenClipboardGuard {
        fn open() -> Result<Self, String> {
            for _ in 0..8 {
                if unsafe { OpenClipboard(Some(HWND::default())) }.is_ok() {
                    return Ok(Self);
                }
                thread::sleep(Duration::from_millis(12));
            }
            Err("Clipboard is currently locked by another application".into())
        }
    }

    impl Drop for OpenClipboardGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }

    pub fn sequence_number() -> u32 {
        unsafe { GetClipboardSequenceNumber() }
    }

    fn register_format(name: &str) -> u32 {
        let wide: Vec<u16> = OsStr::new(name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        unsafe { RegisterClipboardFormatW(PCWSTR(wide.as_ptr())) }
    }

    unsafe fn alloc_and_copy(bytes: &[u8]) -> Result<HGLOBAL, String> {
        let hmem = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }
            .map_err(|e| format!("GlobalAlloc failed: {e}"))?;
        let dst = unsafe { GlobalLock(hmem) };
        if dst.is_null() {
            return Err("GlobalLock failed".into());
        }
        unsafe {
            copy_nonoverlapping(bytes.as_ptr(), dst as *mut u8, bytes.len());
            let _ = GlobalUnlock(hmem);
        }
        Ok(hmem)
    }

    pub fn read_unicode_text(max_chars: usize) -> ClipboardText {
        let _guard = match OpenClipboardGuard::open() {
            Ok(g) => g,
            Err(e) => return ClipboardText::Unavailable(e),
        };

        unsafe {
            if IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_err() {
                let first_fmt = EnumClipboardFormats(0);
                if first_fmt == 0 {
                    return ClipboardText::Empty;
                }
                return ClipboardText::UnsupportedFormat;
            }

            let Ok(handle) = GetClipboardData(CF_UNICODETEXT.0 as u32) else {
                return ClipboardText::Empty;
            };
            let hglobal = HGLOBAL(handle.0);
            let ptr = GlobalLock(hglobal) as *const u16;
            if ptr.is_null() {
                return ClipboardText::Empty;
            }

            let byte_len = GlobalSize(hglobal);
            let max_u16 = byte_len / 2;
            let slice = std::slice::from_raw_parts(ptr, max_u16);
            let nul_pos = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
            let raw = String::from_utf16_lossy(&slice[..nul_pos]);
            let _ = GlobalUnlock(hglobal);

            let trimmed_empty = raw.trim().is_empty();
            if trimmed_empty {
                return ClipboardText::Empty;
            }

            let mut truncated = false;
            let text = if raw.chars().count() > max_chars {
                truncated = true;
                raw.chars().take(max_chars).collect()
            } else {
                raw
            };
            ClipboardText::Text { text, truncated }
        }
    }

    pub fn write_unicode_text(text: &str) -> Result<(), String> {
        let _guard = OpenClipboardGuard::open()?;
        let utf16: Vec<u16> = OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let bytes = unsafe {
            std::slice::from_raw_parts(utf16.as_ptr() as *const u8, utf16.len() * size_of::<u16>())
        };

        unsafe {
            EmptyClipboard().map_err(|e| format!("EmptyClipboard failed: {e}"))?;
            let hmem = alloc_and_copy(bytes)?;
            SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(hmem.0)))
                .map_err(|e| format!("SetClipboardData(CF_UNICODETEXT) failed: {e}"))?;
        }
        Ok(())
    }

    pub fn write_image(
        bgra_pixels: &[u8],
        width: u32,
        height: u32,
        png_bytes: &[u8],
    ) -> Result<(), String> {
        let expected = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4);
        if bgra_pixels.len() < expected || width == 0 || height == 0 {
            return Err("Invalid image dimensions for clipboard write".into());
        }

        // Build bottom-up 32-bit BI_RGB DIB (universally supported by Chromium/WebView2 & Windows).
        let header = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: height as i32, // positive = bottom-up
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: expected as u32,
            ..Default::default()
        };

        let mut dib = Vec::with_capacity(size_of::<BITMAPINFOHEADER>() + expected);
        let header_bytes = unsafe {
            std::slice::from_raw_parts(
                &header as *const BITMAPINFOHEADER as *const u8,
                size_of::<BITMAPINFOHEADER>(),
            )
        };
        dib.extend_from_slice(header_bytes);

        let stride = (width as usize) * 4;
        for row in (0..height as usize).rev() {
            let start = row * stride;
            dib.extend_from_slice(&bgra_pixels[start..start + stride]);
        }

        let png_fmt = register_format("PNG");
        let _guard = OpenClipboardGuard::open()?;
        unsafe {
            EmptyClipboard().map_err(|e| format!("EmptyClipboard failed: {e}"))?;
            let h_dib = alloc_and_copy(&dib)?;
            SetClipboardData(CF_DIB.0 as u32, Some(HANDLE(h_dib.0)))
                .map_err(|e| format!("SetClipboardData(CF_DIB) failed: {e}"))?;

            if png_fmt != 0 && !png_bytes.is_empty() {
                let h_png = alloc_and_copy(png_bytes)?;
                let _ = SetClipboardData(png_fmt, Some(HANDLE(h_png.0)));
            }
        }
        Ok(())
    }

    /// Snapshot of safe memory-backed clipboard formats for non-destructive restoration.
    pub struct ClipboardSnapshot {
        entries: Vec<(u32, Vec<u8>)>,
    }

    impl ClipboardSnapshot {
        pub fn capture() -> Option<Self> {
            let _guard = OpenClipboardGuard::open().ok()?;
            let mut entries = Vec::new();
            let html_fmt = register_format("HTML Format");
            let rtf_fmt = register_format("Rich Text Format");
            let png_fmt = register_format("PNG");

            unsafe {
                let mut fmt = EnumClipboardFormats(0);
                while fmt != 0 {
                    let is_safe_global = fmt == CF_UNICODETEXT.0 as u32
                        || fmt == CF_TEXT.0 as u32
                        || fmt == CF_OEMTEXT.0 as u32
                        || fmt == CF_DIB.0 as u32
                        || (html_fmt != 0 && fmt == html_fmt)
                        || (rtf_fmt != 0 && fmt == rtf_fmt)
                        || (png_fmt != 0 && fmt == png_fmt);

                    if is_safe_global && let Ok(handle) = GetClipboardData(fmt) {
                        let hglobal = HGLOBAL(handle.0);
                        let sz = GlobalSize(hglobal);
                        if sz > 0 && sz <= MAX_SNAPSHOT_FORMAT_BYTES {
                            let ptr = GlobalLock(hglobal) as *const u8;
                            if !ptr.is_null() {
                                let buf = std::slice::from_raw_parts(ptr, sz).to_vec();
                                let _ = GlobalUnlock(hglobal);
                                entries.push((fmt, buf));
                            }
                        }
                    }

                    fmt = EnumClipboardFormats(fmt);
                }
            }

            Some(Self { entries })
        }

        pub fn restore(self) {
            if self.entries.is_empty() {
                return;
            }
            let Ok(_guard) = OpenClipboardGuard::open() else {
                return;
            };
            unsafe {
                if EmptyClipboard().is_err() {
                    return;
                }
                for (fmt, bytes) in self.entries {
                    if let Ok(hmem) = alloc_and_copy(&bytes) {
                        let _ = SetClipboardData(fmt, Some(HANDLE(hmem.0)));
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod linux_impl {
    use std::io::Write;
    use std::process::{Command, Stdio};

    use super::ClipboardText;
    use crate::platform::linux::is_wayland_session;

    fn run_read_cmd(cmd: &str, args: &[&str]) -> Option<Result<String, ()>> {
        let out = Command::new(cmd).args(args).output().ok()?;
        if !out.status.success() {
            return Some(Err(()));
        }
        Some(Ok(String::from_utf8_lossy(&out.stdout).into_owned()))
    }

    fn format_text(raw: String, max_chars: usize) -> ClipboardText {
        if raw.trim().is_empty() {
            return ClipboardText::Empty;
        }
        let mut truncated = false;
        let text = if raw.chars().count() > max_chars {
            truncated = true;
            raw.chars().take(max_chars).collect()
        } else {
            raw
        };
        ClipboardText::Text { text, truncated }
    }

    pub fn read_unicode_text(max_chars: usize) -> ClipboardText {
        let candidates: &[(&str, &[&str])] = if is_wayland_session() {
            &[
                (
                    "wl-paste",
                    &["--no-newline", "--type", "text/plain;charset=utf-8"],
                ),
                ("wl-paste", &["--no-newline"]),
                ("xclip", &["-o", "-selection", "clipboard"]),
                ("xsel", &["-ob"]),
            ]
        } else {
            &[
                ("xclip", &["-o", "-selection", "clipboard"]),
                ("xsel", &["-ob"]),
                ("wl-paste", &["--no-newline"]),
            ]
        };

        let mut any_tool = false;
        for (bin, args) in candidates {
            if let Some(res) = run_read_cmd(bin, args) {
                any_tool = true;
                if let Ok(raw) = res {
                    return format_text(raw, max_chars);
                }
            }
        }

        if any_tool {
            ClipboardText::Empty
        } else {
            ClipboardText::Unavailable(
                "Install `wl-clipboard` (Wayland) or `xclip` (X11) for clipboard access.".into(),
            )
        }
    }

    pub fn read_primary_selection(max_chars: usize) -> ClipboardText {
        let candidates: &[(&str, &[&str])] = if is_wayland_session() {
            &[
                ("wl-paste", &["--primary", "--no-newline"]),
                ("xclip", &["-o", "-selection", "primary"]),
                ("xsel", &["-op"]),
            ]
        } else {
            &[
                ("xclip", &["-o", "-selection", "primary"]),
                ("xsel", &["-op"]),
            ]
        };

        for (bin, args) in candidates {
            if let Some(Ok(raw)) = run_read_cmd(bin, args) {
                return format_text(raw, max_chars);
            }
        }
        ClipboardText::Empty
    }

    fn pipe_to_cmd(bin: &str, args: &[&str], data: &[u8]) -> Option<Result<(), String>> {
        let mut child = Command::new(bin)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;

        if let Some(mut stdin) = child.stdin.take()
            && let Err(err) = stdin.write_all(data)
        {
            return Some(Err(format!("{bin} write failed: {err}")));
        }

        match child.wait() {
            Ok(status) if status.success() => Some(Ok(())),
            Ok(status) => Some(Err(format!("{bin} exited with {status}"))),
            Err(err) => Some(Err(format!("{bin} wait failed: {err}"))),
        }
    }

    pub fn write_unicode_text(text: &str) -> Result<(), String> {
        let candidates: &[(&str, &[&str])] = if is_wayland_session() {
            &[
                ("wl-copy", &[]),
                ("xclip", &["-i", "-selection", "clipboard"]),
                ("xsel", &["-ib"]),
            ]
        } else {
            &[
                ("xclip", &["-i", "-selection", "clipboard"]),
                ("xsel", &["-ib"]),
                ("wl-copy", &[]),
            ]
        };

        for (bin, args) in candidates {
            if let Some(res) = pipe_to_cmd(bin, args, text.as_bytes()) {
                return res;
            }
        }
        Err("Install `wl-clipboard` (Wayland) or `xclip` (X11) to write to the clipboard.".into())
    }

    pub fn write_png_image(png_bytes: &[u8]) -> Result<(), String> {
        if png_bytes.is_empty() {
            return Err("Empty PNG image buffer".into());
        }
        let candidates: &[(&str, &[&str])] = if is_wayland_session() {
            &[
                ("wl-copy", &["--type", "image/png"]),
                (
                    "xclip",
                    &["-selection", "clipboard", "-t", "image/png", "-i"],
                ),
            ]
        } else {
            &[
                (
                    "xclip",
                    &["-selection", "clipboard", "-t", "image/png", "-i"],
                ),
                ("wl-copy", &["--type", "image/png"]),
            ]
        };

        for (bin, args) in candidates {
            if let Some(res) = pipe_to_cmd(bin, args, png_bytes) {
                return res;
            }
        }
        Err("Install `wl-clipboard` (Wayland) or `xclip` (X11) to copy images.".into())
    }
}

#[cfg(target_os = "macos")]
mod macos_impl {
    use std::io::Write;
    use std::process::{Command, Stdio};

    use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG};
    use objc2_foundation::NSData;

    use super::ClipboardText;

    pub fn read_unicode_text(max_chars: usize) -> ClipboardText {
        let Ok(out) = Command::new("/usr/bin/pbpaste").output() else {
            return ClipboardText::Unavailable("macOS pbpaste unavailable".into());
        };
        if !out.status.success() {
            return ClipboardText::Empty;
        }
        let raw = String::from_utf8_lossy(&out.stdout).into_owned();
        if raw.trim().is_empty() {
            return ClipboardText::Empty;
        }
        let mut truncated = false;
        let text = if raw.chars().count() > max_chars {
            truncated = true;
            raw.chars().take(max_chars).collect()
        } else {
            raw
        };
        ClipboardText::Text { text, truncated }
    }

    pub fn write_unicode_text(text: &str) -> Result<(), String> {
        let mut child = Command::new("/usr/bin/pbcopy")
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn pbcopy: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| format!("Failed writing to pbcopy: {e}"))?;
        }
        let status = child
            .wait()
            .map_err(|e| format!("Failed waiting for pbcopy: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("pbcopy exited with {status}"))
        }
    }

    pub fn write_png_image(png_bytes: &[u8]) -> Result<(), String> {
        if png_bytes.is_empty() {
            return Err("Empty PNG image buffer".into());
        }
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        let data = NSData::with_bytes(png_bytes);
        let ok = unsafe { pasteboard.setData_forType(Some(&data), NSPasteboardTypePNG) };
        if ok {
            Ok(())
        } else {
            Err("NSPasteboard failed to store PNG data".into())
        }
    }
}
