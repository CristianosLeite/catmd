//! Splitting markdown source into plain-markdown segments and fenced code
//! blocks, following CommonMark fence rules — including fences nested in
//! block quotes and indented fence openers.

pub enum Segment<'a> {
    Markdown(String),
    /// `body` is the logical content of the block per CommonMark: quote
    /// markers and the opener's indentation are stripped, but line endings
    /// (CRLF vs LF) and a missing final newline are preserved exactly.
    Code {
        lang: &'a str,
        body: String,
    },
}

/// An open fenced code block and the container context it lives in.
#[derive(Clone, Copy)]
struct Fence {
    fence_char: char,
    fence_len: usize,
    /// Leading spaces of the opener; stripped from each content line.
    indent: usize,
    /// Number of `>` block-quote markers wrapping the fence.
    quotes: usize,
}

/// Splits a fence opener line into its fence characters and info string,
/// e.g. "```rust" -> Some(("```", "rust")). Up to 3 leading spaces allowed.
fn parse_fence(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let Some(fence_char @ ('`' | '~')) = trimmed.chars().next() else {
        return None;
    };
    let fence_len = trimmed.chars().take_while(|&c| c == fence_char).count();
    if fence_len < 3 {
        return None;
    }
    let (fence, info) = trimmed.split_at(fence_len);
    // CommonMark: an info string on a backtick fence may not contain backticks.
    if fence_char == '`' && info.contains('`') {
        return None;
    }
    Some((fence, info.trim()))
}

/// Strips one trailing line ending ("\n" or "\r\n") for line inspection.
fn without_ending(raw_line: &str) -> &str {
    let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
    line.strip_suffix('\r').unwrap_or(line)
}

/// Strips exactly `depth` block-quote markers (up to 3 spaces, `>`, and one
/// optional space each). Returns None if the line has fewer markers.
fn strip_quote_markers(line: &str, depth: usize) -> Option<&str> {
    let mut rest = line;
    for _ in 0..depth {
        let trimmed = rest.trim_start_matches(' ');
        if rest.len() - trimmed.len() > 3 {
            return None;
        }
        rest = trimmed.strip_prefix('>')?;
        rest = rest.strip_prefix(' ').unwrap_or(rest);
    }
    Some(rest)
}

/// Strips and counts all leading block-quote markers.
fn strip_all_quote_markers(line: &str) -> (&str, usize) {
    let mut depth = 0;
    let mut rest = line;
    while let Some(stripped) = strip_quote_markers(rest, 1) {
        rest = stripped;
        depth += 1;
    }
    (rest, depth)
}

fn leading_spaces(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// CommonMark: content lines lose up to the opener's indentation.
fn strip_indent(line: &str, max: usize) -> &str {
    &line[leading_spaces(line).min(max)..]
}

pub fn split_segments(src: &str) -> Vec<Segment<'_>> {
    let raw_lines: Vec<&str> = src.split_inclusive('\n').collect();
    let mut segments = Vec::new();
    let mut markdown = String::new();
    let mut fence: Option<Fence> = None;
    let mut lang = "";
    let mut code = String::new();
    let mut i = 0;

    while i < raw_lines.len() {
        let raw_line = raw_lines[i];
        let line = without_ending(raw_line);
        let ending = &raw_line[line.len()..];

        match fence {
            None => {
                let (content, quotes) = strip_all_quote_markers(line);
                if let Some((open, info)) = parse_fence(content) {
                    if !markdown.is_empty() {
                        segments.push(Segment::Markdown(std::mem::take(&mut markdown)));
                    }
                    fence = Some(Fence {
                        fence_char: open.chars().next().unwrap(),
                        fence_len: open.len(),
                        indent: leading_spaces(content),
                        quotes,
                    });
                    lang = info.split_whitespace().next().unwrap_or("");
                } else {
                    markdown.push_str(line);
                    markdown.push('\n');
                }
                i += 1;
            }
            Some(f) => {
                let Some(content) = strip_quote_markers(line, f.quotes) else {
                    // The surrounding block quote ended, which closes the code
                    // block (CommonMark). Reprocess this line as fresh input.
                    segments.push(Segment::Code {
                        lang,
                        body: std::mem::take(&mut code),
                    });
                    fence = None;
                    lang = "";
                    continue;
                };
                let closes = parse_fence(content).is_some_and(|(close, info)| {
                    info.is_empty() && close.starts_with(f.fence_char) && close.len() >= f.fence_len
                });
                if closes {
                    segments.push(Segment::Code {
                        lang,
                        body: std::mem::take(&mut code),
                    });
                    fence = None;
                    lang = "";
                } else {
                    code.push_str(strip_indent(content, f.indent));
                    code.push_str(ending);
                }
                i += 1;
            }
        }
    }
    if fence.is_some() {
        // Unclosed fence: everything to EOF is code (CommonMark behavior).
        segments.push(Segment::Code { lang, body: code });
    } else if !markdown.is_empty() {
        segments.push(Segment::Markdown(markdown));
    }
    segments
}

#[cfg(test)]
mod tests;
