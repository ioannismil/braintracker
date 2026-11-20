//! Grabs the text selected in whichever app is in front, by sending it Ctrl+C.
//!
//! Whatever was on the clipboard before is put back afterwards, in every format it had:
//! files copied in Explorer (including "cut"), formatted text from Word or a browser (HTML,
//! RTF), images, plain text. So using "capture selection" doesn't disturb what you'd copied.

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

        let saved = super::snapshot::take();

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

        let text = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok());
        if let Some(saved) = saved {
            super::snapshot::restore(&saved);
        }
        text.map(|t| t.trim_end().to_string()).filter(|t| !t.trim().is_empty())
    }
}

/// A copy of everything on the clipboard, in every format that can be put back.
#[cfg(windows)]
pub(crate) mod snapshot {
    use std::thread::sleep;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{GlobalFree, HANDLE};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardFormatNameW,
        OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};

    /// Skip anything bigger than this altogether (e.g. a huge range copied in Excel).
    const MAX_TOTAL: usize = 64 << 20;

    // Standard formats whose data is a plain block of memory, so it can be copied and put back:
    // SYLK, DIF, TIFF, DIB, pen data, RIFF, WAVE, Unicode text, files (HDROP), locale, DIBV5.
    // Left out: GDI handles (bitmaps, metafiles, palettes), which Windows re-creates from the
    // DIB/text versions anyway, and owner-drawn/private formats that only their app understands.
    const MEMORY_FORMATS: [u32; 11] = [4, 5, 6, 8, 10, 11, 12, 13, 15, 16, 17];
    const CF_TEXT: u32 = 1;
    const CF_OEMTEXT: u32 = 7;
    const CF_UNICODETEXT: u32 = 13;
    /// Registered formats that are live OLE objects (pointers into the app that copied),
    /// which can't be stored and put back.
    const OLE_FORMATS: [&str; 10] = [
        "DataObject",
        "Ole Private Data",
        "Embed Source",
        "Embedded Object",
        "Link Source",
        "Link Source Descriptor",
        "Object Descriptor",
        "Native",
        "OwnerLink",
        "ObjectLink",
    ];

    pub struct Snapshot(pub(crate) Vec<(u32, Vec<u8>)>);

    /// The clipboard, opened (another app may hold it for a moment, so retry briefly).
    struct Open;

    impl Open {
        fn new() -> Option<Self> {
            for _ in 0..25 {
                if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                    return Some(Open);
                }
                sleep(Duration::from_millis(10));
            }
            None
        }
    }

    impl Drop for Open {
        fn drop(&mut self) {
            unsafe { CloseClipboard() };
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn format_name(format: u32) -> Option<String> {
        let mut buf = [0u16; 256];
        let n = unsafe { GetClipboardFormatNameW(format, buf.as_mut_ptr(), buf.len() as i32) };
        (n > 0).then(|| String::from_utf16_lossy(&buf[..n as usize]))
    }

    fn can_keep(format: u32, has_unicode: bool) -> bool {
        match format {
            // Windows makes these from the Unicode text; storing both risks a code-page mismatch.
            CF_TEXT | CF_OEMTEXT => !has_unicode,
            f if MEMORY_FORMATS.contains(&f) => true,
            // Registered formats (HTML Format, Rich Text Format, the shell's file formats…).
            f if f >= 0xC000 => format_name(f).is_some_and(|n| !OLE_FORMATS.contains(&n.as_str())),
            _ => false,
        }
    }

    /// Copies everything on the clipboard. None if the clipboard couldn't be opened.
    pub fn take() -> Option<Snapshot> {
        let _open = Open::new()?;
        let mut formats = Vec::new();
        let mut f = 0;
        loop {
            f = unsafe { EnumClipboardFormats(f) };
            if f == 0 {
                break;
            }
            formats.push(f);
        }
        let has_unicode = formats.contains(&CF_UNICODETEXT);
        let mut out = Vec::new();
        let mut total = 0usize;
        for format in formats {
            if !can_keep(format, has_unicode) {
                continue;
            }
            unsafe {
                let h = GetClipboardData(format);
                if h.is_null() {
                    continue;
                }
                let size = GlobalSize(h);
                if size == 0 || total + size > MAX_TOTAL {
                    continue;
                }
                let p = GlobalLock(h);
                if p.is_null() {
                    continue;
                }
                let bytes = std::slice::from_raw_parts(p as *const u8, size).to_vec();
                GlobalUnlock(h);
                total += size;
                out.push((format, bytes));
            }
        }
        Some(Snapshot(out))
    }

    fn put(format: u32, bytes: &[u8]) {
        unsafe {
            let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1));
            if h.is_null() {
                return;
            }
            let p = GlobalLock(h);
            if p.is_null() {
                GlobalFree(h);
                return;
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
            GlobalUnlock(h);
            // On success the clipboard owns the memory; otherwise it's ours to free.
            if SetClipboardData(format, h as HANDLE).is_null() {
                GlobalFree(h);
            }
        }
    }

    /// Puts a snapshot back. It's marked so Windows' clipboard history (Win+V) and cloud
    /// clipboard don't record it a second time: it's already there from when you copied it.
    pub fn restore(s: &Snapshot) -> bool {
        let Some(_open) = Open::new() else { return false };
        unsafe { EmptyClipboard() };
        for (format, bytes) in &s.0 {
            put(*format, bytes);
        }
        if !s.0.is_empty() {
            for name in ["CanIncludeInClipboardHistory", "CanUploadToCloudClipboard"] {
                let f = unsafe { RegisterClipboardFormatW(wide(name).as_ptr()) };
                if f != 0 {
                    put(f, &0u32.to_le_bytes());
                }
            }
        }
        true
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn register(name: &str) -> u32 {
            unsafe { RegisterClipboardFormatW(wide(name).as_ptr()) }
        }

        /// Saves whatever is on the clipboard, empties it, and puts it back, for checking
        /// real-world content by hand: `cargo test -- --ignored restores_the_current_clipboard`
        #[test]
        #[ignore]
        fn restores_the_current_clipboard() {
            let s = take().expect("clipboard opens");
            {
                let _open = Open::new().unwrap();
                unsafe { EmptyClipboard() };
            }
            assert!(restore(&s));
            println!("restored {} formats", s.0.len());
        }

        /// Uses the real clipboard (your current clipboard is saved and put back around it),
        /// so it's opt-in: `cargo test -- --ignored clipboard`
        #[test]
        #[ignore]
        fn clipboard_round_trips_every_format() {
            let yours = take().expect("clipboard opens");

            let html = register("HTML Format");
            let custom = register("BrainTracker test format");
            let text: Vec<u8> = "héllo"
                .encode_utf16()
                .chain(std::iter::once(0))
                .flat_map(|c| c.to_le_bytes())
                .collect();
            let original = Snapshot(vec![
                (CF_UNICODETEXT, text.clone()),
                (html, b"Version:0.9\r\n<b>bold</b>\0".to_vec()),
                (custom, vec![1, 2, 3, 4]),
            ]);
            assert!(restore(&original));
            let copy = take().expect("clipboard opens");
            assert!(restore(&yours), "your clipboard is put back");

            let find = |f: u32| copy.0.iter().find(|(g, _)| *g == f).map(|(_, b)| b.clone());
            // Windows may round sizes up, so compare the start of each block.
            assert!(find(CF_UNICODETEXT).unwrap().starts_with(&text));
            assert!(find(html).unwrap().starts_with(b"Version:0.9\r\n<b>bold</b>"));
            assert!(find(custom).unwrap().starts_with(&[1, 2, 3, 4]));
            assert!(find(CF_TEXT).is_none(), "the ANSI copy Windows makes isn't stored again");
        }
    }
}

#[cfg(windows)]
pub use imp::copy_selection;

#[cfg(not(windows))]
pub fn copy_selection() -> Option<String> {
    None
}
