//! macOS clipboard access via NSPasteboard (PNG first, TIFF fallback),
//! plus optional downscaling for agent-friendly image sizes.

use std::sync::Mutex;

use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeTIFF};

/// Read the current clipboard image as PNG bytes, if any.
pub fn read_clipboard_png() -> Option<Vec<u8>> {
    let pb = NSPasteboard::generalPasteboard();

    // Direct PNG data (screenshots, most apps).
    if let Some(data) = pb.dataForType(unsafe { NSPasteboardTypePNG }) {
        let v = data.to_vec();
        if v.starts_with(&[0x89, b'P', b'N', b'G']) {
            return Some(v);
        }
    }

    // TIFF fallback (e.g. images copied from some apps) → re-encode as PNG.
    if let Some(data) = pb.dataForType(unsafe { NSPasteboardTypeTIFF }) {
        let tiff = data.to_vec();
        if let Ok(img) = image::load_from_memory(&tiff) {
            let mut png = Vec::new();
            if img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).is_ok() {
                return Some(png);
            }
        }
    }
    None
}

/// NSPasteboard change counter; used to cache the processed image per clipboard state.
pub fn change_count() -> i64 {
    let pb = NSPasteboard::generalPasteboard();
    pb.changeCount() as i64
}

/// Downscale so that width <= max_width (keeps aspect). Returns original on any failure.
pub fn downscale_png(png: Vec<u8>, max_width: u32) -> Vec<u8> {
    let result = (|| -> Option<Vec<u8>> {
        let img = image::load_from_memory(&png).ok()?;
        if img.width() <= max_width {
            return None; // nothing to do
        }
        let scaled = img.resize(max_width, u32::MAX, image::imageops::FilterType::Triangle);
        let mut out = Vec::new();
        scaled
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .ok()?;
        Some(out)
    })();
    match result {
        Some(v) if v.len() < png.len() => v,
        _ => png,
    }
}

/// Cache: (change_count, downscale_enabled, max_width) -> processed bytes.
static CACHE: Mutex<Option<(i64, bool, u32, Vec<u8>)>> = Mutex::new(None);

/// Read clipboard and apply downscale config, cached per clipboard state.
pub fn clipboard_png_processed(downscale: bool, max_width: u32) -> Option<Vec<u8>> {
    let cc = change_count();
    if let Some((c, d, w, ref bytes)) = *CACHE.lock().unwrap() {
        if c == cc && d == downscale && w == max_width {
            return Some(bytes.clone());
        }
    }
    let raw = read_clipboard_png()?;
    let processed = if downscale {
        downscale_png(raw, max_width)
    } else {
        raw
    };
    *CACHE.lock().unwrap() = Some((cc, downscale, max_width, processed.clone()));
    Some(processed)
}
