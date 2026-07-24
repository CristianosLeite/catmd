//! Styling Markdown for the terminal: termimad for prose, syntect for
//! syntax-highlighted code blocks.
//!
//! All document-derived text is passed through `text::sanitize` before any
//! styling is added, so documents cannot inject terminal escape sequences.
//! Layout is computed in terminal display columns (`text::display_width`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::as_24_bit_terminal_escaped;
use termimad::MadSkin;
use termimad::crossterm::style::Color;

use crate::image::{Mode, kitty, load_image};
use crate::segment::{Segment, split_segments};
use crate::text::{display_width, sanitize, truncate_display, truncate_styled};

const CODE_INDENT: &str = "    ";
const BUTTON_LABEL: &str = "[ copy ]";
const DIM: &str = "\x1b[38;5;240m";
const BUTTON_STYLE: &str = "\x1b[1m\x1b[38;5;114m";
const RESET: &str = "\x1b[0m";
/// Cap for the fence language shown in a block header, in display columns.
const MAX_LANG_COLS: usize = 24;

/// A clickable [ copy ] region in a rendered document. `line` is a view line
/// index; `col_start`/`col_end` are terminal display columns.
pub struct CopyButton {
    pub line: usize,
    pub col_start: u16,
    pub col_end: u16,
    pub lang: String,
    pub body: String,
}

/// A kitty-protocol image placed in a rendered document. `line` is the view
/// line index of its first placeholder row; the image body spans `rows`
/// placeholder lines of `cols` cells. The viewer transmits `png` once and
/// re-places (with vertical cropping) as the document scrolls.
pub struct ImageSpan {
    pub line: usize,
    pub cols: u16,
    pub rows: u16,
    /// Pixel height of the transmitted image, for crop calculations.
    pub img_h: u32,
    pub id: u32,
    pub png: Rc<Vec<u8>>,
}

pub struct RenderedDoc {
    pub lines: Vec<String>,
    pub buttons: Vec<CopyButton>,
    pub images: Vec<ImageSpan>,
}

/// A ready-to-transmit kitty image: PNG bytes, pixel dimensions, and its
/// terminal-side id.
struct KittyPayload {
    png: Rc<Vec<u8>>,
    width: u32,
    height: u32,
    id: u32,
}

/// Aggregate bounds for the image cache: per-image limits alone would let a
/// document with many distinct images exhaust memory during the eager view
/// build. Once either budget is hit, further images fall back to text (the
/// failure is cached, so they are not retried on every rebuild).
const CACHE_MAX_TOTAL_BYTES: usize = 96 * 1024 * 1024;
const CACHE_MAX_ENTRIES: usize = 256;

pub struct Renderer {
    skin: MadSkin,
    syntaxes: SyntaxSet,
    theme: Theme,
    mode: Mode,
    /// Terminal cell size in pixels; refreshed on resize (font or monitor
    /// changes alter it along with the column count).
    cell: Cell<kitty::CellSize>,
    next_image_id: Cell<u32>,
    /// Encoded images by canonicalized path, kept for the renderer's
    /// lifetime so viewer rebuilds (every terminal resize) do not reopen and
    /// re-decode files. Load failures are cached too. Entries are already
    /// downscaled to the cache bound in `image::load_image`.
    image_cache: RefCell<HashMap<PathBuf, Result<KittyPayload, String>>>,
    cache_bytes: Cell<usize>,
    cache_entries: Cell<usize>,
    cache_budget: (usize, usize),
}

impl Renderer {
    /// Test-only default: no image graphics, a typical cell size.
    /// Production code selects a mode via `with_images`.
    #[cfg(test)]
    pub fn new() -> Self {
        Self::with_images(
            Mode::Text,
            kitty::CellSize {
                width: 8,
                height: 16,
            },
        )
    }

    /// A renderer drawing images in `mode`; `cell` is the terminal's cell
    /// size in pixels (used by the kitty mode to lay images out in cells).
    pub fn with_images(mode: Mode, cell: kitty::CellSize) -> Self {
        let mut skin = MadSkin::default();
        skin.set_headers_fg(Color::AnsiValue(214));
        skin.headers[0].set_fg(Color::AnsiValue(208));
        skin.bold.set_fg(Color::AnsiValue(231));
        skin.italic.set_fg(Color::AnsiValue(153));
        skin.inline_code.set_fg(Color::AnsiValue(114));
        skin.inline_code.set_bg(Color::AnsiValue(236));
        skin.code_block.set_fg(Color::AnsiValue(114));
        skin.code_block.set_bg(Color::AnsiValue(236));

        let mut themes = ThemeSet::load_defaults();
        Self {
            skin,
            syntaxes: SyntaxSet::load_defaults_newlines(),
            theme: themes
                .themes
                .remove("base16-ocean.dark")
                .expect("default theme"),
            mode,
            cell: Cell::new(cell),
            // Namespaced by process id: kitty image ids are global to the
            // terminal, and colliding with another program's ids would
            // overwrite its images.
            next_image_id: Cell::new((std::process::id() & 0x3FFF) << 16 | 1),
            image_cache: RefCell::new(HashMap::new()),
            cache_bytes: Cell::new(0),
            cache_entries: Cell::new(0),
            cache_budget: (CACHE_MAX_TOTAL_BYTES, CACHE_MAX_ENTRIES),
        }
    }

    /// Updates the terminal cell metrics; the viewer calls this on resize
    /// so image layout tracks font/monitor changes, not just column count.
    pub fn set_cell(&self, cell: kitty::CellSize) {
        self.cell.set(cell);
    }

    /// The graphics command emitter matching the active mode.
    pub fn graphics(&self) -> kitty::Graphics {
        match self.mode {
            Mode::Kitty { tmux } => kitty::Graphics { tmux },
            Mode::Text => kitty::Graphics { tmux: false },
        }
    }

    #[cfg(test)]
    fn set_cache_budget(&mut self, bytes: usize, entries: usize) {
        self.cache_budget = (bytes, entries);
    }

    /// Loads an image and encodes it for kitty transmission, charging the
    /// aggregate cache budget.
    fn load(&self, path: &Path) -> Result<KittyPayload, String> {
        let (max_bytes, max_entries) = self.cache_budget;
        if self.cache_entries.get() >= max_entries {
            return Err("image cache entry budget exhausted".to_string());
        }
        let img = load_image(path)?;
        let png = kitty::encode_png(&img)?;
        if self.cache_bytes.get() + png.len() > max_bytes {
            return Err("image cache byte budget exhausted".to_string());
        }
        self.cache_bytes.set(self.cache_bytes.get() + png.len());
        self.cache_entries.set(self.cache_entries.get() + 1);
        let id = self.next_image_id.get();
        self.next_image_id.set(id + 1);
        Ok(KittyPayload {
            png: Rc::new(png),
            width: img.width(),
            height: img.height(),
            id,
        })
    }

    fn syntax_for(&self, lang: &str) -> &SyntaxReference {
        self.syntaxes
            .find_syntax_by_token(lang)
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text())
    }

    /// Highlights one sanitized line. Callers must sanitize first: syntect
    /// styles text but does not strip control characters, and the error
    /// fallback returns the input verbatim.
    fn highlight_line(&self, highlighter: &mut HighlightLines, line: &str) -> String {
        match highlighter.highlight_line(line, &self.syntaxes) {
            Ok(ranges) => as_24_bit_terminal_escaped(&ranges, false),
            Err(_) => line.to_string(),
        }
    }

    /// Display name for a fence language: sanitized, width-capped, never empty.
    fn lang_name(lang: &str) -> String {
        let clean = truncate_display(&sanitize(lang), MAX_LANG_COLS);
        if clean.is_empty() {
            "code".to_string()
        } else {
            clean
        }
    }

    fn resolve(dest: &str, base_dir: Option<&Path>) -> PathBuf {
        let path = base_dir.map_or_else(|| PathBuf::from(dest), |dir| dir.join(dest));
        // Canonicalize so aliases (`img.png`, `./img.png`, symlinks) share
        // one cache entry; a missing file keeps the raw path and fails in
        // `load_image` with a proper message.
        path.canonicalize().unwrap_or(path)
    }

    fn caption(&self, alt: &str, width: usize) -> String {
        let caption = truncate_display(&sanitize(alt), width);
        format!("{DIM}{caption}{RESET}")
    }

    /// Cached kitty payload for an image destination.
    fn kitty_image(&self, dest: &str, base_dir: Option<&Path>) -> Result<KittyPayload, String> {
        let path = Self::resolve(dest, base_dir);
        let mut cache = self.image_cache.borrow_mut();
        match cache.entry(path).or_insert_with_key(|path| self.load(path)) {
            Ok(payload) => Ok(KittyPayload {
                png: Rc::clone(&payload.png),
                width: payload.width,
                height: payload.height,
                id: payload.id,
            }),
            Err(err) => Err(err.clone()),
        }
    }

    /// Plain cat-like rendering (non-interactive mode). Write errors are
    /// returned, not panicked on: SIGPIPE stays ignored (Rust's default) so
    /// a vanished reader surfaces here as `BrokenPipe` for the caller.
    pub fn render_plain(
        &self,
        out: &mut impl Write,
        src: &str,
        width: usize,
        base_dir: Option<&Path>,
    ) -> io::Result<()> {
        for segment in split_segments(src) {
            match segment {
                Segment::Markdown(md) => {
                    write!(out, "{}", self.skin.text(&sanitize(&md), Some(width)))?;
                }
                Segment::Image { alt, dest, raw } if matches!(self.mode, Mode::Kitty { .. }) => {
                    match self.kitty_image(&dest, base_dir) {
                        Ok(image) => {
                            let gfx = self.graphics();
                            let (cols, rows) =
                                kitty::layout(image.width, image.height, width, self.cell.get());
                            writeln!(out)?;
                            // Placement leaves the cursor put (C=1); step
                            // over the image's rows explicitly.
                            write!(out, "{}", gfx.transmit(image.id, &image.png))?;
                            write!(out, "{}", gfx.place(image.id, cols, rows, None))?;
                            for _ in 0..rows {
                                writeln!(out)?;
                            }
                            if !alt.is_empty() {
                                writeln!(out, "{}", self.caption(&alt, width))?;
                            }
                            writeln!(out)?;
                        }
                        Err(_) => {
                            write!(out, "{}", self.skin.text(&sanitize(&raw), Some(width)))?;
                        }
                    }
                }
                // No graphics capability (or an unloadable image, above):
                // the line renders as ordinary markdown, exactly as before
                // image support.
                Segment::Image { raw, .. } => {
                    write!(out, "{}", self.skin.text(&sanitize(&raw), Some(width)))?;
                }
                Segment::Code { lang, body } => {
                    let mut highlighter = HighlightLines::new(self.syntax_for(lang), &self.theme);
                    writeln!(out)?;
                    for line in body.lines() {
                        let clean = sanitize(line);
                        writeln!(
                            out,
                            "{CODE_INDENT}{}{RESET}",
                            self.highlight_line(&mut highlighter, &clean)
                        )?;
                    }
                    writeln!(out)?;
                }
            }
        }
        Ok(())
    }

    /// Renders to addressable screen lines with [ copy ] button positions,
    /// for the interactive viewer. `first_block` is the number of code blocks
    /// already rendered (buttons keep numbering across multiple files).
    pub fn render_doc(
        &self,
        src: &str,
        width: usize,
        first_block: usize,
        base_dir: Option<&Path>,
    ) -> RenderedDoc {
        let mut lines: Vec<String> = Vec::new();
        let mut buttons: Vec<CopyButton> = Vec::new();
        let mut images: Vec<ImageSpan> = Vec::new();
        for segment in split_segments(src) {
            match segment {
                Segment::Markdown(md) => {
                    let text = format!("{}", self.skin.text(&sanitize(&md), Some(width)));
                    lines.extend(text.lines().map(str::to_string));
                }
                Segment::Image { alt, dest, raw } if matches!(self.mode, Mode::Kitty { .. }) => {
                    match self.kitty_image(&dest, base_dir) {
                        Ok(image) => {
                            let (cols, rows) =
                                kitty::layout(image.width, image.height, width, self.cell.get());
                            lines.push(String::new());
                            let first = lines.len();
                            // Empty placeholder rows reserve scroll space;
                            // the viewer draws the image over them.
                            lines.extend(std::iter::repeat_with(String::new).take(rows as usize));
                            if !alt.is_empty() {
                                lines.push(self.caption(&alt, width));
                            }
                            lines.push(String::new());
                            images.push(ImageSpan {
                                line: first,
                                cols,
                                rows,
                                img_h: image.height,
                                id: image.id,
                                png: image.png,
                            });
                        }
                        Err(_) => {
                            let text = format!("{}", self.skin.text(&sanitize(&raw), Some(width)));
                            lines.extend(text.lines().map(str::to_string));
                        }
                    }
                }
                // No graphics capability (or an unloadable image, above):
                // the line renders as ordinary markdown, exactly as before
                // image support.
                Segment::Image { raw, .. } => {
                    let text = format!("{}", self.skin.text(&sanitize(&raw), Some(width)));
                    lines.extend(text.lines().map(str::to_string));
                }
                Segment::Code { lang, body } => {
                    let number = first_block + buttons.len() + 1;
                    let name = Self::lang_name(lang);
                    let full_prefix = format!("┌─ [{number}] {name} ");
                    let header_line = lines.len();

                    // Reserve the button first; the prefix yields on narrow
                    // terminals. Below the minimum, the button is omitted
                    // entirely (col_start == col_end registers no hit box) —
                    // number-key copying still works.
                    let button_cols = BUTTON_LABEL.len() + 1;
                    let (header, col_start, col_end) = if width >= button_cols + 2 {
                        let avail = width - button_cols;
                        let prefix = truncate_display(&full_prefix, avail);
                        let dashes = avail - display_width(&prefix);
                        (
                            format!(
                                "{DIM}{prefix}{}{RESET} {BUTTON_STYLE}{BUTTON_LABEL}{RESET}",
                                "─".repeat(dashes)
                            ),
                            (width - BUTTON_LABEL.len()) as u16,
                            width as u16,
                        )
                    } else {
                        let prefix = truncate_display(&full_prefix, width);
                        (format!("{DIM}{prefix}{RESET}"), 0, 0)
                    };
                    lines.push(header);
                    let mut highlighter = HighlightLines::new(self.syntax_for(lang), &self.theme);
                    let max_code_cols = width.saturating_sub(3);
                    for line in body.lines() {
                        if width < 3 {
                            // No room for content next to the gutter.
                            lines.push(format!("{DIM}{}{RESET}", truncate_display("│ ", width)));
                            continue;
                        }
                        // Highlight the full line, then clip the styled
                        // output: syntect is stateful across lines, so a
                        // display-clipped token (an unclosed `/*` or quote)
                        // must not poison the rest of the block.
                        let styled = self.highlight_line(&mut highlighter, &sanitize(line));
                        lines.push(format!(
                            "{DIM}│{RESET} {}{RESET}",
                            truncate_styled(&styled, max_code_cols)
                        ));
                    }
                    lines.push(format!(
                        "{DIM}└{}{RESET}",
                        "─".repeat(width.saturating_sub(1))
                    ));

                    buttons.push(CopyButton {
                        line: header_line,
                        col_start,
                        col_end,
                        lang: name,
                        body,
                    });
                }
            }
        }
        RenderedDoc {
            lines,
            buttons,
            images,
        }
    }
}

#[cfg(test)]
mod tests;
