// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-331: embedding rights, images, glyph texts, errors and limits.

mod common;

use atelier_fmt_pdf::{
    Destination, NoEmbedReason, OutlineItem, PdfDate, PdfError, PdfInfo, PdfLimits, PdfNotice,
    PdfOptions, PdfWriter, render_pdf,
};
use atelier_font::FontFace;
use atelier_geom::Rect;
use atelier_image::{DecodeLimits, Image, ImageError, decode_jpeg, encode_png_rgba};
use atelier_render::{Command, DisplayList, DisplayPath, PathElement, PositionedGlyph};
use common::reader::{Obj, Pdf, content_ops, extract_text};
use common::*;

const JPEG_YCCK: &[u8] = include_bytes!("../../image/tests/fixtures/jpeg/adobe-ycck.jpg");

fn open(bytes: &[u8]) -> Pdf {
    Pdf::open(bytes.to_vec()).unwrap_or_else(|e| panic!("invalid PDF: {e}"))
}

fn render(pages: &[DisplayList], res: &Res) -> Result<Vec<u8>, PdfError> {
    render_pdf(pages, res, &PdfOptions::default()).map(|o| o.bytes)
}

fn err(pages: &[DisplayList], res: &Res, options: &PdfOptions) -> PdfError {
    render_pdf(pages, res, options).expect_err("the document should be refused")
}

/// A copy of the font with another `OS/2.fsType`.
fn with_fs_type(data: &[u8], fs_type: u16) -> &'static [u8] {
    let mut v = data.to_vec();
    let tables = usize::from(u16::from_be_bytes([v[4], v[5]]));
    for i in 0..tables {
        let r = 12 + 16 * i;
        if &v[r..r + 4] == b"OS/2" {
            let off = u32::from_be_bytes([v[r + 8], v[r + 9], v[r + 10], v[r + 11]]) as usize;
            v[off + 8..off + 10].copy_from_slice(&fs_type.to_be_bytes());
            return Box::leak(v.into_boxed_slice());
        }
    }
    panic!("no OS/2 table");
}

fn image_objects(pdf: &Pdf) -> Vec<(u32, Obj)> {
    (1..pdf.offsets.len() as u32)
        .map(|n| (n, pdf.object(n)))
        .filter(|(_, o)| {
            matches!(o, Obj::Stream(d, _)
                if d.get("Subtype") == Some(&Obj::Name("Image".to_owned())))
        })
        .collect()
}

fn ops_of(pdf: &Pdf, page: &common::reader::Dict) -> Vec<(String, Vec<Obj>)> {
    content_ops(&pdf.stream(&pdf.get(page, "Contents")))
}

fn one_run(font_id: u64, text: &str, res: &mut Res) -> DisplayList {
    let run = res.text(font_id, text, 10.0, 40.0, BLACK);
    list(300.0, 80.0, vec![run])
}

fn image_page(id: u64) -> DisplayList {
    list(
        100.0,
        100.0,
        vec![Command::DrawImage {
            image_id: id,
            bounds: Rect::from_xywh(10.0, 10.0, 60.0, 40.0).unwrap(),
        }],
    )
}

// ------------------------------------------------------------ embedding rights

#[test]
fn restricted_font_is_drawn_as_paths_with_a_notice() {
    for (fs_type, reason) in [
        (0x0002, NoEmbedReason::Restricted),
        (0x0200, NoEmbedReason::BitmapOnly),
    ] {
        let mut res = Res::default();
        res.add_font(
            1,
            1,
            with_fs_type(embedded("Carlito", 400, false), fs_type),
            14.0,
        );
        let page = one_run(1, "Hello, Wörld", &mut res);
        let out = render_pdf(&[page], &res, &PdfOptions::default()).unwrap();
        assert_eq!(
            out.notices,
            [PdfNotice::FontDrawnAsPaths { font_id: 1, reason }]
        );
        let pdf = open(&out.bytes);
        let (_, pg) = &pdf.pages()[0];
        // No font resource, no text operator, no font file anywhere.
        let resources = pdf.get(pg, "Resources");
        assert!(pdf.opt(resources.dict(), "Font").is_none());
        let ops = ops_of(&pdf, pg);
        assert!(ops.iter().all(|(op, _)| op != "TJ" && op != "BT"));
        assert!(ops.iter().filter(|(op, _)| op == "f").count() >= 10);
        assert!(!String::from_utf8_lossy(&out.bytes).contains("FontFile"));
        assert!(extract_text(&pdf, pg).is_empty());
    }
}

#[test]
fn restricted_font_needs_outlines_and_known_glyphs() {
    let mut res = Res::default();
    res.add_font(
        1,
        1,
        with_fs_type(embedded("Carlito", 400, false), 0x0002),
        14.0,
    );
    let glyph = |g: u32| {
        list(
            100.0,
            100.0,
            vec![Command::DrawGlyphRun {
                font_id: 1,
                color: BLACK,
                glyphs: vec![PositionedGlyph {
                    glyph_id: g,
                    position: p(5.0, 20.0),
                    advance: p(5.0, 0.0),
                }],
            }],
        )
    };
    assert_eq!(
        err(&[glyph(36)], &res, &PdfOptions::default()),
        PdfError::MissingOutline {
            font_id: 1,
            glyph_id: 36
        }
    );
    assert_eq!(
        err(&[glyph(60_000)], &res, &PdfOptions::default()),
        PdfError::UnknownGlyph {
            font_id: 1,
            glyph_id: 60_000
        }
    );
}

#[test]
fn font_forbidding_subsets_is_embedded_whole() {
    let source = with_fs_type(embedded("Carlito", 400, false), 0x0100);
    let mut res = Res::default();
    res.add_font(1, 1, source, 14.0);
    let page = one_run(1, "Hello Wörld", &mut res);
    let options = PdfOptions {
        compress: false,
        ..PdfOptions::default()
    };
    let out = render_pdf(&[page], &res, &options).unwrap();
    assert_eq!(out.notices, [PdfNotice::FontEmbeddedWhole { font_id: 1 }]);
    let pdf = open(&out.bytes);
    let (_, pg) = &pdf.pages()[0];
    let resources = pdf.get(pg, "Resources");
    let fonts = pdf.get(resources.dict(), "Font");
    let (_, f) = fonts.dict().iter().next().unwrap();
    let t0 = pdf.resolve(f);
    let cid = pdf.resolve(&t0.dict()["DescendantFonts"].arr()[0]);
    let desc = pdf.get(cid.dict(), "FontDescriptor");
    // The program is the font file, byte for byte, and the name has no subset tag.
    let program = pdf.get(desc.dict(), "FontFile2");
    let Obj::Stream(d, raw) = &program else {
        panic!("font file is not a stream")
    };
    assert_eq!(raw.as_slice(), source);
    assert_eq!(d["Length1"].int() as usize, source.len());
    assert!(!t0.dict()["BaseFont"].name().contains('+'));
    // Glyph ids are the source ones, and the text is still extractable.
    let face = FontFace::parse(source).unwrap();
    let codes: Vec<u16> = ops_of(&pdf, pg)
        .iter()
        .filter(|(op, _)| op == "TJ")
        .flat_map(|(_, a)| a[0].arr().to_vec())
        .filter_map(|o| match o {
            Obj::Str(b) => Some(b),
            _ => None,
        })
        .flat_map(|b| {
            b.chunks(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(codes.contains(&face.glyph_index('H')));
    assert!(codes.contains(&face.glyph_index('ö')));
    assert_eq!(extract_text(&pdf, pg), ["Hello Wörld"]);
}

#[test]
fn fonts_with_rights_that_allow_subsets_have_no_notice() {
    for fs_type in [0x0000, 0x0004, 0x0008, 0x0004 | 0x0008] {
        let mut res = Res::default();
        res.add_font(
            1,
            1,
            with_fs_type(embedded("Carlito", 400, false), fs_type),
            14.0,
        );
        let page = one_run(1, "abc", &mut res);
        let out = render_pdf(&[page], &res, &PdfOptions::default()).unwrap();
        assert!(out.notices.is_empty(), "fsType {fs_type:#x}");
    }
}

// --------------------------------------------------------------------- images

fn strip_adobe(jpeg: &[u8]) -> Vec<u8> {
    let mut out = jpeg[..2].to_vec();
    let mut at = 2;
    while jpeg[at] == 0xff && jpeg[at + 1] != 0xda {
        let len = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
        if jpeg[at + 1] != 0xee {
            out.extend_from_slice(&jpeg[at..at + 2 + len]);
        }
        at += 2 + len;
    }
    out.extend_from_slice(&jpeg[at..]);
    out
}

#[test]
fn jpeg_is_passed_through_with_its_color_space() {
    let no_adobe = strip_adobe(JPEG_CMYK);
    assert_ne!(no_adobe.len(), JPEG_CMYK.len());
    let cases: Vec<(&str, Vec<u8>, &str, bool)> = vec![
        ("baseline", JPEG_420.to_vec(), "DeviceRGB", false),
        ("gray", JPEG_GRAY.to_vec(), "DeviceGray", false),
        ("progressive", JPEG_PROGRESSIVE.to_vec(), "DeviceRGB", false),
        ("cmyk adobe", JPEG_CMYK.to_vec(), "DeviceCMYK", true),
        ("ycck adobe", JPEG_YCCK.to_vec(), "DeviceCMYK", true),
        ("cmyk plain", no_adobe, "DeviceCMYK", false),
    ];
    for (name, bytes, space, inverted) in cases {
        let mut res = Res::default();
        res.add_image(1, "image/jpeg", bytes.clone());
        let out = render_pdf(&[image_page(1)], &res, &PdfOptions::default()).unwrap();
        let pdf = open(&out.bytes);
        let images = image_objects(&pdf);
        assert_eq!(images.len(), 1, "{name}");
        let Obj::Stream(d, raw) = &images[0].1 else {
            unreachable!()
        };
        assert_eq!(raw, &bytes, "{name}: the JPEG is copied unchanged");
        assert_eq!(d["Filter"].name(), "DCTDecode", "{name}");
        assert_eq!(d["ColorSpace"].name(), space, "{name}");
        assert_eq!(d["BitsPerComponent"].int(), 8, "{name}");
        assert!(!d.contains_key("SMask"), "{name}");
        let decoded = decode_jpeg(&bytes, DecodeLimits::default()).unwrap();
        assert_eq!(d["Width"].int(), i64::from(decoded.width), "{name}");
        assert_eq!(d["Height"].int(), i64::from(decoded.height), "{name}");
        match (d.get("Decode"), inverted) {
            (Some(a), true) => {
                let v: Vec<f64> = a.arr().iter().map(Obj::num).collect();
                assert_eq!(v, [1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0], "{name}");
            }
            (None, false) => {}
            (got, _) => panic!("{name}: unexpected /Decode {got:?}"),
        }
    }
}

#[test]
fn jpeg_that_cannot_be_passed_is_decoded() {
    // An RGB-tagged 3-component JPEG without Adobe marker is not YCbCr: the
    // writer must not hand it to DCTDecode. Rewrite the component ids of a
    // baseline file to "RGB".
    let mut bytes = JPEG_420.to_vec();
    let sof = bytes
        .windows(2)
        .position(|w| w == [0xff, 0xc0])
        .expect("SOF0");
    for (k, id) in (*b"RGB").iter().copied().enumerate() {
        bytes[sof + 10 + 3 * k] = id;
    }
    // The scan header refers to the ids too.
    let sos = bytes
        .windows(2)
        .position(|w| w == [0xff, 0xda])
        .expect("SOS");
    for (k, id) in (*b"RGB").iter().copied().enumerate() {
        bytes[sos + 5 + 2 * k] = id;
    }
    let mut res = Res::default();
    res.add_image(1, "image/jpeg", bytes);
    let out = render_pdf(&[image_page(1)], &res, &PdfOptions::default()).unwrap();
    let pdf = open(&out.bytes);
    let images = image_objects(&pdf);
    assert_eq!(images.len(), 1);
    let Obj::Stream(d, _) = &images[0].1 else {
        unreachable!()
    };
    assert_eq!(d["Filter"].name(), "FlateDecode", "{d:?}");
}

#[test]
fn png_with_alpha_gets_a_soft_mask() {
    let mut res = Res::default();
    res.add_image(1, "image/png", png_rgba(8, 6, true));
    let out = render_pdf(&[image_page(1)], &res, &PdfOptions::default()).unwrap();
    let pdf = open(&out.bytes);
    let images = image_objects(&pdf);
    assert_eq!(images.len(), 2, "colour image and mask");
    let color = images
        .iter()
        .find(|(_, o)| o.dict().contains_key("SMask"))
        .expect("an image with a mask");
    let d = color.1.dict();
    assert_eq!(d["ColorSpace"].name(), "DeviceRGB");
    assert_eq!((d["Width"].int(), d["Height"].int()), (8, 6));
    let mask = pdf.get(d, "SMask");
    let md = mask.dict();
    assert_eq!(md["ColorSpace"].name(), "DeviceGray");
    assert_eq!(md["BitsPerComponent"].int(), 8);
    assert_eq!((md["Width"].int(), md["Height"].int()), (8, 6));
    let rgb = pdf.stream(&color.1);
    let alpha = pdf.stream(&mask);
    assert_eq!(rgb.len(), 8 * 6 * 3);
    assert_eq!(alpha.len(), 8 * 6);
    for y in 0..6usize {
        for x in 0..8usize {
            assert_eq!(
                alpha[y * 8 + x] as usize,
                (x + y) * 255 / 14,
                "alpha {x},{y}"
            );
            assert_eq!(rgb[(y * 8 + x) * 3] as usize, x * 255 / 8, "red {x},{y}");
            assert_eq!(rgb[(y * 8 + x) * 3 + 1] as usize, y * 255 / 6, "green");
            assert_eq!(rgb[(y * 8 + x) * 3 + 2], 128, "blue");
        }
    }
}

#[test]
fn opaque_and_gray_pngs_have_no_mask() {
    let mut res = Res::default();
    res.add_image(1, "image/png", png_rgba(8, 8, false));
    let pixels: Vec<u8> = (0..16u8)
        .flat_map(|v| [v * 16, v * 16, v * 16, 255])
        .collect();
    let gray = encode_png_rgba(
        &Image {
            width: 4,
            height: 4,
            pixels,
        },
        DecodeLimits::default(),
    )
    .unwrap();
    res.add_image(2, "image/png", gray);
    let out = render_pdf(
        &[image_page(1), image_page(2)],
        &res,
        &PdfOptions::default(),
    )
    .unwrap();
    let pdf = open(&out.bytes);
    let images = image_objects(&pdf);
    assert_eq!(images.len(), 2);
    assert!(images.iter().all(|(_, o)| !o.dict().contains_key("SMask")));
    let spaces: Vec<&str> = images
        .iter()
        .map(|(_, o)| o.dict()["ColorSpace"].name())
        .collect();
    assert_eq!(spaces, ["DeviceRGB", "DeviceGray"]);
    let gray_obj = &images[1].1;
    assert_eq!(
        pdf.stream(gray_obj),
        (0..16u8).map(|v| v * 16).collect::<Vec<_>>()
    );
}

#[test]
fn an_image_used_many_times_is_written_once() {
    let mut res = Res::default();
    res.add_image(5, "image/png", png_rgba(16, 16, false));
    res.add_image(6, "image/jpeg", JPEG_420.to_vec());
    let page = |n: usize| {
        let mut c = Vec::new();
        for i in 0..n {
            for id in [5, 6] {
                c.push(Command::DrawImage {
                    image_id: id,
                    bounds: Rect::from_xywh(5.0 * i as f64, 5.0, 20.0, 20.0).unwrap(),
                });
            }
        }
        list(200.0, 100.0, c)
    };
    let out = render_pdf(&[page(3), page(1), page(2)], &res, &PdfOptions::default()).unwrap();
    let pdf = open(&out.bytes);
    assert_eq!(image_objects(&pdf).len(), 2);
    let refs: Vec<Vec<u32>> = pdf
        .pages()
        .iter()
        .map(|(_, pg)| {
            let r = pdf.get(pg, "Resources");
            let x = pdf.get(r.dict(), "XObject");
            x.dict().values().map(Obj::reference).collect()
        })
        .collect();
    assert_eq!(refs[0], refs[1]);
    assert_eq!(refs[0], refs[2]);
}

#[test]
fn invalid_images_are_typed_errors() {
    let o = PdfOptions::default();
    let try_image = |mime: &'static str, bytes: Vec<u8>| {
        let mut res = Res::default();
        res.add_image(1, mime, bytes);
        err(&[image_page(1)], &res, &o)
    };
    let is_image = |e: PdfError| assert!(matches!(e, PdfError::Image(_)), "{e:?}");
    is_image(try_image("image/png", b"not an image at all".to_vec()));
    is_image(try_image("image/png", JPEG_420.to_vec())); // MIME does not match
    is_image(try_image(
        "image/jpeg",
        JPEG_420[..JPEG_420.len() - 40].to_vec(),
    ));
    is_image(try_image("image/jpeg", JPEG_420[..20].to_vec()));
    is_image(try_image("image/jpeg", vec![0xff, 0xd8, 0xff]));
    let png = png_rgba(8, 8, true);
    is_image(try_image("image/png", png[..png.len() - 20].to_vec()));
    let mut bad_crc = png.clone();
    let n = bad_crc.len();
    bad_crc[n / 2] ^= 0x55;
    is_image(try_image("image/png", bad_crc));
    is_image(try_image("image/gif", Vec::new()));
    // Unknown image id.
    let res = Res::default();
    assert_eq!(err(&[image_page(9)], &res, &o), PdfError::MissingImage(9));
    assert_eq!(
        try_image("image/bmp", b"BM".to_vec()),
        PdfError::Image(ImageError::Truncated)
    );
}

// ------------------------------------------------------------- glyph texts

#[test]
fn one_glyph_with_two_texts_is_extracted_correctly() {
    // The "fi" ligature glyph stands for U+FB01 on one line and for "f"+"i" on
    // another (and for "fi" in a third font id of the same program).
    let mut res = Res::default();
    standard_fonts(&mut res);
    let face = FontFace::parse(embedded("Carlito", 400, false)).unwrap();
    let lig = u32::from(face.glyph_index('\u{FB01}'));
    assert_ne!(lig, 0, "Carlito has the fi ligature glyph");
    res.texts.insert((CARLITO, lig), "\u{FB01}".to_owned());
    res.texts.insert((CARLITO_BIG, lig), "fi".to_owned());
    let run = |font_id: u64, x: f64| Command::DrawGlyphRun {
        font_id,
        color: BLACK,
        glyphs: vec![PositionedGlyph {
            glyph_id: lig,
            position: p(x, 30.0),
            advance: p(8.0, 0.0),
        }],
    };
    // Same text first, then the other, then the first again, in one run.
    let l = list(
        300.0,
        60.0,
        vec![
            run(CARLITO, 10.0),
            run(CARLITO_BIG, 40.0),
            run(CARLITO, 70.0),
            Command::DrawGlyphRun {
                font_id: CARLITO_BIG,
                color: BLACK,
                glyphs: (0..3)
                    .map(|i| PositionedGlyph {
                        glyph_id: lig,
                        position: p(100.0 + 8.0 * f64::from(i), 30.0),
                        advance: p(8.0, 0.0),
                    })
                    .collect(),
            },
        ],
    );
    let off = PdfOptions {
        infer_spaces: false,
        ..PdfOptions::default()
    };
    let out = render_pdf(&[l], &res, &off).unwrap();
    assert!(out.notices.is_empty(), "{:?}", out.notices);
    let pdf = open(&out.bytes);
    let (_, pg) = &pdf.pages()[0];
    assert_eq!(
        extract_text(&pdf, pg),
        ["\u{FB01}", "fi", "\u{FB01}", "fi", "fi", "fi"]
    );
    // The page is still balanced marked content.
    let ops = ops_of(&pdf, pg);
    let count = |n: &str| ops.iter().filter(|(op, _)| op == n).count();
    assert_eq!(count("BDC"), 4);
    assert_eq!(count("EMC"), 4);
    assert_eq!(count("BT"), count("ET"));
}

// --------------------------------------------------------- errors and limits

fn simple_page(res: &mut Res) -> DisplayList {
    standard_fonts(res);
    list(
        200.0,
        100.0,
        vec![
            fill(rect_path(10.0, 10.0, 50.0, 20.0), BLACK),
            res.text(CARLITO, "abc", 10.0, 60.0, BLACK),
        ],
    )
}

#[test]
fn unbalanced_or_invalid_lists_are_refused() {
    let res = Res::default();
    let o = PdfOptions::default();
    let r = Rect::from_xywh(0.0, 0.0, 10.0, 10.0).unwrap();
    let raw = |commands: Vec<Command>| DisplayList {
        page: page(100.0, 100.0),
        commands,
    };
    for commands in [
        vec![Command::PopClip],
        vec![Command::PushClip { bounds: r }],
        vec![Command::PopTransform],
        vec![Command::PushClip { bounds: r }, Command::PopTransform],
    ] {
        assert_eq!(err(&[raw(commands)], &res, &o), PdfError::InvalidList);
    }
    let mut c = color(0.0, 0.0, 0.0, 1.0);
    c.alpha = f32::NAN;
    assert_eq!(
        err(
            &[raw(vec![fill(rect_path(0.0, 0.0, 5.0, 5.0), c)])],
            &res,
            &o
        ),
        PdfError::InvalidList
    );
    // The failing page is the second one: nothing is returned.
    let good = raw(vec![fill(rect_path(0.0, 0.0, 5.0, 5.0), BLACK)]);
    assert_eq!(
        err(&[good, raw(vec![Command::PopClip])], &res, &o),
        PdfError::InvalidList
    );
}

#[test]
fn compounded_transforms_are_refused_as_invalid_lists() {
    // Each transform is acceptable, their product is not finite.
    let t = atelier_render::DisplayTransform::new(1.0e100, 0.0, 0.0, 1.0e100, 0.0, 0.0).unwrap();
    let l = DisplayList {
        page: page(100.0, 100.0),
        commands: vec![
            Command::PushTransform { transform: t },
            Command::PushTransform { transform: t },
            Command::PushTransform { transform: t },
            Command::PopTransform,
            Command::PopTransform,
            Command::PopTransform,
        ],
    };
    assert_eq!(
        err(&[l], &Res::default(), &PdfOptions::default()),
        PdfError::InvalidList
    );
}

#[test]
fn values_beyond_pdf_numbers_are_refused() {
    let res = Res::default();
    let o = PdfOptions::default();
    let huge = |x: f64| list(100.0, 100.0, vec![fill(rect_path(0.0, 0.0, x, 5.0), BLACK)]);
    assert_eq!(
        err(&[huge(1.0e12)], &res, &o),
        PdfError::CoordinateOutOfRange
    );
    assert_eq!(
        err(&[huge(1.0e100)], &res, &o),
        PdfError::CoordinateOutOfRange
    );
    let link = list(
        100.0,
        100.0,
        vec![Command::Link {
            bounds: Rect::from_xywh(0.0, 0.0, 1.0e12, 5.0).unwrap(),
            target: "https://example.org/".to_owned(),
        }],
    );
    assert_eq!(err(&[link], &res, &o), PdfError::CoordinateOutOfRange);
    // 14 400 units is the largest page side.
    let wide = list(14_401.0, 100.0, vec![]);
    assert_eq!(err(&[wide], &res, &o), PdfError::PageTooLarge);
    assert!(render(&[list(14_400.0, 14_400.0, vec![])], &res).is_ok());
}

#[test]
fn missing_fonts_unknown_glyphs_and_bad_fonts() {
    let o = PdfOptions::default();
    let mut res = Res::default();
    let page = simple_page(&mut res);
    let empty = Res::default();
    assert_eq!(
        err(std::slice::from_ref(&page), &empty, &o),
        PdfError::MissingFont(CARLITO)
    );
    let glyph = |g: u32| {
        list(
            100.0,
            100.0,
            vec![Command::DrawGlyphRun {
                font_id: CARLITO,
                color: BLACK,
                glyphs: vec![PositionedGlyph {
                    glyph_id: g,
                    position: p(5.0, 20.0),
                    advance: p(5.0, 0.0),
                }],
            }],
        )
    };
    for g in [u32::MAX, 65_536, 65_535, 20_000] {
        assert_eq!(
            err(&[glyph(g)], &res, &o),
            PdfError::UnknownGlyph {
                font_id: CARLITO,
                glyph_id: g
            }
        );
    }
    // A font file that is not a font, a size that is not a size.
    let mut bad = Res::default();
    bad.add_font(
        1,
        1,
        b"this is not a font file, only text padding.......",
        12.0,
    );
    let l = list(
        50.0,
        50.0,
        vec![Command::DrawGlyphRun {
            font_id: 1,
            color: BLACK,
            glyphs: vec![PositionedGlyph {
                glyph_id: 1,
                position: p(1.0, 1.0),
                advance: p(1.0, 0.0),
            }],
        }],
    );
    assert!(matches!(
        err(std::slice::from_ref(&l), &bad, &o),
        PdfError::Font(_)
    ));
    for size in [0.0, -3.0, f64::NAN, f64::INFINITY] {
        let mut r = Res::default();
        r.add_font(1, 1, embedded("Carlito", 400, false), size);
        assert_eq!(
            err(std::slice::from_ref(&l), &r, &o),
            PdfError::CoordinateOutOfRange
        );
    }
}

#[test]
fn glyph_texts_have_a_length_limit() {
    let mut res = Res::default();
    let page = simple_page(&mut res);
    let g = *res.texts.keys().next().unwrap();
    res.texts.insert(g, "x".repeat(33));
    assert_eq!(
        err(&[page], &res, &PdfOptions::default()),
        PdfError::LimitExceeded("glyph text")
    );
}

#[test]
fn empty_documents_and_page_budgets() {
    let mut res = Res::default();
    let page = simple_page(&mut res);
    let o = PdfOptions::default();
    assert_eq!(err(&[], &res, &o), PdfError::EmptyDocument);
    let limited = PdfOptions {
        limits: PdfLimits {
            max_pages: 2,
            ..PdfLimits::default()
        },
        ..PdfOptions::default()
    };
    assert!(render_pdf(&[page.clone(), page.clone()], &res, &limited).is_ok());
    assert_eq!(
        err(&[page.clone(), page.clone(), page.clone()], &res, &limited),
        PdfError::LimitExceeded("pages")
    );
    // The streaming writer refuses the page and stays usable.
    let mut w = PdfWriter::new(Vec::new(), limited).unwrap();
    w.add_page(&page, &res).unwrap();
    w.add_page(&page, &res).unwrap();
    assert_eq!(
        w.add_page(&page, &res),
        Err(PdfError::LimitExceeded("pages"))
    );
    assert_eq!(w.page_count(), 2);
    let (bytes, _) = w.finish(&res).unwrap();
    assert_eq!(open(&bytes).pages().len(), 2);
}

#[test]
fn byte_object_and_content_budgets() {
    let mut res = Res::default();
    let page = simple_page(&mut res);
    let with = |f: &dyn Fn(&mut PdfLimits)| {
        let mut limits = PdfLimits::default();
        f(&mut limits);
        PdfOptions {
            limits,
            ..PdfOptions::default()
        }
    };
    let cases: Vec<(PdfOptions, PdfError)> = vec![
        (
            with(&|l| l.max_output_bytes = 200),
            PdfError::LimitExceeded("output size"),
        ),
        (
            with(&|l| l.max_objects = 4),
            PdfError::LimitExceeded("object count"),
        ),
        (
            with(&|l| l.max_content_bytes = 20),
            PdfError::LimitExceeded("page content size"),
        ),
    ];
    for (options, expected) in cases {
        assert_eq!(err(std::slice::from_ref(&page), &res, &options), expected);
    }
    // Fonts: one program is fine, a second one is over the budget.
    let two = {
        let mut r = Res::default();
        standard_fonts(&mut r);
        let c = vec![
            r.text(CARLITO, "a", 5.0, 20.0, BLACK),
            r.text(CALADEA, "b", 25.0, 20.0, BLACK),
        ];
        (list(100.0, 50.0, c), r)
    };
    let one_font = with(&|l| l.max_fonts = 1);
    assert_eq!(
        err(std::slice::from_ref(&two.0), &two.1, &one_font),
        PdfError::LimitExceeded("fonts")
    );
    assert!(render_pdf(&[two.0], &two.1, &with(&|l| l.max_fonts = 2)).is_ok());
}

#[test]
fn image_budgets() {
    let mut res = Res::default();
    res.add_image(1, "image/png", png_rgba(16, 16, true));
    res.add_image(2, "image/jpeg", JPEG_420.to_vec());
    let with = |f: &dyn Fn(&mut PdfLimits)| {
        let mut limits = PdfLimits::default();
        f(&mut limits);
        PdfOptions {
            limits,
            ..PdfOptions::default()
        }
    };
    let bytes_cap = with(&|l| l.max_image_bytes = 30);
    let pixel_cap = with(&|l| l.max_image_pixels = 10);
    for id in [1, 2] {
        assert_eq!(
            err(&[image_page(id)], &res, &bytes_cap),
            PdfError::LimitExceeded("image bytes")
        );
    }
    // Passed-through JPEG and decoded PNG both stop at the pixel budget.
    assert_eq!(
        err(&[image_page(2)], &res, &pixel_cap),
        PdfError::LimitExceeded("image pixels")
    );
    assert!(
        matches!(
            err(&[image_page(1)], &res, &pixel_cap),
            PdfError::Image(_) | PdfError::LimitExceeded(_)
        ),
        "decoded image over the pixel budget"
    );
}

#[test]
fn pending_content_budget_for_cff_fonts() {
    let data: &'static [u8] = include_bytes!("../../../tests/font-cff-reference/cff-oracle.otf");
    let mut res = Res::default();
    res.add_font(1, 1, data, 20.0);
    res.texts.insert((1, 1), "A".to_owned());
    let glyph = Command::DrawGlyphRun {
        font_id: 1,
        color: BLACK,
        glyphs: vec![PositionedGlyph {
            glyph_id: 1,
            position: p(5.0, 20.0),
            advance: p(10.0, 0.0),
        }],
    };
    let l = list(100.0, 50.0, vec![glyph]);
    let options = PdfOptions {
        limits: PdfLimits {
            max_pending_bytes: 1,
            ..PdfLimits::default()
        },
        ..PdfOptions::default()
    };
    assert_eq!(
        err(&[l], &res, &options),
        PdfError::LimitExceeded("pending page content")
    );
}

fn item(title: &str, page: usize, children: Vec<OutlineItem>) -> OutlineItem {
    OutlineItem {
        title: title.to_owned(),
        destination: Destination {
            page,
            top: Some(0.0),
        },
        children,
        open: false,
    }
}

#[test]
fn option_errors_are_reported() {
    let mut res = Res::default();
    let page = simple_page(&mut res);
    let go = |options: PdfOptions| err(std::slice::from_ref(&page), &res, &options);
    let dest = |n: &str| (n.to_owned(), Destination { page: 0, top: None });
    let bad = |what: &'static str| PdfError::InvalidOptions(what);

    let o = PdfOptions {
        named_destinations: vec![dest("a"), dest("a")],
        ..PdfOptions::default()
    };
    assert_eq!(go(o), bad("duplicate destination name"));
    for name in ["", "é", "tab\t"] {
        let o = PdfOptions {
            named_destinations: vec![dest(name)],
            ..PdfOptions::default()
        };
        assert_eq!(
            go(o),
            bad("destination name must be printable ASCII"),
            "{name:?}"
        );
    }
    let o = PdfOptions {
        named_destinations: vec![(
            "a".to_owned(),
            Destination {
                page: 0,
                top: Some(f64::NAN),
            },
        )],
        ..PdfOptions::default()
    };
    assert_eq!(go(o), bad("destination position"));
    for lang in ["", "fr_FR", "fr FR", &"a".repeat(36)] {
        let o = PdfOptions {
            language: Some(lang.to_owned()),
            ..PdfOptions::default()
        };
        assert_eq!(go(o), bad("language tag"), "{lang:?}");
    }
    let date = |f: &dyn Fn(&mut PdfDate)| {
        let mut d = PdfDate {
            year: 2026,
            month: 10,
            day: 13,
            hour: 12,
            minute: 0,
            second: 0,
            utc_offset_minutes: 60,
        };
        f(&mut d);
        PdfOptions {
            info: PdfInfo {
                creation_date: Some(d),
                ..PdfInfo::default()
            },
            ..PdfOptions::default()
        }
    };
    for o in [
        date(&|d| d.month = 13),
        date(&|d| d.month = 0),
        date(&|d| d.day = 32),
        date(&|d| d.hour = 24),
        date(&|d| d.minute = 60),
        date(&|d| d.year = 0),
        date(&|d| d.utc_offset_minutes = 15 * 60),
    ] {
        assert_eq!(go(o), bad("date"));
    }
    let o = PdfOptions {
        info: PdfInfo {
            title: Some("x".repeat(65_537)),
            ..PdfInfo::default()
        },
        ..PdfOptions::default()
    };
    assert_eq!(go(o), PdfError::LimitExceeded("metadata text"));
}

#[test]
fn outline_errors_and_budgets() {
    let mut res = Res::default();
    let page = simple_page(&mut res);
    let go = |outlines: Vec<OutlineItem>, limits: PdfLimits| {
        err(
            std::slice::from_ref(&page),
            &res,
            &PdfOptions {
                outlines,
                limits,
                ..PdfOptions::default()
            },
        )
    };
    let l = PdfLimits::default();
    assert_eq!(
        go(vec![item("x", 1, vec![])], l),
        PdfError::InvalidDestination
    );
    assert_eq!(
        go(vec![item("x", 0, vec![item("y", usize::MAX, vec![])])], l),
        PdfError::InvalidDestination
    );
    let deep = vec![item("a", 0, vec![item("b", 0, vec![item("c", 0, vec![])])])];
    assert_eq!(
        go(
            deep,
            PdfLimits {
                max_outline_depth: 2,
                ..l
            }
        ),
        PdfError::LimitExceeded("outline depth")
    );
    let many = vec![item("a", 0, vec![]), item("b", 0, vec![])];
    assert_eq!(
        go(
            many,
            PdfLimits {
                max_outline_items: 1,
                ..l
            }
        ),
        PdfError::LimitExceeded("outline items")
    );
    // A named destination on a missing page fails at the end of the document.
    let o = PdfOptions {
        named_destinations: vec![("n".to_owned(), Destination { page: 4, top: None })],
        ..PdfOptions::default()
    };
    assert_eq!(
        err(std::slice::from_ref(&page), &res, &o),
        PdfError::InvalidDestination
    );
}

#[test]
fn unresolved_links_are_notices_not_annotations() {
    let res = Res::default();
    let l = list(
        100.0,
        100.0,
        vec![
            Command::Link {
                bounds: Rect::from_xywh(0.0, 0.0, 10.0, 10.0).unwrap(),
                target: "#nowhere".to_owned(),
            },
            Command::Link {
                bounds: Rect::from_xywh(0.0, 20.0, 10.0, 10.0).unwrap(),
                target: String::new(),
            },
        ],
    );
    let out = render_pdf(&[l], &res, &PdfOptions::default()).unwrap();
    assert_eq!(
        out.notices,
        [
            PdfNotice::UnresolvedLink {
                page: 0,
                target: "#nowhere".to_owned()
            },
            PdfNotice::UnresolvedLink {
                page: 0,
                target: String::new()
            }
        ]
    );
    let pdf = open(&out.bytes);
    assert!(!pdf.pages()[0].1.contains_key("Annots"));
}

#[test]
fn a_failing_sink_is_a_typed_error() {
    struct Failing(usize);
    impl std::io::Write for Failing {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            if self.0 == 0 {
                Err(std::io::ErrorKind::BrokenPipe.into())
            } else {
                self.0 -= 1;
                Ok(b.len())
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut res = Res::default();
    let page = simple_page(&mut res);
    // Fails at the header, in the middle, and at the very end.
    for budget in [0, 3, 12] {
        let result = PdfWriter::new(Failing(budget), PdfOptions::default()).and_then(|mut w| {
            w.add_page(&page, &res)?;
            w.finish(&res).map(|_| ())
        });
        assert_eq!(
            result,
            Err(PdfError::Io(std::io::ErrorKind::BrokenPipe)),
            "budget {budget}"
        );
    }
}

#[test]
fn strings_and_names_are_escaped() {
    let mut res = Res::default();
    let page = simple_page(&mut res);
    let title = "Rapport (final) \\ « été » \u{1F600} ) (";
    let options = PdfOptions {
        info: PdfInfo {
            title: Some(title.to_owned()),
            author: Some("A(B)C\\D".to_owned()),
            ..PdfInfo::default()
        },
        named_destinations: vec![("a(b)\\c d".to_owned(), Destination { page: 0, top: None })],
        outlines: vec![item("Sommaire (1)", 0, vec![])],
        ..PdfOptions::default()
    };
    let l = list(
        200.0,
        100.0,
        vec![Command::Link {
            bounds: Rect::from_xywh(0.0, 0.0, 10.0, 10.0).unwrap(),
            target: "https://example.org/a)b(c\\d é".to_owned(),
        }],
    );
    let out = render_pdf(&[page, l], &res, &options).unwrap();
    let pdf = open(&out.bytes);
    let info = pdf.resolve(&pdf.trailer["Info"]);
    // Unicode title: UTF-16BE with a byte order mark; ASCII author: escaped literal.
    let t = info.dict()["Title"].str_bytes();
    let units: Vec<u16> = t[2..]
        .chunks(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    assert_eq!(&t[..2], [0xFE, 0xFF]);
    assert_eq!(String::from_utf16(&units).unwrap(), title);
    assert_eq!(info.dict()["Author"].str_bytes(), b"A(B)C\\D");
    let (_, pg) = &pdf.pages()[1];
    let annot = pdf.resolve(&pg["Annots"].arr()[0]);
    let action = pdf.get(annot.dict(), "A");
    assert_eq!(
        action.dict()["URI"].str_bytes(),
        b"https://example.org/a)b(c\\d%20%C3%A9"
    );
}

#[test]
fn every_document_has_its_own_id_and_the_same_bytes_every_time() {
    let mut res = Res::default();
    let a = simple_page(&mut res);
    let b = one_run(CARLITO, "different", &mut res);
    let (x1, x2) = (
        render(std::slice::from_ref(&a), &res).unwrap(),
        render(std::slice::from_ref(&a), &res).unwrap(),
    );
    let y = render(&[b], &res).unwrap();
    assert_eq!(x1, x2);
    let id = |bytes: &[u8]| open(bytes).trailer["ID"].clone();
    assert_eq!(id(&x1), id(&x2));
    assert_ne!(id(&x1), id(&y));
}

#[test]
fn non_painting_content_is_skipped() {
    // Transparent paint, empty paths and empty glyph runs write nothing.
    let mut res = Res::default();
    standard_fonts(&mut res);
    let clear = color(1.0, 0.0, 0.0, 0.0);
    let l = list(
        100.0,
        100.0,
        vec![
            fill(rect_path(0.0, 0.0, 50.0, 50.0), clear),
            Command::FillPath {
                path: DisplayPath::new(Vec::<PathElement>::new()).unwrap(),
                color: BLACK,
                rule: atelier_render::FillRule::NonZero,
            },
            Command::DrawGlyphRun {
                font_id: CARLITO,
                color: BLACK,
                glyphs: vec![],
            },
        ],
    );
    let bytes = render(&[l], &res).unwrap();
    let pdf = open(&bytes);
    let ops = ops_of(&pdf, &pdf.pages()[0].1);
    let names: Vec<&str> = ops.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["cm"]);
}

#[test]
fn colour_and_alpha_levels_survive_truncating_and_rounding_readers() {
    // MuPDF truncates `value * 255`, Acrobat rounds: every 8-bit level must
    // come back unchanged from both.
    let res = Res::default();
    let mut commands = Vec::new();
    for v in 0..=255u32 {
        let g = v as f32 / 255.0;
        commands.push(fill(rect_path(0.0, 0.0, 5.0, 5.0), color(g, g, g, 1.0)));
        if (1..255).contains(&v) {
            commands.push(fill(rect_path(0.0, 0.0, 5.0, 5.0), color(0.0, 0.0, 0.0, g)));
        }
    }
    let bytes = render(&[list(10.0, 10.0, commands)], &res).unwrap();
    let pdf = open(&bytes);
    let (_, pg) = &pdf.pages()[0];
    let levels: Vec<f64> = ops_of(&pdf, pg)
        .iter()
        .filter(|(op, _)| op == "g")
        .map(|(_, a)| a[0].num())
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    for x in &levels {
        let (t, r) = ((x * 255.0).floor() as u32, (x * 255.0).round() as u32);
        assert_eq!(t, r, "{x} reads differently when truncated and rounded");
        seen.insert(r);
    }
    assert_eq!(
        seen.len(),
        256,
        "missing {:?}",
        (0..256).filter(|v| !seen.contains(v)).collect::<Vec<_>>()
    );
    let resources = pdf.get(pg, "Resources");
    let gs = pdf.get(resources.dict(), "ExtGState");
    assert_eq!(gs.dict().len(), 255, "alpha levels 1..=255");
    for (name, r) in gs.dict() {
        let level: u32 = name.trim_start_matches("GS").parse().unwrap();
        let ca = pdf.resolve(r).dict()["ca"].num();
        assert_eq!((ca * 255.0).floor() as u32, level, "{name}");
        assert_eq!((ca * 255.0).round() as u32, level, "{name}");
    }
}
