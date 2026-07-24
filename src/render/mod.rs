//! Styling Markdown for the terminal: termimad for prose, syntect for
//! syntax-highlighted code blocks.
//!
//! All document-derived text is passed through `text::sanitize` before any
//! styling is added, so documents cannot inject terminal escape sequences.
//! Layout is computed in terminal display columns (`text::display_width`).

use std::io::{self, Write};

use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::as_24_bit_terminal_escaped;
use termimad::MadSkin;
use termimad::crossterm::style::Color;

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

pub struct RenderedDoc {
    pub lines: Vec<String>,
    pub buttons: Vec<CopyButton>,
}

pub struct Renderer {
    skin: MadSkin,
    syntaxes: SyntaxSet,
    theme: Theme,
}

impl Renderer {
    pub fn new() -> Self {
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
        }
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

    /// Plain cat-like rendering (non-interactive mode). Write errors are
    /// returned, not panicked on: SIGPIPE stays ignored (Rust's default) so
    /// a vanished reader surfaces here as `BrokenPipe` for the caller.
    pub fn render_plain(&self, out: &mut impl Write, src: &str, width: usize) -> io::Result<()> {
        for segment in split_segments(src) {
            match segment {
                Segment::Markdown(md) => {
                    write!(out, "{}", self.skin.text(&sanitize(&md), Some(width)))?;
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
    pub fn render_doc(&self, src: &str, width: usize, first_block: usize) -> RenderedDoc {
        let mut lines: Vec<String> = Vec::new();
        let mut buttons: Vec<CopyButton> = Vec::new();
        for segment in split_segments(src) {
            match segment {
                Segment::Markdown(md) => {
                    let text = format!("{}", self.skin.text(&sanitize(&md), Some(width)));
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
        RenderedDoc { lines, buttons }
    }
}

#[cfg(test)]
mod tests;
