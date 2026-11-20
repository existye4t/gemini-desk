use std::ffi::OsStr;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;

use tao::platform::windows::WindowExtWindows;
use tao::window::Window;
use tracing::warn;
use windows::Win32::Foundation::{
    COLORREF, CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT,
    POINT, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR,
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, MonitorFromWindow,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_DWORD, REG_SZ, RegCloseKey,
    RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, BringWindowToTop, FindWindowW, GA_ROOTOWNER, GW_OWNER, GWL_EXSTYLE,
    GetAncestor, GetCursorPos, GetForegroundWindow, GetWindow, GetWindowLongPtrW,
    GetWindowThreadProcessId, IsChild, LWA_ALPHA, PostMessageW, SW_RESTORE, SW_SHOW,
    SetForegroundWindow, SetLayeredWindowAttributes, SetWindowLongPtrW, ShowWindow, WM_APP,
    WS_EX_LAYERED,
};
use windows::core::PCWSTR;

use super::{ActivationTarget, WorkArea};

/// Custom WM_APP messages posted by a secondary launch to wake the primary instance.
pub const WM_ACTIVATE_MAIN: u32 = WM_APP + 0x101;
pub const WM_ACTIVATE_QUICK_CHAT: u32 = WM_APP + 0x102;
pub const WM_ACTIVATE_COMPANION: u32 = WM_APP + 0x103;
pub const WM_ACTIVATE_SETTINGS: u32 = WM_APP + 0x104;

pub const MAIN_WINDOW_TITLE_BASE: &str = "GeminiDesk — Unofficial Gemini Client";

const SINGLE_INSTANCE_MUTEX: &str = "Local\\GeminiDesk_SingleInstance_v1";
const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE_NAME: &str = "GeminiDesk";
const PERSONALIZE_SUBKEY: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";

fn to_wide_null(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Win32 COLORREF uses `0x00BBGGRR`.
const fn colorref(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

pub fn hwnd_of(window: &Window) -> HWND {
    HWND(window.hwnd() as _)
}

/// Reads Windows 11 `AppsUseLightTheme` registry key (`0` = dark mode).
pub fn is_system_dark_mode() -> bool {
    unsafe {
        let subkey = to_wide_null(PERSONALIZE_SUBKEY);
        let value_name = to_wide_null("AppsUseLightTheme");
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            KEY_READ,
            &mut hkey,
        )
        .is_err()
        {
            return true;
        }

        let mut reg_type = REG_DWORD;
        let mut data: u32 = 0;
        let mut data_len = size_of::<u32>() as u32;
        let res = RegQueryValueExW(
            hkey,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&mut reg_type),
            Some(&mut data as *mut u32 as *mut u8),
            Some(&mut data_len),
        );
        let _ = RegCloseKey(hkey);

        if res.is_ok() { data == 0 } else { true }
    }
}

/// Applies Windows 11 dark/light titlebar, exact caption RGB tint, and native rounded corners.
pub fn style_window_with_caption(window: &Window, dark: bool, caption_rgb: (u8, u8, u8)) {
    let hwnd = hwnd_of(window);
    unsafe {
        let dark_val: u32 = if dark { 1 } else { 0 };
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_val as *const u32 as *const _,
            size_of::<u32>() as u32,
        );

        let caption_color = colorref(caption_rgb.0, caption_rgb.1, caption_rgb.2);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            &caption_color as *const u32 as *const _,
            size_of::<u32>() as u32,
        );

        let corner: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const DWM_WINDOW_CORNER_PREFERENCE as *const _,
            size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );
    }
}

/// Applies Windows 11 floating card styling to the borderless Quick Chat window:
/// rounded corners, refined neutral hairline border, and native DWM drop shadow.
pub fn style_quick_chat_window(window: &Window, dark: bool) {
    let hwnd = hwnd_of(window);
    unsafe {
        let dark_val: u32 = if dark { 1 } else { 0 };
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_val as *const u32 as *const _,
            size_of::<u32>() as u32,
        );

        let corner: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const DWM_WINDOW_CORNER_PREFERENCE as *const _,
            size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );

        let border_color: u32 = if dark {
            colorref(0x2F, 0x2F, 0x32)
        } else {
            colorref(0xD8, 0xD8, 0xDD)
        };
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &border_color as *const u32 as *const _,
            size_of::<u32>() as u32,
        );

        let margins = MARGINS {
            cxLeftWidth: 0,
            cxRightWidth: 0,
            cyTopHeight: 0,
            cyBottomHeight: 1,
        };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
    }
}

/// Sets per-window alpha opacity (`55..=100%`) for the Floating Companion window.
/// Removes `WS_EX_LAYERED` when `opacity_pct >= 100` to keep full opaque DWM performance.
pub fn set_window_opacity(window: &Window, opacity_pct: u8) {
    let hwnd = hwnd_of(window);
    let clamped = opacity_pct.clamp(55, 100);
    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let layered_bit = WS_EX_LAYERED.0 as isize;
        if clamped >= 100 {
            if (ex_style & layered_bit) != 0 {
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style & !layered_bit);
            }
        } else {
            if (ex_style & layered_bit) == 0 {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style | layered_bit);
            }
            let alpha = ((clamped as u16 * 255) / 100).clamp(140, 255) as u8;
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
        }
    }
}

/// Returns the physical work area (`rcWork`) of the monitor containing the mouse cursor.
pub fn active_monitor_work_area() -> Option<WorkArea> {
    unsafe {
        let mut pt = POINT::default();
        GetCursorPos(&mut pt).ok()?;
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        work_area_from_hmonitor(hmon)
    }
}

/// Returns the physical work area (`rcWork`) of the monitor containing `window`.
pub fn monitor_work_area_for_window(window: &Window) -> Option<WorkArea> {
    unsafe {
        let hmon = MonitorFromWindow(hwnd_of(window), MONITOR_DEFAULTTONEAREST);
        work_area_from_hmonitor(hmon)
    }
}

unsafe fn work_area_from_hmonitor(
    hmon: windows::Win32::Graphics::Gdi::HMONITOR,
) -> Option<WorkArea> {
    if hmon.is_invalid() {
        return None;
    }
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(hmon, &mut info).as_bool() } {
        return None;
    }
    let rc = info.rcWork;
    Some(WorkArea {
        x: rc.left,
        y: rc.top,
        width: rc.right - rc.left,
        height: rc.bottom - rc.top,
    })
}

/// Reliably restores and focuses a window on Windows 11 even when invoked from a global hotkey.
pub fn focus_window(window: &Window) {
    let hwnd = hwnd_of(window);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        if window.is_minimized() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }

        let fg = GetForegroundWindow();
        let current_thread = GetCurrentThreadId();
        let mut fg_pid = 0u32;
        let fg_thread = if !fg.0.is_null() {
            GetWindowThreadProcessId(fg, Some(&mut fg_pid))
        } else {
            0
        };

        let _ = AllowSetForegroundWindow(u32::MAX);
        if fg_thread != 0 && fg_thread != current_thread {
            let _ = windows::Win32::System::Threading::AttachThreadInput(
                fg_thread,
                current_thread,
                true,
            );
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            let _ = windows::Win32::System::Threading::AttachThreadInput(
                fg_thread,
                current_thread,
                false,
            );
        } else {
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
        }
    }
    window.set_focus();
}

/// Returns true if `window` is still the active foreground window, owns/parents the
/// foreground window (e.g. WebView2's child `Chrome_WidgetWin_1`, a popup, or a native
/// file picker dialog), or if there is no external foreground window.
pub fn is_window_still_active_or_owned(window: &Window) -> bool {
    unsafe {
        let target = hwnd_of(window);
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return true;
        }
        if fg == target || IsChild(target, fg).as_bool() {
            return true;
        }
        if GetAncestor(fg, GA_ROOTOWNER) == target {
            return true;
        }
        let my_pid = GetCurrentProcessId();
        let mut fg_pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut fg_pid));
        if fg_pid == my_pid {
            return true;
        }
        if let Ok(owner) = GetWindow(fg, GW_OWNER)
            && !owner.0.is_null()
        {
            let mut owner_pid = 0u32;
            GetWindowThreadProcessId(owner, Some(&mut owner_pid));
            if owner_pid == my_pid {
                return true;
            }
        }
        false
    }
}

/// Registers or removes GeminiDesk from `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
pub fn set_launch_at_startup(enabled: bool) -> Result<(), String> {
    unsafe {
        let subkey = to_wide_null(RUN_SUBKEY);
        let value_name = to_wide_null(RUN_VALUE_NAME);
        let mut hkey = HKEY::default();
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            KEY_SET_VALUE,
            &mut hkey,
        )
        .ok()
        .map_err(|e| format!("Failed to open Windows startup registry key: {e}"))?;

        let result = if enabled {
            let exe_path = std::env::current_exe()
                .map_err(|e| format!("Failed to resolve executable path: {e}"))?;
            let command = format!("\"{}\" --minimized", exe_path.display());
            let wide = to_wide_null(&command);
            let bytes = std::slice::from_raw_parts(
                wide.as_ptr() as *const u8,
                wide.len() * size_of::<u16>(),
            );
            RegSetValueExW(
                hkey,
                PCWSTR(value_name.as_ptr()),
                Some(0),
                REG_SZ,
                Some(bytes),
            )
            .ok()
            .map_err(|e| format!("Failed to write startup registry value: {e}"))
        } else {
            let err = RegDeleteValueW(hkey, PCWSTR(value_name.as_ptr()));
            if err.is_ok() || err == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                Err(format!("Failed to remove startup registry value: {err:?}"))
            }
        };

        let _ = RegCloseKey(hkey);
        result
    }
}

pub fn is_launch_at_startup_enabled() -> bool {
    unsafe {
        let subkey = to_wide_null(RUN_SUBKEY);
        let value_name = to_wide_null(RUN_VALUE_NAME);
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            KEY_READ,
            &mut hkey,
        )
        .is_err()
        {
            return false;
        }
        let mut reg_type = REG_SZ;
        let mut len = 0u32;
        let exists = RegQueryValueExW(
            hkey,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&mut reg_type),
            None,
            Some(&mut len),
        )
        .is_ok();
        let _ = RegCloseKey(hkey);
        exists && len > 0
    }
}

pub struct SingleInstanceHandle(HANDLE);

impl Drop for SingleInstanceHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub fn acquire_single_instance(target: ActivationTarget) -> Option<SingleInstanceHandle> {
    unsafe {
        let name = to_wide_null(SINGLE_INSTANCE_MUTEX);
        let handle = match CreateMutexW(None, false, PCWSTR(name.as_ptr())) {
            Ok(h) => h,
            Err(err) => {
                warn!(
                    ?err,
                    "CreateMutexW failed; continuing without single-instance lock"
                );
                return Some(SingleInstanceHandle(HANDLE::default()));
            }
        };

        if GetLastError() == ERROR_ALREADY_EXISTS {
            let title = to_wide_null(MAIN_WINDOW_TITLE_BASE);
            if let Ok(hwnd) = FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr()))
                && !hwnd.0.is_null()
            {
                let msg = match target {
                    ActivationTarget::MainWindow => WM_ACTIVATE_MAIN,
                    ActivationTarget::QuickChat => WM_ACTIVATE_QUICK_CHAT,
                    ActivationTarget::Companion => WM_ACTIVATE_COMPANION,
                    ActivationTarget::Settings => WM_ACTIVATE_SETTINGS,
                };
                let _ = PostMessageW(Some(hwnd), msg, WPARAM(0), LPARAM(0));
            }
            let _ = CloseHandle(handle);
            return None;
        }

        Some(SingleInstanceHandle(handle))
    }
}

const ACTIVATION_SUBCLASS_ID: usize = 0x4745_4D49;

pub fn install_activation_subclass(
    window: &Window,
    proxy: tao::event_loop::EventLoopProxy<crate::app::AppEvent>,
) {
    let hwnd = hwnd_of(window);
    let boxed = Box::into_raw(Box::new(proxy));
    unsafe {
        if !SetWindowSubclass(
            hwnd,
            Some(activation_subclass_proc),
            ACTIVATION_SUBCLASS_ID,
            boxed as usize,
        )
        .as_bool()
        {
            drop(Box::from_raw(boxed));
            warn!("Failed to install single-instance activation subclass");
        }
    }
}

unsafe extern "system" fn activation_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    uid_subclass: usize,
    ref_data: usize,
) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::WM_NCDESTROY;

    if ref_data != 0 {
        let proxy =
            unsafe { &*(ref_data as *const tao::event_loop::EventLoopProxy<crate::app::AppEvent>) };
        match msg {
            WM_ACTIVATE_MAIN => {
                let _ = proxy.send_event(crate::app::AppEvent::ShowMainWindow);
                return LRESULT(0);
            }
            WM_ACTIVATE_QUICK_CHAT => {
                let _ = proxy.send_event(crate::app::AppEvent::ToggleQuickChat);
                return LRESULT(0);
            }
            WM_ACTIVATE_COMPANION => {
                let _ = proxy.send_event(crate::app::AppEvent::ToggleCompanion);
                return LRESULT(0);
            }
            WM_ACTIVATE_SETTINGS => {
                let _ = proxy.send_event(crate::app::AppEvent::OpenSettings(None));
                return LRESULT(0);
            }
            WM_NCDESTROY => unsafe {
                let _ = RemoveWindowSubclass(hwnd, Some(activation_subclass_proc), uid_subclass);
                drop(Box::from_raw(
                    ref_data as *mut tao::event_loop::EventLoopProxy<crate::app::AppEvent>,
                ));
            },
            _ => {}
        }
    }

    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
