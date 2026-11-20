//! Permission-aware selected-text capture from the active foreground application.
//!
//! Capture strategy (strictly on explicit user shortcut invocation — never background):
//! 1. **Windows UI Automation (`IUIAutomation`)**:
//!    - Inspects the focused accessibility element via documented COM APIs.
//!    - Explicitly checks `CurrentIsPassword()` and REFUSES capture on password fields.
//!    - Reads `IUIAutomationTextPattern::GetSelection()` without touching the clipboard
//!      or simulating any keystrokes.
//! 2. **Optional Non-Destructive Clipboard Fallback** (when `allow_clipboard_fallback` is true
//!    and the foreground app doesn't expose UIA `TextPattern`, e.g., some Electron/Gecko views):
//!    - Still enforces `CurrentIsPassword()` refusal first.
//!    - Snapshots existing clipboard formats, sends a clean `Ctrl+C`, waits briefly for
//!      clipboard sequence change, reads `CF_UNICODETEXT`, and immediately restores the
//!      user's original clipboard snapshot.
//! 3. **Manual-Paste Fallback**:
//!    - If neither mechanism captures text, returns `SelectionOutcome::Unavailable` so the
//!      Prompt Review sheet opens cleanly for explicit manual paste.

use super::CaptureSource;

#[derive(Debug, Clone)]
pub enum SelectionOutcome {
    Captured {
        text: String,
        source: CaptureSource,
        truncated: bool,
    },
    ProtectedField,
    Unavailable,
}

/// Captures selected text from the currently focused external application.
pub fn capture_foreground_selection(
    allow_clipboard_fallback: bool,
    max_chars: usize,
) -> SelectionOutcome {
    #[cfg(target_os = "windows")]
    {
        windows_impl::capture(allow_clipboard_fallback, max_chars)
    }
    #[cfg(target_os = "linux")]
    {
        let _ = allow_clipboard_fallback;
        linux_impl::capture(max_chars)
    }
    #[cfg(target_os = "macos")]
    {
        // Avoid prompting for broad macOS Accessibility keystroke synthesis;
        // open the editable review sheet cleanly for explicit user paste.
        let _ = (allow_clipboard_fallback, max_chars);
        SelectionOutcome::Unavailable
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let _ = (allow_clipboard_fallback, max_chars);
        SelectionOutcome::Unavailable
    }
}

#[cfg(target_os = "linux")]
mod linux_impl {
    use tracing::info;

    use super::{CaptureSource, SelectionOutcome};
    use crate::productivity::clipboard::{self, ClipboardText};

    /// Reads the Wayland/X11 Primary Selection strictly on explicit user shortcut invocation.
    /// Does not monitor selections in the background or synthesize keystrokes.
    pub fn capture(max_chars: usize) -> SelectionOutcome {
        match clipboard::read_primary_selection(max_chars) {
            ClipboardText::Text { text, truncated } => {
                info!(
                    chars = text.chars().count(),
                    "Captured selected text via Linux Primary Selection"
                );
                SelectionOutcome::Captured {
                    text,
                    source: CaptureSource::UiAutomation,
                    truncated,
                }
            }
            _ => SelectionOutcome::Unavailable,
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::mem::size_of;
    use std::thread;
    use std::time::Duration;

    use tracing::{info, warn};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_C,
        VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_MENU, VK_RMENU, VK_RSHIFT, VK_SHIFT,
    };
    use windows::core::Interface;

    use super::{CaptureSource, SelectionOutcome};
    use crate::productivity::clipboard::{self, ClipboardSnapshot, ClipboardText};

    struct ComGuard(bool);
    impl ComGuard {
        fn init() -> Self {
            let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            Self(hr.is_ok())
        }
    }
    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    enum UiaProbe {
        Text(String),
        ProtectedPassword,
        NotAvailable,
    }

    fn probe_uia_selection(max_chars: usize) -> UiaProbe {
        let _com = ComGuard::init();
        unsafe {
            let Ok(uia): Result<IUIAutomation, _> =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
            else {
                return UiaProbe::NotAvailable;
            };

            let Ok(focused) = uia.GetFocusedElement() else {
                return UiaProbe::NotAvailable;
            };

            // Security requirement: NEVER capture from password / protected credential inputs.
            if let Ok(is_pwd) = focused.CurrentIsPassword()
                && is_pwd.as_bool()
            {
                return UiaProbe::ProtectedPassword;
            }

            let Ok(pattern_unk) = focused.GetCurrentPattern(UIA_TextPatternId) else {
                return UiaProbe::NotAvailable;
            };
            let Ok(text_pattern) = pattern_unk.cast::<IUIAutomationTextPattern>() else {
                return UiaProbe::NotAvailable;
            };
            let Ok(selection) = text_pattern.GetSelection() else {
                return UiaProbe::NotAvailable;
            };
            let Ok(count) = selection.Length() else {
                return UiaProbe::NotAvailable;
            };
            if count <= 0 {
                return UiaProbe::NotAvailable;
            }

            let mut combined = String::new();
            for i in 0..count {
                if let Ok(range) = selection.GetElement(i)
                    && let Ok(bstr) = range.GetText(max_chars as i32)
                {
                    let piece = bstr.to_string();
                    if !piece.is_empty() {
                        if !combined.is_empty() {
                            combined.push('\n');
                        }
                        combined.push_str(&piece);
                    }
                }
            }

            if combined.trim().is_empty() {
                UiaProbe::NotAvailable
            } else {
                UiaProbe::Text(combined)
            }
        }
    }

    fn key_input(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if key_up {
                        KEYEVENTF_KEYUP
                    } else {
                        Default::default()
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn send_clean_ctrl_c() {
        // Release Alt/Shift modifiers that triggered the global hotkey before sending Ctrl+C
        // so the foreground application receives a clean copy command and never mutates text.
        let inputs = [
            key_input(VK_MENU, true),
            key_input(VK_LMENU, true),
            key_input(VK_RMENU, true),
            key_input(VK_SHIFT, true),
            key_input(VK_LSHIFT, true),
            key_input(VK_RSHIFT, true),
            key_input(VK_CONTROL, false),
            key_input(VK_C, false),
            key_input(VK_C, true),
            key_input(VK_CONTROL, true),
            key_input(VK_LCONTROL, true),
        ];
        unsafe {
            let _ = SendInput(&inputs, size_of::<INPUT>() as i32);
        }
    }

    pub fn capture(allow_clipboard_fallback: bool, max_chars: usize) -> SelectionOutcome {
        match probe_uia_selection(max_chars) {
            UiaProbe::ProtectedPassword => {
                warn!("Refused selection capture: focused control is a protected password field");
                return SelectionOutcome::ProtectedField;
            }
            UiaProbe::Text(raw) => {
                let mut truncated = false;
                let text = if raw.chars().count() > max_chars {
                    truncated = true;
                    raw.chars().take(max_chars).collect()
                } else {
                    raw
                };
                info!(
                    chars = text.chars().count(),
                    "Captured selected text via Windows UI Automation"
                );
                return SelectionOutcome::Captured {
                    text,
                    source: CaptureSource::UiAutomation,
                    truncated,
                };
            }
            UiaProbe::NotAvailable => {}
        }

        if !allow_clipboard_fallback {
            return SelectionOutcome::Unavailable;
        }

        // Non-destructive clipboard fallback: snapshot -> Ctrl+C -> wait for seq bump -> restore.
        let snapshot = ClipboardSnapshot::capture();
        let before_seq = clipboard::sequence_number();
        send_clean_ctrl_c();

        let mut changed = false;
        for _ in 0..14 {
            thread::sleep(Duration::from_millis(15));
            if clipboard::sequence_number() != before_seq {
                changed = true;
                break;
            }
        }

        let outcome = if changed {
            match clipboard::read_unicode_text(max_chars) {
                ClipboardText::Text { text, truncated } => {
                    info!(
                        chars = text.chars().count(),
                        "Captured selected text via non-destructive clipboard fallback"
                    );
                    SelectionOutcome::Captured {
                        text,
                        source: CaptureSource::ClipboardFallback,
                        truncated,
                    }
                }
                _ => SelectionOutcome::Unavailable,
            }
        } else {
            SelectionOutcome::Unavailable
        };

        if let Some(snap) = snapshot {
            snap.restore();
        }

        outcome
    }
}
