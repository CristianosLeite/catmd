//! Loading images and deciding how to draw them. Rendering itself uses the
//! kitty graphics protocol (`kitty` submodule) on terminals that support
//! it; on any other terminal an image reference stays ordinary markdown
//! text. Protocol output is generated escape sequences and must not be
//! passed through `text::sanitize`.

pub mod kitty;

use std::path::Path;

use image::{Limits, Rgba32FImage, RgbaImage, imageops};

/// How images are drawn in the terminal.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// No graphics capability: image references render as markdown text.
    Text,
    /// Kitty graphics protocol: real pixels in terminals that support it.
    /// `tmux` wraps every command in a tmux passthrough sequence.
    Kitty { tmux: bool },
}

/// Picks the image mode for the current terminal from environment
/// heuristics (there is no reliable passive query; precedent: clipboard
/// backend selection also keys off the environment). `CATMD_IMAGES=kitty`
/// forces the protocol on — inside tmux the commands are then emitted in
/// tmux passthrough form, which needs `allow-passthrough` enabled in tmux —
/// and `CATMD_IMAGES=none` forces it off.
pub fn detect_mode() -> Mode {
    detect_mode_from(|name| std::env::var(name).ok())
}

fn detect_mode_from(env: impl Fn(&str) -> Option<String>) -> Mode {
    let get = |name: &str| env(name).unwrap_or_default().to_lowercase();
    let term = get("TERM");
    let tmux = term.starts_with("tmux") || term.starts_with("screen") || !get("TMUX").is_empty();
    match get("CATMD_IMAGES").as_str() {
        "kitty" => return Mode::Kitty { tmux },
        "none" | "off" | "text" => return Mode::Text,
        _ => {}
    }
    // Inside tmux/screen the outer terminal cannot be identified, so images
    // stay off unless the user opts in with CATMD_IMAGES=kitty above.
    if tmux {
        return Mode::Text;
    }
    let term_program = get("TERM_PROGRAM");
    if term.contains("kitty")
        || term.contains("ghostty")
        || !get("KITTY_WINDOW_ID").is_empty()
        || term_program == "wezterm"
        || term_program == "ghostty"
        || !get("KONSOLE_VERSION").is_empty()
    {
        Mode::Kitty { tmux: false }
    } else {
        Mode::Text
    }
}

/// Decoder guards against adversarial or corrupt files: dimension and
/// allocation limits far above anything worth drawing in a terminal, but
/// small enough that decoding cannot exhaust memory.
const MAX_DECODE_DIM: u32 = 16_384;
const MAX_DECODE_ALLOC: u64 = 128 * 1024 * 1024;
/// Largest encoded file worth opening; a second resource bound alongside
/// the decoder limits.
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Loaded images are downscaled to fit this box before caching; bounds the
/// memory a cached image holds while staying above any terminal size.
const MAX_WIDTH: u32 = 1600;
const MAX_HEIGHT: u32 = 3200;

/// Loads and decodes an image with explicit resource limits, downscaled to
/// the cacheable bound. Any failure (missing file, undecodable format,
/// excessive dimensions) is returned as a message so callers can fall back
/// to textual rendering.
pub fn load_image(path: &Path) -> Result<RgbaImage, String> {
    // Untrusted documents can point at FIFOs or device files, whose open or
    // read would block indefinitely. Open nonblocking (a no-op for regular
    // files), then check the *opened handle*'s metadata — checking the path
    // first would race against the file being swapped.
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
        .open(path)
        .map_err(|err| err.to_string())?;
    let meta = file.metadata().map_err(|err| err.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".to_string());
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err("image file too large".to_string());
    }
    let mut reader = image::ImageReader::new(std::io::BufReader::new(file))
        .with_guessed_format()
        .map_err(|err| err.to_string())?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODE_DIM);
    limits.max_image_height = Some(MAX_DECODE_DIM);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let img = reader.decode().map_err(|err| err.to_string())?.to_rgba8();
    Ok(fit_within(&img, MAX_WIDTH, MAX_HEIGHT))
}

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let c = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0 + 0.5) as u8
}

/// Downscales to fit within `max_w` x `max_h` (aspect preserved; images that
/// already fit are returned as-is). Resampling happens in linear light with
/// premultiplied alpha: averaging gamma-encoded sRGB values crushes fine
/// bright detail — thin light text on a dark screenshot fades to black.
/// Lanczos3 keeps edges crisp at large reductions.
fn fit_within(img: &RgbaImage, max_w: u32, max_h: u32) -> RgbaImage {
    let (w, h) = img.dimensions();
    if w <= max_w && h <= max_h {
        return img.clone();
    }
    let ratio = f64::min(
        f64::from(max_w) / f64::from(w),
        f64::from(max_h) / f64::from(h),
    );
    let nw = ((f64::from(w) * ratio).round() as u32).max(1);
    let nh = ((f64::from(h) * ratio).round() as u32).max(1);

    let mut linear = Rgba32FImage::new(w, h);
    for (src, dst) in img.pixels().zip(linear.pixels_mut()) {
        let a = f32::from(src.0[3]) / 255.0;
        dst.0 = [
            srgb_to_linear(src.0[0]) * a,
            srgb_to_linear(src.0[1]) * a,
            srgb_to_linear(src.0[2]) * a,
            a,
        ];
    }
    let resized = imageops::resize(&linear, nw, nh, imageops::FilterType::Lanczos3);
    let mut out = RgbaImage::new(nw, nh);
    for (src, dst) in resized.pixels().zip(out.pixels_mut()) {
        let a = src.0[3].clamp(0.0, 1.0);
        let unmul = if a > 0.0 { 1.0 / a } else { 0.0 };
        dst.0 = [
            linear_to_srgb(src.0[0] * unmul),
            linear_to_srgb(src.0[1] * unmul),
            linear_to_srgb(src.0[2] * unmul),
            (a * 255.0 + 0.5) as u8,
        ];
    }
    out
}

#[cfg(test)]
mod tests;
