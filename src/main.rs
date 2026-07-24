// The clipboard holder relies on Linux-specific APIs (arboard::SetExtLinux,
// setsid/fork via libc). Fail at compile time with a clear message rather
// than half-working on other platforms.
#[cfg(not(target_os = "linux"))]
compile_error!("catmd currently supports Linux only; see README.md (Platform support)");

mod cli;
mod clipboard;
mod render;
mod segment;
mod text;
mod viewer;

use std::io::{IsTerminal, Read};
use std::process::ExitCode;

/// A Markdown document ready to render: its display title and source text.
pub struct Doc {
    pub title: String,
    pub source: String,
}

fn read_stdin() -> std::io::Result<String> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    Ok(buf)
}

/// Reads every requested file ("-" meaning stdin), reporting errors like cat:
/// failures go to stderr, the remaining files are still rendered. Only the
/// first "-" reads stdin; repeats are skipped — rereading would hit EOF and
/// render a confusing empty "stdin" document.
fn read_docs(files: &[String]) -> (Vec<Doc>, bool) {
    let mut docs = Vec::new();
    let mut failed = false;
    let mut stdin_used = false;
    for path in files {
        let (title, source) = if path == "-" {
            if stdin_used {
                continue;
            }
            stdin_used = true;
            ("stdin".to_string(), read_stdin())
        } else {
            (path.clone(), std::fs::read_to_string(path))
        };
        match source {
            Ok(source) => docs.push(Doc { title, source }),
            Err(err) => {
                eprintln!("catmd: {path}: {err}");
                failed = true;
            }
        }
    }
    (docs, failed)
}

fn terminal_width() -> usize {
    termimad::crossterm::terminal::size().map_or(80, |(w, _)| w as usize)
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some(clipboard::HOLD_FLAG) {
        return clipboard::hold_clipboard();
    }

    let options = match cli::parse(std::env::args().skip(1)) {
        Ok(cli::Command::Help) => {
            print!("{}", cli::USAGE);
            return ExitCode::SUCCESS;
        }
        Ok(cli::Command::Version) => {
            println!("catmd {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Ok(cli::Command::Run(options)) => options,
        Err(message) => {
            eprintln!("catmd: {message}");
            eprint!("{}", cli::USAGE);
            return ExitCode::FAILURE;
        }
    };

    let (docs, mut failed) = read_docs(&options.files);
    if docs.is_empty() {
        return ExitCode::FAILURE;
    }

    let renderer = render::Renderer::new();
    if std::io::stdout().is_terminal() && !options.plain {
        if let Err(err) = viewer::run(&renderer, &docs) {
            eprintln!("catmd: {err}");
            failed = true;
        }
    } else {
        // SIGPIPE stays ignored (Rust's default) so a pipe write failure is
        // an EPIPE error, never a signal death — signal death would skip
        // destructors like the viewer's TermGuard and could leave the
        // terminal in raw mode.
        let width = terminal_width();
        let mut out = std::io::stdout().lock();
        for doc in &docs {
            if let Err(err) = renderer.render_plain(&mut out, &doc.source, width) {
                if err.kind() == std::io::ErrorKind::BrokenPipe {
                    // The reader went away (`catmd file.md | head`): quit
                    // quietly, like cat.
                    break;
                }
                eprintln!("catmd: {err}");
                failed = true;
                break;
            }
        }
    }

    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
