//! Sanitizing and measuring untrusted text for terminal display.
//!
//! Document content must never reach the terminal raw: it can carry CSI/OSC
//! escape sequences that rewrite the screen, alter terminal state, or set
//! the clipboard. All layout math is done in terminal display columns, not
//! `char` counts, so wide characters (CJK, emoji) cannot break alignment.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const TAB: &str = "    ";

/// Bidi control characters (`char::is_control` is false for these). They can
/// visually reorder rendered text (Trojan-source style), so display strips
/// them like the terminal controls.
const fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

/// Replaces terminal control characters so document content cannot inject
/// escape sequences. Tabs expand to spaces and newlines are kept; every
/// other control character (C0, DEL, C1 — including ESC and lone CR) is
/// dropped, as are bidi controls that could visually reorder the display.
/// Application styling must be added after sanitizing, never before.
pub fn sanitize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\t' => out.push_str(TAB),
            '\n' => out.push('\n'),
            c if c.is_control() || is_bidi_control(c) => {}
            c => out.push(c),
        }
    }
    out
}

/// True when `text` contains characters that are invisible in (or removed
/// from) the rendered display — controls, bidi controls, zero-width
/// characters — so what the user saw cannot be trusted to match these bytes.
/// CRLF line endings and tabs render visibly and do not count. ZWJ/ZWNJ are
/// excluded too: they are legitimate in emoji and several scripts.
pub fn has_hidden_chars(text: &str) -> bool {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let hidden = match c {
            '\n' | '\t' => false,
            '\r' => chars.peek() != Some(&'\n'),
            c if c.is_control() => true,
            c => is_bidi_control(c) || matches!(c, '\u{200B}' | '\u{2060}' | '\u{FEFF}'),
        };
        if hidden {
            return true;
        }
    }
    false
}

/// Width of `text` in terminal display columns.
pub fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// Truncates `text` to at most `max_cols` terminal display columns,
/// never splitting a wide character in half.
pub fn truncate_display(text: &str, max_cols: usize) -> String {
    let mut cols = 0;
    let mut out = String::new();
    for c in text.chars() {
        let width = UnicodeWidthChar::width(c).unwrap_or(0);
        if cols + width > max_cols {
            break;
        }
        cols += width;
        out.push(c);
    }
    out
}

/// Truncates *styled* text (application-added CSI sequences, e.g. syntect
/// colors) to `max_cols` display columns: escape sequences are copied
/// verbatim and count zero columns, and are never split. The input's plain
/// text must already be sanitized; the caller appends its own reset.
pub fn truncate_styled(styled: &str, max_cols: usize) -> String {
    let mut cols = 0;
    let mut out = String::new();
    let mut chars = styled.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            out.push(c);
            out.push(chars.next().unwrap());
            for esc in chars.by_ref() {
                out.push(esc);
                // CSI ends at its final byte (0x40-0x7E).
                if ('\u{40}'..='\u{7e}').contains(&esc) {
                    break;
                }
            }
            continue;
        }
        let width = UnicodeWidthChar::width(c).unwrap_or(0);
        if cols + width > max_cols {
            break;
        }
        cols += width;
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests;
