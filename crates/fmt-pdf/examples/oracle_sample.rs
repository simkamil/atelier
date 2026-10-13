// SPDX-License-Identifier: AGPL-3.0-or-later
//! Writes sample PDFs and the matching `atelier-raster` rendering for the
//! external-oracle scripts in `tools/` (nothing in the test suite depends on
//! them).
//!
//! `cargo run -p atelier-fmt-pdf --example oracle_sample -- <outdir> [font.ttf|font.otf]`

#[path = "../tests/common/mod.rs"]
mod common;

use atelier_fmt_pdf::{Destination, OutlineItem, PdfDate, PdfInfo, PdfOptions, render_pdf};
use atelier_raster::display_list::{RenderOptions, Resolution, render_png};
use common::*;
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("output directory"));
    std::fs::create_dir_all(&dir).unwrap();

    // 1. The typical page.
    let mut res = Res::default();
    let page = typical_page(&mut res);
    let options = PdfOptions {
        info: PdfInfo {
            title: Some("Page type — Atelier".to_owned()),
            author: Some("Zoé Müller".to_owned()),
            creation_date: Some(PdfDate {
                year: 2026,
                month: 10,
                day: 12,
                hour: 9,
                minute: 0,
                second: 0,
                utc_offset_minutes: 0,
            }),
            ..PdfInfo::default()
        },
        ..PdfOptions::default()
    };
    let out = render_pdf(std::slice::from_ref(&page), &res, &options).unwrap();
    std::fs::write(dir.join("sample.pdf"), &out.bytes).unwrap();
    let opts = RenderOptions {
        resolution: Resolution::Dpi(96.0),
        ..RenderOptions::default()
    };
    std::fs::write(
        dir.join("sample-raster.png"),
        render_png(&page, &res, &opts).unwrap(),
    )
    .unwrap();
    let mut expected = String::new();
    for l in LINES {
        expected.push_str(l);
        expected.push('\n');
    }
    expected.push_str("Éléments d’un écrit — Œuvre\nTexte tourné de quinze degrés\nBig\n");
    std::fs::write(dir.join("sample.txt"), expected).unwrap();

    // 2. Three pages, shared fonts, links and bookmarks.
    let mut res = Res::default();
    standard_fonts(&mut res);
    // Same program as CARLITO, own font id: it carries the literal U+FB01 of
    // page 2, while CARLITO's ligature glyph stands for "fi" on page 3.
    res.add_font(8, 1, embedded("Carlito", 400, false), 12.0);
    let texts = [
        "Premier chapitre : où l’été déjà s’achève.",
        "Deuxième chapitre : Œdipe, Noël, façade, naïveté, ﬁn.",
        "Troisième chapitre : office, difficile, affluent, flux, final.",
    ];
    let pages: Vec<_> = texts
        .iter()
        .map(|t| {
            list(
                595.0,
                842.0,
                vec![
                    res.text(CARLITO_BOLD, "Document de test", 56.0, 80.0, BLACK),
                    res.text(
                        if t.contains('\u{FB01}') { 8 } else { CARLITO },
                        t,
                        56.0,
                        120.0,
                        BLACK,
                    ),
                    atelier_render::Command::Link {
                        bounds: atelier_geom::Rect::from_xywh(56.0, 106.0, 300.0, 18.0).unwrap(),
                        target: "#chap2".to_owned(),
                    },
                    atelier_render::Command::Link {
                        bounds: atelier_geom::Rect::from_xywh(56.0, 140.0, 100.0, 18.0).unwrap(),
                        target: "https://example.org/".to_owned(),
                    },
                ],
            )
        })
        .collect();
    let item = |t: &str, page: usize| OutlineItem {
        title: t.to_owned(),
        destination: Destination {
            page,
            top: Some(100.0),
        },
        children: vec![],
        open: true,
    };
    let options = PdfOptions {
        named_destinations: vec![(
            "chap2".to_owned(),
            Destination {
                page: 1,
                top: Some(100.0),
            },
        )],
        outlines: vec![
            item("Chapitre 1", 0),
            item("Chapitre 2", 1),
            item("Chapitre 3", 2),
        ],
        ..PdfOptions::default()
    };
    let out = render_pdf(&pages, &res, &options).unwrap();
    std::fs::write(dir.join("multi.pdf"), &out.bytes).unwrap();
    let mut expected = String::new();
    for t in texts {
        expected.push_str("Document de test\n");
        expected.push_str(t);
        expected.push('\n');
    }
    std::fs::write(dir.join("multi.txt"), expected).unwrap();

    // 3. Every JPEG kind passed through, with the raster rendering of the decoded pixels.
    let mut res = Res::default();
    let ycck: &[u8] = include_bytes!("../../image/tests/fixtures/jpeg/adobe-ycck.jpg");
    let kinds: [(&str, &[u8]); 6] = [
        ("baseline", JPEG_420),
        ("gray", JPEG_GRAY),
        ("progressive", JPEG_PROGRESSIVE),
        ("cmyk", JPEG_CMYK),
        ("ycck", ycck),
        ("png", &[]),
    ];
    let mut commands = Vec::new();
    for (i, (_, bytes)) in kinds.iter().enumerate() {
        let id = 20 + i as u64;
        if bytes.is_empty() {
            res.add_image(id, "image/png", png_rgba(16, 16, true));
        } else {
            res.add_image(id, "image/jpeg", bytes.to_vec());
        }
        commands.push(atelier_render::Command::DrawImage {
            image_id: id,
            bounds: atelier_geom::Rect::from_xywh(10.0 + 70.0 * i as f64, 10.0, 60.0, 60.0)
                .unwrap(),
        });
    }
    let page = list(430.0, 80.0, commands);
    let out = render_pdf(std::slice::from_ref(&page), &res, &PdfOptions::default()).unwrap();
    std::fs::write(dir.join("jpegs.pdf"), &out.bytes).unwrap();
    std::fs::write(
        dir.join("jpegs-raster.png"),
        render_png(&page, &res, &opts).unwrap(),
    )
    .unwrap();

    // 4. Optional: any TrueType or OpenType/CFF font from disk.
    if let Some(path) = args.next() {
        let data: &'static [u8] = Box::leak(std::fs::read(&path).unwrap().into_boxed_slice());
        let mut res = Res::default();
        res.add_font(1, 1, data, 14.0);
        let text = "Voilà un texte accentué, déjà vu : office, difficile.";
        let page = list(595.0, 200.0, vec![res.text(1, text, 40.0, 60.0, BLACK)]);
        let out = render_pdf(&[page], &res, &PdfOptions::default()).unwrap();
        std::fs::write(dir.join("font.pdf"), &out.bytes).unwrap();
        std::fs::write(dir.join("font.txt"), format!("{text}\n")).unwrap();
        println!("font notices: {:?}", out.notices);
    }
}
