use screenshots::Screen;
use std::path::Path;

pub struct ScreenshotResult {
    pub width: u32,
    pub height: u32,
    pub png_bytes: Vec<u8>,
}

pub fn capture_primary_screen() -> Result<ScreenshotResult, String> {
    let screens = Screen::all().map_err(|e| format!("Failed to enumerate screens: {e}"))?;
    let primary = screens.into_iter().next()
        .ok_or_else(|| "No screens found".to_string())?;
    let captured  = primary.capture()
        .map_err(|e| format!("Capture failed: {e}"))?;
    let width     = captured.width();
    let height    = captured.height();
    // screenshots 0.8 re-exports image 0.24 types; extract raw RGBA and re-encode with image 0.25.
    let raw_rgba  = captured.into_raw();
    let img_buf   = image::RgbaImage::from_raw(width, height, raw_rgba)
        .ok_or_else(|| "Buffer size mismatch during PNG encoding".to_string())?;
    let mut png_bytes = Vec::new();
    image::DynamicImage::ImageRgba8(img_buf)
        .write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .map_err(|e| format!("PNG encode failed: {e}"))?;
    Ok(ScreenshotResult { width, height, png_bytes })
}

pub fn save_to_file(result: &ScreenshotResult, path: &str) -> Result<String, String> {
    let p = Path::new(path);
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Could not create output directory: {e}"))?;
        }
    }
    std::fs::write(path, &result.png_bytes)
        .map_err(|e| format!("Failed to write screenshot: {e}"))?;
    Ok(format!("Saved screenshot to {} ({}×{})", path, result.width, result.height))
}

pub fn copy_to_clipboard(png_bytes: &[u8]) -> Result<(), String> {
    use arboard::{Clipboard, ImageData};
    use std::borrow::Cow;

    // Decode PNG → raw RGBA (arboard needs raw pixels, not PNG bytes)
    let img = image::load_from_memory(png_bytes)
        .map_err(|e| format!("Could not decode screenshot for clipboard: {e}"))?;
    let rgba    = img.to_rgba8();
    let (w, h)  = rgba.dimensions();

    let mut clipboard = Clipboard::new()
        .map_err(|e| format!("Could not access clipboard: {e}"))?;
    clipboard.set_image(ImageData {
        width:  w as usize,
        height: h as usize,
        bytes:  Cow::Owned(rgba.into_raw()),
    }).map_err(|e| format!("Could not write to clipboard: {e}"))?;

    Ok(())
}
