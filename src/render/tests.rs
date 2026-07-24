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
    let doc = renderer.render_doc("# T\n\n```rust\nlet x = 1;\n```\n", WIDTH, 0, None);
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
    let doc = renderer.render_doc("```\nx\n```\n", WIDTH, 0, None);
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
    let doc = renderer.render_doc("```\na\n```\n\n```\nb\n```\n", WIDTH, 5, None);
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
    let doc = renderer.render_doc(&format!("```\n{long}\n```\n"), WIDTH, 0, None);
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
        None,
    );
    let short = renderer.render_doc("```rust\nlet s = \"a\";\nlet x = 1;\n```\n", WIDTH, 0, None);
    // In both docs `let x = 1;` is the second code line (header + 2).
    let styled_after_clip = &clipped.lines[clipped.buttons[0].line + 2];
    let styled_after_short = &short.lines[short.buttons[0].line + 2];
    assert_eq!(styled_after_clip, styled_after_short);
}

#[test]
fn unlabeled_block_is_called_code() {
    let renderer = Renderer::new();
    let doc = renderer.render_doc("```\nx\n```\n", WIDTH, 0, None);
    assert_eq!(doc.buttons[0].lang, "code");
}

#[test]
fn document_escape_sequences_never_reach_display_lines() {
    let renderer = Renderer::new();
    let src = "# T \x1b[2J\n\n```text\nSAFE\x1b[2JINJECTED\n\x1b]52;c;evil\x07osc\n```\n";
    let doc = renderer.render_doc(src, WIDTH, 0, None);
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
    let doc = renderer.render_doc("```日本語\nx\n```\n", WIDTH, 0, None);
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
        let doc = renderer.render_doc("```rust\nlet x = 1;\n```\n", width, 0, None);
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
    let doc = renderer.render_doc("```rust\nx\n```\n", 20, 0, None);
    let button = &doc.buttons[0];
    assert_eq!(button.col_end as usize, 20);
    assert_eq!(
        (button.col_end - button.col_start) as usize,
        BUTTON_LABEL.len()
    );
    assert!(strip_ansi(&doc.lines[button.line]).ends_with(BUTTON_LABEL));
}

/// Writes a 2x2 PNG into a fresh temp directory and returns (dir, filename).
fn tiny_png(name: &str) -> (std::path::PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("catmd-render-test-{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    let mut img = image::RgbaImage::new(2, 2);
    for px in img.pixels_mut() {
        *px = image::Rgba([255, 0, 0, 255]);
    }
    let file = format!("{name}.png");
    img.save(dir.join(&file)).unwrap();
    (dir, file)
}

#[test]
fn text_mode_keeps_images_as_markdown_even_when_loadable() {
    let (dir, file) = tiny_png("textmode");
    let renderer = Renderer::new();
    let doc = renderer.render_doc(&format!("# T\n\n![a pic]({file})\n"), WIDTH, 0, Some(&dir));
    std::fs::remove_dir_all(&dir).ok();
    assert!(doc.images.is_empty());
    let text = doc.lines.iter().map(|l| strip_ansi(l)).collect::<String>();
    assert!(text.contains("a pic"), "image line lost: {text:?}");
    assert!(!text.contains('▀'));
}

#[test]
fn missing_image_falls_back_to_markdown_text() {
    let renderer = Renderer::new();
    let doc = renderer.render_doc("![alt text](nope/missing.png)\n", WIDTH, 0, None);
    let text = doc.lines.iter().map(|l| strip_ansi(l)).collect::<String>();
    assert!(text.contains("alt text"), "fallback lost alt: {text:?}");
}

#[test]
fn text_mode_plain_rendering_has_no_graphics_escapes() {
    let (dir, file) = tiny_png("textplain");
    let renderer = Renderer::new();
    let mut out = Vec::new();
    renderer
        .render_plain(&mut out, &format!("![x]({file})\n"), WIDTH, Some(&dir))
        .unwrap();
    std::fs::remove_dir_all(&dir).ok();
    let text = String::from_utf8(out).unwrap();
    assert!(!text.contains("\x1b_G"));
    assert!(strip_ansi(&text).contains('x'), "alt lost: {text:?}");
}

#[test]
fn wide_code_lines_are_clipped_by_display_columns() {
    let renderer = Renderer::new();
    let wide = "日".repeat(200);
    let doc = renderer.render_doc(&format!("```\n{wide}\n```\n"), WIDTH, 0, None);
    for line in &doc.lines {
        assert!(
            crate::text::display_width(&strip_ansi(line)) <= WIDTH,
            "line wider than {WIDTH} columns: {line:?}"
        );
    }
}

#[test]
fn cached_images_survive_viewer_rebuilds_without_disk_reads() {
    let (dir, file) = tiny_png("cached");
    let renderer = kitty_renderer();
    let src = format!("![x]({file})\n");
    let first = renderer.render_doc(&src, WIDTH, 0, Some(&dir));
    // Deleting the file proves the second render is served from the cache,
    // as happens on every terminal resize.
    std::fs::remove_dir_all(&dir).ok();
    let second = renderer.render_doc(&src, WIDTH, 0, Some(&dir));
    assert_eq!(first.images.len(), 1);
    assert_eq!(second.images.len(), 1);
    assert_eq!(first.images[0].id, second.images[0].id);
    assert_eq!(first.lines, second.lines);
}

#[test]
fn decoded_destinations_reach_files_with_awkward_names() {
    let dir = std::env::temp_dir().join("catmd-render-test-awkward");
    std::fs::create_dir_all(&dir).unwrap();
    let mut img = image::RgbaImage::new(2, 2);
    for px in img.pixels_mut() {
        *px = image::Rgba([255, 0, 0, 255]);
    }
    for name in ["a(b).png", "my pic.png", "imagé.png"] {
        img.save(dir.join(name)).unwrap();
    }
    let renderer = kitty_renderer();
    for dest in [
        "a\\(b\\).png",
        "my%20pic.png",
        "<my pic.png>",
        "imag%C3%A9.png",
    ] {
        let doc = renderer.render_doc(&format!("![x]({dest})\n"), WIDTH, 0, Some(&dir));
        assert_eq!(doc.images.len(), 1, "destination {dest:?} did not resolve");
    }
    std::fs::remove_dir_all(&dir).ok();
}

fn kitty_renderer() -> Renderer {
    Renderer::with_images(
        Mode::Kitty { tmux: false },
        kitty::CellSize {
            width: 8,
            height: 16,
        },
    )
}

#[test]
fn kitty_mode_reserves_placeholder_rows_and_records_the_span() {
    let (dir, file) = tiny_png("kitty-doc");
    let renderer = kitty_renderer();
    let doc = renderer.render_doc(&format!("![pic]({file})\n"), WIDTH, 0, Some(&dir));
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(doc.images.len(), 1);
    let span = &doc.images[0];
    // A 2x2 image in 8x16 cells occupies one cell.
    assert_eq!((span.cols, span.rows), (1, 1));
    assert!(span.img_h > 0);
    assert!(!span.png.is_empty());
    // Placeholder rows are empty; no half blocks anywhere.
    for row in 0..span.rows as usize {
        assert_eq!(doc.lines[span.line + row], "");
    }
    assert!(!doc.lines.iter().any(|l| l.contains('▀')));
    // The caption still renders as text.
    assert!(
        doc.lines
            .iter()
            .any(|l| l.starts_with(DIM) && strip_ansi(l) == "pic")
    );
}

#[test]
fn kitty_mode_missing_image_falls_back_to_markdown_text() {
    let renderer = kitty_renderer();
    let doc = renderer.render_doc("![alt text](missing.png)\n", WIDTH, 0, None);
    assert!(doc.images.is_empty());
    let text = doc.lines.iter().map(|l| strip_ansi(l)).collect::<String>();
    assert!(text.contains("alt text"));
}

#[test]
fn kitty_mode_plain_rendering_emits_the_protocol_inline() {
    let (dir, file) = tiny_png("kitty-plain");
    let renderer = kitty_renderer();
    let mut out = Vec::new();
    renderer
        .render_plain(&mut out, &format!("![x]({file})\n"), WIDTH, Some(&dir))
        .unwrap();
    std::fs::remove_dir_all(&dir).ok();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("\x1b_Ga=t"), "no transmit in output");
    assert!(text.contains("\x1b_Ga=p"), "no placement in output");
    assert!(!text.contains('▀'));
}

#[test]
fn cache_budget_bounds_aggregate_image_memory() {
    let dir = std::env::temp_dir().join("catmd-render-test-budget");
    std::fs::create_dir_all(&dir).unwrap();
    let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
    for i in 0..4 {
        img.save(dir.join(format!("p{i}.png"))).unwrap();
    }
    let mut renderer = kitty_renderer();
    // Room for roughly one tiny PNG: later images must fall back to text.
    renderer.set_cache_budget(100, 8);
    let src = "![a](p0.png)\n\n![b](p1.png)\n\n![c](p2.png)\n\n![d](p3.png)\n";
    let doc = renderer.render_doc(src, WIDTH, 0, Some(&dir));
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        doc.images.len() < 4,
        "budget did not limit caching: {} images",
        doc.images.len()
    );
    // Over-budget images still appear, as markdown text.
    let text = doc.lines.iter().map(|l| strip_ansi(l)).collect::<String>();
    assert!(
        text.contains("p3.png"),
        "over-budget image vanished: {text:?}"
    );
}

#[test]
fn entry_budget_bounds_cache_count() {
    let dir = std::env::temp_dir().join("catmd-render-test-entries");
    std::fs::create_dir_all(&dir).unwrap();
    let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
    for i in 0..3 {
        img.save(dir.join(format!("e{i}.png"))).unwrap();
    }
    let mut renderer = kitty_renderer();
    renderer.set_cache_budget(usize::MAX, 2);
    let doc = renderer.render_doc(
        "![a](e0.png)\n\n![b](e1.png)\n\n![c](e2.png)\n",
        WIDTH,
        0,
        Some(&dir),
    );
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(doc.images.len(), 2);
}

#[test]
fn alias_paths_share_one_cache_entry() {
    let (dir, file) = tiny_png("alias");
    let renderer = kitty_renderer();
    let doc = renderer.render_doc(
        &format!("![a]({file})\n\n![b](./{file})\n"),
        WIDTH,
        0,
        Some(&dir),
    );
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(doc.images.len(), 2);
    assert_eq!(
        doc.images[0].id, doc.images[1].id,
        "aliases were cached twice"
    );
}

#[test]
fn updated_cell_metrics_change_image_layout() {
    let dir = std::env::temp_dir().join("catmd-render-test-cell");
    std::fs::create_dir_all(&dir).unwrap();
    image::RgbaImage::from_pixel(64, 64, image::Rgba([255, 0, 0, 255]))
        .save(dir.join("sq.png"))
        .unwrap();
    let renderer = kitty_renderer();
    let src = "![sq](sq.png)\n";
    let before = renderer.render_doc(src, WIDTH, 0, Some(&dir));
    // Halving the cell height (e.g. a smaller font) must double the rows.
    renderer.set_cell(kitty::CellSize {
        width: 8,
        height: 8,
    });
    let after = renderer.render_doc(src, WIDTH, 0, Some(&dir));
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(before.images[0].rows * 2, after.images[0].rows);
}
