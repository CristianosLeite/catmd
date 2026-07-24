# catmd

`cat` for Markdown: render `.md` files nicely formatted in your terminal,
with an interactive viewer and one-click copying of code blocks.

```
catmd README.md
```

![catmd rendering its own README in a terminal](assets/sample.png)

## Features

- **Formatted output** — colored headers, **bold**/*italic*, inline code,
  lists, quotes, and drawn tables (powered by [termimad])
- **Syntax-highlighted code blocks** — fenced blocks are highlighted by
  language (powered by [syntect]); unknown languages fall back to plain text
- **Inline images** — local images are drawn at full pixel resolution in
  terminals that support the [kitty graphics protocol]; elsewhere the
  image reference stays ordinary markdown text (see
  [Images](#images) below)
- **Interactive viewer** — when stdout is a terminal, catmd opens a
  scrollable view where every code block has a clickable `[ copy ]` button
- **Native clipboard** — copying works without any external tools, and the
  copied text survives after catmd exits
- **cat-like behavior** — multiple files, stdin, pipes, and familiar exit
  codes

[termimad]: https://crates.io/crates/termimad
[syntect]: https://crates.io/crates/syntect
[kitty graphics protocol]: https://sw.kovidgoyal.net/kitty/graphics-protocol/

## Platform support

**Linux only.** The clipboard holder relies on Linux-specific APIs
(`arboard`'s Linux extensions, `setsid`/`fork`), so the crate refuses to
compile on other platforms with a clear error rather than half-working.
macOS/Windows support would need a platform-specific clipboard holder;
contributions welcome.

## Installation

### Quick install

```sh
curl -fsSL https://raw.githubusercontent.com/CristianosLeite/catmd/main/scripts/install.sh | bash
```

This downloads the source, installs the build prerequisites for your
distro, builds the release binary, and installs `catmd` into
`~/.cargo/bin` (make sure it is on your `PATH`). It builds the latest
development version from the `main` branch — as with any `curl | bash`
installer, feel free to read
[`scripts/install.sh`](scripts/install.sh) before running it. From a
local checkout the same thing is just:

```sh
./scripts/install.sh
```

### From source

Requires a Rust toolchain, version 1.97.1 or newer (edition 2024).

```sh
cargo install --locked --path .
```

This installs the `catmd` binary into `~/.cargo/bin` (make sure it is on
your `PATH`).

### Build script

If you don't have a toolchain set up yet, `scripts/build-linux.sh` does the
whole thing: it detects your distro (Debian/Ubuntu, Fedora/RHEL, Arch,
openSUSE, Alpine), installs the build prerequisites (C compiler,
`pkg-config`, `curl`) with the native package manager, bootstraps Rust via
[rustup] if it's missing, and builds the release binary.

```sh
./scripts/build-linux.sh            # install prerequisites + build target/release/catmd
./scripts/build-linux.sh --install  # also install to ~/.cargo/bin
```

Package installation uses `sudo` when run as a regular user; if everything
is already installed, the script skips straight to the build and needs no
privileges.

[rustup]: https://rustup.rs

## Usage

```sh
catmd file.md              # open file.md in the interactive viewer
catmd a.md b.md            # several files in one scrollable view
curl -s example.com/x.md | catmd     # read from stdin
catmd - < notes.md         # "-" also means stdin
catmd file.md | less -R    # piped output is plain formatted text
catmd --plain file.md      # force plain output in a terminal
catmd -- -dashed-name.md   # "--" ends option parsing (dash-prefixed files)
catmd --help               # show help
catmd --version            # show version
```

### Interactive viewer

Opens automatically when stdout is a terminal (skip it with `--plain`).

| Key / action           | Effect                                                                                                                                           |
|------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------|
| `↑`/`↓` or `k`/`j`     | scroll one line                                                                                                                                  |
| mouse wheel            | scroll                                                                                                                                           |
| `PageUp` / `PageDown`  | scroll one screen (also `Space`)                                                                                                                 |
| `g` / `G` (Home / End) | jump to top / bottom                                                                                                                             |
| click `[ copy ]`       | copy that code block to the clipboard                                                                                                            |
| type a block number    | copy that block; fires as soon as the number is unambiguous, `Enter` confirms an ambiguous prefix (e.g. `1` when block 12 exists), `Esc` cancels |
| `q`, `Esc`, `Ctrl-C`   | quit                                                                                                                                             |

Every code block is drawn in a numbered box:

```
┌─ [1] bash ──────────────────────────────── [ copy ]
│ sudo apt install something
└────────────────────────────────────────────────────
```

Clicking `[ copy ]` (or pressing `1`) puts the block's source text on the
system clipboard; the status bar confirms the copy. Fences inside block
quotes (`> ```…`) and fences indented up to 3 spaces (e.g. in a list item)
get copy buttons too; the copied text is the block's CommonMark content —
quote markers and opener indentation stripped, line endings preserved.
Fences nested deeper than that are rendered as prose without a button.

The copied text is byte-exact, but the *display* strips terminal control
characters and bidi overrides for safety. When a copied block contains
characters that are invisible on screen (controls, bidi overrides,
zero-width characters), the status bar adds **“⚠ contains hidden
characters”** — review such a block before pasting it into a shell.

Note: while the viewer is open it captures the mouse, so normal
drag-to-select is disabled — most terminals still allow native selection
while holding `Shift`.

### Images

An image referenced on a line of its own is rendered as a real picture
when the terminal can draw one:

```markdown
![architecture diagram](docs/architecture.png)
```

**Where images render.** catmd detects support for the [kitty graphics
protocol] from the environment: kitty, Ghostty, WezTerm, and Konsole
qualify. In any other terminal — and always in piped output — the image
reference is rendered as ordinary markdown text, so nothing is lost, just
not drawn. The alt text appears as a dim caption under a rendered image.

**What is supported.** Local PNG, JPEG, GIF (first frame), WebP, and BMP
files, resolved relative to the markdown file's directory (the current
directory for stdin). The destination may use CommonMark escapes and
percent-encoding (`my%20pic.png`). Remote `http(s)` URLs, reference-style
images (`![alt][ref]`), and images in the middle of a sentence stay as
text. Anything unloadable — missing file, unsupported format such as SVG —
falls back to text as well; a document never fails to render because of
its images.

**Overrides.** `CATMD_IMAGES=kitty` forces the protocol on when detection
misses (inside tmux this emits tmux passthrough sequences, which require
`allow-passthrough` enabled in your tmux configuration);
`CATMD_IMAGES=none` disables images entirely.

**Resource limits.** Image files are capped at 64 MiB, decoding is bounded
in dimensions and memory, and a document referencing many distinct images
stops rendering new ones (as text, gracefully) once a global budget is
reached — a hostile markdown file cannot exhaust memory through images.

### Exit codes

Like `cat`: `0` on success, `1` if any file could not be read (the
remaining files are still rendered, errors go to stderr).

## Clipboard mechanics

Copying tries, in order:

1. **System tools** — `wl-copy` (Wayland) or `xclip`/`xsel` (X11), if
   installed
2. **Native** — a direct connection to the display server via [arboard];
   no external tools needed. Because a Linux clipboard only lives as long
   as the process that set it, catmd hands the text to a small detached
   `catmd` background process that serves the clipboard until another
   application takes ownership (the same trick `wl-copy` uses)
3. **OSC 52** — a terminal escape sequence, useful over SSH; requires
   terminal support. There is no way to detect that support, so its status
   message says so ("via osc52 (needs terminal support)")

The status bar shows which mechanism was used (e.g. "via native").

[arboard]: https://crates.io/crates/arboard

## Development

```sh
cargo test      # unit + integration tests
cargo clippy    # lints
cargo run -- README.md
```

### Code layout

| File                 | Responsibility                                            |
|----------------------|-----------------------------------------------------------|
| `src/main.rs`        | entry point: reads files, picks interactive vs. plain     |
| `src/cli/mod.rs`     | argument parsing and usage text                           |
| `src/segment/mod.rs` | splits markdown into prose, fenced code, and image lines  |
| `src/render/mod.rs`  | styling: termimad for prose, syntect for code; image cache |
| `src/image/mod.rs`   | image loading, resource limits, terminal detection        |
| `src/image/kitty.rs` | kitty graphics protocol commands (incl. tmux passthrough) |
| `src/viewer/mod.rs`  | interactive full-screen viewer (scroll, mouse, copy)      |
| `src/text/mod.rs`    | sanitization and display-width math                       |
| `src/clipboard.rs`   | clipboard back-ends and the detached holder process       |
| `src/*/tests.rs`     | unit tests for the matching module                        |
| `tests/cli.rs`       | end-to-end tests against the built binary                 |

## License

MIT — see [LICENSE](LICENSE).
