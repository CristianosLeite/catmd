use super::*;

/// Renders segments to a compact notation for assertions.
fn outline(src: &str) -> Vec<String> {
    split_segments(src)
        .iter()
        .map(|segment| match segment {
            Segment::Markdown(text) => format!("md:{}", text.escape_debug()),
            Segment::Code { lang, body } => {
                format!("code({lang}):{}", body.escape_debug())
            }
            Segment::Image { alt, dest, raw } => {
                format!("img({alt}|{dest}):{}", raw.escape_debug())
            }
        })
        .collect()
}

#[test]
fn markdown_only() {
    assert_eq!(outline("# Title\ntext\n"), ["md:# Title\\ntext\\n"]);
}

#[test]
fn code_block_is_extracted_with_language() {
    let src = "before\n```rust\nlet x = 1;\n```\nafter\n";
    assert_eq!(
        outline(src),
        ["md:before\\n", "code(rust):let x = 1;\\n", "md:after\\n"]
    );
}

#[test]
fn tilde_fence_works() {
    assert_eq!(outline("~~~sh\nls\n~~~\n"), ["code(sh):ls\\n"]);
}

#[test]
fn only_first_word_of_info_string_is_the_language() {
    assert_eq!(outline("```rust ignore\nx\n```\n"), ["code(rust):x\\n"]);
}

#[test]
fn closing_fence_may_be_longer_but_not_shorter() {
    // A 4-backtick fence is not closed by 3 backticks.
    let src = "````\ninner\n```\n````\n";
    assert_eq!(outline(src), ["code():inner\\n```\\n"]);
    // But a longer closer does close a shorter opener.
    assert_eq!(outline("```\nx\n````\n"), ["code():x\\n"]);
}

#[test]
fn fence_indented_up_to_three_spaces() {
    assert_eq!(outline("   ```\nx\n   ```\n"), ["code():x\\n"]);
    // Four spaces of indent is an indented code block, not a fence.
    assert_eq!(outline("    ```\n"), ["md:    ```\\n"]);
}

#[test]
fn backtick_info_string_may_not_contain_backticks() {
    assert_eq!(outline("``` a`b\ntext\n"), ["md:``` a`b\\ntext\\n"]);
}

#[test]
fn tilde_close_does_not_end_backtick_block() {
    assert_eq!(outline("```\n~~~\n```\n"), ["code():~~~\\n"]);
}

#[test]
fn closing_fence_must_have_no_info_string() {
    assert_eq!(
        outline("```\nx\n``` rust\n```\n"),
        ["code():x\\n``` rust\\n"]
    );
}

#[test]
fn unclosed_fence_runs_to_eof() {
    assert_eq!(
        outline("text\n```py\na = 1\n"),
        ["md:text\\n", "code(py):a = 1\\n"]
    );
}

#[test]
fn empty_input_yields_no_segments() {
    assert!(split_segments("").is_empty());
}

#[test]
fn crlf_line_endings_are_preserved_in_code_bodies() {
    assert_eq!(outline("```\r\nx\r\n```\r\n"), ["code():x\\r\\n"]);
}

#[test]
fn missing_final_newline_is_preserved_in_unclosed_block() {
    assert_eq!(outline("```\nx"), ["code():x"]);
}

#[test]
fn markdown_display_text_is_lf_normalized() {
    // Markdown segments are display-only, so CRLF may be normalized there.
    assert_eq!(outline("a\r\nb\r\n"), ["md:a\\nb\\n"]);
}

#[test]
fn fence_inside_a_block_quote_is_recognized() {
    let src = "> ```rust\n> let x = 1;\n> ```\n";
    assert_eq!(outline(src), ["code(rust):let x = 1;\\n"]);
}

#[test]
fn fence_inside_a_nested_block_quote() {
    let src = "> > ```\n> > x\n> > ```\n";
    assert_eq!(outline(src), ["code():x\\n"]);
}

#[test]
fn quote_block_ending_closes_the_fence() {
    // The block quote ends without a closing fence; the code block ends with
    // it and the following line is ordinary markdown again.
    let src = "> ```\n> x\nafter\n";
    assert_eq!(outline(src), ["code():x\\n", "md:after\\n"]);
}

#[test]
fn quoted_fence_between_prose_keeps_surrounding_markdown() {
    let src = "> quote\n> ```sh\n> ls\n> ```\n> more\n";
    assert_eq!(
        outline(src),
        ["md:> quote\\n", "code(sh):ls\\n", "md:> more\\n"]
    );
}

#[test]
fn opener_indentation_is_stripped_from_content() {
    // CommonMark: content lines lose up to the opener's indentation (2 here);
    // deeper indentation keeps the difference, shallower is unharmed.
    let src = "  ```\n  a\n    b\n a\n  ```\n";
    assert_eq!(outline(src), ["code():a\\n  b\\na\\n"]);
}

#[test]
fn list_item_fence_with_two_space_indent_gets_a_block() {
    let src = "- item\n  ```py\n  x = 1\n  ```\n";
    assert_eq!(outline(src), ["md:- item\\n", "code(py):x = 1\\n"]);
}

#[test]
fn standalone_image_line_is_extracted() {
    let src = "before\n![alt text](assets/pic.png)\nafter\n";
    assert_eq!(
        outline(src),
        [
            "md:before\\n",
            "img(alt text|assets/pic.png):![alt text](assets/pic.png)",
            "md:after\\n"
        ]
    );
}

#[test]
fn image_title_and_angle_brackets_are_accepted() {
    assert_eq!(
        outline("![a](pic.png \"title\")\n"),
        ["img(a|pic.png):![a](pic.png \\\"title\\\")"]
    );
    assert_eq!(
        outline("![a](<my pic.png>)\n"),
        ["img(a|my pic.png):![a](<my pic.png>)"]
    );
}

#[test]
fn image_alt_may_contain_nested_brackets() {
    assert_eq!(
        outline("![a [b] c](pic.png)\n"),
        ["img(a [b] c|pic.png):![a [b] c](pic.png)"]
    );
}

#[test]
fn remote_image_stays_markdown() {
    let src = "![badge](https://example.com/b.svg)\n";
    assert_eq!(outline(src), ["md:![badge](https://example.com/b.svg)\\n"]);
}

#[test]
fn mid_sentence_image_stays_markdown() {
    let src = "see ![icon](i.png) here\n";
    assert_eq!(outline(src), ["md:see ![icon](i.png) here\\n"]);
}

#[test]
fn reference_style_image_stays_markdown() {
    assert_eq!(outline("![alt][ref]\n"), ["md:![alt][ref]\\n"]);
}

#[test]
fn indented_image_line_is_an_indented_code_block() {
    assert_eq!(outline("    ![a](p.png)\n"), ["md:    ![a](p.png)\\n"]);
    // Up to three spaces is still an image.
    assert_eq!(outline("   ![a](p.png)\n"), ["img(a|p.png):   ![a](p.png)"]);
}

#[test]
fn image_syntax_inside_a_fence_stays_code() {
    let src = "```\n![a](p.png)\n```\n";
    assert_eq!(outline(src), ["code():![a](p.png)\\n"]);
}

#[test]
fn quoted_image_line_stays_markdown() {
    assert_eq!(outline("> ![a](p.png)\n"), ["md:> ![a](p.png)\\n"]);
}

#[test]
fn backslash_escapes_in_the_destination_are_resolved() {
    assert_eq!(
        outline("![d](a\\(b\\).png)\n"),
        ["img(d|a(b).png):![d](a\\\\(b\\\\).png)"]
    );
}

#[test]
fn percent_encoded_destinations_are_decoded() {
    assert_eq!(
        outline("![d](my%20pic.png)\n"),
        ["img(d|my pic.png):![d](my%20pic.png)"]
    );
}

#[test]
fn invalid_percent_sequences_stay_literal() {
    assert_eq!(outline("![d](50%.png)\n"), ["img(d|50%.png):![d](50%.png)"]);
    assert_eq!(
        outline("![d](a%zz.png)\n"),
        ["img(d|a%zz.png):![d](a%zz.png)"]
    );
}

#[test]
fn percent_encoded_remote_destination_is_still_rejected() {
    // http%3A%2F%2F decodes to http:// — the remote check runs on the
    // decoded destination so encoding cannot smuggle a URL past it.
    let src = "![b](http%3A%2F%2Fexample.com/x.png)\n";
    assert_eq!(outline(src), ["md:![b](http%3A%2F%2Fexample.com/x.png)\\n"]);
}

#[test]
fn escaped_brackets_in_alt_text_still_form_an_image() {
    assert_eq!(
        outline("![a\\]b](pic.png)\n"),
        ["img(a]b|pic.png):![a\\\\]b](pic.png)"]
    );
    assert_eq!(
        outline("![\\[x\\]](pic.png)\n"),
        ["img([x]|pic.png):![\\\\[x\\\\]](pic.png)"]
    );
}

#[test]
fn backslash_runs_in_alt_text_are_counted_correctly() {
    // Even run: `\\` is an escaped backslash (unescaped to one in alt),
    // and the `]` closes the alt.
    assert_eq!(
        outline("![a\\\\](pic.png)\n"),
        ["img(a\\|pic.png):![a\\\\\\\\](pic.png)"]
    );
    // Nested brackets containing escapes still balance: the escaped `]`
    // does not close the nested `[`, the following `]` does.
    assert_eq!(
        outline("![a [b\\]] c](pic.png)\n"),
        ["img(a [b]] c|pic.png):![a [b\\\\]] c](pic.png)"]
    );
}
