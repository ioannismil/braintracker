//! Grabs the text selected in whichever app is in front, by sending it Ctrl+C.
//!
//! The clipboard's previous contents (text or image) are put back afterwards, so using
//! "capture selection" doesn't disturb what you had copied.

#[cfg(windows)]
mod imp {
    use std::thread::sleep;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
        VK_C, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
    };

    const MODIFIERS: [VIRTUAL_KEY; 8] =
        [VK_LSHIFT, VK_RSHIFT, VK_LCONTROL, VK_RCONTROL, VK_LMENU, VK_RMENU, VK_LWIN, VK_RWIN];
    /// An unassigned virtual key: tapping it stops a lone Alt/Win release from opening a menu.
    const VK_DUMMY: VIRTUAL_KEY = 0xE8;

    fn held(vk: VIRTUAL_KEY) -> bool {
        unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
    }

    fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn send(inputs: &[INPUT]) {
        unsafe {
            SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
        }
    }

    enum Saved {
        Text(String),
        Image(arboard::ImageData<'static>),
        Nothing,
    }

    pub fn copy_selection() -> Option<String> {
        // The shortcut's own keys are probably still down; Ctrl+C with Shift or Alt held would
        // be a different command (Ctrl+Shift+C opens dev tools in browsers). Give the user a
        // moment to let go, then release whatever is still held.
        let deadline = Instant::now() + Duration::from_millis(600);
        while MODIFIERS.iter().any(|&vk| held(vk)) && Instant::now() < deadline {
            sleep(Duration::from_millis(10));
        }
        let still: Vec<INPUT> = MODIFIERS.iter().filter(|&&vk| held(vk)).map(|&vk| key(vk, true)).collect();
        if !still.is_empty() {
            send(&[key(VK_DUMMY, false), key(VK_DUMMY, true)]);
            send(&still);
        }

        let mut clipboard = arboard::Clipboard::new().ok()?;
        let saved = match clipboard.get_text() {
            Ok(t) => Saved::Text(t),
            Err(_) => clipboard.get_image().map(Saved::Image).unwrap_or(Saved::Nothing),
        };

        let before = unsafe { GetClipboardSequenceNumber() };
        send(&[key(VK_CONTROL, false), key(VK_C, false), key(VK_C, true), key(VK_CONTROL, true)]);

        // Nothing selected (or an app that ignores Ctrl+C) → the clipboard never changes.
        let deadline = Instant::now() + Duration::from_millis(500);
        while unsafe { GetClipboardSequenceNumber() } == before {
            if Instant::now() >= deadline {
                return None;
            }
            sleep(Duration::from_millis(10));
        }
        sleep(Duration::from_millis(20)); // let the app finish writing every format

        let text = clipboard.get_text().ok();
        let _ = match saved {
            Saved::Text(t) => clipboard.set_text(t),
            Saved::Image(i) => clipboard.set_image(i),
            Saved::Nothing => clipboard.clear(),
        };
        text.map(|t| t.trim_end().to_string()).filter(|t| !t.trim().is_empty())
    }
}

#[cfg(windows)]
pub use imp::copy_selection;

#[cfg(not(windows))]
pub fn copy_selection() -> Option<String> {
    None
}
