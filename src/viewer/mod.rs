//! Interactive full-screen viewer: scrolling, mouse support, and clickable
//! [ copy ] buttons on code blocks.

use std::io::{self, Write};

use termimad::crossterm::{
    cursor,
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseButton, MouseEventKind,
    },
    execute, queue,
    style::Print,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

use std::collections::HashSet;

use crate::Doc;
use crate::clipboard;
use crate::image::kitty;
use crate::render::{CopyButton, ImageSpan, Renderer};
use crate::text::{display_width, has_hidden_chars, sanitize, truncate_display};

const TITLE_STYLE: &str = "\x1b[1m\x1b[38;5;208m";
const STATUS_STYLE: &str = "\x1b[7m";
const RESET: &str = "\x1b[0m";

struct View {
    lines: Vec<String>,
    buttons: Vec<CopyButton>,
    images: Vec<ImageSpan>,
}

/// Renders all documents into one scrollable view. Multiple documents get
/// title separators; button and image lines are offset to view coordinates.
fn build(renderer: &Renderer, docs: &[Doc], width: usize) -> View {
    let mut lines: Vec<String> = Vec::new();
    let mut buttons: Vec<CopyButton> = Vec::new();
    let mut images: Vec<ImageSpan> = Vec::new();
    for doc in docs {
        if docs.len() > 1 {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push(format!(
                "{TITLE_STYLE}═══ {} ═══{RESET}",
                sanitize(&doc.title)
            ));
            lines.push(String::new());
        }
        let offset = lines.len();
        let rendered =
            renderer.render_doc(&doc.source, width, buttons.len(), doc.base_dir.as_deref());
        lines.extend(rendered.lines);
        buttons.extend(rendered.buttons.into_iter().map(|mut button| {
            button.line += offset;
            button
        }));
        images.extend(rendered.images.into_iter().map(|mut image| {
            image.line += offset;
            image
        }));
    }
    View {
        lines,
        buttons,
        images,
    }
}

/// The part of an image span visible in the viewport starting at `scroll`:
/// (screen row, visible rows, crop). None when scrolled fully out of view.
fn visible_slice(
    image: &ImageSpan,
    scroll: usize,
    content_height: usize,
) -> Option<(u16, u16, Option<kitty::Crop>)> {
    let top = image.line.max(scroll);
    let bottom = (image.line + image.rows as usize).min(scroll + content_height);
    if top >= bottom {
        return None;
    }
    let visible = (bottom - top) as u16;
    let crop = if visible == image.rows {
        None
    } else {
        let px_per_row = f64::from(image.img_h) / f64::from(image.rows);
        Some(kitty::Crop {
            y_px: ((top - image.line) as f64 * px_per_row) as u32,
            h_px: ((f64::from(visible) * px_per_row) as u32).max(1),
        })
    };
    Some(((top - scroll) as u16, visible, crop))
}

/// Multi-digit block selection: typed digits accumulate and the copy fires
/// as soon as the number is unambiguous (no further digit could still name
/// an existing block). Enter confirms an ambiguous prefix early.
struct BlockPicker {
    pending: String,
}

enum Pick {
    /// Copy this button index now.
    Copy(usize),
    /// Waiting for more digits; show this status message.
    Pending(String),
    /// The typed number names no block; show this message.
    Invalid(String),
}

impl BlockPicker {
    const fn new() -> Self {
        Self {
            pending: String::new(),
        }
    }

    const fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    fn clear(&mut self) {
        self.pending.clear();
    }

    fn push_digit(&mut self, digit: char, total: usize) -> Pick {
        self.pending.push(digit);
        match self.pending.parse::<usize>() {
            Ok(number) if (1..=total).contains(&number) => {
                if number * 10 > total {
                    self.pending.clear();
                    Pick::Copy(number - 1)
                } else {
                    Pick::Pending(format!(
                        " copy block {number}… (more digits, or Enter to confirm)"
                    ))
                }
            }
            _ => {
                let message = format!(" ✗ no code block {}", self.pending);
                self.pending.clear();
                Pick::Invalid(message)
            }
        }
    }

    /// Enter: copy the pending number as typed, if any.
    fn confirm(&mut self) -> Option<usize> {
        let number: usize = self.pending.parse().ok()?;
        self.pending.clear();
        (number >= 1).then(|| number - 1)
    }
}

/// Restores the terminal on drop, so every exit path — normal quit, an error
/// during setup or the event loop, or a panic — leaves the terminal usable.
/// Restoring states that were never entered is harmless.
struct TermGuard;

impl Drop for TermGuard {
    fn drop(&mut self) {
        let mut out = io::stdout();
        let _ = execute!(
            out,
            Print("\x1b[?7h"),
            cursor::Show,
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

pub fn run(renderer: &Renderer, docs: &[Doc]) -> io::Result<()> {
    let mut out = io::stdout();
    terminal::enable_raw_mode()?;
    let _guard = TermGuard;
    // \x1b[?7l disables line wrap so overlong lines clip instead of breaking layout.
    execute!(
        out,
        EnterAlternateScreen,
        EnableMouseCapture,
        cursor::Hide,
        Print("\x1b[?7l")
    )?;
    event_loop(renderer, docs, &mut out)
}

fn event_loop(renderer: &Renderer, docs: &[Doc], out: &mut io::Stdout) -> io::Result<()> {
    let (mut width, mut height) = terminal::size()?;
    let mut view = build(renderer, docs, width as usize);
    let mut scroll = 0usize;
    let mut flash: Option<String> = None;
    let mut picker = BlockPicker::new();
    // Kitty image data transmitted to the terminal so far. Ids are stable
    // across rebuilds (they live in the renderer's cache), so a resize does
    // not retransmit.
    let mut transmitted: HashSet<u32> = HashSet::new();
    let gfx = renderer.graphics();

    let result = loop {
        let content_height = (height as usize).saturating_sub(1);
        let max_scroll = view.lines.len().saturating_sub(content_height);
        scroll = scroll.min(max_scroll);
        if let Err(err) = draw(
            out,
            &view,
            docs,
            scroll,
            content_height,
            width as usize,
            flash.as_deref(),
            &mut transmitted,
            gfx,
        ) {
            break Err(err);
        }

        let event = match event::read() {
            Ok(event) => event,
            Err(err) => break Err(err),
        };
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                flash = None;
                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        break Ok(());
                    }
                    KeyCode::Char(digit @ '0'..='9') => {
                        match picker.push_digit(digit, view.buttons.len()) {
                            Pick::Copy(index) => flash = copy_block(&view, index),
                            Pick::Pending(message) | Pick::Invalid(message) => {
                                flash = Some(message);
                            }
                        }
                        continue;
                    }
                    // Ctrl+J is a line feed — in raw mode crossterm delivers a
                    // literal '\n' this way, so treat it exactly like Enter.
                    KeyCode::Enter | KeyCode::Char('j')
                        if key.code == KeyCode::Enter
                            || key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        if let Some(index) = picker.confirm() {
                            flash = copy_block(&view, index);
                        }
                        continue;
                    }
                    // Esc cancels a pending block number before it quits.
                    KeyCode::Esc if !picker.is_empty() => {
                        picker.clear();
                        continue;
                    }
                    KeyCode::Char('q') | KeyCode::Esc => break Ok(()),
                    KeyCode::Up | KeyCode::Char('k') => scroll = scroll.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => scroll = (scroll + 1).min(max_scroll),
                    KeyCode::PageUp => scroll = scroll.saturating_sub(content_height),
                    KeyCode::PageDown | KeyCode::Char(' ') => {
                        scroll = (scroll + content_height).min(max_scroll)
                    }
                    KeyCode::Home | KeyCode::Char('g') => scroll = 0,
                    KeyCode::End | KeyCode::Char('G') => scroll = max_scroll,
                    _ => {}
                }
                picker.clear();
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => scroll = scroll.saturating_sub(3),
                MouseEventKind::ScrollDown => scroll = (scroll + 3).min(max_scroll),
                MouseEventKind::Down(MouseButton::Left) => {
                    flash = None;
                    if (mouse.row as usize) < content_height
                        && let Some(hit) = view.buttons.iter().position(|b| {
                            b.line == scroll + mouse.row as usize
                                && mouse.column >= b.col_start
                                && mouse.column < b.col_end
                        })
                    {
                        flash = copy_block(&view, hit);
                    }
                }
                _ => {}
            },
            Event::Resize(new_width, new_height) => {
                width = new_width;
                height = new_height;
                // Font/monitor changes can alter the pixel size of a cell,
                // not just the column count; refresh before laying out.
                renderer.set_cell(kitty::cell_size());
                view = build(renderer, docs, width as usize);
            }
            _ => {}
        }
    };

    // Free transmitted image data in the terminal; leaving the alternate
    // screen removes the placements but not the stored pixels.
    if !transmitted.is_empty() {
        for id in &transmitted {
            queue!(out, Print(gfx.free(*id)))?;
        }
        out.flush()?;
    }
    result
}

fn copy_block(view: &View, index: usize) -> Option<String> {
    let button = view.buttons.get(index)?;
    Some(match clipboard::copy(&button.body) {
        Ok(via) => {
            // The display strips controls and bidi characters but the
            // clipboard is byte-exact — warn when they differ, so a block
            // that *looks* harmless is not pasted into a shell blindly.
            let hidden = if has_hidden_chars(&button.body) {
                " · ⚠ contains hidden characters"
            } else {
                ""
            };
            format!(
                " ✓ copied block {} ({}) to clipboard via {via}{hidden}",
                index + 1,
                button.lang
            )
        }
        Err(err) => format!(" ✗ copy failed: {err}"),
    })
}

fn status_line(
    view: &View,
    docs: &[Doc],
    scroll: usize,
    content_height: usize,
    width: usize,
) -> String {
    let titles = sanitize(
        &docs
            .iter()
            .map(|doc| doc.title.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    );
    let end = (scroll + content_height).min(view.lines.len());
    // An empty document must read "0-0/0", not "1-0/0".
    let start = (scroll + 1).min(end);
    let copy_hint = match view.buttons.len() {
        0 => String::new(),
        1 => " · click [ copy ] or press 1".to_string(),
        n => format!(" · click [ copy ] or type 1-{n}"),
    };
    // Mouse capture swallows drag-selection; holding Shift hands the drag
    // back to the terminal, so the hint belongs next to the mouse actions.
    let info = format!(
        " · {start}-{end}/{} · ↑↓ scroll{copy_hint} · shift+drag select · q quit",
        view.lines.len()
    );
    // A long filename must not push the key hints off-screen.
    let avail = width.saturating_sub(display_width(&info) + 2);
    let mut title = truncate_display(&titles, avail);
    if title != titles {
        title.push('…');
    }
    format!(" {title}{info}")
}

#[allow(clippy::too_many_arguments)]
fn draw(
    out: &mut io::Stdout,
    view: &View,
    docs: &[Doc],
    scroll: usize,
    content_height: usize,
    width: usize,
    flash: Option<&str>,
    transmitted: &mut HashSet<u32>,
    gfx: kitty::Graphics,
) -> io::Result<()> {
    for row in 0..content_height {
        queue!(
            out,
            cursor::MoveTo(0, row as u16),
            Clear(ClearType::CurrentLine)
        )?;
        if let Some(line) = view.lines.get(scroll + row) {
            queue!(out, Print(line))?;
        }
    }

    if !view.images.is_empty() {
        // Placements are tied to screen positions, so scrolling means
        // removing them all and re-placing the visible slices. The pixel
        // data itself is transmitted only once per image.
        queue!(out, Print(gfx.delete_placements()))?;
        for image in &view.images {
            let Some((row, rows, crop)) = visible_slice(image, scroll, content_height) else {
                continue;
            };
            if transmitted.insert(image.id) {
                queue!(out, Print(gfx.transmit(image.id, &image.png)))?;
            }
            queue!(
                out,
                cursor::MoveTo(0, row),
                Print(gfx.place(image.id, image.cols, rows, crop))
            )?;
        }
    }

    let status = match flash {
        Some(message) => message.to_string(),
        None => status_line(view, docs, scroll, content_height, width),
    };
    let status = truncate_display(&status, width);
    let pad = width.saturating_sub(display_width(&status));
    queue!(
        out,
        cursor::MoveTo(0, content_height as u16),
        Clear(ClearType::CurrentLine),
        Print(format!("{STATUS_STYLE}{status}{}{RESET}", " ".repeat(pad)))
    )?;
    out.flush()
}

#[cfg(test)]
mod tests;
