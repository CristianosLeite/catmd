//! Case-insensitive text search over the rendered view.
//!
//! Matching runs on the *plain* text of a view line: rendered lines carry SGR
//! sequences from termimad and syntect, and those bytes must never be visible
//! to the query — searching for "38" must not hit a color code. Highlighting
//! walks the styled line instead, so a match keeps the line's own colors
//! around it.

use std::iter::Peekable;
use std::str::Chars;

/// Highlight for a match, and for the one the viewer is parked on.
const HIT_STYLE: &str = "\x1b[48;5;220m\x1b[38;5;16m";
const CURRENT_STYLE: &str = "\x1b[48;5;208m\x1b[1m\x1b[38;5;16m";
const RESET: &str = "\x1b[0m";

/// One occurrence: a view line, the plain-char offset it starts at, and its
/// length in visible characters. The length is per-hit because case folding
/// can expand a character (ß matches "ss"), so occurrences of one query can
/// span different numbers of visible characters.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Hit {
    line: usize,
    start: usize,
    len: usize,
}

/// What committing a query did.
pub enum Found {
    /// Scroll to this view line.
    Line(usize),
    /// Nothing matched; show this message.
    Missing(String),
    /// The query was empty, so the prompt just closed.
    Empty,
}

pub struct Search {
    /// The query being typed; `Some` only while the prompt is open.
    input: Option<String>,
    /// The committed query, as typed (for display).
    query: String,
    /// The committed query, case-folded (for matching).
    needle: Vec<char>,
    /// Every occurrence, in view order.
    hits: Vec<Hit>,
    /// Index into `hits` of the occurrence the viewer is parked on.
    current: usize,
}

impl Search {
    pub const fn new() -> Self {
        Self {
            input: None,
            query: String::new(),
            needle: Vec::new(),
            hits: Vec::new(),
            current: 0,
        }
    }

    pub const fn is_open(&self) -> bool {
        self.input.is_some()
    }

    /// True once a query has matched something, i.e. highlights are on screen.
    pub const fn is_active(&self) -> bool {
        !self.hits.is_empty()
    }

    /// Opens an empty prompt; any previous search keeps highlighting until
    /// the new query is committed.
    pub fn open(&mut self) {
        self.input = Some(String::new());
    }

    pub fn push(&mut self, c: char) {
        if let Some(input) = &mut self.input {
            input.push(c);
        }
    }

    pub fn backspace(&mut self) {
        if let Some(input) = &mut self.input {
            input.pop();
        }
    }

    /// Closes the prompt, discarding what was typed. A committed search from
    /// before the prompt was opened survives.
    pub fn clear_input(&mut self) {
        self.input = None;
    }

    /// Drops the search entirely, highlights included.
    pub fn clear(&mut self) {
        self.input = None;
        self.query.clear();
        self.needle.clear();
        self.hits.clear();
        self.current = 0;
    }

    /// Runs the typed query over `lines` and parks on the first hit at or
    /// after line `from`, wrapping to the top when there is none below.
    pub fn commit(&mut self, lines: &[String], from: usize) -> Found {
        let input = self.input.take().unwrap_or_default();
        if input.is_empty() {
            self.clear();
            return Found::Empty;
        }
        self.needle = input.chars().flat_map(fold).collect();
        self.query = input;
        self.hits = scan(lines, &self.needle);
        if self.hits.is_empty() {
            self.current = 0;
            return Found::Missing(format!(" ✗ no match for “{}”", self.query));
        }
        self.current = self
            .hits
            .iter()
            .position(|hit| hit.line >= from)
            .unwrap_or(0);
        Found::Line(self.hits[self.current].line)
    }

    /// Moves to the next (or previous) occurrence, wrapping around.
    pub fn advance(&mut self, forward: bool) -> Option<usize> {
        let last = self.hits.len().checked_sub(1)?;
        self.current = if forward {
            if self.current >= last {
                0
            } else {
                self.current + 1
            }
        } else if self.current == 0 {
            last
        } else {
            self.current - 1
        };
        Some(self.hits[self.current].line)
    }

    /// Re-scans after the view is rebuilt: a resize re-wraps every line, so
    /// the recorded line numbers no longer mean anything.
    pub fn refresh(&mut self, lines: &[String]) {
        if self.needle.is_empty() {
            return;
        }
        self.hits = scan(lines, &self.needle);
        self.current = self.current.min(self.hits.len().saturating_sub(1));
    }

    /// The view line of the occurrence the viewer is parked on.
    pub fn current_line(&self) -> Option<usize> {
        self.hits.get(self.current).map(|hit| hit.line)
    }

    /// The whole status bar while the prompt is open.
    pub fn prompt(&self) -> Option<String> {
        self.input
            .as_ref()
            .map(|input| format!(" /{input}▏ · Enter search · Esc cancel"))
    }

    /// The status-bar fragment for a committed search.
    pub fn hint(&self) -> Option<String> {
        self.is_active().then(|| {
            format!(
                " · /{} {}/{} · n next · N prev",
                self.query,
                self.current + 1,
                self.hits.len()
            )
        })
    }

    /// `line` re-styled with its matches highlighted, or `None` when it holds
    /// none — the caller then prints the line untouched.
    pub fn highlight(&self, line: usize, styled: &str) -> Option<String> {
        if self.needle.is_empty() {
            return None;
        }
        let first = self.hits.partition_point(|hit| hit.line < line);
        let end = self.hits.partition_point(|hit| hit.line <= line);
        (first < end).then(|| {
            paint(
                styled,
                &self.hits[first..end],
                self.hits.get(self.current).copied(),
            )
        })
    }
}

/// Case fold for both haystack and needle. Uppercasing before lowercasing
/// approximates Unicode case folding with only the std tables: it equates
/// the Greek final sigma with sigma (ς → Σ → σ) and expands ß to "ss".
/// A fold can yield several characters, so callers track how many visible
/// characters each folded run came from.
fn fold(c: char) -> impl Iterator<Item = char> {
    c.to_uppercase().flat_map(char::to_lowercase)
}

/// Consumes a CSI sequence whose ESC the caller already took, returning it
/// whole (ESC included).
fn take_csi(chars: &mut Peekable<Chars<'_>>) -> String {
    let mut esc = String::from('\x1b');
    if let Some(bracket) = chars.next() {
        esc.push(bracket);
    }
    for c in chars.by_ref() {
        esc.push(c);
        // CSI ends at its final byte (0x40-0x7E).
        if ('\u{40}'..='\u{7e}').contains(&c) {
            break;
        }
    }
    esc
}

/// The folded plain characters of a styled line, plus, for each folded
/// character, the index of the visible character it came from. CSI sequences
/// contribute nothing, so the recorded indices count visible characters only.
fn folded_plain(styled: &str) -> (Vec<char>, Vec<usize>) {
    let mut out = Vec::new();
    let mut source = Vec::new();
    let mut index = 0usize;
    let mut chars = styled.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            take_csi(&mut chars);
            continue;
        }
        for folded in fold(c) {
            out.push(folded);
            source.push(index);
        }
        index += 1;
    }
    (out, source)
}

/// Every non-overlapping occurrence of `needle` (already folded), in view
/// order. Offsets and lengths are in visible characters; a match landing
/// inside a fold expansion (needle "s" against "ß") covers the whole
/// expanded character, and one character is never reported twice.
fn scan(lines: &[String], needle: &[char]) -> Vec<Hit> {
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<Hit> = Vec::new();
    for (line, styled) in lines.iter().enumerate() {
        let (hay, source) = folded_plain(styled);
        let mut at = 0;
        while at + needle.len() <= hay.len() {
            if hay[at..at + needle.len()] != *needle {
                at += 1;
                continue;
            }
            let start = source[at];
            let len = source[at + needle.len() - 1] - start + 1;
            let overlaps = hits
                .last()
                .is_some_and(|prev| prev.line == line && prev.start + prev.len > start);
            if !overlaps {
                hits.push(Hit { line, start, len });
            }
            at += needle.len();
        }
    }
    hits
}

/// Rewrites `styled` with `hits` highlighted. The line's own SGR state is
/// tracked so text after a match keeps its original color, and styling that
/// starts inside a match is re-covered instead of overpainting the highlight.
fn paint(styled: &str, hits: &[Hit], current: Option<Hit>) -> String {
    let mut out = String::with_capacity(styled.len() + hits.len() * 32);
    let mut active = String::new();
    let mut chars = styled.chars().peekable();
    let mut index = 0usize;
    let mut style: Option<&str> = None;
    let mut end = 0usize;
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            let esc = take_csi(&mut chars);
            if esc.ends_with('m') {
                if esc == RESET {
                    active.clear();
                } else {
                    active.push_str(&esc);
                }
            }
            out.push_str(&esc);
            if let Some(style) = style {
                out.push_str(style);
            }
            continue;
        }
        if style.is_none()
            && let Some(hit) = hits.iter().find(|hit| hit.start == index)
        {
            let chosen = if current == Some(*hit) {
                CURRENT_STYLE
            } else {
                HIT_STYLE
            };
            out.push_str(chosen);
            style = Some(chosen);
            end = index + hit.len;
        }
        out.push(c);
        index += 1;
        if style.is_some() && index == end {
            out.push_str(RESET);
            out.push_str(&active);
            style = None;
        }
    }
    // A match running to the end of the line still has to close its style.
    if style.is_some() {
        out.push_str(RESET);
        out.push_str(&active);
    }
    out
}

#[cfg(test)]
mod tests;
