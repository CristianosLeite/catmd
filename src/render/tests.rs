use super::*;

/// Removes CSI escape sequences (\x1b[...letter) for content assertions.
fn strip_ansi(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for esc in chars.by_ref() {
                if esc.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

const WIDTH: usize = 60;

#[test]
fn buttons_point_at_their_header_line() {
    let renderer = Renderer::new();
    let doc = renderer.render_doc("# T\n\n```rust\nlet x = 1;\n```\n", WIDTH, 0);
    assert_eq!(doc.buttons.len(), 1);
    let button = &doc.buttons[0];
    let header = strip_ansi(&doc.lines[button.line]);
    assert!(header.contains("[1] rust"), "header was: {header}");
    assert!(header.contains(BUTTON_LABEL));
    assert_eq!(button.body, "let x = 1;\n");
    assert_eq!(button.lang, "rust");
}

#[test]
fn button_occupies_the_rightmost_columns() {
    let renderer = Renderer::new();
    let doc = renderer.render_doc("```\nx\n```\n", WIDTH, 0);
    let button = &doc.buttons[0];
    assert_eq!(button.col_end as usize, WIDTH);
    assert_eq!(
        (button.col_end - button.col_start) as usize,
        BUTTON_LABEL.len()
    );
    // The visible header is exactly `width` columns wide.
    let header = strip_ansi(&doc.lines[button.line]);
    assert_eq!(header.chars().count(), WIDTH);
    // The button text sits exactly in the clickable range.
    let shown: String = header
        .chars()
        .skip(button.col_start as usize)
        .take(BUTTON_LABEL.len())
        .collect();
    assert_eq!(shown, BUTTON_LABEL);
}

#[test]
fn numbering_continues_from_first_block() {
    let renderer = Renderer::new();
    let doc = renderer.render_doc("```\na\n```\n\n```\nb\n```\n", WIDTH, 5);
    let headers: Vec<String> = doc
        .buttons
        .iter()
        .map(|b| strip_ansi(&doc.lines[b.line]))
        .collect();
    assert!(headers[0].contains("[6]"), "was: {}", headers[0]);
    assert!(headers[1].contains("[7]"), "was: {}", headers[1]);
}

#[test]
fn long_code_lines_are_clipped_to_width() {
    let renderer = Renderer::new();
    let long = "x".repeat(500);
    let doc = renderer.render_doc(&format!("```\n{long}\n```\n"), WIDTH, 0);
    for line in &doc.lines {
        assert!(
            strip_ansi(line).chars().count() <= WIDTH,
            "line wider than {WIDTH}: {line}"
        );
    }
    // The full body is still copied, not the clipped display version.
    assert_eq!(doc.buttons[0].body.trim_end(), long);
}

#[test]
fn display_clipping_does_not_poison_highlighting_of_later_lines() {
    // The long string literal is clipped on screen; the highlighter must
    // still see the whole line, or every following line would be colored
    // as if the string never closed.
    let renderer = Renderer::new();
    let long_line = format!("let s = \"{}\";", "a".repeat(120));
    let clipped = renderer.render_doc(
        &format!("```rust\n{long_line}\nlet x = 1;\n```\n"),
        WIDTH,
        0,
    );
    let short = renderer.render_doc("```rust\nlet s = \"a\";\nlet x = 1;\n```\n", WIDTH, 0);
    // In both docs `let x = 1;` is the second code line (header + 2).
    let styled_after_clip = &clipped.lines[clipped.buttons[0].line + 2];
    let styled_after_short = &short.lines[short.buttons[0].line + 2];
    assert_eq!(styled_after_clip, styled_after_short);
}

#[test]
fn unlabeled_block_is_called_code() {
    let renderer = Renderer::new();
    let doc = renderer.render_doc("```\nx\n```\n", WIDTH, 0);
    assert_eq!(doc.buttons[0].lang, "code");
}

#[test]
fn document_escape_sequences_never_reach_display_lines() {
    let renderer = Renderer::new();
    let src = "# T \x1b[2J\n\n```text\nSAFE\x1b[2JINJECTED\n\x1b]52;c;evil\x07osc\n```\n";
    let doc = renderer.render_doc(src, WIDTH, 0);
    for line in &doc.lines {
        assert!(!line.contains("\x1b[2J"), "CSI leaked into: {line:?}");
        assert!(!line.contains("\x1b]52"), "OSC leaked into: {line:?}");
    }
    // The clipboard body stays byte-exact — sanitizing is display-only.
    assert_eq!(
        doc.buttons[0].body,
        "SAFE\x1b[2JINJECTED\n\x1b]52;c;evil\x07osc\n"
    );
}

#[test]
fn wide_language_label_keeps_header_geometry() {
    let renderer = Renderer::new();
    let doc = renderer.render_doc("```日本語\nx\n```\n", WIDTH, 0);
    let button = &doc.buttons[0];
    let header = strip_ansi(&doc.lines[button.line]);
    // Header spans exactly WIDTH display columns and the clickable range
    // ends at the right edge.
    assert_eq!(crate::text::display_width(&header), WIDTH);
    assert_eq!(button.col_end as usize, WIDTH);
    assert!(header.ends_with(BUTTON_LABEL));
}

#[test]
fn narrow_terminals_never_overflow_and_keep_blocks_copyable() {
    let renderer = Renderer::new();
    for width in [1, 8, 10, 20] {
        let doc = renderer.render_doc("```rust\nlet x = 1;\n```\n", width, 0);
        for line in &doc.lines {
            assert!(
                crate::text::display_width(&strip_ansi(line)) <= width,
                "width {width}: line overflows: {line:?}"
            );
        }
        // The block always exists for number-key copying...
        assert_eq!(doc.buttons.len(), 1);
        let button = &doc.buttons[0];
        assert_eq!(button.body, "let x = 1;\n");
        // ...and any registered hit box lies within the visible columns.
        assert!(button.col_end as usize <= width || button.col_start == button.col_end);
    }
}

#[test]
fn wide_terminals_still_show_the_button_at_the_right_edge() {
    let renderer = Renderer::new();
    let doc = renderer.render_doc("```rust\nx\n```\n", 20, 0);
    let button = &doc.buttons[0];
    assert_eq!(button.col_end as usize, 20);
    assert_eq!(
        (button.col_end - button.col_start) as usize,
        BUTTON_LABEL.len()
    );
    assert!(strip_ansi(&doc.lines[button.line]).ends_with(BUTTON_LABEL));
}

#[test]
fn wide_code_lines_are_clipped_by_display_columns() {
    let renderer = Renderer::new();
    let wide = "日".repeat(200);
    let doc = renderer.render_doc(&format!("```\n{wide}\n```\n"), WIDTH, 0);
    for line in &doc.lines {
        assert!(
            crate::text::display_width(&strip_ansi(line)) <= WIDTH,
            "line wider than {WIDTH} columns: {line:?}"
        );
    }
}
