use super::*;

#[test]
fn sanitize_strips_csi_sequences() {
    assert_eq!(sanitize("SAFE\x1b[2JINJECTED"), "SAFE[2JINJECTED");
}

#[test]
fn sanitize_strips_osc_sequences() {
    // OSC 52 (clipboard write) — ESC and BEL are control characters.
    assert_eq!(sanitize("\x1b]52;c;payload\x07x"), "]52;c;payloadx");
}

#[test]
fn sanitize_strips_c1_controls_and_cr() {
    assert_eq!(sanitize("a\u{9b}2Jb"), "a2Jb"); // U+009B is a one-char CSI
    assert_eq!(sanitize("a\rb"), "ab");
}

#[test]
fn sanitize_expands_tabs_and_keeps_newlines() {
    assert_eq!(sanitize("a\tb\nc"), "a    b\nc");
}

#[test]
fn sanitize_keeps_plain_unicode() {
    assert_eq!(sanitize("héllo 日本 🎉"), "héllo 日本 🎉");
}

#[test]
fn sanitize_strips_bidi_controls() {
    // RLO (U+202E) reverses rendering order — the Trojan-source primitive.
    assert_eq!(sanitize("a\u{202E}cba\u{202C}d"), "acbad");
    // Isolates (U+2066-2069) and the Arabic letter mark are stripped too.
    assert_eq!(sanitize("x\u{2066}y\u{2069}\u{061C}z"), "xyz");
}

#[test]
fn has_hidden_chars_flags_display_copy_divergence() {
    assert!(!has_hidden_chars("plain text\nwith lines\n"));
    assert!(!has_hidden_chars("tabs\tand crlf\r\nare visible"));
    assert!(has_hidden_chars("esc \x1b[2J"));
    assert!(has_hidden_chars("lone cr\rhere"));
    assert!(has_hidden_chars("bidi \u{202E}evil"));
    assert!(has_hidden_chars("zero\u{200B}width"));
    // ZWJ is legitimate (emoji sequences, several scripts) — no warning.
    assert!(!has_hidden_chars("family \u{1F468}\u{200D}\u{1F469}"));
}

#[test]
fn truncate_styled_counts_only_visible_columns() {
    let styled = "\x1b[38;2;1;2;3mabc\x1b[0mdef";
    assert_eq!(truncate_styled(styled, 4), "\x1b[38;2;1;2;3mabc\x1b[0md");
    // Never cuts inside an escape sequence, and a full fit is unchanged.
    assert_eq!(truncate_styled(styled, 6), styled);
    assert_eq!(truncate_styled(styled, 0), "\x1b[38;2;1;2;3m");
}

#[test]
fn truncate_styled_respects_wide_chars() {
    let styled = "\x1b[31m日本\x1b[0m";
    assert_eq!(truncate_styled(styled, 3), "\x1b[31m日");
}

#[test]
fn display_width_counts_wide_chars_as_two() {
    assert_eq!(display_width("abc"), 3);
    assert_eq!(display_width("日本"), 4);
    // A combining mark occupies no column of its own.
    assert_eq!(display_width("e\u{0301}"), 1);
}

#[test]
fn truncate_display_respects_columns_not_chars() {
    assert_eq!(truncate_display("abcdef", 3), "abc");
    // "日" is 2 columns; 3 columns cannot fit half of "本".
    assert_eq!(truncate_display("日本", 3), "日");
    assert_eq!(truncate_display("日本", 4), "日本");
}

#[test]
fn truncate_display_keeps_short_text() {
    assert_eq!(truncate_display("ok", 10), "ok");
}
