use super::*;

use image::{Rgba, RgbaImage};

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

#[test]
fn missing_file_is_an_error_not_a_panic() {
    assert!(load_image(Path::new("/nonexistent/nope.png")).is_err());
}

#[test]
fn undecodable_file_is_an_error() {
    let path = std::env::temp_dir().join("catmd-test-not-an-image.png");
    std::fs::write(&path, b"not a png").unwrap();
    assert!(load_image(&path).is_err());
    std::fs::remove_file(&path).ok();
}

#[test]
fn png_file_round_trips_with_colors_intact() {
    let path = std::env::temp_dir().join("catmd-test-tiny.png");
    let mut img = RgbaImage::new(2, 2);
    img.put_pixel(0, 0, Rgba(RED));
    img.put_pixel(1, 0, Rgba(BLUE));
    img.put_pixel(0, 1, Rgba(BLUE));
    img.put_pixel(1, 1, Rgba(RED));
    img.save(&path).unwrap();
    let loaded = load_image(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(loaded.dimensions(), (2, 2));
    assert_eq!(loaded.get_pixel(0, 0).0, RED);
    assert_eq!(loaded.get_pixel(1, 0).0, BLUE);
}

#[test]
fn oversized_images_are_downscaled_to_the_cache_bound() {
    let path = std::env::temp_dir().join("catmd-test-cache-bound.png");
    RgbaImage::from_pixel(3_000, 2, Rgba(RED))
        .save(&path)
        .unwrap();
    let wide = load_image(&path).unwrap();
    RgbaImage::from_pixel(2, 8_000, Rgba(BLUE))
        .save(&path)
        .unwrap();
    let tall = load_image(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert!(wide.width() <= MAX_WIDTH);
    assert!(tall.height() <= MAX_HEIGHT);
}

#[test]
fn downscaling_preserves_aspect_ratio() {
    let img = RgbaImage::from_pixel(3200, 1600, Rgba(RED));
    let fit = fit_within(&img, MAX_WIDTH, MAX_HEIGHT);
    assert_eq!(fit.dimensions(), (1600, 800));
}

#[test]
fn gamma_correct_downscaling_keeps_bright_detail_visible() {
    // Alternating black/white columns averaged to half size: linear-light
    // averaging yields a bright mid gray (~188 in sRGB), while naive sRGB
    // averaging would give a dark 128.
    let mut img = RgbaImage::from_pixel(100, 2, Rgba([0, 0, 0, 255]));
    for y in 0..2 {
        for x in (0..100).step_by(2) {
            img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
        }
    }
    let fit = fit_within(&img, 50, 1);
    let px = fit.get_pixel(25, 0).0;
    assert!(px[0] > 160, "bright detail crushed to {px:?}");
}

fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let pairs: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |name: &str| {
        pairs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }
}

#[test]
fn kitty_terminals_are_detected_from_the_environment() {
    for env in [
        vec![("TERM", "xterm-kitty")],
        vec![("TERM", "xterm-256color"), ("KITTY_WINDOW_ID", "1")],
        vec![("TERM", "xterm-ghostty")],
        vec![("TERM", "xterm-256color"), ("TERM_PROGRAM", "WezTerm")],
    ] {
        assert!(
            detect_mode_from(env_of(&env)) == Mode::Kitty { tmux: false },
            "{env:?} should detect kitty"
        );
    }
}

#[test]
fn incapable_and_multiplexed_terminals_keep_images_as_text() {
    for env in [
        vec![("TERM", "xterm-256color")],
        vec![("TERM", "tmux-256color"), ("KITTY_WINDOW_ID", "1")],
        vec![
            ("TERM", "xterm-kitty"),
            ("TMUX", "/tmp/tmux-1000/default,1,0"),
        ],
        vec![("TERM", "screen")],
    ] {
        assert!(
            detect_mode_from(env_of(&env)) == Mode::Text,
            "{env:?} should fall back to text"
        );
    }
}

#[test]
fn catmd_images_env_overrides_detection() {
    assert!(
        detect_mode_from(env_of(&[
            ("TERM", "xterm-256color"),
            ("CATMD_IMAGES", "kitty")
        ])) == Mode::Kitty { tmux: false }
    );
    assert!(
        detect_mode_from(env_of(&[("TERM", "xterm-kitty"), ("CATMD_IMAGES", "none")]))
            == Mode::Text
    );
}

#[test]
fn fifo_is_rejected_without_blocking() {
    let path = std::env::temp_dir().join(format!("catmd-test-fifo-{}", std::process::id()));
    let cpath = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
    let start = std::time::Instant::now();
    let result = load_image(&path);
    std::fs::remove_file(&path).ok();
    assert!(result.is_err(), "FIFO must not decode");
    assert!(
        start.elapsed() < std::time::Duration::from_secs(1),
        "load blocked on the FIFO"
    );
}

#[test]
fn device_files_are_rejected() {
    assert!(load_image(Path::new("/dev/null")).is_err());
    assert!(load_image(Path::new("/dev/zero")).is_err());
}

#[test]
fn oversized_files_are_rejected_before_decoding() {
    let path = std::env::temp_dir().join(format!("catmd-test-huge-{}", std::process::id()));
    // A sparse file: huge length, no disk usage, instant to create.
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(MAX_FILE_BYTES + 1).unwrap();
    drop(file);
    let result = load_image(&path);
    std::fs::remove_file(&path).ok();
    assert_eq!(result.unwrap_err(), "image file too large");
}

#[test]
fn forced_kitty_inside_tmux_requests_passthrough_wrapping() {
    let mode = detect_mode_from(env_of(&[
        ("TERM", "tmux-256color"),
        ("TMUX", "/tmp/tmux-1000/default,1,0"),
        ("CATMD_IMAGES", "kitty"),
    ]));
    assert!(mode == Mode::Kitty { tmux: true });
    // Outside tmux the override needs no wrapping.
    let mode = detect_mode_from(env_of(&[
        ("TERM", "xterm-256color"),
        ("CATMD_IMAGES", "kitty"),
    ]));
    assert!(mode == Mode::Kitty { tmux: false });
}
