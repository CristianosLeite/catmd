use super::*;

fn doc(title: &str, source: &str) -> Doc {
    Doc {
        title: title.to_string(),
        source: source.to_string(),
        base_dir: None,
    }
}

#[test]
fn build_offsets_buttons_across_documents() {
    let renderer = Renderer::new();
    let docs = [
        doc("a.md", "```sh\nls\n```\n"),
        doc("b.md", "```py\nx\n```\n"),
    ];
    let view = build(&renderer, &docs, 60);
    assert_eq!(view.buttons.len(), 2);
    for button in &view.buttons {
        let header = &view.lines[button.line];
        assert!(
            header.contains("[ copy ]"),
            "button.line points at: {header}"
        );
    }
    // Numbering continues across documents.
    assert!(view.lines[view.buttons[1].line].contains("[2]"));
    // Separators appear for multiple documents.
    assert!(view.lines.iter().any(|l| l.contains("═══ a.md ═══")));
}

#[test]
fn single_document_has_no_separator() {
    let renderer = Renderer::new();
    let docs = [doc("only.md", "hello\n")];
    let view = build(&renderer, &docs, 60);
    assert!(!view.lines.iter().any(|l| l.contains("═══")));
}

fn assert_copies(pick: Pick, expected_index: usize) {
    match pick {
        Pick::Copy(index) => assert_eq!(index, expected_index),
        Pick::Pending(msg) => panic!("expected copy, got pending: {msg}"),
        Pick::Invalid(msg) => panic!("expected copy, got invalid: {msg}"),
    }
}

#[test]
fn single_digit_copies_immediately_when_unambiguous() {
    // 5 blocks: every single digit 1-5 is final ("50" cannot exist).
    let mut picker = BlockPicker::new();
    assert_copies(picker.push_digit('3', 5), 2);
    assert!(picker.is_empty());
}

#[test]
fn ambiguous_digit_waits_for_more_input() {
    // 12 blocks: "1" could be 1, 10, 11, or 12.
    let mut picker = BlockPicker::new();
    assert!(matches!(picker.push_digit('1', 12), Pick::Pending(_)));
    assert!(!picker.is_empty());
    // "12" is unambiguous — copies block 12 (index 11).
    assert_copies(picker.push_digit('2', 12), 11);
}

#[test]
fn enter_confirms_an_ambiguous_prefix() {
    let mut picker = BlockPicker::new();
    assert!(matches!(picker.push_digit('1', 12), Pick::Pending(_)));
    assert_eq!(picker.confirm(), Some(0));
    assert!(picker.is_empty());
}

#[test]
fn unambiguous_digit_fires_even_with_many_blocks() {
    // 12 blocks: "3" cannot extend (30 > 12), so it fires at once.
    let mut picker = BlockPicker::new();
    assert_copies(picker.push_digit('3', 12), 2);
}

#[test]
fn out_of_range_numbers_are_rejected() {
    let mut picker = BlockPicker::new();
    assert!(matches!(picker.push_digit('7', 3), Pick::Invalid(_)));
    assert!(picker.is_empty());
    // "13" with 12 blocks: '1' pends, '3' makes it invalid.
    assert!(matches!(picker.push_digit('1', 12), Pick::Pending(_)));
    assert!(matches!(picker.push_digit('3', 12), Pick::Invalid(_)));
    assert!(picker.is_empty());
}

#[test]
fn zero_is_not_a_block() {
    let mut picker = BlockPicker::new();
    assert!(matches!(picker.push_digit('0', 12), Pick::Invalid(_)));
    assert_eq!(picker.confirm(), None);
}

#[test]
fn status_line_of_an_empty_document_reads_zero_zero() {
    let renderer = Renderer::new();
    let docs = [doc("empty.md", "")];
    let view = build(&renderer, &docs, 80);
    let status = status_line(&view, &docs, 0, 20, 80, &Search::new());
    assert!(status.contains("0-0/0"), "status was: {status}");
}

#[test]
fn status_line_keeps_hints_when_title_is_long() {
    let renderer = Renderer::new();
    let docs = [doc(&"x".repeat(200), "hello\n")];
    let view = build(&renderer, &docs, 80);
    let status = status_line(&view, &docs, 0, 20, 80, &Search::new());
    assert!(status.chars().count() <= 80);
    assert!(status.contains("q quit"));
    assert!(status.contains('…'));
}

#[test]
fn status_line_mentions_shift_drag_selection() {
    let renderer = Renderer::new();
    let docs = [doc("a.md", "hello\n")];
    let view = build(&renderer, &docs, 80);
    let status = status_line(&view, &docs, 0, 20, 80, &Search::new());
    assert!(status.contains("shift+drag select"), "status was: {status}");
}

#[test]
fn status_line_advertises_search() {
    let renderer = Renderer::new();
    let docs = [doc("a.md", "hello\n")];
    let view = build(&renderer, &docs, 80);
    let status = status_line(&view, &docs, 0, 20, 80, &Search::new());
    assert!(status.contains("^F find"), "status was: {status}");
}

#[test]
fn a_running_search_replaces_the_copy_hint() {
    let renderer = Renderer::new();
    let docs = [doc("a.md", "```sh\nls\n```\n")];
    let view = build(&renderer, &docs, 80);
    let mut search = Search::new();
    search.open();
    search.push('l');
    search.push('s');
    search.commit(&view.lines, 0);
    let status = status_line(&view, &docs, 0, 20, 80, &search);
    assert!(status.contains("/ls 1/"), "status was: {status}");
    assert!(!status.contains("[ copy ]"), "status was: {status}");
}

#[test]
fn narrow_terminals_drop_hints_instead_of_the_quit_key() {
    let renderer = Renderer::new();
    let docs = [doc("a.md", "```sh\nls\n```\n")];
    let view = build(&renderer, &docs, 40);
    let status = status_line(&view, &docs, 0, 20, 40, &Search::new());
    assert!(status.chars().count() <= 40, "status was: {status}");
    assert!(status.contains("q quit"), "status was: {status}");
    assert!(!status.contains("shift+drag"), "status was: {status}");
}

#[test]
fn a_resize_keeps_the_parked_match_in_the_viewport() {
    // A match near the bottom of a tall document…
    let mut before: Vec<String> = vec![String::new(); 100];
    before.push("needle".to_string());
    let mut search = Search::new();
    search.open();
    for c in "needle".chars() {
        search.push(c);
    }
    search.commit(&before, 0);
    let content_height = 20;
    let scroll = scroll_to(search.current_line().unwrap(), content_height);
    assert!((scroll..scroll + content_height).contains(&100));

    // …moves further down when narrower wrapping inserts lines before it.
    let mut after: Vec<String> = vec![String::new(); 150];
    after.push("needle".to_string());
    search.refresh(&after);
    let line = search.current_line().unwrap();
    assert_eq!(line, 150);
    // The event loop re-follows it: the new scroll shows the new line, which
    // the stale offset (still at ~94) would not.
    assert!(!(scroll..scroll + content_height).contains(&line));
    let scroll = scroll_to(line, content_height);
    assert!((scroll..scroll + content_height).contains(&line));
}

#[test]
fn scroll_to_keeps_context_above_a_match() {
    // A third of the viewport stays above the match…
    assert_eq!(scroll_to(30, 30), 20);
    // …but matches near the top do not scroll past the start.
    assert_eq!(scroll_to(3, 30), 0);
}

fn span(line: usize, rows: u16, img_h: u32) -> ImageSpan {
    ImageSpan {
        line,
        cols: 10,
        rows,
        img_h,
        id: 1,
        png: std::rc::Rc::new(Vec::new()),
    }
}

#[test]
fn fully_visible_image_needs_no_crop() {
    let (row, rows, crop) = visible_slice(&span(5, 10, 200), 0, 30).unwrap();
    assert_eq!((row, rows), (5, 10));
    assert!(crop.is_none());
}

#[test]
fn image_scrolled_past_the_top_is_cropped_from_above() {
    // Span at lines 5..15, scrolled to 8: rows 8..15 visible (7 rows).
    let (row, rows, crop) = visible_slice(&span(5, 10, 200), 8, 30).unwrap();
    assert_eq!(row, 0);
    assert_eq!(rows, 7);
    let crop = crop.unwrap();
    // 3 of 10 rows hidden -> 60 of 200 px skipped, 140 px shown.
    assert_eq!(crop.y_px, 60);
    assert_eq!(crop.h_px, 140);
}

#[test]
fn image_reaching_below_the_viewport_is_cropped_from_below() {
    // Span at lines 20..30, viewport shows lines 0..25: rows 20..25 visible.
    let (row, rows, crop) = visible_slice(&span(20, 10, 200), 0, 25).unwrap();
    assert_eq!(row, 20);
    assert_eq!(rows, 5);
    let crop = crop.unwrap();
    assert_eq!(crop.y_px, 0);
    assert_eq!(crop.h_px, 100);
}

#[test]
fn offscreen_images_are_skipped() {
    assert!(visible_slice(&span(50, 10, 200), 0, 30).is_none());
    assert!(visible_slice(&span(0, 10, 200), 20, 30).is_none());
}

#[test]
fn build_offsets_image_spans_across_documents() {
    let dir = std::env::temp_dir().join("catmd-viewer-test-images");
    std::fs::create_dir_all(&dir).unwrap();
    let mut img = image::RgbaImage::new(2, 2);
    for px in img.pixels_mut() {
        *px = image::Rgba([255, 0, 0, 255]);
    }
    img.save(dir.join("p.png")).unwrap();
    let renderer = Renderer::with_images(
        crate::image::Mode::Kitty { tmux: false },
        kitty::CellSize {
            width: 8,
            height: 16,
        },
    );
    let docs = [
        Doc {
            title: "a.md".to_string(),
            source: "![x](p.png)\n".to_string(),
            base_dir: Some(dir.clone()),
        },
        Doc {
            title: "b.md".to_string(),
            source: "![y](p.png)\n".to_string(),
            base_dir: Some(dir.clone()),
        },
    ];
    let view = build(&renderer, &docs, 60);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(view.images.len(), 2);
    assert!(view.images[1].line > view.images[0].line);
    // Both spans point at empty placeholder rows in view coordinates.
    for image in &view.images {
        assert_eq!(view.lines[image.line], "");
    }
    // The same file is cached once: both spans share one id and payload.
    assert_eq!(view.images[0].id, view.images[1].id);
}
