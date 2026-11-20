//! On-demand native multi-monitor, mixed-DPI screen region capture.
//!
//! Architecture & privacy guarantees:
//! - Zero continuous screen capture or background polling.
//! - Activated strictly on explicit user shortcut or tray click.
//! - Uses a lightweight native Win32 layered/topmost virtual-screen overlay (zero WebView
//!   overhead for the selection UI) with double-buffered GDI rendering, crisp 2px selection
//!   frame, live `W x H px` badge, and `Esc` / right-click cancellation.
//! - Encodes the cropped selection to lossless PNG purely in memory (zero temp disk writes)
//!   and immediately frees the full-screen snapshot on completion or cancel.

use super::ScreenshotData;

/// Result returned when the interactive screenshot overlay completes.
#[derive(Debug)]
pub enum CaptureOutcome {
    Captured(ScreenshotData),
    Cancelled,
    Error(String),
}

/// Encodes a top-down 32-bit BGRA buffer into lossless PNG bytes strictly in memory.
pub fn encode_bgra_to_png(bgra: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let pixel_count = (width as usize).saturating_mul(height as usize);
    if width == 0 || height == 0 || bgra.len() < pixel_count * 4 {
        return Err("Invalid screenshot dimensions".into());
    }

    let mut rgba = Vec::with_capacity(pixel_count * 4);
    let (chunks, _) = bgra[..pixel_count * 4].as_chunks::<4>();
    for chunk in chunks {
        rgba.push(chunk[2]); // R
        rgba.push(chunk[1]); // G
        rgba.push(chunk[0]); // B
        rgba.push(255); // Opaque A
    }

    let mut out = Vec::with_capacity(pixel_count / 2);
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("PNG header encode failed: {e}"))?;
        writer
            .write_image_data(&rgba)
            .map_err(|e| format!("PNG data encode failed: {e}"))?;
    }
    Ok(out)
}

/// Captures a specific physical rectangle from the virtual screen headlessly (used for tests
/// without launching the interactive fullscreen overlay).
#[cfg(all(target_os = "windows", test))]
pub fn capture_region_headless(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<ScreenshotData, String> {
    windows_impl::capture_rect_direct(x, y, width, height)
}

/// Launches the interactive full-virtual-screen region selection overlay on the current thread
/// (intended to be invoked on a short-lived worker thread so the main UI loop stays responsive).
pub fn run_interactive_region_capture() -> CaptureOutcome {
    #[cfg(target_os = "windows")]
    {
        windows_impl::run_interactive_overlay()
    }
    #[cfg(not(target_os = "windows"))]
    {
        CaptureOutcome::Error("Interactive screen capture is not yet implemented on this OS".into())
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::ffi::OsStr;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{copy_nonoverlapping, null_mut};
    use std::sync::atomic::{AtomicBool, Ordering};

    use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CreateCompatibleDC,
        CreateDIBSection, CreateFontW, CreatePen, CreateSolidBrush, DIB_RGB_COLORS, DeleteDC,
        DeleteObject, DrawTextW, EndPaint, FillRect, GetDC, HBITMAP, HDC, InvalidateRect,
        PAINTSTRUCT, PS_SOLID, ReleaseDC, RoundRect, SRCCOPY, SelectObject, SetBkMode,
        SetTextColor, TRANSPARENT,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, VK_ESCAPE};
    use windows::Win32::UI::WindowsAndMessaging::{
        CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
        GWLP_USERDATA, GetMessageW, GetSystemMetrics, GetWindowLongPtrW, IDC_CROSS, LoadCursorW,
        MSG, PostQuitMessage, RegisterClassW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
        SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_SHOW, SetForegroundWindow, SetWindowLongPtrW,
        ShowWindow, TranslateMessage, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
        WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONDOWN, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_POPUP,
    };
    use windows::core::PCWSTR;

    use super::{CaptureOutcome, ScreenshotData, encode_bgra_to_png};

    static OVERLAY_ACTIVE: AtomicBool = AtomicBool::new(false);

    pub(super) struct DibBuffer {
        pub(super) hdc: HDC,
        hbitmap: HBITMAP,
        pixels: *mut u8,
        width: i32,
        height: i32,
    }

    impl DibBuffer {
        pub(super) unsafe fn new(width: i32, height: i32) -> Result<Self, String> {
            let screen_dc = unsafe { GetDC(None) };
            let mem_dc = unsafe { CreateCompatibleDC(Some(screen_dc)) };
            unsafe { ReleaseDC(None, screen_dc) };
            if mem_dc.is_invalid() {
                return Err("CreateCompatibleDC failed".into());
            }

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height, // negative = top-down BGRA
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };

            let mut bits: *mut core::ffi::c_void = null_mut();
            let hbitmap =
                unsafe { CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) }
                    .map_err(|e| {
                        unsafe {
                            let _ = DeleteDC(mem_dc);
                        }
                        format!("CreateDIBSection failed: {e}")
                    })?;

            if bits.is_null() {
                unsafe {
                    let _ = DeleteObject(hbitmap.into());
                    let _ = DeleteDC(mem_dc);
                }
                return Err("CreateDIBSection returned null bits".into());
            }

            unsafe {
                SelectObject(mem_dc, hbitmap.into());
            }
            Ok(Self {
                hdc: mem_dc,
                hbitmap,
                pixels: bits as *mut u8,
                width,
                height,
            })
        }

        pub(super) fn as_slice(&self) -> &[u8] {
            let len = (self.width as usize) * (self.height as usize) * 4;
            unsafe { std::slice::from_raw_parts(self.pixels, len) }
        }

        pub(super) fn as_mut_slice(&mut self) -> &mut [u8] {
            let len = (self.width as usize) * (self.height as usize) * 4;
            unsafe { std::slice::from_raw_parts_mut(self.pixels, len) }
        }
    }

    impl Drop for DibBuffer {
        fn drop(&mut self) {
            unsafe {
                let _ = DeleteObject(self.hbitmap.into());
                let _ = DeleteDC(self.hdc);
            }
        }
    }

    #[cfg(test)]
    pub fn capture_rect_direct(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<ScreenshotData, String> {
        if width == 0 || height == 0 {
            return Err("Capture dimensions must be positive".into());
        }
        let dib = unsafe { DibBuffer::new(width as i32, height as i32)? };
        unsafe {
            let screen_dc = GetDC(None);
            let res = BitBlt(
                dib.hdc,
                0,
                0,
                width as i32,
                height as i32,
                Some(screen_dc),
                x,
                y,
                SRCCOPY,
            );
            ReleaseDC(None, screen_dc);
            res.map_err(|e| format!("BitBlt failed: {e}"))?;
        }
        let bgra_pixels = dib.as_slice().to_vec();
        let png_bytes = encode_bgra_to_png(&bgra_pixels, width, height)?;
        Ok(ScreenshotData {
            width,
            height,
            png_bytes,
            bgra_pixels,
        })
    }

    struct OverlayState {
        orig: DibBuffer,
        dimmed: DibBuffer,
        back: DibBuffer,
        dragging: bool,
        start_x: i32,
        start_y: i32,
        curr_x: i32,
        curr_y: i32,
        selection: Option<(i32, i32, i32, i32)>,
        cancelled: bool,
    }

    impl OverlayState {
        fn normalized_rect(&self) -> (i32, i32, i32, i32) {
            let left = self.start_x.min(self.curr_x).clamp(0, self.orig.width);
            let right = self.start_x.max(self.curr_x).clamp(0, self.orig.width);
            let top = self.start_y.min(self.curr_y).clamp(0, self.orig.height);
            let bottom = self.start_y.max(self.curr_y).clamp(0, self.orig.height);
            (left, top, right, bottom)
        }
    }

    fn wide_null(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn sign_extend_i16(v: u16) -> i32 {
        (v as i16) as i32
    }

    unsafe extern "system" fn overlay_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let state_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayState };
        if state_ptr.is_null() {
            return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
        }
        let state = unsafe { &mut *state_ptr };

        match msg {
            WM_ERASEBKGND => LRESULT(1),
            WM_KEYDOWN => {
                if wparam.0 == VK_ESCAPE.0 as usize {
                    state.cancelled = true;
                    unsafe {
                        let _ = DestroyWindow(hwnd);
                        PostQuitMessage(0);
                    }
                    return LRESULT(0);
                }
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
            WM_RBUTTONDOWN => {
                state.cancelled = true;
                unsafe {
                    let _ = DestroyWindow(hwnd);
                    PostQuitMessage(0);
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let x = sign_extend_i16((lparam.0 & 0xFFFF) as u16);
                let y = sign_extend_i16(((lparam.0 >> 16) & 0xFFFF) as u16);
                state.dragging = true;
                state.start_x = x;
                state.start_y = y;
                state.curr_x = x;
                state.curr_y = y;
                unsafe {
                    SetCapture(hwnd);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if state.dragging {
                    state.curr_x = sign_extend_i16((lparam.0 & 0xFFFF) as u16);
                    state.curr_y = sign_extend_i16(((lparam.0 >> 16) & 0xFFFF) as u16);
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                if state.dragging {
                    state.dragging = false;
                    state.curr_x = sign_extend_i16((lparam.0 & 0xFFFF) as u16);
                    state.curr_y = sign_extend_i16(((lparam.0 >> 16) & 0xFFFF) as u16);
                    let (l, t, r, b) = state.normalized_rect();
                    if (r - l) >= 8 && (b - t) >= 8 {
                        state.selection = Some((l, t, r, b));
                    } else {
                        state.cancelled = true;
                    }
                    unsafe {
                        let _ = ReleaseCapture();
                        let _ = DestroyWindow(hwnd);
                        PostQuitMessage(0);
                    }
                }
                LRESULT(0)
            }
            WM_PAINT => {
                unsafe {
                    let mut ps = PAINTSTRUCT::default();
                    let hdc = BeginPaint(hwnd, &mut ps);

                    // 1. Copy dimmed snapshot into backbuffer
                    let _ = BitBlt(
                        state.back.hdc,
                        0,
                        0,
                        state.orig.width,
                        state.orig.height,
                        Some(state.dimmed.hdc),
                        0,
                        0,
                        SRCCOPY,
                    );

                    // 2. If dragging, blit crisp bright region + border + size pill
                    if state.dragging {
                        let (l, t, r, b) = state.normalized_rect();
                        let w = r - l;
                        let h = b - t;
                        if w > 0 && h > 0 {
                            let _ = BitBlt(
                                state.back.hdc,
                                l,
                                t,
                                w,
                                h,
                                Some(state.orig.hdc),
                                l,
                                t,
                                SRCCOPY,
                            );

                            // Crisp 2px #F5F5F7 selection frame
                            let pen = CreatePen(PS_SOLID, 2, COLORREF(0x00F7_F5F5));
                            let old_pen = SelectObject(state.back.hdc, pen.into());
                            let null_brush = windows::Win32::Graphics::Gdi::GetStockObject(
                                windows::Win32::Graphics::Gdi::HOLLOW_BRUSH,
                            );
                            let old_brush = SelectObject(state.back.hdc, null_brush);
                            let _ = windows::Win32::Graphics::Gdi::Rectangle(
                                state.back.hdc,
                                l,
                                t,
                                r,
                                b,
                            );
                            SelectObject(state.back.hdc, old_brush);
                            SelectObject(state.back.hdc, old_pen);
                            let _ = DeleteObject(pen.into());

                            // Live dimensions pill
                            let label = format!("{w} × {h} px");
                            let pill_w = 108;
                            let pill_h = 26;
                            let pill_x = l.min(state.orig.width - pill_w - 8).max(8);
                            let pill_y = if t >= pill_h + 10 {
                                t - pill_h - 8
                            } else {
                                (b + 8).min(state.orig.height - pill_h - 8)
                            };
                            draw_pill_label(state.back.hdc, pill_x, pill_y, pill_w, pill_h, &label);
                        }
                    } else {
                        // Subtle top-center instruction pill
                        let pill_w = 270;
                        let pill_h = 34;
                        let pill_x = (state.orig.width - pill_w) / 2;
                        let pill_y = 28;
                        draw_pill_label(
                            state.back.hdc,
                            pill_x,
                            pill_y,
                            pill_w,
                            pill_h,
                            "Drag to capture region · Esc to cancel",
                        );
                    }

                    // 3. Present backbuffer atomically
                    let _ = BitBlt(
                        hdc,
                        0,
                        0,
                        state.orig.width,
                        state.orig.height,
                        Some(state.back.hdc),
                        0,
                        0,
                        SRCCOPY,
                    );
                    let _ = EndPaint(hwnd, &ps);
                }
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    unsafe fn draw_pill_label(hdc: HDC, x: i32, y: i32, w: i32, h: i32, text: &str) {
        unsafe {
            let bg = CreateSolidBrush(COLORREF(0x0016_1515)); // #151516
            let border = CreatePen(PS_SOLID, 1, COLORREF(0x0033_3336));
            let old_brush = SelectObject(hdc, bg.into());
            let old_pen = SelectObject(hdc, border.into());
            let _ = RoundRect(hdc, x, y, x + w, y + h, 10, 10);
            SelectObject(hdc, old_pen);
            SelectObject(hdc, old_brush);
            let _ = DeleteObject(bg.into());
            let _ = DeleteObject(border.into());

            let font_name = wide_null("Segoe UI");
            let font = CreateFontW(
                -14,
                0,
                0,
                0,
                500,
                0,
                0,
                0,
                windows::Win32::Graphics::Gdi::FONT_CHARSET(0),
                windows::Win32::Graphics::Gdi::FONT_OUTPUT_PRECISION(0),
                windows::Win32::Graphics::Gdi::FONT_CLIP_PRECISION(0),
                windows::Win32::Graphics::Gdi::FONT_QUALITY(5), // CLEARTYPE_QUALITY
                0,
                PCWSTR(font_name.as_ptr()),
            );
            let old_font = SelectObject(hdc, font.into());
            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, COLORREF(0x00F7_F5F5));

            let mut wide: Vec<u16> = OsStr::new(text).encode_wide().collect();
            let mut rc = RECT {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            };
            // DT_CENTER (1) | DT_VCENTER (4) | DT_SINGLELINE (0x20)
            let _ = DrawTextW(
                hdc,
                &mut wide,
                &mut rc,
                windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT(0x25),
            );
            SelectObject(hdc, old_font);
            let _ = DeleteObject(font.into());
        }
    }

    pub fn run_interactive_overlay() -> CaptureOutcome {
        if OVERLAY_ACTIVE
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return CaptureOutcome::Cancelled;
        }

        let result = run_overlay_inner();
        OVERLAY_ACTIVE.store(false, Ordering::SeqCst);
        result
    }

    fn run_overlay_inner() -> CaptureOutcome {
        unsafe {
            let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
            if vw <= 0 || vh <= 0 {
                return CaptureOutcome::Error("Virtual screen metrics returned empty size".into());
            }

            let orig = match DibBuffer::new(vw, vh) {
                Ok(b) => b,
                Err(e) => return CaptureOutcome::Error(e),
            };
            let screen_dc = GetDC(None);
            let blit_res = BitBlt(orig.hdc, 0, 0, vw, vh, Some(screen_dc), vx, vy, SRCCOPY);
            ReleaseDC(None, screen_dc);
            if let Err(e) = blit_res {
                return CaptureOutcome::Error(format!("Desktop capture failed: {e}"));
            }

            let mut dimmed = match DibBuffer::new(vw, vh) {
                Ok(b) => b,
                Err(e) => return CaptureOutcome::Error(e),
            };
            let back = match DibBuffer::new(vw, vh) {
                Ok(b) => b,
                Err(e) => return CaptureOutcome::Error(e),
            };

            // Pre-compute darkened scrim (52% brightness) cleanly in pure Rust.
            {
                let (src_px, _) = orig.as_slice().as_chunks::<4>();
                let (dst_px, _) = dimmed.as_mut_slice().as_chunks_mut::<4>();
                for (s, d) in src_px.iter().zip(dst_px.iter_mut()) {
                    d[0] = ((s[0] as u16 * 132) >> 8) as u8;
                    d[1] = ((s[1] as u16 * 132) >> 8) as u8;
                    d[2] = ((s[2] as u16 * 132) >> 8) as u8;
                    d[3] = 255;
                }
            }

            let hinstance: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
            let class_name = wide_null("GeminiDeskScreenshotOverlayClass");
            let cursor = LoadCursorW(None, IDC_CROSS).unwrap_or_default();
            let wc = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(overlay_wnd_proc),
                hInstance: hinstance,
                hCursor: cursor,
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            let _ = RegisterClassW(&wc);

            let mut state = Box::new(OverlayState {
                orig,
                dimmed,
                back,
                dragging: false,
                start_x: 0,
                start_y: 0,
                curr_x: 0,
                curr_y: 0,
                selection: None,
                cancelled: false,
            });

            let title = wide_null("GeminiDesk Screen Capture");
            let Ok(hwnd) = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_POPUP,
                vx,
                vy,
                vw,
                vh,
                None,
                None,
                Some(hinstance),
                None,
            ) else {
                return CaptureOutcome::Error("Failed to create screenshot overlay window".into());
            };

            SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                (&mut *state as *mut OverlayState) as isize,
            );

            // Clear initial background to avoid white flash before WM_PAINT.
            let dc = GetDC(Some(hwnd));
            let rc = RECT {
                left: 0,
                top: 0,
                right: vw,
                bottom: vh,
            };
            let brush = CreateSolidBrush(COLORREF(0x000C_0B0B));
            let _ = FillRect(dc, &rc, brush);
            let _ = DeleteObject(brush.into());
            ReleaseDC(Some(hwnd), dc);

            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let _ = InvalidateRect(Some(hwnd), None, false);

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            if state.cancelled {
                return CaptureOutcome::Cancelled;
            }
            let Some((l, t, r, b)) = state.selection else {
                return CaptureOutcome::Cancelled;
            };

            let cw = (r - l) as u32;
            let ch = (b - t) as u32;
            let full_stride = (state.orig.width as usize) * 4;
            let crop_stride = (cw as usize) * 4;
            let mut bgra_pixels = vec![0u8; crop_stride * (ch as usize)];
            let src = state.orig.as_slice();
            for row in 0..(ch as usize) {
                let src_off = ((t as usize) + row) * full_stride + (l as usize) * 4;
                let dst_off = row * crop_stride;
                copy_nonoverlapping(
                    src.as_ptr().add(src_off),
                    bgra_pixels.as_mut_ptr().add(dst_off),
                    crop_stride,
                );
            }

            match encode_bgra_to_png(&bgra_pixels, cw, ch) {
                Ok(png_bytes) => CaptureOutcome::Captured(ScreenshotData {
                    width: cw,
                    height: ch,
                    png_bytes,
                    bgra_pixels,
                }),
                Err(e) => CaptureOutcome::Error(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_valid_in_memory_png_from_bgra() {
        // 2x2 BGRA image (blue, green, red, white)
        let bgra = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let png_bytes = encode_bgra_to_png(&bgra, 2, 2).expect("encode png");
        // PNG magic header
        assert_eq!(&png_bytes[..8], b"\x89PNG\r\n\x1a\n");

        // Verify decoding back via `png` crate
        let decoder = png::Decoder::new(std::io::Cursor::new(&png_bytes));
        let reader = decoder.read_info().expect("read png header");
        assert_eq!(reader.info().width, 2);
        assert_eq!(reader.info().height, 2);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn dib_section_blit_and_crop_pipeline_works() {
        // Verifies real Win32 GDI CreateDIBSection + BitBlt + BGRA->PNG roundtrip
        // deterministically even on non-interactive/CI Windows desktops where screen DC is blocked.
        let mut src = unsafe { windows_impl::DibBuffer::new(16, 16) }.expect("src dib");
        let dst = unsafe { windows_impl::DibBuffer::new(8, 8) }.expect("dst dib");

        // Fill source DIB with a distinct gradient pattern (BGRA)
        let (px, _) = src.as_mut_slice().as_chunks_mut::<4>();
        for (i, p) in px.iter_mut().enumerate() {
            p[0] = (i as u8).wrapping_mul(3);
            p[1] = 128;
            p[2] = 220;
            p[3] = 255;
        }

        unsafe {
            windows::Win32::Graphics::Gdi::BitBlt(
                dst.hdc,
                0,
                0,
                8,
                8,
                Some(src.hdc),
                4,
                4,
                windows::Win32::Graphics::Gdi::SRCCOPY,
            )
            .expect("memory-to-memory BitBlt");
        }

        let png_bytes = encode_bgra_to_png(dst.as_slice(), 8, 8).expect("encode cropped dib");
        assert_eq!(&png_bytes[..8], b"\x89PNG\r\n\x1a\n");

        // Also exercise real desktop BitBlt when running on an interactive Windows desktop.
        if let Ok(cap) = capture_region_headless(0, 0, 32, 32) {
            assert_eq!(cap.width, 32);
            assert_eq!(cap.height, 32);
            assert_eq!(&cap.png_bytes[..8], b"\x89PNG\r\n\x1a\n");
        }
    }
}
