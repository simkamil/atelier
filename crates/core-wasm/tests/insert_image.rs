// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-270: inline pictures in the editor session (insertion, undo, layout,
//! EXIF, display list, bitmap, refusals, plain-text export).

use atelier_core_wasm::editor::EditorSession;
use atelier_image::{
    DecodeLimits, Image, PngColor, decode_jpeg_with_metadata, encode_png, encode_png_rgba,
};
use atelier_render::{Command, DisplayList};

const PAGE_WIDTH: f64 = 595.276;
const PAGE_HEIGHT: f64 = 841.89;
const MARGIN: f64 = 54.0;

fn command(op: u16, body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::from(&b"ATED\x01\x00"[..]);
    bytes.extend_from_slice(&op.to_le_bytes());
    bytes.extend_from_slice(body);
    bytes
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn f64_at(bytes: &[u8], at: usize) -> f64 {
    f64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

struct State {
    text: String,
    flags: u16,
    anchor: u32,
    focus: u32,
    caret_height: f64,
    pages: Vec<DisplayList>,
}

fn parse(response: &[u8]) -> State {
    assert_eq!(&response[..4], b"ATES");
    let text_len = u32_at(response, 24) as usize;
    let page_count = u32_at(response, 28) as usize;
    let text = String::from_utf8(response[64..64 + text_len].to_vec()).unwrap();
    let mut at = 64 + text_len;
    let mut pages = Vec::new();
    for _ in 0..page_count {
        let len = u32_at(response, at) as usize;
        pages.push(DisplayList::decode(&response[at + 4..at + 4 + len]).unwrap());
        at += 4 + len;
    }
    State {
        text,
        flags: u16::from_le_bytes(response[6..8].try_into().unwrap()),
        anchor: u32_at(response, 16),
        focus: u32_at(response, 20),
        caret_height: f64_at(response, 52),
        pages,
    }
}

fn apply(session: &EditorSession, bytes: &[u8]) -> (EditorSession, Vec<u8>) {
    session.transact(bytes).unwrap()
}
fn load(text: &str) -> EditorSession {
    apply(&EditorSession::default(), &command(1, text.as_bytes())).0
}
fn caret_at(session: &EditorSession, offset: u32) -> EditorSession {
    let mut body = offset.to_le_bytes().to_vec();
    body.extend_from_slice(&offset.to_le_bytes());
    apply(session, &command(7, &body)).0
}
fn insert_image(
    session: &EditorSession,
    bytes: &[u8],
) -> Result<(EditorSession, State), &'static str> {
    let mut body = 100u64.to_le_bytes().to_vec();
    body.extend_from_slice(bytes);
    session
        .transact(&command(26, &body))
        .map(|(s, r)| (s, parse(&r)))
        .map_err(|e| e.0)
}
fn state(session: &EditorSession) -> State {
    parse(&apply(session, &command(0, &[])).1)
}
fn images(page: &DisplayList) -> Vec<(u64, f64, f64, f64, f64)> {
    page.commands
        .iter()
        .filter_map(|c| match c {
            Command::DrawImage { image_id, bounds } => Some((
                *image_id,
                bounds.left(),
                bounds.top(),
                bounds.width(),
                bounds.height(),
            )),
            _ => None,
        })
        .collect()
}

fn png(width: u32, height: u32, seed: u8) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            pixels.extend_from_slice(&[
                (x * 40) as u8 ^ seed,
                (y * 70) as u8,
                seed,
                if (x + y) % 5 == 0 { 128 } else { 255 },
            ]);
        }
    }
    encode_png_rgba(
        &Image {
            width,
            height,
            pixels,
        },
        DecodeLimits {
            max_pixels: 1 << 26,
            max_input_bytes: 1 << 28,
        },
    )
    .unwrap()
}

fn jpeg() -> Vec<u8> {
    include_bytes!("../../image/tests/fixtures/jpeg/baseline-444.jpg").to_vec()
}
fn jpeg_with_orientation(orientation: u8) -> Vec<u8> {
    let jpeg = jpeg();
    let mut tiff = b"II".to_vec();
    tiff.extend_from_slice(&42u16.to_le_bytes());
    tiff.extend_from_slice(&8u32.to_le_bytes());
    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0x0112u16.to_le_bytes());
    tiff.extend_from_slice(&3u16.to_le_bytes());
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&u16::from(orientation).to_le_bytes());
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&0u32.to_le_bytes());
    let mut payload = b"Exif\0\0".to_vec();
    payload.extend_from_slice(&tiff);
    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&[0xff, 0xe1]);
    out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&payload);
    out.extend_from_slice(&jpeg[2..]);
    out
}

fn is_picture_char(c: char) -> bool {
    ('\u{100000}'..='\u{10fffd}').contains(&c)
}

#[test]
fn inserting_a_png_puts_one_inline_picture_at_the_caret_on_the_baseline() {
    let session = caret_at(&load("abcd"), 2);
    let (session, st) = insert_image(&session, &png(40, 20, 1)).unwrap();
    let chars: Vec<char> = st.text.chars().collect();
    assert_eq!(chars.len(), 5);
    assert!(is_picture_char(chars[2]));
    assert_eq!(
        (chars[0], chars[1], chars[3], chars[4]),
        ('a', 'b', 'c', 'd')
    );
    // The caret sits just after the picture scalar (4 UTF-8 bytes).
    assert_eq!((st.anchor, st.focus), (6, 6));
    let pics = images(&st.pages[0]);
    assert_eq!(pics.len(), 1);
    let (id, x, y, w, h) = pics[0];
    assert_eq!(id, 0);
    // 40x20 px at 96 dpi -> 30x15 pt, after "ab", bottom on the first baseline (54 + 15 + 3 - 3).
    assert!(x > MARGIN && x < MARGIN + 30.0);
    assert!((w - 30.0).abs() < 1e-9 && (h - 15.0).abs() < 1e-9);
    assert!((y + h - (MARGIN + 15.0)).abs() < 1e-9, "bottom {y} + {h}");
    // The line is taller than a text line: ascent 15 + descent 3.
    assert!((st.caret_height - 18.0).abs() < 1e-9);
    // Text after the picture follows it on the same line.
    let _ = session;
}

#[test]
fn undo_removes_the_picture_and_redo_restores_it_in_one_step() {
    let base = caret_at(&load("abcd"), 2);
    let (inserted, st) = insert_image(&base, &png(8, 8, 2)).unwrap();
    assert_ne!(st.flags & 4, 0, "dirty after insertion");
    assert_ne!(st.flags & 1, 0, "can undo");
    let (undone, _) = apply(&inserted, &command(5, &[]));
    let after_undo = state(&undone);
    assert_eq!(after_undo.text, "abcd");
    assert!(images(&after_undo.pages[0]).is_empty());
    assert_eq!(after_undo.flags & 4, 0, "clean again");
    assert_ne!(after_undo.flags & 2, 0, "can redo");
    let (redone, _) = apply(&undone, &command(6, &[]));
    let after_redo = state(&redone);
    assert_eq!(after_redo.text, st.text);
    assert_eq!(images(&after_redo.pages[0]).len(), 1);
}

#[test]
fn inserting_over_a_selection_replaces_it_and_undo_restores_the_text() {
    let mut body = 1u32.to_le_bytes().to_vec();
    body.extend_from_slice(&3u32.to_le_bytes());
    let session = apply(&load("abcd"), &command(7, &body)).0;
    let (inserted, st) = insert_image(&session, &png(8, 8, 3)).unwrap();
    assert_eq!(st.text.chars().filter(|c| is_picture_char(*c)).count(), 1);
    assert!(st.text.starts_with('a') && st.text.ends_with('d') && st.text.chars().count() == 3);
    let undone = state(&apply(&inserted, &command(5, &[])).0);
    assert_eq!(undone.text, "abcd");
}

#[test]
fn a_picture_wider_than_the_text_area_shrinks_proportionally() {
    let (_, st) = insert_image(&load(""), &png(1000, 400, 4)).unwrap();
    let (_, _, y, w, h) = images(&st.pages[0])[0];
    assert!((w - (PAGE_WIDTH - 2.0 * MARGIN)).abs() < 1e-9, "width {w}");
    assert!((w / h - 2.5).abs() < 1e-9);
    assert!((y - MARGIN).abs() < 1e-9, "top {y}");
}

#[test]
fn a_picture_taller_than_a_page_shrinks_to_fit_the_page() {
    let (_, st) = insert_image(&load(""), &png(100, 3000, 5)).unwrap();
    let (_, _, y, w, h) = images(&st.pages[0])[0];
    assert!(
        y >= MARGIN - 1e-9 && y + h <= PAGE_HEIGHT - MARGIN + 1e-9,
        "{y} {h}"
    );
    assert!((w / h - 100.0 / 3000.0).abs() < 1e-9);
}

#[test]
fn exif_orientation_swaps_the_drawn_size_and_is_kept_with_the_original_bytes() {
    // Stored 19x17.
    let (_, plain) = insert_image(&load(""), &jpeg_with_orientation(1)).unwrap();
    let (_, _, _, w1, h1) = images(&plain.pages[0])[0];
    assert!((w1 - 19.0 * 0.75).abs() < 1e-9 && (h1 - 17.0 * 0.75).abs() < 1e-9);
    for orientation in 2..=8u8 {
        let (_, st) = insert_image(&load(""), &jpeg_with_orientation(orientation)).unwrap();
        let (_, _, _, w, h) = images(&st.pages[0])[0];
        if orientation >= 5 {
            assert!(
                (w - 17.0 * 0.75).abs() < 1e-9 && (h - 19.0 * 0.75).abs() < 1e-9,
                "orientation {orientation}"
            );
        } else {
            assert!(
                (w - w1).abs() < 1e-9 && (h - h1).abs() < 1e-9,
                "orientation {orientation}"
            );
        }
    }
}

#[test]
fn the_display_bitmap_is_oriented_and_matches_the_decoded_pixels() {
    let bytes = jpeg_with_orientation(6);
    let raw = decode_jpeg_with_metadata(&bytes, DecodeLimits::default()).unwrap();
    assert_eq!(raw.orientation, 6);
    let (session, _) = insert_image(&load(""), &bytes).unwrap();
    let response = apply(&session, &command(27, &0u32.to_le_bytes())).1;
    assert_eq!(&response[..4], b"ATIM");
    let (w, h) = (u32_at(&response, 12), u32_at(&response, 16));
    assert_eq!((w, h), (17, 19), "rotated 90 degrees: 19x17 stored");
    assert_eq!(u32_at(&response, 20), w * h * 4);
    let pixels = &response[24..];
    // Orientation 6: oriented (x, y) shows stored (y, stored_h - 1 - x).
    for (x, y) in [(0u32, 0u32), (16, 0), (0, 18), (5, 7)] {
        let (sx, sy) = (y, 17 - 1 - x);
        let expected = &raw.image.pixels[((sy * 19 + sx) * 4) as usize..][..4];
        assert_eq!(
            &pixels[((y * w + x) * 4) as usize..][..4],
            expected,
            "({x},{y})"
        );
    }
    assert!(
        apply(&session, &command(5, &[]))
            .0
            .transact(&command(27, &9u32.to_le_bytes()))
            .is_err()
    );
}

#[test]
fn a_large_picture_gets_a_bounded_display_bitmap() {
    let big = png(3000, 1500, 6);
    let (session, _) = insert_image(&load(""), &big).unwrap();
    let response = apply(&session, &command(27, &0u32.to_le_bytes())).1;
    let (w, h) = (u32_at(&response, 12), u32_at(&response, 16));
    assert_eq!(w, 1600);
    assert_eq!(h, 800);
    assert_eq!(response.len(), 24 + (w * h * 4) as usize);
}

#[test]
fn original_bytes_survive_in_every_supported_format() {
    // GIF 1x1 and BMP 1x1 are written by hand.
    let gif: Vec<u8> = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;".to_vec();
    let bmp: Vec<u8> = {
        let mut b = b"BM".to_vec();
        b.extend_from_slice(&58u32.to_le_bytes());
        b.extend_from_slice(&[0, 0, 0, 0]);
        b.extend_from_slice(&54u32.to_le_bytes());
        b.extend_from_slice(&40u32.to_le_bytes());
        b.extend_from_slice(&1i32.to_le_bytes());
        b.extend_from_slice(&1i32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&24u16.to_le_bytes());
        b.extend_from_slice(&[0; 24]);
        b.extend_from_slice(&[10, 20, 30, 0]);
        b
    };
    for (name, bytes) in [
        ("png", png(3, 3, 7)),
        ("jpeg", jpeg()),
        ("gif", gif),
        ("bmp", bmp),
    ] {
        let (session, st) =
            insert_image(&load(""), &bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(images(&st.pages[0]).len(), 1, "{name}");
        let response = apply(&session, &command(27, &0u32.to_le_bytes())).1;
        assert_eq!(&response[..4], b"ATIM", "{name}");
    }
}

#[test]
fn unsupported_corrupt_and_oversized_files_are_refused_with_distinct_codes() {
    let session = load("abc");
    let webp = b"RIFF\x24\0\0\0WEBPVP8 \x18\0\0\0".to_vec();
    assert_eq!(
        insert_image(&session, &webp).err(),
        Some("EDITOR_IMAGE_UNSUPPORTED")
    );
    let tiff = b"II*\0\x08\0\0\0\0\0".to_vec();
    assert_eq!(
        insert_image(&session, &tiff).err(),
        Some("EDITOR_IMAGE_UNSUPPORTED")
    );
    assert_eq!(
        insert_image(&session, b"just some text").err(),
        Some("EDITOR_IMAGE_UNSUPPORTED")
    );
    let good = png(16, 16, 8);
    assert_eq!(
        insert_image(&session, &good[..good.len() / 2]).err(),
        Some("EDITOR_IMAGE_CORRUPT")
    );
    let mut flipped = good.clone();
    let mid = flipped.len() / 2;
    flipped[mid] ^= 0xff;
    assert!(insert_image(&session, &flipped).is_err());
    let truncated_jpeg = jpeg();
    assert_eq!(
        insert_image(&session, &truncated_jpeg[..truncated_jpeg.len() / 3]).err(),
        Some("EDITOR_IMAGE_CORRUPT")
    );
    // 5000 x 5000 gray = 25 Mpixel: over the decode budget, tiny once compressed.
    let bomb = encode_png(
        5000,
        5000,
        PngColor::Gray8,
        &vec![0u8; 25_000_000],
        DecodeLimits {
            max_pixels: 1 << 26,
            max_input_bytes: 1 << 28,
        },
    )
    .unwrap();
    assert!(bomb.len() < 1 << 20);
    assert_eq!(
        insert_image(&session, &bomb).err(),
        Some("EDITOR_IMAGE_TOO_LARGE")
    );
    // Over the 8 MiB encoded limit: refused, never decoded.
    let mut huge = b"\x89PNG\r\n\x1a\n".to_vec();
    huge.resize(8 * 1024 * 1024 + 1, 0);
    assert!(insert_image(&session, &huge).is_err());
    // A refusal leaves the session untouched and usable.
    let st = state(&session);
    assert_eq!(st.text, "abc");
    assert_eq!(st.flags & 4, 0);
}

fn distinct_png(n: u32) -> Vec<u8> {
    encode_png_rgba(
        &Image {
            width: 2,
            height: 2,
            pixels: [n as u8, (n >> 8) as u8, 7, 255].repeat(4),
        },
        DecodeLimits::default(),
    )
    .unwrap()
}

#[test]
fn the_picture_count_is_bounded() {
    let mut session = load("");
    let mut refused = None;
    for n in 0..600u32 {
        match insert_image(&session, &distinct_png(n)) {
            Ok((next, _)) => session = next,
            Err(code) => {
                refused = Some((n, code));
                break;
            }
        }
    }
    let (n, code) = refused.expect("the store must fill up");
    assert_eq!(code, "EDITOR_IMAGE_LIMIT");
    assert_eq!(n, 512);
    assert_eq!(state(&session).text.chars().count(), 512);
}

#[test]
fn identical_pictures_share_one_stored_copy() {
    let bytes = png(4, 4, 9);
    let (session, first) = insert_image(&load(""), &bytes).unwrap();
    let (_, second) = insert_image(&session, &bytes).unwrap();
    let chars: Vec<char> = second.text.chars().collect();
    assert_eq!(chars.len(), 2);
    assert_eq!(chars[0], chars[1]);
    assert_eq!(first.text.chars().next(), Some(chars[0]));
}

#[test]
fn plain_text_export_and_dirty_state_use_the_object_replacement_character() {
    let session = caret_at(&load("ab"), 1);
    let (session, st) = insert_image(&session, &png(4, 4, 10)).unwrap();
    assert_ne!(st.flags & 4, 0);
    let export = apply(&session, &command(10, &[])).1;
    assert_eq!(&export[..4], b"ATEX");
    assert_eq!(std::str::from_utf8(&export[16..]).unwrap(), "a\u{fffc}b");
    let revision = u64::from_le_bytes(export[8..16].try_into().unwrap());
    let saved = apply(&session, &command(11, &revision.to_le_bytes())).0;
    assert_eq!(state(&saved).flags & 4, 0);
    // Undo after saving is a change again.
    let undone = apply(&saved, &command(5, &[])).0;
    assert_ne!(state(&undone).flags & 4, 0);
}

#[test]
fn loading_text_resets_pictures_and_never_confuses_existing_private_use_scalars() {
    let (session, _) = insert_image(&load("x"), &png(4, 4, 11)).unwrap();
    // A new document containing the first picture scalar must not show a picture.
    let reloaded = apply(&session, &command(1, "\u{100000}".as_bytes())).0;
    let st = state(&reloaded);
    assert!(images(&st.pages[0]).is_empty());
    let mut body = 4u32.to_le_bytes().to_vec();
    body.extend_from_slice(&4u32.to_le_bytes());
    let at_end = apply(&reloaded, &command(7, &body)).0;
    let (_, inserted) = insert_image(&at_end, &png(4, 4, 12)).unwrap();
    assert_eq!(
        images(&inserted.pages[0]).len(),
        1,
        "only the new picture is drawn"
    );
    let chars: Vec<char> = inserted.text.chars().collect();
    assert_eq!(chars[0], '\u{100000}');
    assert_ne!(chars[1], '\u{100000}');
}

#[test]
fn text_lines_without_pictures_keep_their_exact_geometry() {
    let st = state(&load("one\ntwo\nthree"));
    assert_eq!(st.caret_height, 14.0);
    assert!(images(&st.pages[0]).is_empty());
}

#[test]
fn a_tall_picture_pushes_following_lines_down_and_paginates_without_overflow() {
    let mut session = load(
        "top
",
    );
    session = caret_at(&session, 4);
    let newline = |s: &EditorSession| {
        let mut body = 1u64.to_le_bytes().to_vec();
        body.push(10);
        apply(s, &command(2, &body)).0
    };
    // ~700 pt tall pictures, one per line: two cannot share a page.
    let mut last = None;
    for seed in [13, 14, 15] {
        let (next, st) = insert_image(&session, &png(10, 933, seed)).unwrap();
        session = newline(&next);
        last = Some(st);
    }
    let st = state(&session);
    assert!(st.pages.len() >= 3, "pages {}", st.pages.len());
    for (index, page) in st.pages.iter().enumerate() {
        for (_, _, y, _, h) in images(page) {
            assert!(y >= MARGIN - 1e-9, "page {index} top {y}");
            assert!(
                y + h <= PAGE_HEIGHT - MARGIN + 1e-9,
                "page {index} bottom {}",
                y + h
            );
        }
    }
    // The first picture sits right under the one text line of 14 pt.
    let (_, _, y, _, _) = images(&st.pages[0])[0];
    assert!((y - (MARGIN + 14.0)).abs() < 1e-9, "first picture top {y}");
    // Later pictures start at the top margin of their own page.
    let (_, _, y, _, _) = images(&st.pages[1])[0];
    assert!((y - MARGIN).abs() < 1e-9, "second picture top {y}");
    assert!(last.is_some());
}

#[test]
fn clicking_inside_a_picture_selects_its_line_not_the_neighbouring_text() {
    let session = load("a\nb\n");
    let session = caret_at(&session, 2);
    let (session, st) = insert_image(&session, &png(200, 400, 16)).unwrap();
    let (_, x, y, w, h) = images(&st.pages[0])[0];
    // HitTest at the picture's centre, collapsing the selection there.
    let mut body = 0u32.to_le_bytes().to_vec();
    body.extend_from_slice(&(x + w / 2.0).to_le_bytes());
    body.extend_from_slice(&(y + h / 2.0).to_le_bytes());
    body.push(0);
    let hit = parse(&apply(&session, &command(9, &body)).1);
    // Offsets 2 (before the picture) and 6 (after) are the picture line's slots.
    assert!(hit.focus == 2 || hit.focus == 6, "offset {}", hit.focus);
    assert_eq!(hit.anchor, hit.focus);
}

#[test]
fn selection_rectangles_cover_the_whole_picture_line_height() {
    let session = caret_at(&load("ab"), 1);
    let (session, st) = insert_image(&session, &png(40, 80, 17)).unwrap();
    let (_, _, y, _, h) = images(&st.pages[0])[0];
    let mut body = 0u32.to_le_bytes().to_vec();
    body.extend_from_slice(&u32::try_from(st.text.len()).unwrap().to_le_bytes());
    let selected = apply(&session, &command(7, &body)).1;
    let rects_at = {
        let text_len = u32_at(&selected, 24) as usize;
        let mut at = 64 + text_len;
        for _ in 0..u32_at(&selected, 28) {
            at += 4 + u32_at(&selected, at) as usize;
        }
        at
    };
    let count = u32_at(&selected, 60) as usize;
    assert!(count > 0);
    let mut tallest: f64 = 0.0;
    for i in 0..count {
        tallest = tallest.max(f64_at(&selected, rects_at + i * 36 + 28));
    }
    assert!(
        (tallest - (h + 3.0)).abs() < 1e-9,
        "selection height {tallest} for picture {y} {h}"
    );
}

fn export_docx(session: &EditorSession) -> Result<(u32, u32, Vec<u8>), &'static str> {
    session
        .transact(&command(28, &[]))
        .map(|(_, r)| {
            assert_eq!(&r[..4], b"ATDX");
            (u32_at(&r, 8), u32_at(&r, 12), r[16..].to_vec())
        })
        .map_err(|e| e.0)
}

#[test]
fn docx_export_stores_the_original_bytes_with_size_and_position() {
    use atelier_fmt_ooxml::docx_image::read_pictures;
    let original = png(400, 200, 21);
    let session = caret_at(&load("ab\r\ncd"), 1);
    let (session, _) = insert_image(&session, &original).unwrap();
    let (_, pictures, bytes) = export_docx(&session).unwrap();
    assert_eq!(pictures, 1);
    let imported = atelier_fmt_ooxml::import_docx(&bytes).unwrap();
    let found = read_pictures(&imported);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].bytes, original, "original bytes, not re-encoded");
    assert_eq!(found[0].offset, 1);
    // 400x200 px = 300x150 pt, within the 487.3 pt text width.
    assert_eq!(
        (found[0].width_emu, found[0].height_emu),
        (3_810_000, 1_905_000)
    );
    let texts: Vec<String> = imported
        .document
        .paragraph_ids()
        .iter()
        .map(|p| imported.document.paragraph(*p).unwrap().text.clone())
        .collect();
    assert_eq!(texts, ["ab", "cd"]);
}

#[test]
fn docx_export_keeps_the_format_and_orientation_bytes_of_a_jpeg() {
    use atelier_fmt_ooxml::docx_image::read_pictures;
    let original = jpeg_with_orientation(6);
    let (session, _) = insert_image(&load("x"), &original).unwrap();
    let (_, _, bytes) = export_docx(&session).unwrap();
    let imported = atelier_fmt_ooxml::import_docx(&bytes).unwrap();
    let found = read_pictures(&imported);
    assert_eq!(found[0].bytes, original);
    assert_eq!(found[0].part, "word/media/image1.jpeg");
    // Stored 19x17 px, rotated: 17x19 px -> 12.75 x 14.25 pt.
    assert_eq!(
        (found[0].width_emu, found[0].height_emu),
        (161_925, 180_975)
    );
}

#[test]
fn docx_export_of_plain_text_and_edge_cases_does_not_fail() {
    assert!(export_docx(&load("")).is_ok());
    assert!(export_docx(&load("a\n\n\nb")).is_ok());
    // A picture before a combining mark moves to the next grapheme boundary.
    let session = caret_at(&load("e\u{301}x"), 1);
    let (session, _) = insert_image(&session, &png(4, 4, 22)).unwrap();
    assert!(export_docx(&session).is_ok());
    // Too many paragraphs are refused, not truncated.
    let many = "\n".repeat(16_385);
    assert_eq!(export_docx(&load(&many)).err(), Some("EDITOR_EXPORT_LIMIT"));
}
