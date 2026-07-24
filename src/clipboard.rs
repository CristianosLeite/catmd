use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, ExitCode, Stdio};

use base64::Engine;

pub const HOLD_FLAG: &str = "--__hold-clipboard";

/// Copies text to the system clipboard, returning the mechanism used.
/// Tries system clipboard tools, then a native connection to the display
/// server, then falls back to the OSC 52 escape sequence (understood by
/// many terminals, works over SSH).
pub fn copy(text: &str) -> Result<&'static str, String> {
    let on_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let on_x11 = std::env::var_os("DISPLAY").is_some();
    if on_wayland && pipe_to("wl-copy", &[], text) {
        return Ok("wl-copy");
    }
    if on_x11 {
        if pipe_to("xclip", &["-selection", "clipboard"], text) {
            return Ok("xclip");
        }
        if pipe_to("xsel", &["--clipboard", "--input"], text) {
            return Ok("xsel");
        }
    }
    if (on_wayland || on_x11) && native_copy(text) {
        return Ok("native");
    }
    // There is no way to detect OSC 52 support: writing the sequence always
    // "succeeds", so be honest that this one is best-effort.
    osc52(text)
        .map(|()| "osc52 (needs terminal support)")
        .map_err(|err| err.to_string())
}

/// Hands the text to a detached `catmd --__hold-clipboard` child. A Linux
/// clipboard only lives as long as the process that set it, so the child
/// keeps serving it after catmd exits, until another app takes ownership.
fn native_copy(text: &str) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let mut cmd = Command::new(exe);
    cmd.arg(HOLD_FLAG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    unsafe {
        use std::os::unix::process::CommandExt;
        // New session: the holder must survive this process and its
        // terminal. The double fork reparents the holder to init right away,
        // so it never becomes our zombie when the clipboard is replaced
        // while the viewer is still open; the short-lived intermediate is
        // reaped by the wait() below. (Only async-signal-safe calls here.)
        cmd.pre_exec(|| {
            libc::setsid();
            match libc::fork() {
                -1 => Err(std::io::Error::last_os_error()),
                0 => Ok(()), // the grandchild goes on to exec the holder
                _ => libc::_exit(0),
            }
        });
    }
    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    // The pipes lead to the holder; `child` is the already-exited
    // intermediate. It prints "ok" once the clipboard is claimed, EOF if
    // it failed.
    let written = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok());
    let mut line = String::new();
    let claimed = written
        && child
            .stdout
            .take()
            .is_some_and(|out| BufReader::new(out).read_line(&mut line).is_ok())
        && line.trim() == "ok";
    let _ = child.wait();
    claimed
}

/// Entry point for the hidden holder mode: reads the text from stdin,
/// claims the clipboard, and serves it until another app replaces it.
pub fn hold_clipboard() -> ExitCode {
    use arboard::SetExtLinux;
    let mut text = String::new();
    if std::io::stdin().read_to_string(&mut text).is_err() {
        return ExitCode::FAILURE;
    }
    let Ok(mut clipboard) = arboard::Clipboard::new() else {
        return ExitCode::FAILURE;
    };
    if clipboard.set().text(text.clone()).is_err() {
        return ExitCode::FAILURE;
    }
    println!("ok");
    let _ = std::io::stdout().flush();
    let _ = clipboard.set().wait().text(text);
    ExitCode::SUCCESS
}

fn pipe_to(cmd: &str, args: &[&str], text: &str) -> bool {
    let Ok(mut child) = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let written = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok());
    // Reap even on a failed write, or the dead tool lingers as a zombie.
    let succeeded = child.wait().is_ok_and(|status| status.success());
    written && succeeded
}

fn osc52(text: &str) -> std::io::Result<()> {
    let mut out = std::io::stdout();
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    write!(out, "\x1b]52;c;{encoded}\x07")?;
    out.flush()
}
