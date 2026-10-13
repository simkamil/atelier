// SPDX-License-Identifier: AGPL-3.0-or-later
//! Random and mutated display lists with a fixed seed: the writer never
//! panics, every document it returns is read back by the test reader, and the
//! same input always gives the same bytes.

mod common;

use atelier_fmt_pdf::{PdfError, PdfNotice, PdfOptions, render_pdf};
use atelier_geom::Rect;
use atelier_render::{
    Command, DisplayList, DisplayPath, DisplayTransform, FillRule, LineCap, LineJoin, PathElement,
    PositionedGlyph, StrokeStyle,
};
use common::reader::{Obj, Pdf, content_ops, extract_text};
use common::*;
use std::collections::BTreeMap;

const WORDS: [&str; 8] = [
    "office",
    "Évolution",
    "fi ffi",
    "Œuvre",
    "naïve",
    "(a)\\b",
    "x",
    "A longer line of ordinary text",
];

fn restricted(data: &[u8]) -> &'static [u8] {
    let mut v = data.to_vec();
    let tables = usize::from(u16::from_be_bytes([v[4], v[5]]));
    for i in 0..tables {
        let r = 12 + 16 * i;
        if &v[r..r + 4] == b"OS/2" {
            let off = u32::from_be_bytes([v[r + 8], v[r + 9], v[r + 10], v[r + 11]]) as usize;
            v[off + 8..off + 10].copy_from_slice(&2u16.to_be_bytes());
        }
    }
    Box::leak(v.into_boxed_slice())
}

fn resources() -> Res {
    let mut res = Res::default();
    standard_fonts(&mut res);
    res.add_font(7, 70, restricted(embedded("Carlito", 400, false)), 13.0);
    res.add_image(10, "image/png", png_rgba(9, 7, true));
    res.add_image(11, "image/png", png_rgba(5, 5, false));
    res.add_image(12, "image/jpeg", JPEG_420.to_vec());
    res.add_image(13, "image/jpeg", JPEG_GRAY.to_vec());
    res.add_image(14, "image/jpeg", JPEG_CMYK.to_vec());
    res.add_image(15, "image/jpeg", JPEG_PROGRESSIVE.to_vec());
    res.add_image(16, "image/png", b"broken".to_vec());
    res
}

fn pt(rng: &mut Rng, span: f64) -> atelier_geom::Point {
    p(rng.float(-span, span), rng.float(-span, span))
}

fn random_color(rng: &mut Rng) -> atelier_render::Color {
    let c = |r: &mut Rng| r.float(0.0, 1.0) as f32;
    let (red, green, blue) = (c(rng), c(rng), c(rng));
    let alpha = [1.0, 1.0, 0.5, 0.0, 0.25][rng.below(5)];
    color(red, green, blue, alpha)
}

fn random_path(rng: &mut Rng, span: f64) -> DisplayPath {
    let mut e = vec![PathElement::MoveTo(pt(rng, span))];
    for _ in 0..rng.below(8) {
        e.push(match rng.below(5) {
            0 | 1 => PathElement::LineTo(pt(rng, span)),
            2 => PathElement::QuadraticTo {
                control: pt(rng, span),
                to: pt(rng, span),
            },
            3 => PathElement::CubicTo {
                control1: pt(rng, span),
                control2: pt(rng, span),
                to: pt(rng, span),
            },
            _ => PathElement::Close,
        });
    }
    DisplayPath::new(e).unwrap()
}

fn random_command(
    rng: &mut Rng,
    res: &mut Res,
    clips: &mut usize,
    transforms: &mut usize,
) -> Command {
    let span = 150.0;
    match rng.below(14) {
        0 | 1 => Command::FillPath {
            path: random_path(rng, span),
            color: random_color(rng),
            rule: if rng.below(2) == 0 {
                FillRule::NonZero
            } else {
                FillRule::EvenOdd
            },
        },
        2 => Command::StrokePath {
            path: random_path(rng, span),
            color: random_color(rng),
            style: StrokeStyle::new(
                rng.float(0.1, 12.0),
                [LineCap::Butt, LineCap::Round, LineCap::Square][rng.below(3)],
                [LineJoin::Miter, LineJoin::Round, LineJoin::Bevel][rng.below(3)],
                rng.float(1.0, 10.0),
            )
            .unwrap(),
        },
        3..=5 => {
            let font = [1, 2, 3, 4, 5, 6, 7][rng.below(7)];
            let text = WORDS[rng.below(WORDS.len())];
            let (x, y) = (rng.float(-20.0, 150.0), rng.float(0.0, 150.0));
            res.text(font, text, x, y, random_color(rng))
        }
        6 => Command::DrawGlyphRun {
            // Raw glyph ids: mostly valid for the font, sometimes far outside.
            font_id: [1, 4][rng.below(2)],
            color: random_color(rng),
            glyphs: (0..1 + rng.below(6))
                .map(|i| PositionedGlyph {
                    glyph_id: if rng.below(40) == 0 {
                        rng.below(200_000) as u32
                    } else {
                        rng.below(300) as u32
                    },
                    position: p(10.0 + 9.0 * i as f64, rng.float(10.0, 140.0)),
                    advance: p(9.0, 0.0),
                })
                .collect(),
        },
        7 | 8 => Command::DrawImage {
            image_id: if rng.below(25) == 0 {
                [16, 99][rng.below(2)]
            } else {
                10 + rng.below(6) as u64
            },
            bounds: Rect::from_xywh(
                rng.float(-10.0, 100.0),
                rng.float(-10.0, 100.0),
                rng.float(1.0, 80.0),
                rng.float(1.0, 80.0),
            )
            .unwrap(),
        },
        9 => {
            *clips += 1;
            Command::PushClip {
                bounds: Rect::from_xywh(
                    rng.float(0.0, 80.0),
                    rng.float(0.0, 80.0),
                    rng.float(1.0, 120.0),
                    rng.float(1.0, 120.0),
                )
                .unwrap(),
            }
        }
        10 if *clips > 0 => {
            *clips -= 1;
            Command::PopClip
        }
        11 => {
            *transforms += 1;
            let a = rng.float(0.0, std::f64::consts::TAU);
            let s = rng.float(0.3, 2.0);
            Command::PushTransform {
                transform: DisplayTransform::new(
                    s * a.cos(),
                    s * a.sin(),
                    -s * a.sin(),
                    s * a.cos(),
                    rng.float(-30.0, 60.0),
                    rng.float(-30.0, 60.0),
                )
                .unwrap(),
            }
        }
        12 if *transforms > 0 => {
            *transforms -= 1;
            Command::PopTransform
        }
        10 | 12 => Command::FillPath {
            path: random_path(rng, span),
            color: random_color(rng),
            rule: FillRule::NonZero,
        },
        _ => Command::Link {
            bounds: Rect::from_xywh(rng.float(0.0, 100.0), rng.float(0.0, 100.0), 30.0, 12.0)
                .unwrap(),
            target: [
                "https://example.org/?q=(1)",
                "#chap",
                "#missing",
                "",
                "mailto:a@b.c",
            ][rng.below(5)]
            .to_owned(),
        },
    }
}

fn random_page(rng: &mut Rng, res: &mut Res, case: usize) -> DisplayList {
    let (mut clips, mut transforms) = (0, 0);
    let count = rng.below(30);
    let mut commands: Vec<Command> = (0..count)
        .map(|_| random_command(rng, res, &mut clips, &mut transforms))
        .collect();
    if case % 6 != 0 {
        commands.extend(std::iter::repeat_n(Command::PopClip, clips));
        commands.extend(std::iter::repeat_n(Command::PopTransform, transforms));
    }
    if case % 4 == 1 && !commands.is_empty() {
        for _ in 0..1 + rng.below(3) {
            let i = rng.below(commands.len());
            match rng.below(5) {
                0 => {
                    commands.remove(i);
                }
                1 => commands.insert(i, commands[i].clone()),
                2 => {
                    let j = rng.below(commands.len());
                    commands.swap(i, j);
                }
                3 => {
                    if let Command::FillPath { color, .. } = &mut commands[i] {
                        color.alpha = f32::NAN;
                    }
                }
                _ => {
                    if let Command::FillPath { path, .. } = &mut commands[i] {
                        let mut e = path.elements.clone();
                        e.insert(0, PathElement::MoveTo(p(1.0e12, 0.0)));
                        if let Ok(q) = DisplayPath::new(e) {
                            *path = q;
                        }
                    }
                }
            }
            if commands.is_empty() {
                break;
            }
        }
    }
    let (w, h) = [(200.0, 200.0), (595.0, 842.0), (50.0, 30.0)][rng.below(3)];
    // Pages that failed validation are still handed to the writer.
    DisplayList {
        page: page(w, h),
        commands,
    }
}

/// Everything the reader can check without a viewer.
fn check_document(bytes: &[u8], pages: usize, skip_text: bool) {
    let pdf = Pdf::open(bytes.to_vec()).unwrap_or_else(|e| panic!("unreadable PDF: {e}"));
    let leaves = pdf.pages();
    assert_eq!(leaves.len(), pages);
    for (_, pg) in &leaves {
        let content = pdf.get(pg, "Contents");
        let ops = content_ops(&pdf.stream(&content));
        let (mut depth, mut text, mut marked) = (0i32, false, 0i32);
        for (op, _) in &ops {
            match op.as_str() {
                "q" => depth += 1,
                "Q" => depth -= 1,
                "BT" => {
                    assert!(!text);
                    text = true;
                }
                "ET" => {
                    assert!(text);
                    text = false;
                }
                "BDC" => marked += 1,
                "EMC" => marked -= 1,
                _ => {}
            }
            assert!(depth >= 0 && marked >= 0, "unbalanced {op}");
        }
        assert_eq!((depth, text, marked), (0, false, 0));
        if !skip_text {
            let _ = extract_text(&pdf, pg);
        }
        // Every resource named by the page exists.
        let res = pdf.get(pg, "Resources");
        for (key, name_prefix) in [("Font", "/F"), ("XObject", "/Im"), ("ExtGState", "/GS")] {
            if let Some(dict) = pdf.opt(res.dict(), key) {
                for (n, r) in dict.dict() {
                    let _ = pdf.resolve(r);
                    let used = ops
                        .iter()
                        .any(|(_, args)| args.iter().any(|a| matches!(a, Obj::Name(x) if x == n)));
                    assert!(used, "{name_prefix}{n} declared but unused");
                }
            }
        }
    }
}

#[test]
fn random_and_mutated_lists_never_panic_and_always_read_back() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let (mut ok, mut failed) = (0usize, 0usize);
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut notices: BTreeMap<String, usize> = BTreeMap::new();
    for case in 0..200 {
        let mut res = resources();
        let pages: Vec<DisplayList> = (0..1 + rng.below(3))
            .map(|_| random_page(&mut rng, &mut res, case))
            .collect();
        let options = PdfOptions {
            compress: case % 3 != 0,
            infer_spaces: case % 5 != 0,
            named_destinations: vec![(
                "chap".to_owned(),
                atelier_fmt_pdf::Destination {
                    page: 0,
                    top: Some(10.0),
                },
            )],
            ..PdfOptions::default()
        };
        match render_pdf(&pages, &res, &options) {
            Ok(out) => {
                ok += 1;
                let missing_text = out
                    .notices
                    .iter()
                    .any(|n| matches!(n, PdfNotice::MissingGlyphText { .. }));
                for n in &out.notices {
                    let key = format!("{n:?}");
                    *notices
                        .entry(key.split([' ', '(']).next().unwrap().to_owned())
                        .or_insert(0) += 1;
                }
                check_document(&out.bytes, pages.len(), missing_text);
                let again = render_pdf(&pages, &res, &options).unwrap();
                assert_eq!(again, out, "case {case} is not deterministic");
            }
            Err(e) => {
                failed += 1;
                let _ = e.to_string();
                let key = format!("{e:?}");
                *kinds
                    .entry(key.split(['(', ' ']).next().unwrap().to_owned())
                    .or_insert(0) += 1;
                assert_eq!(render_pdf(&pages, &res, &options), Err(e));
            }
        }
    }
    println!("campaign: {ok} documents, {failed} typed errors {kinds:?}, notices {notices:?}");
    assert!(ok > 100, "{ok} documents, {failed} errors {kinds:?}");
    assert!(failed > 25, "{ok} documents, {failed} errors {kinds:?}");
    assert!(kinds.len() >= 4, "{kinds:?}");
    assert!(kinds.contains_key("InvalidList"), "{kinds:?}");
    let _ = PdfError::EmptyDocument;
}
