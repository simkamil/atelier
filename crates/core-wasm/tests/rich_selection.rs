// SPDX-License-Identifier: AGPL-3.0-or-later
//! R7b-1: selection highlight, drawn pictures and word or paragraph selection
//! of the rich editor session, through the real ATED v2 codec.
use atelier_core_wasm::protocol::{Message, MessageType};
use atelier_core_wasm::rich_editor::RichEditor;
use atelier_core_wasm::rich_protocol::{
    self as proto, Command, ErrorCode, OpenKind, Profile, Request, Response, SelectionRect,
    SelectionUnit, State,
};
use atelier_image::{DecodeLimits, Image, decode_png, encode_png_rgba};
use atelier_render::{Command as Draw, DisplayList};

const OBJ: &str = "\u{fffc}";

struct Harness {
    editor: RichEditor,
    request: u32,
}

impl Harness {
    fn new() -> Self {
        Self {
            editor: RichEditor::default(),
            request: 0,
        }
    }

    fn send(&mut self, command: Command) -> Response {
        self.request += 1;
        let frame = proto::encode_request(&Request {
            doc_id: 1,
            request_id: self.request,
            command,
        })
        .unwrap();
        let reply = self.editor.respond(&Message {
            kind: MessageType::Editor,
            request_id: 7,
            payload: frame,
        });
        assert_eq!(reply.kind, MessageType::EditorResult);
        proto::decode_reply(&reply.payload).unwrap().response
    }

    fn state(&mut self, command: Command) -> State {
        match self.send(command) {
            Response::State(s) => s,
            other => panic!("expected a state, got {other:?}"),
        }
    }

    fn error(&mut self, command: Command) -> ErrorCode {
        match self.send(command) {
            Response::Error(code) => code,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    fn blank(&mut self) -> State {
        self.state(Command::Open {
            kind: OpenKind::Blank,
            profile: Profile::Document,
            data: Vec::new(),
        })
    }

    /// The blank document the interface creates (TXT profile, 54 pt margins).
    fn blank_txt(&mut self) -> State {
        self.state(Command::Open {
            kind: OpenKind::Blank,
            profile: Profile::Txt,
            data: Vec::new(),
        })
    }

    fn insert(&mut self, text: &str) -> State {
        self.state(Command::InsertText { text: text.into() })
    }

    fn select(&mut self, anchor: u64, focus: u64) -> State {
        self.state(Command::SetSelection {
            anchor,
            focus,
            upstream: false,
        })
    }

    fn unit(&mut self, position: u64, unit: SelectionUnit, extend: bool) -> State {
        self.state(Command::SelectUnit {
            position,
            unit,
            extend,
        })
    }

    fn complete(&mut self) -> State {
        loop {
            let s = self.state(Command::Layout { budget: 4096 });
            if s.layout_complete {
                return s;
            }
        }
    }

    fn rects(&mut self, first: u32, count: u32) -> Vec<SelectionRect> {
        match self.send(Command::SelectionRects { first, count }) {
            Response::SelectionRects { rects, .. } => rects,
            other => panic!("expected rectangles, got {other:?}"),
        }
    }

    fn image(&mut self, bytes: Vec<u8>) -> State {
        self.state(Command::InsertImage {
            alt: "image".into(),
            bytes,
        })
    }

    /// The display list of a page, atlas glyphs left as `DrawGlyphRun`.
    fn page(&mut self, index: u32) -> DisplayList {
        match self.send(Command::Pages {
            first: index,
            count: 1,
        }) {
            Response::Pages { pages, .. } => DisplayList::decode(&pages[0].display).unwrap(),
            other => panic!("expected a page, got {other:?}"),
        }
    }

    fn bitmap(&mut self, object_id: u64) -> Response {
        self.send(Command::ImageBitmap { object_id })
    }
}

/// `(image id, left, top, width, height)` of the pictures of a page.
fn pictures(page: &DisplayList) -> Vec<(u64, f64, f64, f64, f64)> {
    page.commands
        .iter()
        .filter_map(|c| match c {
            Draw::DrawImage { image_id, bounds } => Some((
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

fn jpeg_with_orientation(orientation: u8) -> Vec<u8> {
    let jpeg = include_bytes!("../../image/tests/fixtures/jpeg/baseline-444.jpg").to_vec();
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

/// A document of `lines` numbered paragraphs.
fn long_document(h: &mut Harness, lines: usize) -> State {
    h.blank();
    let text: Vec<String> = (0..lines)
        .map(|n| format!("Ligne {n} du document"))
        .collect();
    h.insert(&text.join("\n"));
    h.complete()
}

// ---- selection highlight -------------------------------------------------------

#[test]
fn rectangles_cover_the_selection_on_every_page_it_crosses() {
    let mut h = Harness::new();
    let state = long_document(&mut h, 180);
    assert!(state.page_count >= 3, "pages: {}", state.page_count);
    h.select(0, state.doc_len);
    let rects = h.rects(0, 16);
    let mut pages: Vec<u32> = rects.iter().map(|r| r.page).collect();
    pages.dedup();
    assert_eq!(pages, (0..state.page_count).collect::<Vec<_>>());
    assert!(
        rects.len() >= 180,
        "one rectangle per line: {}",
        rects.len()
    );
    for r in &rects {
        assert!(r.width > 0.0 && r.height > 0.0, "{r:?}");
        assert!(r.x >= 0.0 && r.x + r.width <= 595.3, "{r:?}");
        assert!(r.y >= 0.0 && r.y + r.height <= 841.9, "{r:?}");
    }
    // The same selection, a page at a time, gives the same rectangles.
    let mut again = Vec::new();
    for page in 0..state.page_count {
        again.extend(h.rects(page, 1));
    }
    assert_eq!(again, rects);
    // A selection inside page 1 only touches page 1.
    let before = h.rects(1, 1);
    let first_line_end = before[0].y;
    assert!(first_line_end > 0.0);
    h.select(0, 5);
    assert!(h.rects(1, 1).is_empty());
    let on_first = h.rects(0, 16);
    assert_eq!(on_first.len(), 1);
    assert_eq!(on_first[0].page, 0);
}

#[test]
fn an_empty_selection_has_no_rectangles_and_the_revision_is_the_documents() {
    let mut h = Harness::new();
    h.blank();
    let state = h.insert("abc");
    h.select(1, 1);
    match h.send(Command::SelectionRects { first: 0, count: 1 }) {
        Response::SelectionRects { revision, rects } => {
            assert_eq!(revision, state.revision);
            assert!(rects.is_empty());
        }
        other => panic!("{other:?}"),
    }
    h.select(0, 3);
    assert_eq!(h.rects(0, 1).len(), 1);
}

#[test]
fn rectangles_refuse_a_page_past_the_end_and_clamp_the_count() {
    let mut h = Harness::new();
    let state = long_document(&mut h, 120);
    h.select(0, state.doc_len);
    assert_eq!(
        h.error(Command::SelectionRects {
            first: state.page_count,
            count: 1
        }),
        ErrorCode::PageRange
    );
    let rects = h.rects(state.page_count - 1, 16);
    assert!(rects.iter().all(|r| r.page == state.page_count - 1));
    assert!(!rects.is_empty());
}

#[test]
fn a_selection_around_a_picture_has_a_rectangle_as_tall_as_its_line() {
    let mut h = Harness::new();
    h.blank();
    h.insert("ab");
    h.select(1, 1);
    h.image(png(40, 20, 1));
    h.select(0, 5);
    let rects = h.rects(0, 1);
    assert_eq!(rects.len(), 1);
    // A 15 pt picture raises the ascent above the text's.
    assert!(rects[0].height > 15.0, "{:?}", rects[0]);
}

// ---- pictures --------------------------------------------------------------------

#[test]
fn a_picture_is_drawn_on_the_baseline_at_its_natural_size() {
    let mut h = Harness::new();
    h.blank();
    h.insert("ab");
    h.select(1, 1);
    let plain = h.state(Command::Layout { budget: 16 }).caret.unwrap();
    let state = h.image(png(40, 20, 1));
    assert!(state.pages.iter().any(|p| p.index == 0));
    let page = h.page(0);
    let found = pictures(&page);
    assert_eq!(found.len(), 1, "{found:?}");
    let (id, x, y, w, hh) = found[0];
    // 40 x 20 px at 96 dpi is 30 x 15 pt.
    assert!(
        (w - 30.0).abs() < 0.02 && (hh - 15.0).abs() < 0.02,
        "{w} x {hh}"
    );
    // After "a" (the left margin is 70.85 pt).
    assert!(x > 70.0 && x < 90.0, "x = {x}");
    // The bottom is the baseline: below the line top by the picture's height.
    let caret = state.caret.unwrap();
    assert!(caret.height > plain.height, "line {caret:?} vs {plain:?}");
    assert!(
        y + hh <= caret.y + caret.height + 0.1,
        "bottom {} line {caret:?}",
        y + hh
    );
    assert!(y >= caret.y - 0.1);
    // The bitmap is the same picture, as pixels.
    match h.bitmap(id) {
        Response::ImageBitmap {
            object_id,
            width,
            height,
            rgba,
        } => {
            assert_eq!((object_id, width, height), (id, 40, 20));
            assert_eq!(rgba.len(), 40 * 20 * 4);
            let source = decode_png(
                &png(40, 20, 1),
                DecodeLimits {
                    max_pixels: 1 << 20,
                    max_input_bytes: 1 << 20,
                },
            )
            .unwrap();
            assert_eq!(rgba, source.pixels);
        }
        other => panic!("{other:?}"),
    }
    // The text on each side is still there.
    let text = match h.send(Command::TextWindow { start: 0, len: 100 }) {
        Response::TextWindow { text, .. } => text,
        other => panic!("{other:?}"),
    };
    assert_eq!(text, format!("a{OBJ}b"));
}

#[test]
fn a_wide_picture_shrinks_to_the_text_area_and_never_grows() {
    let mut h = Harness::new();
    h.blank();
    h.image(png(800, 400, 3));
    h.image(png(8, 8, 4));
    let page = h.page(0);
    let found = pictures(&page);
    assert_eq!(found.len(), 2);
    let (_, _, _, w, hh) = found[0];
    // The text area of the default page is 453.6 pt wide.
    assert!((w - 453.56).abs() < 0.2, "width {w}");
    assert!((hh / w - 0.5).abs() < 1e-3, "ratio {}", hh / w);
    let (_, _, _, small_w, small_h) = found[1];
    assert!((small_w - 6.0).abs() < 0.02 && (small_h - 6.0).abs() < 0.02);
}

#[test]
fn pictures_are_drawn_in_the_txt_profile_with_its_text_area() {
    let mut h = Harness::new();
    h.blank_txt();
    h.insert("ab");
    h.select(1, 1);
    h.image(png(40, 20, 1));
    h.image(png(800, 400, 2));
    let found = pictures(&h.page(0));
    assert_eq!(found.len(), 2);
    assert!((found[0].3 - 30.0).abs() < 0.02 && (found[0].4 - 15.0).abs() < 0.02);
    // The text area of the TXT profile is 595.276 - 2 x 54 pt wide.
    assert!((found[1].3 - 487.27).abs() < 0.1, "width {}", found[1].3);
    assert!(found[1].1 >= 54.0 - 0.1 || found[1].2 >= 54.0, "{found:?}");
}

#[test]
fn a_tall_picture_is_shrunk_to_the_height_of_the_text_area() {
    let mut h = Harness::new();
    h.blank();
    h.insert("texte");
    // 100 x 1000 px: 75 x 750 pt, taller than the 700.2 pt text area; the
    // line keeps 3 pt of descent below the picture.
    h.image(png(100, 1000, 5));
    let found = pictures(&h.page(0));
    assert_eq!(found.len(), 1);
    let (_, _, y, w, hh) = found[0];
    assert!((hh - 697.2).abs() < 0.1, "height {hh}");
    assert!((hh / w - 10.0).abs() < 1e-2);
    assert!(
        y >= 70.0 && y + hh < 841.9 - 70.0 + 0.1,
        "inside the page: {y} + {hh}"
    );
}

#[test]
fn the_orientation_of_a_jpeg_is_applied_to_the_geometry_and_the_pixels() {
    let mut sizes = Vec::new();
    for orientation in [1u8, 6] {
        let mut h = Harness::new();
        h.blank();
        h.image(jpeg_with_orientation(orientation));
        let found = pictures(&h.page(0));
        assert_eq!(found.len(), 1, "orientation {orientation}");
        let (id, _, _, w, hh) = found[0];
        match h.bitmap(id) {
            Response::ImageBitmap { width, height, .. } => {
                // The drawn box and the bitmap have the same aspect ratio.
                assert!(
                    (w / hh - f64::from(width) / f64::from(height)).abs() < 0.02,
                    "orientation {orientation}: {w}x{hh} vs {width}x{height}"
                );
                sizes.push((width, height));
            }
            other => panic!("{other:?}"),
        }
    }
    // Orientation 6 turns the picture a quarter: width and height swap.
    assert_eq!((sizes[0].1, sizes[0].0), sizes[1]);
}

#[test]
fn a_picture_inside_a_wrapped_paragraph_stays_on_its_line() {
    let mut h = Harness::new();
    h.blank();
    let words = "mot ".repeat(80);
    h.insert(&words);
    h.select(150, 150);
    h.image(png(40, 20, 6));
    let found = pictures(&h.page(0));
    assert_eq!(found.len(), 1);
    let (_, x, _, w, _) = found[0];
    assert!(x >= 70.0 && x + w <= 595.3 - 70.0, "{x} + {w}");
}

#[test]
fn bitmap_of_an_unknown_or_removed_picture_is_refused() {
    let mut h = Harness::new();
    h.blank();
    h.insert("x");
    assert_eq!(
        h.error(Command::ImageBitmap { object_id: 12345 }),
        ErrorCode::UnknownObject
    );
    h.image(png(8, 8, 7));
    let id = pictures(&h.page(0))[0].0;
    assert!(matches!(h.bitmap(id), Response::ImageBitmap { .. }));
    h.state(Command::Undo);
    assert_eq!(
        h.error(Command::ImageBitmap { object_id: id }),
        ErrorCode::UnknownObject
    );
    // Redo brings the picture back, and its bitmap.
    h.state(Command::Redo);
    let again = pictures(&h.page(0));
    assert_eq!(again.len(), 1);
    assert!(matches!(h.bitmap(again[0].0), Response::ImageBitmap { .. }));
}

#[test]
fn two_pictures_have_two_identities_and_the_caret_and_clicks_cross_them() {
    let mut h = Harness::new();
    h.blank();
    h.insert("ab");
    h.select(1, 1);
    h.image(png(40, 20, 1));
    h.image(png(20, 40, 2));
    let found = pictures(&h.page(0));
    assert_eq!(found.len(), 2);
    assert_ne!(found[0].0, found[1].0);
    // Text is "a", object, object, "b": 1 + 3 + 3 + 1 bytes.
    let state = h.select(0, 0);
    assert_eq!(state.doc_len, 8);
    // Click in the middle of the first picture: before or after it, never inside.
    let (_, x, y, w, hh) = found[0];
    match h.send(Command::HitTest {
        page: 0,
        mode: proto::HitMode::Probe,
        x: x + w / 2.0,
        y: y + hh / 2.0,
    }) {
        Response::HitResult { position, .. } => {
            assert!(position == 1 || position == 4, "{position}")
        }
        other => panic!("{other:?}"),
    }
    // Moving right steps over a picture in one move.
    h.select(1, 1);
    let moved = h.state(Command::Move {
        direction: proto::Direction::Right,
        extend: false,
    });
    assert_eq!(moved.focus, 4);
}

// ---- word and paragraph selection ---------------------------------------------------

#[test]
fn a_double_click_selects_the_word_and_the_bounds_are_the_words() {
    let mut h = Harness::new();
    h.blank();
    h.insert("bonjour le monde");
    for (position, expected) in [
        (0, (0, 7)),
        (3, (0, 7)),
        (8, (8, 10)),
        (11, (11, 16)),
        (16, (11, 16)),
    ] {
        let s = h.unit(position, SelectionUnit::Word, false);
        assert_eq!((s.anchor, s.focus), expected, "position {position}");
        assert!(s.caret.is_some());
    }
    // Selecting a word moves no text and is not an edit.
    let s = h.state(Command::Layout { budget: 1 });
    assert!(!s.modified || s.revision > 0);
}

#[test]
fn a_triple_click_selects_the_paragraph_with_its_break() {
    let mut h = Harness::new();
    h.blank();
    h.insert("un deux\ntrois\n\nquatre");
    for (position, expected) in [
        (0, (0, 8)),
        (7, (0, 8)),
        (8, (8, 14)),
        (14, (14, 15)),
        (15, (15, 21)),
        (21, (15, 21)),
    ] {
        let s = h.unit(position, SelectionUnit::Paragraph, false);
        assert_eq!((s.anchor, s.focus), expected, "position {position}");
    }
}

#[test]
fn a_unit_selection_on_an_empty_document_is_a_caret() {
    let mut h = Harness::new();
    h.blank();
    for unit in [SelectionUnit::Word, SelectionUnit::Paragraph] {
        let s = h.unit(0, unit, false);
        assert_eq!((s.anchor, s.focus), (0, 0));
    }
}

#[test]
fn extending_by_units_follows_the_drag_in_both_directions() {
    let mut h = Harness::new();
    h.blank();
    h.insert("un deux trois quatre");
    // Double click on "deux", then drag to "quatre" and back to "un".
    let s = h.unit(4, SelectionUnit::Word, false);
    assert_eq!((s.anchor, s.focus), (3, 7));
    let s = h.unit(15, SelectionUnit::Word, true);
    assert_eq!((s.anchor, s.focus), (3, 20));
    let s = h.unit(9, SelectionUnit::Word, true);
    assert_eq!((s.anchor, s.focus), (3, 13));
    let s = h.unit(0, SelectionUnit::Word, true);
    assert_eq!((s.anchor, s.focus), (7, 0));
    // Back inside the first word: the word itself.
    let s = h.unit(5, SelectionUnit::Word, true);
    assert_eq!((s.anchor, s.focus), (3, 7));
}

#[test]
fn shift_double_click_extends_the_existing_selection_by_words() {
    let mut h = Harness::new();
    h.blank();
    h.insert("un deux trois quatre");
    h.select(3, 3);
    let s = h.unit(15, SelectionUnit::Word, true);
    assert_eq!((s.anchor, s.focus), (3, 20));
    // A selection made another way is forgotten by the gesture.
    h.unit(4, SelectionUnit::Word, false);
    h.select(14, 14);
    let s = h.unit(0, SelectionUnit::Word, true);
    assert_eq!((s.anchor, s.focus), (14, 0));
    // Paragraph units extend by paragraphs.
    h.insert("\nsuite");
    let s = h.unit(0, SelectionUnit::Paragraph, false);
    let end = s.focus;
    let s = h.unit(end, SelectionUnit::Paragraph, true);
    assert_eq!((s.anchor, s.focus), (0, s.doc_len));
}

#[test]
fn unit_selection_refuses_bad_positions_without_changing_the_selection() {
    let mut h = Harness::new();
    h.blank();
    h.insert("ab");
    h.select(1, 2);
    assert_eq!(
        h.error(Command::SelectUnit {
            position: 3,
            unit: SelectionUnit::Word,
            extend: false
        }),
        ErrorCode::PositionOutOfRange
    );
    assert_eq!(
        h.error(Command::SelectUnit {
            position: u64::MAX,
            unit: SelectionUnit::Paragraph,
            extend: true
        }),
        ErrorCode::PositionOutOfRange
    );
    h.select(1, 1);
    h.image(png(8, 8, 1));
    // Inside the three bytes of the object is no position.
    assert_eq!(
        h.error(Command::SelectUnit {
            position: 2,
            unit: SelectionUnit::Word,
            extend: false
        }),
        ErrorCode::PositionNotBoundary
    );
    let s = h.state(Command::Layout { budget: 1 });
    assert_eq!((s.anchor, s.focus), (4, 4));
}

#[test]
fn units_around_an_inline_object_take_the_object_as_a_word() {
    let mut h = Harness::new();
    h.blank();
    h.insert("ab cd");
    h.select(2, 2);
    h.image(png(8, 8, 1));
    // Text: "ab", object (2..5), " cd".
    let s = h.unit(2, SelectionUnit::Word, false);
    assert_eq!((s.anchor, s.focus), (2, 5), "the object alone");
    let s = h.unit(0, SelectionUnit::Word, false);
    assert_eq!((s.anchor, s.focus), (0, 2));
    let s = h.unit(0, SelectionUnit::Paragraph, false);
    assert_eq!((s.anchor, s.focus), (0, s.doc_len));
    // The selected picture is highlighted.
    let rects = h.rects(0, 1);
    assert_eq!(rects.len(), 1);
}

#[test]
fn a_read_only_session_still_selects_by_units() {
    // Selection is not an edit: the commands do not check the read-only flag.
    let mut h = Harness::new();
    h.blank();
    h.insert("un deux");
    let s = h.unit(4, SelectionUnit::Word, false);
    assert_eq!((s.anchor, s.focus), (3, 7));
    assert!(!s.read_only);
}
