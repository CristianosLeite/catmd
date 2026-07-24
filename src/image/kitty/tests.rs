use super::*;

const CELL: CellSize = CellSize {
    width: 10,
    height: 20,
};

const RAW: Graphics = Graphics { tmux: false };
const TMUX: Graphics = Graphics { tmux: true };

#[test]
fn layout_preserves_aspect_ratio_at_full_width() {
    // 800x400 px in 10x20 cells: 80 cols wide, 400px tall -> 20 rows.
    let (cols, rows) = layout(800, 400, 80, CELL);
    assert_eq!((cols, rows), (80, 20));
}

#[test]
fn layout_scales_down_to_the_terminal_width() {
    // 1600x400 px into 40 cols: shown at 400px wide -> 100px tall -> 5 rows.
    let (cols, rows) = layout(1600, 400, 40, CELL);
    assert_eq!(cols, 40);
    assert_eq!(rows, 5);
}

#[test]
fn layout_never_enlarges_small_images() {
    // A 30px-wide icon occupies 3 cols, not the whole terminal.
    let (cols, _) = layout(30, 30, 80, CELL);
    assert_eq!(cols, 3);
}

#[test]
fn layout_survives_degenerate_inputs() {
    let (cols, rows) = layout(1, 1, 0, CELL);
    assert!(cols >= 1 && rows >= 1);
    let (cols, rows) = layout(0, 0, 80, CELL);
    assert!(cols >= 1 && rows >= 1);
}

#[test]
fn transmit_chunks_large_payloads() {
    // 9000 bytes -> 12000 base64 chars -> 3 chunks of <= 4096.
    let payload = vec![0u8; 9000];
    let out = RAW.transmit(7, &payload);
    let chunks: Vec<&str> = out.split("\x1b\\").filter(|s| !s.is_empty()).collect();
    assert_eq!(chunks.len(), 3);
    assert!(chunks[0].starts_with("\x1b_Ga=t,q=2,f=100,i=7,m=1;"));
    assert!(chunks[1].starts_with("\x1b_Gm=1;"));
    assert!(chunks[2].starts_with("\x1b_Gm=0;"));
}

#[test]
fn transmit_small_payload_is_a_single_final_chunk() {
    let out = RAW.transmit(3, b"tiny");
    assert!(out.starts_with("\x1b_Ga=t,q=2,f=100,i=3,m=0;"));
    assert!(out.ends_with("\x1b\\"));
    assert_eq!(out.matches("\x1b_G").count(), 1);
}

#[test]
fn place_encodes_geometry_and_keeps_cursor() {
    let out = RAW.place(9, 40, 12, None);
    assert_eq!(out, "\x1b_Ga=p,q=2,i=9,c=40,r=12,C=1\x1b\\");
    let cropped = RAW.place(
        9,
        40,
        6,
        Some(Crop {
            y_px: 100,
            h_px: 120,
        }),
    );
    assert!(cropped.contains(",y=100,h=120"));
}

#[test]
fn every_command_suppresses_terminal_responses() {
    for cmd in [
        RAW.transmit(1, b"x"),
        RAW.place(1, 1, 1, None),
        RAW.delete_placements(),
        RAW.free(1),
    ] {
        assert!(cmd.contains("q=2"), "no q=2 in {cmd:?}");
    }
}

#[test]
fn png_round_trip_encodes() {
    let img = RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 255]));
    let png = encode_png(&img).unwrap();
    assert_eq!(&png[1..4], b"PNG");
}

#[test]
fn tmux_wrapping_uses_passthrough_with_doubled_escapes() {
    let out = TMUX.place(9, 40, 12, None);
    assert!(out.starts_with("\x1bPtmux;"), "no DCS prefix: {out:?}");
    assert!(out.ends_with("\x1b\\"), "no ST suffix: {out:?}");
    // The inner command's ESC bytes are doubled.
    let inner = &out["\x1bPtmux;".len()..out.len() - 2];
    assert!(inner.starts_with("\x1b\x1b_Ga=p"), "inner: {inner:?}");
    assert!(inner.ends_with("\x1b\x1b\\"), "inner: {inner:?}");
    assert!(!inner.contains("\x1bP"), "nested DCS in inner: {inner:?}");
}

#[test]
fn tmux_wrapping_wraps_every_transmit_chunk() {
    let payload = vec![0u8; 9000];
    let out = TMUX.transmit(7, &payload);
    // 3 protocol chunks -> 3 passthrough wrappers.
    assert_eq!(out.matches("\x1bPtmux;").count(), 3);
    // Unwrapping restores the raw chunked form exactly.
    let unwrapped: String = out
        .split("\x1bPtmux;")
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.strip_suffix("\x1b\\")
                .unwrap()
                .replace("\x1b\x1b", "\x1b")
        })
        .collect();
    assert_eq!(unwrapped, RAW.transmit(7, &payload));
}
