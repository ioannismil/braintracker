//! Reads the text in screenshots with Windows' built-in OCR (offline, nothing leaves the PC),
//! so searching for an error code finds the note with the screenshot of that error.
//!
//! One background thread works through a queue: each screenshot as it's pasted or imported,
//! plus — at startup and after restoring a backup — any image not read yet. The text is kept
//! in the `image_text` table and indexed with the entries that show the image (see db.rs).

use crate::{db, db::Db, images_dir, safe_image_name};
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

enum Job {
    /// A screenshot that was just saved.
    Image(String),
    /// Every image in the folder that hasn't been read yet.
    Scan,
}

pub struct OcrState(Mutex<Sender<Job>>);

fn send(app: &AppHandle, job: Job) {
    if let Some(state) = app.try_state::<OcrState>() {
        if let Ok(tx) = state.0.lock() {
            let _ = tx.send(job);
        }
    }
}

/// Reads this screenshot's text soon.
pub fn queue(app: &AppHandle, name: &str) {
    send(app, Job::Image(name.to_string()));
}

/// Reads every screenshot that hasn't been read yet.
pub fn scan(app: &AppHandle) {
    send(app, Job::Scan);
}

/// Starts the reader thread and catches up on images that were never read (e.g. pasted
/// before this feature existed).
pub fn start(app: &AppHandle) {
    let (tx, rx) = channel::<Job>();
    app.manage(OcrState(Mutex::new(tx)));
    scan(app);
    let app = app.clone();
    std::thread::spawn(move || {
        // Without an OCR language installed in Windows there's nothing to do. Images stay
        // unread, so they're picked up on a later start once a language is added.
        let Some(reader) = imp::Reader::new() else { return };
        for job in rx {
            let names = match job {
                Job::Image(name) => vec![name],
                Job::Scan => unread_images(&app),
            };
            for name in names {
                read_one(&app, &reader, &name);
            }
        }
    });
}

fn unread_images(app: &AppHandle) -> Vec<String> {
    let Ok(dir) = images_dir(app) else { return Vec::new() };
    let known = match app.state::<Db>().0.lock() {
        Ok(conn) => db::read_images(&conn).unwrap_or_default(),
        Err(_) => return Vec::new(),
    };
    let Ok(files) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = files
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| safe_image_name(n) && !known.contains(n))
        .collect();
    names.sort_unstable_by(|a, b| b.cmp(a)); // newest first (names start with a timestamp)
    names
}

fn read_one(app: &AppHandle, reader: &imp::Reader, name: &str) {
    if !safe_image_name(name) {
        return;
    }
    let Ok(dir) = images_dir(app) else { return };
    let path = dir.join(name);
    if !path.is_file() {
        return;
    }
    // A file Windows can't decode is recorded with no text, so it isn't retried every start.
    let text = reader.read(&path).unwrap_or_default();
    if let Ok(conn) = app.state::<Db>().0.lock() {
        let _ = db::set_image_text(&conn, name, &text);
    }
}

#[cfg(windows)]
mod imp {
    use std::path::Path;
    use windows::core::HSTRING;
    use windows::Graphics::Imaging::{
        BitmapAlphaMode, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat, BitmapTransform,
        ColorManagementMode, ExifOrientationMode,
    };
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::{FileAccessMode, StorageFile};
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    pub struct Reader {
        engine: OcrEngine,
        max_dim: u32,
    }

    impl Reader {
        /// None if Windows has no OCR language for the user's languages.
        pub fn new() -> Option<Self> {
            // This thread only ever talks to WinRT; an "already initialised" error is fine.
            let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
            let engine = OcrEngine::TryCreateFromUserProfileLanguages().ok()?;
            let max_dim = OcrEngine::MaxImageDimension().unwrap_or(2600).max(1);
            Some(Self { engine, max_dim })
        }

        /// The image's text, one recognised line per line.
        pub fn read(&self, path: &Path) -> windows::core::Result<String> {
            // StorageFile only takes backslashes.
            let path = HSTRING::from(path.to_string_lossy().replace('/', "\\"));
            let file = StorageFile::GetFileFromPathAsync(&path)?.join()?;
            let stream = file.OpenAsync(FileAccessMode::Read)?.join()?;
            let decoder = BitmapDecoder::CreateAsync(&stream)?.join()?;
            let (w, h) = (decoder.PixelWidth()?, decoder.PixelHeight()?);
            if w == 0 || h == 0 {
                return Ok(String::new());
            }

            // Only shrink images over the engine's size limit: enlarging screenshots was
            // tested and makes the engine miss words (e.g. error codes) rather than find more.
            let scale = f64::min(1.0, self.max_dim as f64 / w.max(h) as f64);
            let transform = BitmapTransform::new()?;
            if scale < 1.0 {
                transform.SetScaledWidth(((w as f64 * scale).round() as u32).clamp(1, self.max_dim))?;
                transform.SetScaledHeight(((h as f64 * scale).round() as u32).clamp(1, self.max_dim))?;
                transform.SetInterpolationMode(BitmapInterpolationMode::Fant)?;
            }
            let bitmap = decoder
                .GetSoftwareBitmapTransformedAsync(
                    BitmapPixelFormat::Bgra8,
                    BitmapAlphaMode::Premultiplied,
                    &transform,
                    ExifOrientationMode::RespectExifOrientation,
                    ColorManagementMode::DoNotColorManage,
                )?
                .join()?;

            let result = self.engine.RecognizeAsync(&bitmap)?.join()?;
            let lines = result.Lines()?;
            let mut out = Vec::new();
            for i in 0..lines.Size()? {
                let line = lines.GetAt(i)?.Text()?.to_string_lossy();
                if !line.trim().is_empty() {
                    out.push(super::fix_misreads(&line));
                }
            }
            Ok(out.join("\n"))
        }
    }
}

/// Corrects OCR misreads that would break searching for error codes: the engine often
/// reads the zero in a hex code as a letter O ("Ox80070005").
fn fix_misreads(line: &str) -> String {
    line.split(' ')
        .map(|w| {
            let hex = |s: &str| s.len() >= 2 && s.chars().all(|c| c.is_ascii_hexdigit());
            match w.strip_prefix("Ox").or_else(|| w.strip_prefix("ox")) {
                Some(rest) if hex(rest.trim_end_matches(|c: char| !c.is_ascii_alphanumeric())) => format!("0x{rest}"),
                _ => w.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(not(windows))]
mod imp {
    use std::path::Path;

    pub struct Reader;

    impl Reader {
        pub fn new() -> Option<Self> {
            None
        }

        pub fn read(&self, _path: &Path) -> Result<String, ()> {
            Ok(String::new())
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hex_codes_read_as_letter_o_are_fixed() {
        assert_eq!(super::fix_misreads("error Ox80070005: denied"), "error 0x80070005: denied");
        assert_eq!(super::fix_misreads("code ox1F."), "code 0x1F.");
        assert_eq!(super::fix_misreads("Oxford Oxygen ox"), "Oxford Oxygen ox"); // real words stay
    }

    #[cfg(windows)]
    /// Needs an OCR language in Windows and an image with text, so it's opt-in:
    /// `BT_OCR_SAMPLE=<png> BT_OCR_EXPECT=<word> cargo test -- --ignored ocr`
    #[test]
    #[ignore]
    fn reads_text_from_a_screenshot() {
        let path = std::env::var("BT_OCR_SAMPLE").expect("BT_OCR_SAMPLE");
        let expect = std::env::var("BT_OCR_EXPECT").expect("BT_OCR_EXPECT");
        let reader = super::imp::Reader::new().expect("an OCR language is installed");
        let text = reader.read(std::path::Path::new(&path)).unwrap();
        println!("read: {text:?}");
        assert!(text.contains(&expect), "{text:?} should contain {expect:?}");
    }
}
