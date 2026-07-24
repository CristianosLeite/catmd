use super::*;

fn doc(title: &str, source: &str) -> Doc {
    Doc {
        title: title.to_string(),
        source: source.to_string(),
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
    let status = status_line(&view, &docs, 0, 20, 80);
    assert!(status.contains("0-0/0"), "status was: {status}");
}

#[test]
fn status_line_keeps_hints_when_title_is_long() {
    let renderer = Renderer::new();
    let docs = [doc(&"x".repeat(200), "hello\n")];
    let view = build(&renderer, &docs, 80);
    let status = status_line(&view, &docs, 0, 20, 80);
    assert!(status.chars().count() <= 80);
    assert!(status.contains("q quit"));
    assert!(status.contains('…'));
}
