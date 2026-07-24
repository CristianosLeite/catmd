//! End-to-end tests running the real catmd binary. Stdout is piped in tests,
//! so catmd always takes the plain (non-interactive) rendering path.

use std::io::{Read, Write};
use std::process::{Command, Output, Stdio};

fn catmd(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_catmd"))
        .args(args)
        .output()
        .expect("failed to run catmd")
}

/// Creates a unique temp Markdown file; returns its path.
fn temp_md(name: &str, content: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("catmd-test-{}-{name}.md", std::process::id()));
    std::fs::write(&path, content).expect("write temp file");
    path
}

fn stdout_str(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn help_prints_usage_and_succeeds() {
    let output = catmd(&["--help"]);
    assert!(output.status.success());
    assert!(stdout_str(&output).contains("Usage: catmd"));
}

#[test]
fn version_prints_package_version() {
    let output = catmd(&["--version"]);
    assert!(output.status.success());
    assert_eq!(
        stdout_str(&output).trim(),
        format!("catmd {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn renders_a_markdown_file() {
    let path = temp_md("render", "# Big Title\n\nsome **bold** text\n");
    let output = catmd(&[path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(stdout.contains("Big Title"));
    assert!(stdout.contains("bold"));
    assert!(stdout.contains('\x1b'), "output should be ANSI-styled");
}

#[test]
fn code_blocks_are_highlighted() {
    let path = temp_md("code", "```rust\nfn main() {}\n```\n");
    let output = catmd(&[path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    // Syntect emits 24-bit color sequences for highlighted code.
    assert!(stdout_str(&output).contains("\x1b[38;2;"));
}

#[test]
fn reads_stdin_when_no_files_given() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_catmd"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn catmd");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"# From Stdin\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(stdout_str(&output).contains("From Stdin"));
}

#[test]
fn repeated_dash_reads_stdin_once() {
    let run = |args: &[&str]| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_catmd"))
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn catmd");
        child.stdin.take().unwrap().write_all(b"# Once\n").unwrap();
        child.wait_with_output().unwrap()
    };
    let twice = run(&["-", "-"]);
    let once = run(&["-"]);
    assert!(twice.status.success());
    // The second "-" is skipped: no duplicate content, no empty extra doc.
    assert_eq!(stdout_str(&twice), stdout_str(&once));
}

#[test]
fn vanished_pipe_reader_is_a_quiet_success() {
    // `catmd big.md | head`-style: the reader closes early. SIGPIPE must not
    // kill the process (that would skip destructors) and the EPIPE must not
    // panic or be reported as an error.
    let big = format!("```text\n{}```\n", "line of filler text\n".repeat(20_000));
    let path = temp_md("bigpipe", &big);
    let mut child = Command::new(env!("CARGO_BIN_EXE_catmd"))
        .arg(path.to_str().unwrap())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn catmd");
    // Read a little, then drop the pipe so further writes hit EPIPE.
    let mut stdout = child.stdout.take().unwrap();
    let mut buf = [0u8; 1024];
    let _ = stdout.read(&mut buf).unwrap();
    drop(stdout);
    let status = child.wait().unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(status.success(), "exit was {status:?}, stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr not quiet: {stderr}");
}

#[test]
fn renders_multiple_files_in_order() {
    let first = temp_md("multi1", "# First\n");
    let second = temp_md("multi2", "# Second\n");
    let output = catmd(&[first.to_str().unwrap(), second.to_str().unwrap()]);
    let _ = std::fs::remove_file(&first);
    let _ = std::fs::remove_file(&second);
    let stdout = stdout_str(&output);
    let first_pos = stdout.find("First").expect("First rendered");
    let second_pos = stdout.find("Second").expect("Second rendered");
    assert!(first_pos < second_pos);
}

#[test]
fn missing_file_fails_but_others_still_render() {
    let path = temp_md("partial", "# Still Here\n");
    let output = catmd(&["definitely-missing.md", path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("definitely-missing.md"));
    assert!(stdout_str(&output).contains("Still Here"));
}

#[test]
fn unknown_flag_fails_with_usage() {
    let output = catmd(&["--bogus"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unrecognized option"));
}

#[test]
fn plain_flag_matches_piped_output() {
    let path = temp_md("plain", "# T\n\n```sh\nls\n```\n");
    let piped = catmd(&[path.to_str().unwrap()]);
    let plain = catmd(&["--plain", path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(stdout_str(&piped), stdout_str(&plain));
}

#[test]
fn document_escape_sequences_are_not_emitted() {
    let path = temp_md(
        "inject",
        "before \x1b[2J\n\n```text\nSAFE\x1b[2JINJECTED\n\x1b]52;c;evil\x07osc\n```\n",
    );
    let output = catmd(&["--plain", path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    let stdout = stdout_str(&output);
    assert!(!stdout.contains("\x1b[2J"), "CSI injection reached stdout");
    assert!(!stdout.contains("\x1b]52"), "OSC injection reached stdout");
    assert!(stdout.contains("SAFE"));
    assert!(stdout.contains("INJECTED"));
}

#[test]
fn piped_output_keeps_images_as_markdown_text() {
    // Stdout is piped here, so no graphics protocol may appear even for a
    // loadable image; the image line renders as ordinary markdown.
    let dir = std::env::temp_dir().join(format!("catmd-test-img-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let mut img = image::RgbaImage::new(2, 2);
    for px in img.pixels_mut() {
        *px = image::Rgba([0, 255, 0, 255]);
    }
    img.save(dir.join("pic.png")).expect("write png");
    let md = dir.join("doc.md");
    std::fs::write(&md, "# T\n\n![sample](pic.png)\n").expect("write md");
    let output = catmd(&[md.to_str().unwrap()]);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(
        !stdout.contains("\x1b_G"),
        "graphics escapes in piped output"
    );
    assert!(stdout.contains("sample"), "image alt text missing");
}

#[test]
fn missing_image_falls_back_to_text_and_succeeds() {
    let path = temp_md("noimg", "![alt text](does-not-exist.png)\n");
    let output = catmd(&[path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(stdout.contains("alt text"), "fallback text missing");
}
