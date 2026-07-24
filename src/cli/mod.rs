//! Command-line argument parsing.

pub const USAGE: &str = "\
Usage: catmd [OPTIONS] [FILE]...

Render Markdown FILE(s) formatted to the terminal.
With no FILE, or when FILE is -, read standard input.

When stdout is a terminal, catmd opens an interactive viewer:
scroll with ↑/↓, copy a code block to the clipboard by clicking
its [ copy ] button (or pressing its number), search with Ctrl-F
(n/N for the next/previous match), press q to quit.
When output is piped, catmd prints plain formatted text.

Options:
  -p, --plain    print formatted text without the interactive viewer
  -h, --help     show this help
  -V, --version  show the version
";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    Run(Options),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    pub plain: bool,
    /// Files to render, in order; "-" means stdin. Never empty.
    pub files: Vec<String>,
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut plain = false;
    let mut files: Vec<String> = Vec::new();
    let mut options_ended = false;
    for arg in args {
        if options_ended {
            files.push(arg);
            continue;
        }
        match arg.as_str() {
            "--" => options_ended = true,
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "-p" | "--plain" => plain = true,
            "-" => files.push(arg),
            flag if flag.starts_with('-') && flag.len() > 1 => {
                return Err(format!("unrecognized option '{flag}'"));
            }
            _ => files.push(arg),
        }
    }
    if files.is_empty() {
        files.push("-".to_string());
    }
    Ok(Command::Run(Options { plain, files }))
}

#[cfg(test)]
mod tests;
