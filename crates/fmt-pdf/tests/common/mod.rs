// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test support: a minimal PDF reader written independently of the writer,
//! a resource provider backed by the embedded fonts, and list builders.
#![allow(dead_code)]

pub mod reader;

use atelier_fmt_pdf::{PdfFont, PdfResourceProvider, SvgResourceProvider};
use atelier_font::FontFace;
use atelier_font::embedded::EMBEDDED_FONTS;
use atelier_font::layout::GlyphCluster;
use atelier_geom::{Point, Rect};
use atelier_render::{
    Color, Command, DisplayList, DisplayPath, FillRule, PageInfo, PathElement, PositionedGlyph,
    SvgImage,
};
use std::collections::HashMap;

pub fn p(x: f64, y: f64) -> Point {
    Point::try_new(x, y).unwrap()
}
pub fn page(w: f64, h: f64) -> PageInfo {
    PageInfo::new(Rect::from_xywh(0.0, 0.0, w, h).unwrap()).unwrap()
}
pub fn color(r: f32, g: f32, b: f32, a: f32) -> Color {
    Color::new(r, g, b, a).unwrap()
}
pub const BLACK: Color = Color {
    red: 0.0,
    green: 0.0,
    blue: 0.0,
    alpha: 1.0,
};
pub fn poly(points: &[(f64, f64)]) -> DisplayPath {
    let mut elements = vec![PathElement::MoveTo(p(points[0].0, points[0].1))];
    for &(x, y) in &points[1..] {
        elements.push(PathElement::LineTo(p(x, y)));
    }
    elements.push(PathElement::Close);
    DisplayPath::new(elements).unwrap()
}
pub fn rect_path(x: f64, y: f64, w: f64, h: f64) -> DisplayPath {
    poly(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h)])
}
pub fn fill(path: DisplayPath, c: Color) -> Command {
    Command::FillPath {
        path,
        color: c,
        rule: FillRule::NonZero,
    }
}
pub fn list(w: f64, h: f64, commands: Vec<Command>) -> DisplayList {
    DisplayList::new(page(w, h), commands).unwrap()
}
pub fn embedded(family: &str, weight: u16, italic: bool) -> &'static [u8] {
    EMBEDDED_FONTS
        .iter()
        .find(|f| {
            f.family == family
                && f.weight == weight
                && (f.style == atelier_font::embedded::EmbeddedFontStyle::Italic) == italic
        })
        .unwrap_or_else(|| panic!("no embedded font {family} {weight} {italic}"))
        .data
}

#[derive(Clone, Copy)]
pub struct FontDef {
    pub program_id: u64,
    pub data: &'static [u8],
    pub face_index: usize,
    pub size: f64,
}

#[derive(Default)]
pub struct Res {
    pub fonts: HashMap<u64, FontDef>,
    pub texts: HashMap<(u64, u32), String>,
    pub outlines: HashMap<(u64, u32), DisplayPath>,
    pub images: HashMap<u64, (&'static str, Vec<u8>)>,
}

impl SvgResourceProvider for Res {
    fn glyph_outline(&self, font_id: u64, glyph_id: u32) -> Option<&DisplayPath> {
        self.outlines.get(&(font_id, glyph_id))
    }
    fn image(&self, image_id: u64) -> Option<SvgImage<'_>> {
        self.images
            .get(&image_id)
            .map(|(mime_type, bytes)| SvgImage { mime_type, bytes })
    }
}
impl PdfResourceProvider for Res {
    fn font(&self, font_id: u64) -> Option<PdfFont<'_>> {
        self.fonts.get(&font_id).map(|f| PdfFont {
            program_id: f.program_id,
            data: f.data,
            face_index: f.face_index,
            size: f.size,
        })
    }
    fn glyph_text(&self, font_id: u64, glyph_id: u32) -> Option<&str> {
        self.texts.get(&(font_id, glyph_id)).map(String::as_str)
    }
}

/// A glyph shaped from text: id, source text and advance in page units.
pub struct Shaped {
    pub gid: u16,
    pub text: String,
    pub advance: f64,
}

impl Res {
    pub fn add_font(&mut self, font_id: u64, program_id: u64, data: &'static [u8], size: f64) {
        self.fonts.insert(
            font_id,
            FontDef {
                program_id,
                data,
                face_index: 0,
                size,
            },
        );
    }
    pub fn add_image(&mut self, id: u64, mime: &'static str, bytes: Vec<u8>) {
        self.images.insert(id, (mime, bytes));
    }

    pub fn shape(&self, font_id: u64, text: &str) -> Vec<Shaped> {
        let def = self.fonts[&font_id];
        let face = FontFace::parse_collection(def.data, def.face_index).unwrap();
        let chars: Vec<char> = text.chars().collect();
        let input: Vec<GlyphCluster> = chars
            .iter()
            .enumerate()
            .map(|(i, &c)| GlyphCluster {
                glyph_id: face.glyph_index(c),
                cluster: i..i + 1,
            })
            .collect();
        let clusters = match face.layout() {
            Ok(layout) => layout.apply_ligatures(&input).unwrap_or(input),
            Err(_) => input,
        };
        let k = def.size / f64::from(face.metrics().units_per_em);
        clusters
            .iter()
            .map(|c| Shaped {
                gid: c.glyph_id,
                text: chars[c.cluster.clone()].iter().collect(),
                advance: face
                    .horizontal_metric(c.glyph_id)
                    .map_or(0.0, |m| f64::from(m.advance_width))
                    * k,
            })
            .collect()
    }

    /// Glyph run for `text` starting at `(x, y)`; registers text and outlines.
    /// Spaces are real glyphs, like in the editor's layout.
    pub fn text(&mut self, font_id: u64, text: &str, x: f64, y: f64, c: Color) -> Command {
        let def = self.fonts[&font_id];
        let face = FontFace::parse_collection(def.data, def.face_index).unwrap();
        let k = def.size / f64::from(face.metrics().units_per_em);
        let mut pen = x;
        let mut glyphs = Vec::new();
        for s in self.shape(font_id, text) {
            self.texts
                .entry((font_id, u32::from(s.gid)))
                .or_insert_with(|| s.text.clone());
            if let Ok(path) = face.outline(s.gid) {
                if let Ok(flat) = path.flatten(0.5) {
                    let mut elements = Vec::new();
                    for sub in &flat {
                        for (i, pt) in sub.points.iter().enumerate() {
                            let q = p(pt.x() * k, -pt.y() * k);
                            elements.push(if i == 0 {
                                PathElement::MoveTo(q)
                            } else {
                                PathElement::LineTo(q)
                            });
                        }
                        if sub.closed {
                            elements.push(PathElement::Close);
                        }
                    }
                    self.outlines
                        .entry((font_id, u32::from(s.gid)))
                        .or_insert_with(|| DisplayPath::new(elements).unwrap());
                }
            }
            glyphs.push(PositionedGlyph {
                glyph_id: u32::from(s.gid),
                position: p(pen, y),
                advance: p(s.advance, 0.0),
            });
            pen += s.advance;
        }
        Command::DrawGlyphRun {
            font_id,
            color: c,
            glyphs,
        }
    }
}

/// Small deterministic generator (xorshift64*).
pub struct Rng(pub u64);
impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    pub fn float(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (self.next() >> 11) as f64 / (1u64 << 53) as f64 * (hi - lo)
    }
}

pub const CARLITO: u64 = 1;
pub const CARLITO_ITALIC: u64 = 2;
pub const CARLITO_BOLD: u64 = 3;
pub const CALADEA: u64 = 4;
pub const MONO: u64 = 5;
pub const CARLITO_BIG: u64 = 6;

/// Registers six font ids over five font programs (ids 1 and 6 share a program).
pub fn standard_fonts(res: &mut Res) {
    res.add_font(CARLITO, 1, embedded("Carlito", 400, false), 12.0);
    res.add_font(CARLITO_ITALIC, 2, embedded("Carlito", 400, true), 12.0);
    res.add_font(CARLITO_BOLD, 3, embedded("Carlito", 700, false), 20.0);
    res.add_font(CALADEA, 4, embedded("Caladea", 400, false), 12.0);
    res.add_font(MONO, 5, embedded("Liberation Mono", 400, false), 10.0);
    res.add_font(CARLITO_BIG, 1, embedded("Carlito", 400, false), 18.0);
}

pub fn png_rgba(w: u32, h: u32, alpha: bool) -> Vec<u8> {
    let mut pixels = Vec::new();
    for y in 0..h {
        for x in 0..w {
            pixels.extend_from_slice(&[
                (x * 255 / w.max(1)) as u8,
                (y * 255 / h.max(1)) as u8,
                128,
                if alpha {
                    ((x + y) * 255 / (w + h)) as u8
                } else {
                    255
                },
            ]);
        }
    }
    atelier_image::encode_png_rgba(
        &atelier_image::Image {
            width: w,
            height: h,
            pixels,
        },
        atelier_image::DecodeLimits::default(),
    )
    .unwrap()
}

pub const JPEG_420: &[u8] = include_bytes!("../../../image/tests/fixtures/jpeg/baseline-420.jpg");
pub const JPEG_GRAY: &[u8] = include_bytes!("../../../image/tests/fixtures/jpeg/baseline-gray.jpg");
pub const JPEG_PROGRESSIVE: &[u8] =
    include_bytes!("../../../image/tests/fixtures/jpeg/progressive-420.jpg");
pub const JPEG_CMYK: &[u8] = include_bytes!("../../../image/tests/fixtures/jpeg/baseline-cmyk.jpg");

pub const LINES: [&str; 5] = [
    "Voilà un texte accentué : déjà, où, naïve, garçon, Noël, Œuvre.",
    "Ligatures : office, difficile, affluent, final, flux.",
    "Texte en italique avec des accents éèêëàùç.",
    "Serif: Hello, World! 0123456789",
    "fn main() { println!(\"ok\"); }",
];

/// A page with text in several fonts, shapes, images, clip, transparency, rotation and a link.
pub fn typical_page(res: &mut Res) -> DisplayList {
    use atelier_render::{DisplayTransform, LineCap, LineJoin, StrokeStyle};
    standard_fonts(res);
    res.add_image(10, "image/png", png_rgba(8, 8, true));
    res.add_image(11, "image/jpeg", JPEG_420.to_vec());
    let mut c = vec![fill(
        rect_path(0.0, 0.0, 595.0, 842.0),
        color(0.96, 0.96, 0.94, 1.0),
    )];
    c.push(res.text(
        CARLITO_BOLD,
        "Éléments d’un écrit — Œuvre",
        56.0,
        80.0,
        BLACK,
    ));
    c.push(res.text(CARLITO, LINES[0], 56.0, 120.0, BLACK));
    c.push(res.text(CARLITO, LINES[1], 56.0, 140.0, color(0.1, 0.1, 0.5, 1.0)));
    c.push(res.text(CARLITO_ITALIC, LINES[2], 56.0, 160.0, BLACK));
    c.push(res.text(CALADEA, LINES[3], 56.0, 180.0, BLACK));
    c.push(res.text(MONO, LINES[4], 56.0, 200.0, color(0.0, 0.4, 0.0, 1.0)));
    c.push(res.text(CARLITO_BIG, "Big", 400.0, 120.0, color(0.8, 0.0, 0.0, 0.5)));
    // Shapes and transparency.
    c.push(fill(
        rect_path(60.0, 240.0, 120.0, 80.0),
        color(1.0, 0.0, 0.0, 0.5),
    ));
    c.push(fill(
        rect_path(120.0, 270.0, 120.0, 80.0),
        color(0.0, 0.0, 1.0, 0.5),
    ));
    let star = poly(&[
        (300.0, 240.0),
        (340.0, 350.0),
        (250.0, 280.0),
        (350.0, 280.0),
        (260.0, 350.0),
    ]);
    c.push(Command::FillPath {
        path: star,
        color: color(0.9, 0.6, 0.0, 1.0),
        rule: FillRule::EvenOdd,
    });
    let curve = DisplayPath::new(vec![
        PathElement::MoveTo(p(380.0, 350.0)),
        PathElement::QuadraticTo {
            control: p(420.0, 230.0),
            to: p(460.0, 350.0),
        },
        PathElement::CubicTo {
            control1: p(480.0, 250.0),
            control2: p(520.0, 450.0),
            to: p(540.0, 350.0),
        },
    ])
    .unwrap();
    c.push(Command::StrokePath {
        path: curve,
        color: color(0.2, 0.2, 0.2, 1.0),
        style: StrokeStyle::new(6.0, LineCap::Round, LineJoin::Round, 4.0).unwrap(),
    });
    c.push(Command::StrokePath {
        path: poly(&[(60.0, 380.0), (160.0, 380.0), (110.0, 440.0)]),
        color: color(0.0, 0.0, 0.0, 1.0),
        style: StrokeStyle::new(3.0, LineCap::Square, LineJoin::Miter, 10.0).unwrap(),
    });
    // Images.
    c.push(Command::DrawImage {
        image_id: 10,
        bounds: Rect::from_xywh(200.0, 380.0, 96.0, 96.0).unwrap(),
    });
    c.push(Command::DrawImage {
        image_id: 11,
        bounds: Rect::from_xywh(320.0, 380.0, 76.0, 68.0).unwrap(),
    });
    // Clip.
    c.push(Command::PushClip {
        bounds: Rect::from_xywh(60.0, 500.0, 100.0, 60.0).unwrap(),
    });
    c.push(fill(
        poly(&[(40.0, 480.0), (240.0, 520.0), (100.0, 620.0)]),
        color(0.0, 0.5, 0.5, 1.0),
    ));
    c.push(Command::PopClip);
    // Rotation.
    let (s, k) = (15f64.to_radians().sin(), 15f64.to_radians().cos());
    c.push(Command::PushTransform {
        transform: DisplayTransform::new(k, s, -s, k, 300.0, 520.0).unwrap(),
    });
    c.push(res.text(CARLITO, "Texte tourné de quinze degrés", 0.0, 0.0, BLACK));
    c.push(Command::PopTransform);
    c.push(Command::Link {
        bounds: Rect::from_xywh(56.0, 105.0, 200.0, 20.0).unwrap(),
        target: "https://example.org/a b?x=é".to_owned(),
    });
    list(595.0, 842.0, c)
}
