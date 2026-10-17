// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-331: structure, text, shapes, images, metadata, links and bookmarks,
//! read back with the independent reader in `common/reader.rs`.

mod common;

use atelier_fmt_pdf::{
    Destination, OutlineItem, PdfDate, PdfNotice, PdfOptions, PdfWriter, render_pdf,
};
use atelier_font::FontFace;
use atelier_font::cff::CffFont;
use atelier_render::{Command, DisplayList, PositionedGlyph};
use common::reader::{Obj, Pdf, content_ops, extract_text, parse_tounicode};
use common::*;

fn open(bytes: &[u8]) -> Pdf {
    Pdf::open(bytes.to_vec()).unwrap_or_else(|e| panic!("invalid PDF: {e}"))
}

fn render(pages: &[DisplayList], res: &Res) -> Vec<u8> {
    render_pdf(pages, res, &PdfOptions::default())
        .unwrap()
        .bytes
}

/// The page's resolved `/Font` entries: resource name -> (Type0, CIDFont, descriptor).
fn page_fonts(pdf: &Pdf, page: &common::reader::Dict) -> Vec<(String, Obj, Obj, Obj)> {
    let res = pdf.get(page, "Resources");
    let Some(fonts) = pdf.opt(res.dict(), "Font") else {
        return Vec::new();
    };
    fonts
        .dict()
        .iter()
        .map(|(n, f)| {
            let t0 = pdf.resolve(f);
            let cid = pdf.resolve(&t0.dict()["DescendantFonts"].arr()[0]);
            let desc = pdf.get(cid.dict(), "FontDescriptor");
            (n.clone(), t0, cid, desc)
        })
        .collect()
}

#[test]
fn typical_page_is_a_valid_pdf_with_selectable_text() {
    let mut res = Res::default();
    let page = typical_page(&mut res);
    let out = render_pdf(&[page], &res, &PdfOptions::default()).unwrap();
    assert!(out.notices.is_empty(), "{:?}", out.notices);
    let pdf = open(&out.bytes);
    let pages = pdf.pages();
    assert_eq!(pages.len(), 1);
    let (_, pg) = &pages[0];
    assert_eq!(
        pg["MediaBox"]
            .arr()
            .iter()
            .map(Obj::num)
            .collect::<Vec<_>>(),
        [0.0, 0.0, 595.0, 842.0]
    );

    // Text comes back as it went in, accents and ligatures included.
    let text = extract_text(&pdf, pg);
    for line in LINES {
        assert!(
            text.iter().any(|t| t == line),
            "missing {line:?} in {text:#?}"
        );
    }
    assert!(text.contains(&"Éléments d’un écrit — Œuvre".to_owned()));
    assert!(text.contains(&"Texte tourné de quinze degrés".to_owned()));
    assert!(text.contains(&"Big".to_owned()));

    // Five programs, six ids: five embedded fonts, one per program.
    let fonts = page_fonts(&pdf, pg);
    assert_eq!(fonts.len(), 5);
    for (_, t0, cid, desc) in &fonts {
        let t0d = t0.dict();
        assert_eq!(t0d["Subtype"].name(), "Type0");
        assert_eq!(t0d["Encoding"].name(), "Identity-H");
        let base = t0d["BaseFont"].name();
        let (tag, name) = base.split_once('+').expect("subset prefix");
        assert_eq!(tag.len(), 6);
        assert!(tag.bytes().all(|b| b.is_ascii_uppercase()));
        assert!(!name.is_empty());
        let cd = cid.dict();
        assert_eq!(cd["Subtype"].name(), "CIDFontType2");
        assert_eq!(cd["CIDToGIDMap"].name(), "Identity");
        assert_eq!(
            cd["CIDSystemInfo"].dict()["Ordering"].str_bytes(),
            b"Identity"
        );
        let dd = desc.dict();
        assert_eq!(dd["FontName"].name(), base);
        for k in [
            "Flags",
            "FontBBox",
            "ItalicAngle",
            "Ascent",
            "Descent",
            "CapHeight",
            "StemV",
        ] {
            assert!(dd.contains_key(k), "descriptor lacks {k}");
        }
        // The embedded program is a readable TrueType subset.
        let file = pdf.get(dd, "FontFile2");
        let program = pdf.stream(&file);
        assert_eq!(file.dict()["Length1"].int() as usize, program.len());
        let face = FontFace::parse(&program).expect("subset font parses");
        assert!(face.metrics().number_of_glyphs > 1);
        // /W covers the glyphs and agrees with the program's advances.
        let w = cd["W"].arr();
        let upem = f64::from(face.metrics().units_per_em);
        let mut seen = 0;
        let mut i = 0;
        while i < w.len() {
            let first = w[i].int() as u16;
            for (k, wd) in w[i + 1].arr().iter().enumerate() {
                let gid = first + k as u16;
                let adv = f64::from(face.horizontal_metric(gid).unwrap().advance_width);
                assert!((wd.num() - (adv * 1000.0 / upem).round()).abs() < 1.0);
                seen += 1;
            }
            i += 2;
        }
        assert!(seen > 3);
        // ToUnicode keys are glyph ids of the program.
        let tu = parse_tounicode(&pdf.stream(&pdf.get(t0d, "ToUnicode")));
        assert!(tu.len() > 3);
    }

    // The ligature glyph maps back to two or three characters.
    let carlito = fonts
        .iter()
        .find(|(_, _, _, d)| {
            d.dict()["FontName"].name().contains("Carlito")
                && !d.dict()["FontName"].name().contains("Bold")
                && !d.dict()["FontName"].name().contains("Italic")
        })
        .unwrap();
    let tu = parse_tounicode(&pdf.stream(&pdf.get(carlito.1.dict(), "ToUnicode")));
    assert!(
        tu.values().any(|t| t.chars().count() > 1),
        "no ligature in the CMap: {tu:?}"
    );

    // Content stream shape: balanced q/Q, flip, clip, transparency.
    let content = pdf.get(pg, "Contents");
    let ops = content_ops(&pdf.stream(&content));
    assert_eq!(ops[0].0, "cm");
    let mut depth = 0i32;
    for (op, _) in &ops {
        match op.as_str() {
            "q" => depth += 1,
            "Q" => depth -= 1,
            _ => {}
        }
        assert!(depth >= 0);
    }
    assert_eq!(depth, 0);
    let count = |name: &str| ops.iter().filter(|(o, _)| o == name).count();
    assert_eq!(count("W"), 1);
    assert!(count("f*") == 1 && count("S") == 2 && count("Do") == 2);
    assert!(count("gs") >= 2 && count("BT") == count("ET") && count("BT") >= 8);
    for (_, args) in &ops {
        for a in args {
            if let Obj::Real(r) = a {
                assert!(r.is_finite());
            }
        }
    }
    let raw = String::from_utf8_lossy(&pdf.stream(&content)).into_owned();
    assert!(!raw.contains('e') || raw.contains("TJ")); // no exponent notation
    for tok in raw.split_whitespace() {
        let numeric = tok
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.' || b == b'-');
        if numeric {
            assert!(!tok.ends_with('.') && !tok.starts_with("-0 ") && tok != "-0");
        }
        assert!(!tok.eq_ignore_ascii_case("nan") && !tok.contains("inf"));
    }

    // ExtGState objects carry equal fill and stroke alpha.
    let r = pdf.get(pg, "Resources");
    let gs = pdf.get(r.dict(), "ExtGState");
    assert_eq!(gs.dict().len(), 2); // 0.5 shared by the red rectangle, blue rectangle and text
    for g in gs.dict().values() {
        let g = pdf.resolve(g);
        assert_eq!(g.dict()["ca"], g.dict()["CA"]);
    }
    assert!(pg.contains_key("Group"));

    // Images: PNG with alpha -> Flate + SMask, JPEG as is.
    let xo = pdf.get(r.dict(), "XObject");
    assert_eq!(xo.dict().len(), 2);
    let imgs: Vec<Obj> = xo.dict().values().map(|o| pdf.resolve(o)).collect();
    let jpeg = imgs
        .iter()
        .find(|i| i.dict().get("Filter") == Some(&Obj::Name("DCTDecode".into())))
        .unwrap();
    let Obj::Stream(_, raw_jpeg) = jpeg else {
        panic!()
    };
    assert_eq!(raw_jpeg.as_slice(), JPEG_420);
    assert_eq!(jpeg.dict()["ColorSpace"].name(), "DeviceRGB");
    assert_eq!(
        (jpeg.dict()["Width"].int(), jpeg.dict()["Height"].int()),
        (19, 17)
    );
    let png = imgs
        .iter()
        .find(|i| i.dict().contains_key("SMask"))
        .unwrap();
    assert_eq!(png.dict()["Width"].int(), 8);
    assert_eq!(pdf.stream(png).len(), 8 * 8 * 3);
    let mask = pdf.get(png.dict(), "SMask");
    assert_eq!(mask.dict()["ColorSpace"].name(), "DeviceGray");
    assert_eq!(pdf.stream(&mask).len(), 64);

    // Link annotation with an ASCII-only URI.
    let annots = pg["Annots"].arr();
    assert_eq!(annots.len(), 1);
    let a = pdf.resolve(&annots[0]);
    assert_eq!(a.dict()["Subtype"].name(), "Link");
    let uri = pdf.get(a.dict(), "A");
    assert_eq!(
        uri.dict()["URI"].str_bytes(),
        b"https://example.org/a%20b?x=%C3%A9"
    );
    let rect: Vec<f64> = a.dict()["Rect"].arr().iter().map(Obj::num).collect();
    assert_eq!(rect, [56.0, 842.0 - 125.0, 256.0, 842.0 - 105.0]);
}

#[test]
fn shared_fonts_are_embedded_once_with_the_union_of_glyphs() {
    let mut res = Res::default();
    standard_fonts(&mut res);
    let p1 = list(
        300.0,
        200.0,
        vec![res.text(CARLITO, "abc", 10.0, 50.0, BLACK)],
    );
    let p2 = list(
        300.0,
        200.0,
        vec![res.text(CARLITO_BIG, "xyz Q", 10.0, 50.0, BLACK)],
    );
    let p3 = list(
        300.0,
        200.0,
        vec![res.text(CARLITO, "é", 10.0, 50.0, BLACK)],
    );
    let bytes = render(&[p1, p2, p3], &res);
    let pdf = open(&bytes);
    let pages = pdf.pages();
    assert_eq!(pages.len(), 3);
    // One font object serves the three pages (two font ids, one program).
    let refs: Vec<u32> = pages
        .iter()
        .map(|(_, d)| {
            let r = pdf.get(d, "Resources");
            let f = pdf.get(r.dict(), "Font");
            assert_eq!(f.dict().len(), 1);
            f.dict().values().next().unwrap().reference()
        })
        .collect();
    assert!(refs.iter().all(|&r| r == refs[0]));
    let t0 = pdf.object(refs[0]);
    let cid = pdf.resolve(&t0.dict()["DescendantFonts"].arr()[0]);
    let desc = pdf.get(cid.dict(), "FontDescriptor");
    let program = pdf.stream(&pdf.get(desc.dict(), "FontFile2"));
    let face = FontFace::parse(&program).unwrap();
    let src = FontFace::parse(embedded("Carlito", 400, false)).unwrap();
    let has = |c: char| {
        face.outline(src.glyph_index(c))
            .is_ok_and(|p| !p.is_empty())
    };
    for c in "abcxyzQé".chars() {
        assert!(has(c), "{c} missing from the subset");
    }
    for c in "dhlmnop".chars() {
        assert!(!has(c), "{c} should not be in the subset");
    }
    // Text of the three pages is intact (ToUnicode is shared).
    let texts: Vec<_> = pages.iter().map(|(_, d)| extract_text(&pdf, d)).collect();
    assert_eq!(texts[0], ["abc"]);
    assert_eq!(texts[1], ["xyz Q"]);
    assert_eq!(texts[2], ["é"]);
}

#[test]
fn page_tree_is_balanced_for_many_pages() {
    let res = Res::default();
    let page = list(
        100.0,
        100.0,
        vec![fill(rect_path(0.0, 0.0, 10.0, 10.0), BLACK)],
    );
    for n in [1usize, 2, 31, 32, 33, 64, 1025, 2100] {
        let pages = vec![page.clone(); n];
        let bytes = render(&pages, &res);
        let pdf = open(&bytes);
        assert_eq!(pdf.pages().len(), n, "{n} pages");
        // Depth: 1..=32 pages -> 1 level of /Pages, <= 1024 -> 2, else 3.
        let mut depth = 0;
        let mut node = pdf.object(pdf.root()["Pages"].reference());
        loop {
            depth += 1;
            let kid = pdf.object(node.dict()["Kids"].arr()[0].reference());
            if kid.dict()["Type"].name() == "Page" {
                break;
            }
            node = kid;
        }
        let expect = match n {
            0..=32 => 1,
            33..=1024 => 2,
            _ => 3,
        };
        assert_eq!(depth, expect, "{n} pages");
    }
}

#[test]
fn output_is_deterministic() {
    let build = || {
        let mut res = Res::default();
        let page = typical_page(&mut res);
        render_pdf(&[page.clone(), page], &res, &PdfOptions::default())
            .unwrap()
            .bytes
    };
    let a = build();
    assert_eq!(a, build());
    // The trailer /ID is derived from the content, not from a clock.
    let pdf = open(&a);
    let id = pdf.trailer["ID"].arr();
    assert_eq!(id.len(), 2);
    assert_eq!(id[0].str_bytes().len(), 16);
}

#[test]
fn streaming_writer_matches_render_pdf() {
    let mut res = Res::default();
    let page = typical_page(&mut res);
    let whole = render_pdf(&[page.clone(), page.clone()], &res, &PdfOptions::default()).unwrap();
    let mut w = PdfWriter::new(Vec::new(), PdfOptions::default()).unwrap();
    w.add_page(&page, &res).unwrap();
    assert_eq!(w.page_count(), 1);
    w.add_page(&page, &res).unwrap();
    let (bytes, notices) = w.finish(&res).unwrap();
    assert_eq!(bytes, whole.bytes);
    assert_eq!(notices, whole.notices);
}

#[test]
fn uncompressed_output_is_readable() {
    let mut res = Res::default();
    let page = typical_page(&mut res);
    let options = PdfOptions {
        compress: false,
        ..PdfOptions::default()
    };
    let out = render_pdf(&[page], &res, &options).unwrap();
    let pdf = open(&out.bytes);
    assert!(!String::from_utf8_lossy(&out.bytes).contains("FlateDecode"));
    let (_, pg) = &pdf.pages()[0];
    assert!(extract_text(&pdf, pg).contains(&LINES[0].to_owned()));
}

/// US-328: the six document properties a DOCX supplies (title, subject, author,
/// keywords, creation and modification dates) all reach the Info dictionary.
#[test]
fn info_carries_title_author_subject_keywords_and_both_dates() {
    let mut res = Res::default();
    let page = typical_page(&mut res);
    let date = |day: u8, hour: u8| PdfDate {
        year: 2026,
        month: 10,
        day,
        hour,
        minute: 30,
        second: 0,
        utc_offset_minutes: 0,
    };
    let options = PdfOptions {
        info: atelier_fmt_pdf::PdfInfo {
            title: Some("Rapport « Œuvre »".to_owned()),
            author: Some("Élodie Müller".to_owned()),
            subject: Some("Sujet".to_owned()),
            keywords: Some("un; deux; trois".to_owned()),
            creator: Some("Atelier".to_owned()),
            creation_date: Some(date(12, 8)),
            modification_date: Some(date(13, 9)),
            ..Default::default()
        },
        ..PdfOptions::default()
    };
    let pdf = open(&render_pdf(&[page], &res, &options).unwrap().bytes);
    let info = pdf.resolve(pdf.trailer.get("Info").unwrap());
    let text = |name: &str| {
        let b = info.dict()[name].str_bytes();
        assert_eq!(&b[..2], [0xFE, 0xFF], "{name} is UTF-16BE");
        String::from_utf16(
            &b[2..]
                .chunks(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect::<Vec<_>>(),
        )
        .unwrap()
    };
    assert_eq!(text("Title"), "Rapport « Œuvre »");
    assert_eq!(text("Author"), "Élodie Müller");
    assert_eq!(info.dict()["Subject"].str_bytes(), b"Sujet");
    assert_eq!(info.dict()["Keywords"].str_bytes(), b"un; deux; trois");
    assert_eq!(
        info.dict()["CreationDate"].str_bytes(),
        b"D:20261012083000Z"
    );
    assert_eq!(info.dict()["ModDate"].str_bytes(), b"D:20261013093000Z");
}

#[test]
fn metadata_links_and_bookmarks() {
    let mut res = Res::default();
    standard_fonts(&mut res);
    let mk = |t: &str, res: &mut Res| {
        list(
            400.0,
            600.0,
            vec![
                res.text(CARLITO, t, 20.0, 40.0, BLACK),
                Command::Link {
                    bounds: atelier_geom::Rect::from_xywh(20.0, 30.0, 50.0, 14.0).unwrap(),
                    target: "#intro".to_owned(),
                },
                Command::Link {
                    bounds: atelier_geom::Rect::from_xywh(20.0, 60.0, 50.0, 14.0).unwrap(),
                    target: "#nowhere".to_owned(),
                },
                Command::Link {
                    bounds: atelier_geom::Rect::from_xywh(20.0, 90.0, 50.0, 14.0).unwrap(),
                    target: "mailto:a@b.fr".to_owned(),
                },
            ],
        )
    };
    let pages = [
        mk("Un", &mut res),
        mk("Deux", &mut res),
        mk("Trois", &mut res),
    ];
    let item =
        |title: &str, page: usize, top: Option<f64>, open: bool, children: Vec<OutlineItem>| {
            OutlineItem {
                title: title.to_owned(),
                destination: Destination { page, top },
                children,
                open,
            }
        };
    let options = PdfOptions {
        info: atelier_fmt_pdf::PdfInfo {
            title: Some("Rapport (été) \\ 2026".to_owned()),
            author: Some("Zoé Müller".to_owned()),
            subject: Some("Sujet".to_owned()),
            keywords: Some("a, b".to_owned()),
            creator: Some("Atelier Écriture".to_owned()),
            creation_date: Some(PdfDate {
                year: 2026,
                month: 10,
                day: 12,
                hour: 9,
                minute: 5,
                second: 7,
                utc_offset_minutes: 120,
            }),
            modification_date: Some(PdfDate {
                year: 2026,
                month: 10,
                day: 13,
                hour: 0,
                minute: 0,
                second: 0,
                utc_offset_minutes: 0,
            }),
            ..Default::default()
        },
        language: Some("fr-FR".to_owned()),
        named_destinations: vec![
            (
                "intro".to_owned(),
                Destination {
                    page: 0,
                    top: Some(100.0),
                },
            ),
            ("fin".to_owned(), Destination { page: 2, top: None }),
        ],
        outlines: vec![
            item(
                "Chapitre 1",
                0,
                Some(0.0),
                true,
                vec![
                    item(
                        "Section 1.1",
                        0,
                        Some(300.0),
                        false,
                        vec![item("Sous-section", 1, None, true, vec![])],
                    ),
                    item("Section 1.2", 1, Some(10.0), true, vec![]),
                ],
            ),
            item("Chapitre 2 — fin", 2, None, true, vec![]),
        ],
        ..PdfOptions::default()
    };
    let out = render_pdf(&pages, &res, &options).unwrap();
    assert_eq!(
        out.notices,
        [
            PdfNotice::UnresolvedLink {
                page: 0,
                target: "#nowhere".into()
            },
            PdfNotice::UnresolvedLink {
                page: 1,
                target: "#nowhere".into()
            },
            PdfNotice::UnresolvedLink {
                page: 2,
                target: "#nowhere".into()
            },
        ]
    );
    let pdf = open(&out.bytes);
    let info = pdf.resolve(pdf.trailer.get("Info").unwrap());
    let utf16 = |o: &Obj| {
        let b = o.str_bytes();
        assert_eq!(&b[..2], [0xFE, 0xFF]);
        String::from_utf16(
            &b[2..]
                .chunks(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect::<Vec<_>>(),
        )
        .unwrap()
    };
    assert_eq!(utf16(&info.dict()["Title"]), "Rapport (été) \\ 2026");
    assert_eq!(utf16(&info.dict()["Author"]), "Zoé Müller");
    assert_eq!(info.dict()["Subject"].str_bytes(), b"Sujet");
    assert_eq!(info.dict()["Producer"].str_bytes(), b"Atelier");
    assert_eq!(
        info.dict()["CreationDate"].str_bytes(),
        b"D:20261012090507+02'00'"
    );
    assert_eq!(info.dict()["ModDate"].str_bytes(), b"D:20261013000000Z");
    let root = pdf.root();
    assert_eq!(root["Lang"].str_bytes(), b"fr-FR");
    assert_eq!(root["PageMode"].name(), "UseOutlines");
    // Named destinations.
    let names = pdf.get(&pdf.get(&root, "Names").dict().clone(), "Dests");
    let arr = names.dict()["Names"].arr();
    assert_eq!(arr.len(), 4);
    assert_eq!(arr[0].str_bytes(), b"fin");
    assert_eq!(arr[2].str_bytes(), b"intro");
    let pages_v = pdf.pages();
    let d = arr[3].arr();
    assert_eq!(d[0].reference(), pages_v[0].0);
    assert_eq!(d[1].name(), "XYZ");
    assert_eq!(d[3].num(), 500.0); // 600 - 100
    assert_eq!(arr[1].arr()[0].reference(), pages_v[2].0);
    assert_eq!(arr[1].arr()[3], Obj::Null);
    // Links: named destination and mailto.
    let annots = pages_v[1].1["Annots"].arr();
    assert_eq!(annots.len(), 2);
    assert_eq!(pdf.resolve(&annots[0]).dict()["Dest"].str_bytes(), b"intro");
    assert_eq!(
        pdf.get(pdf.resolve(&annots[1]).dict(), "A").dict()["URI"].str_bytes(),
        b"mailto:a@b.fr"
    );
    // Outline tree.
    let ol = pdf.get(&root, "Outlines");
    assert_eq!(ol.dict()["Type"].name(), "Outlines");
    assert_eq!(ol.dict()["Count"].int(), 4); // ch1, 1.2, (1.1 closed counts 1 but hides its child), ch2 -> 4
    let ch1 = pdf.object(ol.dict()["First"].reference());
    let ch2 = pdf.object(ol.dict()["Last"].reference());
    assert_eq!(utf16(&ch2.dict()["Title"]), "Chapitre 2 — fin");
    assert_eq!(
        ch1.dict()["Next"].reference(),
        ol.dict()["Last"].reference()
    );
    assert_eq!(
        ch2.dict()["Prev"].reference(),
        ol.dict()["First"].reference()
    );
    assert_eq!(
        ch1.dict()["Parent"].reference(),
        root["Outlines"].reference()
    );
    assert_eq!(ch1.dict()["Count"].int(), 2); // 1.1 and 1.2 visible
    let s11 = pdf.object(ch1.dict()["First"].reference());
    assert_eq!(s11.dict()["Count"].int(), -1);
    assert_eq!(s11.dict()["Dest"].arr()[3].num(), 300.0);
    let s12 = pdf.object(s11.dict()["Next"].reference());
    assert_eq!(
        s12.dict()["Prev"].reference(),
        ch1.dict()["First"].reference()
    );
    assert_eq!(
        ch1.dict()["Last"].reference(),
        s11.dict()["Next"].reference()
    );
    assert_eq!(s12.dict()["Dest"].arr()[0].reference(), pages_v[1].0);
}

#[test]
fn glyph_text_outside_the_bmp_and_ligatures() {
    let mut res = Res::default();
    standard_fonts(&mut res);
    let face = FontFace::parse(embedded("Carlito", 400, false)).unwrap();
    let a = u32::from(face.glyph_index('a'));
    let b = u32::from(face.glyph_index('b'));
    let c = u32::from(face.glyph_index('c'));
    res.texts.insert((CARLITO, a), "😀".to_owned());
    res.texts.insert((CARLITO, b), "ffi".to_owned());
    res.texts.insert((CARLITO, c), "e\u{301}".to_owned());
    let run = Command::DrawGlyphRun {
        font_id: CARLITO,
        color: BLACK,
        glyphs: [a, b, c, a]
            .iter()
            .enumerate()
            .map(|(i, &g)| PositionedGlyph {
                glyph_id: g,
                position: p(10.0 + 12.0 * i as f64, 50.0),
                advance: p(12.0, 0.0),
            })
            .collect(),
    };
    let off = PdfOptions {
        infer_spaces: false,
        ..PdfOptions::default()
    };
    let bytes = render_pdf(&[list(200.0, 100.0, vec![run])], &res, &off)
        .unwrap()
        .bytes;
    let pdf = open(&bytes);
    let pages = pdf.pages();
    let text = extract_text(&pdf, &pages[0].1);
    assert_eq!(text, ["😀ffie\u{301}😀"]);
    // The CMap really encodes the emoji as a surrogate pair.
    let (_, t0, _, _) = &page_fonts(&pdf, &pages[0].1)[0];
    let raw = pdf.stream(&pdf.get(t0.dict(), "ToUnicode"));
    assert!(String::from_utf8_lossy(&raw).contains("<D83DDE00>"));
}

#[test]
fn spaces_are_inferred_only_when_missing() {
    let mut res = Res::default();
    standard_fonts(&mut res);
    let face = FontFace::parse(embedded("Carlito", 400, false)).unwrap();
    let mut glyph = |ch: char, x: f64| {
        let g = u32::from(face.glyph_index(ch));
        res.texts.insert((CARLITO, g), ch.to_string());
        PositionedGlyph {
            glyph_id: g,
            position: p(x, 30.0),
            advance: p(6.0, 0.0),
        }
    };
    // "ab" then a 6-unit gap, then "cd": a space is inferred; "ab" stays together.
    let glyphs = vec![
        glyph('a', 10.0),
        glyph('b', 16.0),
        glyph('c', 36.0),
        glyph('d', 42.0),
    ];
    let run = Command::DrawGlyphRun {
        font_id: CARLITO,
        color: BLACK,
        glyphs,
    };
    let page_list = list(100.0, 60.0, vec![run]);
    let bytes = render(std::slice::from_ref(&page_list), &res);
    let pdf = open(&bytes);
    assert_eq!(extract_text(&pdf, &pdf.pages()[0].1), ["ab cd"]);
    let off = PdfOptions {
        infer_spaces: false,
        ..PdfOptions::default()
    };
    let bytes = render_pdf(&[page_list], &res, &off).unwrap().bytes;
    let pdf = open(&bytes);
    assert_eq!(extract_text(&pdf, &pdf.pages()[0].1), ["abcd"]);
}

#[test]
fn missing_glyph_text_is_reported_and_conflicting_text_is_kept() {
    let mut res = Res::default();
    standard_fonts(&mut res);
    let face = FontFace::parse(embedded("Carlito", 400, false)).unwrap();
    let g = u32::from(face.glyph_index('a'));
    let run = |res: &Res| {
        let _ = res;
        Command::DrawGlyphRun {
            font_id: CARLITO,
            color: BLACK,
            glyphs: vec![PositionedGlyph {
                glyph_id: g,
                position: p(1.0, 10.0),
                advance: p(5.0, 0.0),
            }],
        }
    };
    let l = list(50.0, 50.0, vec![run(&res)]);
    let out = render_pdf(std::slice::from_ref(&l), &res, &PdfOptions::default()).unwrap();
    assert_eq!(
        out.notices,
        [PdfNotice::MissingGlyphText {
            font_id: CARLITO,
            glyph_id: g
        }]
    );
    // A page can use two font ids of one program with different texts for a glyph.
    res.texts.insert((CARLITO, g), "a".into());
    res.texts.insert((CARLITO_BIG, g), "b".into());
    let l2 = list(
        50.0,
        50.0,
        vec![
            run(&res),
            Command::DrawGlyphRun {
                font_id: CARLITO_BIG,
                color: BLACK,
                glyphs: vec![PositionedGlyph {
                    glyph_id: g,
                    position: p(20.0, 10.0),
                    advance: p(5.0, 0.0),
                }],
            },
        ],
    );
    let out = render_pdf(&[l2], &res, &PdfOptions::default()).unwrap();
    // No notice: the second text is written as `ActualText`.
    assert!(out.notices.is_empty(), "{:?}", out.notices);
    let pdf = open(&out.bytes);
    assert_eq!(extract_text(&pdf, &pdf.pages()[0].1), ["a", "b"]);
}

#[test]
fn cff_font_uses_cidfonttype0c_and_final_cids() {
    let data: &'static [u8] = include_bytes!("../../../tests/font-cff-reference/cff-oracle.otf");
    let face = FontFace::parse(data).unwrap();
    let n = face.metrics().number_of_glyphs;
    assert!(n >= 4, "fixture has {n} glyphs");
    let mut res = Res::default();
    res.add_font(1, 77, data, 20.0);
    // Page 1 uses glyph 3, page 2 glyph 1: CIDs are renumbered over the union.
    for (gid, ch) in [(3u32, "C"), (1, "A")] {
        res.texts.insert((1, gid), ch.to_owned());
    }
    let mk = |gid: u32, x: f64| {
        list(
            100.0,
            100.0,
            vec![Command::DrawGlyphRun {
                font_id: 1,
                color: BLACK,
                glyphs: vec![PositionedGlyph {
                    glyph_id: gid,
                    position: p(x, 50.0),
                    advance: p(10.0, 0.0),
                }],
            }],
        )
    };
    let bytes = render(&[mk(3, 10.0), mk(1, 20.0)], &res);
    let pdf = open(&bytes);
    let pages = pdf.pages();
    let (_, t0, cid, desc) = &page_fonts(&pdf, &pages[0].1)[0];
    assert_eq!(cid.dict()["Subtype"].name(), "CIDFontType0");
    assert!(!cid.dict().contains_key("CIDToGIDMap"));
    let file = pdf.get(desc.dict(), "FontFile3");
    assert_eq!(file.dict()["Subtype"].name(), "CIDFontType0C");
    let program = pdf.stream(&file);
    let cff = CffFont::parse(&program).expect("subset CFF parses");
    assert_eq!(cff.glyph_count(), 3); // .notdef + two glyphs
    // Text survives although the codes were patched after both pages were seen.
    assert_eq!(extract_text(&pdf, &pages[0].1), ["C"]);
    assert_eq!(extract_text(&pdf, &pages[1].1), ["A"]);
    let tu = parse_tounicode(&pdf.stream(&pdf.get(t0.dict(), "ToUnicode")));
    assert_eq!(tu.len(), 2);
    // Codes in the content streams are the (compact) CFF glyph ids 1 and 2.
    let codes: Vec<u16> = pages
        .iter()
        .map(|(_, d)| {
            let c = pdf.get(d, "Contents");
            let ops = content_ops(&pdf.stream(&c));
            let tj = ops.iter().find(|(o, _)| o == "TJ").unwrap();
            let s = tj.1[0].arr()[0].str_bytes().to_vec();
            u16::from_be_bytes([s[0], s[1]])
        })
        .collect();
    assert_eq!(codes, [2, 1]);
    // /W is keyed by the same CIDs.
    let w = cid.dict()["W"].arr();
    assert_eq!(w[0].int(), 1);
    assert_eq!(w[1].arr().len(), 2);
}
