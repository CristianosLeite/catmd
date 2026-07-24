//! Kitty terminal graphics protocol: transmit PNG data once per image, then
//! draw and crop cell-sized placements of it. Supported by kitty, Ghostty,
//! WezTerm, and Konsole; every command carries `q=2` so terminals that do
//! understand the protocol never write replies into our input stream, and
//! terminals that do not silently ignore the APC sequences.

use base64::Engine;
use image::RgbaImage;

/// Payload bytes per APC chunk, per the protocol specification.
const CHUNK: usize = 4096;

/// Terminal cell size in pixels, for converting image pixels to cell counts.
#[derive(Clone, Copy)]
pub struct CellSize {
    pub width: u32,
    pub height: u32,
}

/// Queries the terminal for its cell size; falls back to a typical 8x16
/// when the terminal does not report pixel dimensions.
pub fn cell_size() -> CellSize {
    if let Ok(win) = termimad::crossterm::terminal::window_size()
        && win.width > 0
        && win.height > 0
        && win.columns > 0
        && win.rows > 0
    {
        return CellSize {
            width: u32::from(win.width / win.columns),
            height: u32::from(win.height / win.rows),
        };
    }
    CellSize {
        width: 8,
        height: 16,
    }
}

/// Cell rectangle an image occupies: capped at `max_cols`, aspect ratio
/// preserved, and never enlarged beyond the image's native pixel size.
pub fn layout(img_w: u32, img_h: u32, max_cols: usize, cell: CellSize) -> (u16, u16) {
    let native_cols = img_w.div_ceil(cell.width.max(1)).max(1);
    let cols = native_cols.min(max_cols as u32).max(1);
    let width_px = cols * cell.width;
    let height_px = (f64::from(img_h) * f64::from(width_px) / f64::from(img_w.max(1))).round();
    let rows = ((height_px / f64::from(cell.height.max(1))).ceil() as u32).max(1);
    (cols as u16, rows.min(u32::from(u16::MAX)) as u16)
}

/// Encodes pixels as PNG for transmission.
pub fn encode_png(img: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|err| err.to_string())?;
    Ok(png)
}

/// Vertical source-pixel slice of a transmitted image, for drawing only the
/// scrolled-into-view part.
#[derive(Clone, Copy)]
pub struct Crop {
    pub y_px: u32,
    pub h_px: u32,
}

/// Emits graphics commands, wrapping each one in a tmux passthrough
/// sequence (`DCS tmux; ... ST`, with embedded ESC bytes doubled) when the
/// session runs inside tmux — tmux with `allow-passthrough` forwards only
/// that wrapped form to the outer terminal and consumes raw APC otherwise.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Graphics {
    pub tmux: bool,
}

impl Graphics {
    fn wrap(&self, command: &str) -> String {
        if self.tmux {
            format!("\x1bPtmux;{}\x1b\\", command.replace('\x1b', "\x1b\x1b"))
        } else {
            command.to_string()
        }
    }

    /// Transmit-only command (`a=t`): stores the PNG under `id` in the
    /// terminal without drawing it. Base64 payload is chunked per the
    /// protocol; each chunk is wrapped individually for tmux.
    pub fn transmit(&self, id: u32, png: &[u8]) -> String {
        let encoded = base64::engine::general_purpose::STANDARD.encode(png);
        let chunks: Vec<&str> = encoded
            .as_bytes()
            .chunks(CHUNK)
            // Base64 output is ASCII, so chunk boundaries are char boundaries.
            .map(|c| std::str::from_utf8(c).unwrap_or_default())
            .collect();
        let mut out = String::with_capacity(encoded.len() + chunks.len() * 24);
        for (i, chunk) in chunks.iter().enumerate() {
            let more = usize::from(i + 1 < chunks.len());
            let command = if i == 0 {
                format!("\x1b_Ga=t,q=2,f=100,i={id},m={more};{chunk}\x1b\\")
            } else {
                format!("\x1b_Gm={more};{chunk}\x1b\\")
            };
            out.push_str(&self.wrap(&command));
        }
        out
    }

    /// Placement command (`a=p`): draws transmitted image `id` at the
    /// cursor, scaled into `cols` x `rows` cells, without moving the cursor
    /// (`C=1`).
    pub fn place(&self, id: u32, cols: u16, rows: u16, crop: Option<Crop>) -> String {
        let crop = crop.map_or(String::new(), |c| format!(",y={},h={}", c.y_px, c.h_px));
        self.wrap(&format!(
            "\x1b_Ga=p,q=2,i={id},c={cols},r={rows},C=1{crop}\x1b\\"
        ))
    }

    /// Removes every visible placement while keeping transmitted data, so a
    /// scrolled frame can re-place images at their new positions.
    pub fn delete_placements(&self) -> String {
        self.wrap("\x1b_Ga=d,d=a,q=2\x1b\\")
    }

    /// Frees the transmitted data for `id` on exit.
    pub fn free(&self, id: u32) -> String {
        self.wrap(&format!("\x1b_Ga=d,d=I,i={id},q=2\x1b\\"))
    }
}

#[cfg(test)]
mod tests;
