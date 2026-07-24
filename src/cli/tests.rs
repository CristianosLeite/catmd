use super::*;

fn parse_strs(args: &[&str]) -> Result<Command, String> {
    parse(args.iter().map(ToString::to_string))
}

fn options(result: Result<Command, String>) -> Options {
    match result.expect("parse should succeed") {
        Command::Run(options) => options,
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn no_args_defaults_to_stdin() {
    assert_eq!(options(parse_strs(&[])).files, ["-"]);
}

#[test]
fn files_are_collected_in_order() {
    let opts = options(parse_strs(&["a.md", "b.md"]));
    assert_eq!(opts.files, ["a.md", "b.md"]);
    assert!(!opts.plain);
}

#[test]
fn dash_is_a_file_meaning_stdin() {
    assert_eq!(options(parse_strs(&["a.md", "-"])).files, ["a.md", "-"]);
}

#[test]
fn plain_flag_both_forms() {
    assert!(options(parse_strs(&["-p", "a.md"])).plain);
    assert!(options(parse_strs(&["a.md", "--plain"])).plain);
}

#[test]
fn help_short_circuits() {
    assert_eq!(parse_strs(&["--help", "a.md"]), Ok(Command::Help));
    assert_eq!(parse_strs(&["-h"]), Ok(Command::Help));
}

#[test]
fn version_flag_both_forms() {
    assert_eq!(parse_strs(&["-V", "a.md"]), Ok(Command::Version));
    assert_eq!(parse_strs(&["--version"]), Ok(Command::Version));
    // Lowercase -v is not version (reserved; matches cat, where -v differs).
    assert!(parse_strs(&["-v"]).is_err());
}

#[test]
fn unknown_flag_is_an_error() {
    assert!(parse_strs(&["--bogus"]).is_err());
    assert!(parse_strs(&["-x"]).is_err());
}

#[test]
fn double_dash_ends_option_parsing() {
    let opts = options(parse_strs(&["--", "-notes.md"]));
    assert_eq!(opts.files, ["-notes.md"]);
    // Even flag-looking arguments become filenames after "--".
    let opts = options(parse_strs(&["-p", "--", "--plain", "-h"]));
    assert!(opts.plain);
    assert_eq!(opts.files, ["--plain", "-h"]);
}

#[test]
fn double_dash_alone_still_defaults_to_stdin() {
    assert_eq!(options(parse_strs(&["--"])).files, ["-"]);
}
