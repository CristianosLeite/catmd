use super::*;

fn lines(raw: &[&str]) -> Vec<String> {
    raw.iter().map(|line| (*line).to_string()).collect()
}

impl Found {
    fn is_line(&self) -> bool {
        matches!(self, Self::Line(_))
    }
}

fn committed(raw: &[&str], query: &str) -> Search {
    let mut search = Search::new();
    search.open();
    for c in query.chars() {
        search.push(c);
    }
    search.commit(&lines(raw), 0);
    search
}

#[test]
fn styling_is_invisible_to_the_query() {
    // "38" appears only inside the color code, never on screen.
    let search = committed(&["\x1b[38;5;114mhello\x1b[0m"], "38");
    assert!(!search.is_active());
    // The visible text still matches.
    let search = committed(&["\x1b[38;5;114mhello\x1b[0m"], "hello");
    assert!(search.is_active());
}

#[test]
fn matching_ignores_case_in_both_directions() {
    assert!(committed(&["The Cargo Manifest"], "cargo").is_active());
    assert!(committed(&["the cargo manifest"], "CARGO").is_active());
}

fn hit(line: usize, start: usize, len: usize) -> Hit {
    Hit { line, start, len }
}

#[test]
fn offsets_skip_escape_sequences() {
    // Styled prefix must not shift the offset of the plain match.
    let hits = scan(&lines(&["\x1b[1mab\x1b[0mcd"]), &['c', 'd']);
    assert_eq!(hits, [hit(0, 2, 2)]);
}

#[test]
fn occurrences_do_not_overlap() {
    let hits = scan(&lines(&["aaaa"]), &['a', 'a']);
    assert_eq!(hits, [hit(0, 0, 2), hit(0, 2, 2)]);
}

#[test]
fn final_sigma_matches_sigma() {
    // Simple folding equates ς and σ through their shared uppercase Σ.
    assert!(committed(&["τέλος"], "ΤΈΛΟΣ").is_active());
    assert!(committed(&["ΤΈΛΟΣ"], "τέλος").is_active());
}

#[test]
fn eszett_matches_double_s() {
    // ß folds to "ss": the match spans all 6 visible chars of "straße".
    let search = committed(&["straße"], "STRASSE");
    assert!(search.is_active());
    assert_eq!(search.hits, [hit(0, 0, 6)]);
    // …and the other way round, "ss" in the haystack matches a ß query.
    assert!(committed(&["PASST"], "ß").is_active());
}

#[test]
fn a_match_inside_a_fold_expansion_covers_the_whole_character() {
    // "s" matches inside ß's "ss" expansion; the single visible ß is
    // highlighted once, not reported as two hits.
    let search = committed(&["aß"], "s");
    assert_eq!(search.hits, [hit(0, 1, 1)]);
}

#[test]
fn hits_are_collected_in_view_order() {
    let hits = scan(&lines(&["one x", "two", "x three x"]), &['x']);
    assert_eq!(
        hits.iter().map(|hit| hit.line).collect::<Vec<_>>(),
        [0, 2, 2]
    );
}

#[test]
fn commit_parks_on_the_first_hit_at_or_below_the_viewport() {
    let mut search = Search::new();
    search.open();
    search.push('x');
    let raw = lines(&["x", "", "", "x", "", "x"]);
    match search.commit(&raw, 3) {
        Found::Line(line) => assert_eq!(line, 3),
        _ => panic!("expected a hit at line 3"),
    }
}

#[test]
fn commit_wraps_to_the_top_when_nothing_lies_below() {
    let mut search = Search::new();
    search.open();
    search.push('x');
    match search.commit(&lines(&["x", "", ""]), 2) {
        Found::Line(line) => assert_eq!(line, 0),
        _ => panic!("expected the search to wrap to line 0"),
    }
}

#[test]
fn a_query_that_matches_nothing_reports_it() {
    let mut search = Search::new();
    search.open();
    search.push('z');
    assert!(matches!(
        search.commit(&lines(&["abc"]), 0),
        Found::Missing(_)
    ));
    assert!(!search.is_active());
    assert!(!search.is_open());
}

#[test]
fn an_empty_query_just_closes_the_prompt() {
    let mut search = Search::new();
    search.open();
    assert!(matches!(search.commit(&lines(&["abc"]), 0), Found::Empty));
    assert!(!search.is_open());
    assert!(!search.is_active());
}

#[test]
fn an_empty_query_keeps_the_previous_search() {
    // Enter on an empty prompt behaves like Esc: the committed search and
    // its highlights survive.
    let mut search = committed(&["x"], "x");
    search.open();
    assert!(matches!(search.commit(&lines(&["x"]), 0), Found::Empty));
    assert!(!search.is_open());
    assert!(search.is_active());
    assert!(search.highlight(0, "x").is_some());
}

#[test]
fn backspace_edits_the_pending_query() {
    let mut search = Search::new();
    search.open();
    search.push('a');
    search.push('z');
    search.backspace();
    assert!(search.commit(&lines(&["abc"]), 0).is_line());
}

#[test]
fn keys_outside_the_prompt_are_ignored() {
    let mut search = Search::new();
    search.push('a');
    search.backspace();
    assert!(!search.is_open());
    assert!(matches!(search.commit(&lines(&["abc"]), 0), Found::Empty));
}

#[test]
fn next_and_previous_wrap_around() {
    let mut search = committed(&["x", "x", "x"], "x");
    assert_eq!(search.advance(true), Some(1));
    assert_eq!(search.advance(true), Some(2));
    assert_eq!(search.advance(true), Some(0));
    assert_eq!(search.advance(false), Some(2));
}

#[test]
fn advancing_without_hits_does_nothing() {
    let mut search = Search::new();
    assert_eq!(search.advance(true), None);
}

#[test]
fn cancelling_the_prompt_keeps_the_previous_search() {
    let mut search = committed(&["x"], "x");
    search.open();
    search.push('z');
    search.clear_input();
    assert!(!search.is_open());
    assert!(search.is_active(), "the committed search must survive");
}

#[test]
fn clearing_drops_the_highlights() {
    let mut search = committed(&["x"], "x");
    search.clear();
    assert!(!search.is_active());
    assert!(search.highlight(0, "x").is_none());
    assert!(search.hint().is_none());
}

#[test]
fn refresh_rescans_after_a_rebuild() {
    let mut search = committed(&["x", "x"], "x");
    search.advance(true);
    // The document re-wrapped: one match left, on a different line.
    search.refresh(&lines(&["", "", "x"]));
    assert_eq!(search.advance(true), Some(2));
    assert!(search.hint().unwrap().contains("1/1"));
}

#[test]
fn current_line_tracks_the_parked_match_through_a_refresh() {
    let mut search = committed(&["x", "", "x"], "x");
    search.advance(true);
    assert_eq!(search.current_line(), Some(2));
    // Re-wrapping pushed lines down; the parked (second) match moved with it.
    search.refresh(&lines(&["", "x", "", "", "x"]));
    assert_eq!(search.current_line(), Some(4));
    assert_eq!(Search::new().current_line(), None);
}

#[test]
fn the_hint_counts_the_parked_match() {
    let mut search = committed(&["x", "x", "x"], "x");
    assert!(search.hint().unwrap().contains("/x 1/3"));
    search.advance(true);
    assert!(search.hint().unwrap().contains("/x 2/3"));
}

#[test]
fn the_prompt_shows_what_is_typed() {
    let mut search = Search::new();
    assert!(search.prompt().is_none());
    search.open();
    search.push('a');
    assert!(search.prompt().unwrap().contains("/a"));
}

#[test]
fn highlighting_marks_matches_and_leaves_other_lines_alone() {
    let search = committed(&["find me", "nothing here"], "me");
    let painted = search.highlight(0, "find me").unwrap();
    assert!(painted.starts_with("find "), "painted: {painted:?}");
    assert!(painted.contains(CURRENT_STYLE));
    assert!(painted.ends_with(RESET));
    assert!(search.highlight(1, "nothing here").is_none());
}

#[test]
fn only_the_parked_match_gets_the_current_style() {
    let mut search = committed(&["x y x"], "x");
    let painted = search.highlight(0, "x y x").unwrap();
    assert!(painted.starts_with(CURRENT_STYLE));
    assert!(painted.contains(HIT_STYLE));
    // Moving on swaps which occurrence is emphasized.
    search.advance(true);
    let painted = search.highlight(0, "x y x").unwrap();
    assert!(painted.starts_with(HIT_STYLE));
}

#[test]
fn highlighting_restores_the_lines_own_style() {
    // The green run continues after the match, so the reset that closes the
    // highlight has to re-assert it.
    let search = committed(&["\x1b[32mabc\x1b[0m"], "a");
    let painted = search.highlight(0, "\x1b[32mabc\x1b[0m").unwrap();
    assert_eq!(
        painted,
        format!("\x1b[32m{CURRENT_STYLE}a{RESET}\x1b[32mbc\x1b[0m")
    );
}

#[test]
fn styling_inside_a_match_does_not_overpaint_the_highlight() {
    // A color change in the middle of the match must be followed by the
    // highlight again, or the rest of the match would lose it.
    let search = committed(&["a\x1b[31mb"], "ab");
    let painted = search.highlight(0, "a\x1b[31mb").unwrap();
    assert_eq!(
        painted,
        format!("{CURRENT_STYLE}a\x1b[31m{CURRENT_STYLE}b{RESET}\x1b[31m")
    );
}

#[test]
fn a_match_at_the_end_of_a_line_closes_its_style() {
    let search = committed(&["ab"], "b");
    let painted = search.highlight(0, "ab").unwrap();
    assert!(painted.ends_with(RESET), "painted: {painted:?}");
}
