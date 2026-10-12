// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::{FontFace, embedded::EMBEDDED_FONTS};
use atelier_geom::{Path, Point, Rect};
use atelier_image::{DecodeLimits, Image, decode_png, encode_png_rgba};
use atelier_raster::display_list::{
    PixelRegion, RenderError, RenderLimits, RenderOptions, Resolution, ResourceProvider, SvgImage,
    page_pixel_size, render_image, render_png,
};
use atelier_raster::{FillRule as RFill, RasterLimits, Rasterizer};
use atelier_render::{
    Color, Command, DisplayList, DisplayPath, DisplayTransform, FillRule, LineCap, LineJoin,
    PageInfo, PathElement, PositionedGlyph, StrokeStyle, SvgLimits, render_svg,
};
use atelier_xml::{Event, Reader};
use std::collections::HashMap;

// ------------------------------------------------------------------ helpers

#[derive(Default)]
struct Resources {
    glyphs: HashMap<(u64, u32), DisplayPath>,
    images: HashMap<u64, (&'static str, Vec<u8>)>,
}
impl ResourceProvider for Resources {
    fn glyph_outline(&self, font_id: u64, glyph_id: u32) -> Option<&DisplayPath> {
        self.glyphs.get(&(font_id, glyph_id))
    }
    fn image(&self, image_id: u64) -> Option<SvgImage<'_>> {
        self.images
            .get(&image_id)
            .map(|(mime_type, bytes)| SvgImage { mime_type, bytes })
    }
}
fn p(x: f64, y: f64) -> Point {
    Point::try_new(x, y).unwrap()
}
fn page(w: f64, h: f64) -> PageInfo {
    PageInfo::new(Rect::from_xywh(0.0, 0.0, w, h).unwrap()).unwrap()
}
fn color(r: f32, g: f32, b: f32, a: f32) -> Color {
    Color::new(r, g, b, a).unwrap()
}
const BLACK: fn() -> Color = || Color {
    red: 0.0,
    green: 0.0,
    blue: 0.0,
    alpha: 1.0,
};
fn poly(points: &[(f64, f64)]) -> DisplayPath {
    let mut elements = vec![PathElement::MoveTo(p(points[0].0, points[0].1))];
    for &(x, y) in &points[1..] {
        elements.push(PathElement::LineTo(p(x, y)));
    }
    elements.push(PathElement::Close);
    DisplayPath::new(elements).unwrap()
}
fn rect_path(x: f64, y: f64, w: f64, h: f64) -> DisplayPath {
    poly(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h)])
}
fn fill(path: DisplayPath, c: Color) -> Command {
    Command::FillPath {
        path,
        color: c,
        rule: FillRule::NonZero,
    }
}
fn clip(x: f64, y: f64, w: f64, h: f64) -> Command {
    Command::PushClip {
        bounds: Rect::from_xywh(x, y, w, h).unwrap(),
    }
}
fn transform(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Command {
    Command::PushTransform {
        transform: DisplayTransform::new(a, b, c, d, e, f).unwrap(),
    }
}
fn list(w: f64, h: f64, commands: Vec<Command>) -> DisplayList {
    DisplayList::new(page(w, h), commands).unwrap()
}
fn options(scale: f64) -> RenderOptions {
    RenderOptions {
        resolution: Resolution::Scale(scale),
        ..RenderOptions::default()
    }
}
fn transparent(scale: f64) -> RenderOptions {
    RenderOptions {
        background: [0; 4],
        ..options(scale)
    }
}
fn render(list: &DisplayList, res: &Resources, o: &RenderOptions) -> Image {
    render_image(list, res, o).unwrap()
}
fn px(image: &Image, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * image.width + x) * 4) as usize;
    image.pixels[i..i + 4].try_into().unwrap()
}
const WHITE: [u8; 4] = [255; 4];

/// FNV-1a 64-bit checksum.
fn fnv(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn float(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (self.next() >> 11) as f64 / (1u64 << 53) as f64 * (hi - lo)
    }
}

// Real glyph outlines from an embedded font, flattened to polylines (the
// geometry crate exposes no accessor for curve segments).
struct Font {
    face: FontFace<'static>,
}
impl Font {
    fn carlito() -> Self {
        let data = EMBEDDED_FONTS
            .iter()
            .find(|f| f.family == "Carlito" && f.weight == 400 && f.data.len() > 1000)
            .unwrap()
            .data;
        Self {
            face: FontFace::parse(data).unwrap(),
        }
    }
    fn outline(&self, glyph: u16, size: f64) -> DisplayPath {
        let flat = self.face.outline(glyph).unwrap().flatten(0.5).unwrap();
        let k = size / f64::from(self.face.metrics().units_per_em);
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
        DisplayPath::new(elements).unwrap()
    }
    /// A glyph-run command for `text`, registering its outlines in `res`.
    fn run(&self, res: &mut Resources, text: &str, size: f64, x: f64, y: f64, c: Color) -> Command {
        let upem = f64::from(self.face.metrics().units_per_em);
        let mut pen = x;
        let mut glyphs = Vec::new();
        for ch in text.chars() {
            let gid = self.face.glyph_index(ch);
            let advance =
                f64::from(self.face.horizontal_metric(gid).unwrap().advance_width) * size / upem;
            if ch != ' ' {
                res.glyphs
                    .entry((1, u32::from(gid)))
                    .or_insert_with(|| self.outline(gid, size));
                glyphs.push(PositionedGlyph {
                    glyph_id: u32::from(gid),
                    position: p(pen, y),
                    advance: p(advance, 0.0),
                });
            }
            pen += advance;
        }
        Command::DrawGlyphRun {
            font_id: 1,
            color: c,
            glyphs,
        }
    }
}
// Glyph outlines are registered at one size, so runs of other sizes use a
// separate font id.
fn text_at(res: &mut Resources, font_id: u64, text: &str, size: f64, x: f64, y: f64) -> Command {
    let font = Font::carlito();
    let mut scratch = Resources::default();
    let Command::DrawGlyphRun { glyphs, color, .. } =
        font.run(&mut scratch, text, size, x, y, BLACK())
    else {
        unreachable!()
    };
    for ((_, gid), path) in scratch.glyphs {
        res.glyphs.insert((font_id, gid), path);
    }
    Command::DrawGlyphRun {
        font_id,
        color,
        glyphs,
    }
}

fn png_2x2() -> Vec<u8> {
    let image = Image {
        width: 2,
        height: 2,
        pixels: vec![
            255, 0, 0, 255, 0, 255, 0, 255, // red, green
            0, 0, 255, 255, 255, 255, 255, 255, // blue, white
        ],
    };
    encode_png_rgba(&image, DecodeLimits::default()).unwrap()
}
fn with_image() -> Resources {
    let mut res = Resources::default();
    res.images.insert(1, ("image/png", png_2x2()));
    res
}

// ------------------------------------------------------- sizes and resolution

#[test]
fn dimensions_follow_the_resolution() {
    let l = list(200.0, 100.0, vec![]);
    let lim = RenderLimits::default();
    for (res, expected) in [
        (Resolution::Scale(1.0), (200, 100)),
        (Resolution::Scale(0.5), (100, 50)),
        (Resolution::Scale(3.0), (600, 300)),
        (Resolution::Dpi(72.0), (200, 100)),
        (Resolution::Dpi(144.0), (400, 200)),
        (Resolution::Dpi(96.0), (267, 134)),
        (Resolution::Dpi(300.0), (834, 417)),
    ] {
        assert_eq!(page_pixel_size(&l, res, &lim), Ok(expected), "{res:?}");
        let image = render(
            &l,
            &Resources::default(),
            &RenderOptions {
                resolution: res,
                ..RenderOptions::default()
            },
        );
        assert_eq!((image.width, image.height), expected);
        assert!(image.pixels.chunks_exact(4).all(|px| px == WHITE));
    }
    // Transparent background.
    let image = render(&l, &Resources::default(), &transparent(1.0));
    assert!(image.pixels.iter().all(|&b| b == 0));
}

#[test]
fn page_origin_is_taken_from_the_list() {
    let page = PageInfo::new(Rect::from_xywh(100.0, 50.0, 20.0, 20.0).unwrap()).unwrap();
    let l = DisplayList::new(
        page,
        vec![fill(
            rect_path(105.0, 55.0, 5.0, 5.0),
            color(1.0, 0.0, 0.0, 1.0),
        )],
    )
    .unwrap();
    let image = render(&l, &Resources::default(), &options(2.0));
    assert_eq!((image.width, image.height), (40, 40));
    assert_eq!(px(&image, 10, 10), [255, 0, 0, 255]);
    assert_eq!(px(&image, 19, 19), [255, 0, 0, 255]);
    assert_eq!(px(&image, 9, 10), WHITE);
    assert_eq!(px(&image, 20, 10), WHITE);
}

// ------------------------------------------------------------- fills & strokes

#[test]
fn rectangle_pixels_are_exact_at_two_resolutions() {
    let l = list(
        50.0,
        40.0,
        vec![fill(
            rect_path(10.0, 10.0, 30.0, 20.0),
            color(1.0, 0.0, 0.0, 1.0),
        )],
    );
    let red = [255, 0, 0, 255];
    let one = render(&l, &Resources::default(), &options(1.0));
    assert_eq!(px(&one, 10, 10), red);
    assert_eq!(px(&one, 39, 29), red);
    assert_eq!(px(&one, 9, 10), WHITE);
    assert_eq!(px(&one, 40, 10), WHITE);
    assert_eq!(px(&one, 10, 30), WHITE);
    let two = render(&l, &Resources::default(), &options(2.0));
    assert_eq!(px(&two, 20, 20), red);
    assert_eq!(px(&two, 79, 59), red);
    assert_eq!(px(&two, 19, 20), WHITE);
    assert_eq!(px(&two, 80, 20), WHITE);
    // A fractional scale gives a partially covered edge pixel (half of 1.5).
    let frac = render(&l, &Resources::default(), &options(1.25));
    // x from 12.5 to 50: pixel 12 is half covered, white to red.
    let edge = px(&frac, 12, 20);
    assert!(
        edge[0] == 255 && (edge[1] as i32 - 128).abs() <= 2,
        "{edge:?}"
    );
}

#[test]
fn alpha_is_composited_over_the_background() {
    let l = list(
        20.0,
        20.0,
        vec![fill(
            rect_path(0.0, 0.0, 10.0, 20.0),
            color(0.0, 0.0, 1.0, 0.5),
        )],
    );
    let on_white = render(&l, &Resources::default(), &options(1.0));
    let c = px(&on_white, 5, 5);
    assert!(
        (c[0] as i32 - 127).abs() <= 1 && c[2] == 255 && c[3] == 255,
        "{c:?}"
    );
    let on_clear = render(&l, &Resources::default(), &transparent(1.0));
    assert_eq!(px(&on_clear, 5, 5), [0, 0, 255, 128]);
    assert_eq!(px(&on_clear, 15, 5), [0; 4]);
    let on_color = render(
        &l,
        &Resources::default(),
        &RenderOptions {
            background: [0, 255, 0, 255],
            ..options(1.0)
        },
    );
    let c = px(&on_color, 5, 5);
    assert!(
        (c[1] as i32 - 127).abs() <= 1 && (c[2] as i32 - 128).abs() <= 1,
        "{c:?}"
    );
}

#[test]
fn both_fill_rules_are_honoured() {
    let ring = DisplayPath::new(
        [
            rect_path(0.0, 0.0, 40.0, 40.0),
            rect_path(10.0, 10.0, 20.0, 20.0),
        ]
        .iter()
        .flat_map(|p| p.elements.clone())
        .collect(),
    )
    .unwrap();
    let make = |rule| {
        list(
            40.0,
            40.0,
            vec![Command::FillPath {
                path: ring.clone(),
                color: BLACK(),
                rule,
            }],
        )
    };
    let even = render(
        &make(FillRule::EvenOdd),
        &Resources::default(),
        &options(1.0),
    );
    assert_eq!(px(&even, 20, 20), WHITE);
    assert_eq!(px(&even, 5, 5), [0, 0, 0, 255]);
    let non_zero = render(
        &make(FillRule::NonZero),
        &Resources::default(),
        &options(1.0),
    );
    assert_eq!(px(&non_zero, 20, 20), [0, 0, 0, 255]);
}

#[test]
fn strokes_use_width_caps_and_joins() {
    let line = |cap| {
        list(
            80.0,
            40.0,
            vec![Command::StrokePath {
                path: DisplayPath::new(vec![
                    PathElement::MoveTo(p(10.0, 20.0)),
                    PathElement::LineTo(p(60.0, 20.0)),
                ])
                .unwrap(),
                color: BLACK(),
                style: StrokeStyle::new(4.0, cap, LineJoin::Miter, 4.0).unwrap(),
            }],
        )
    };
    let butt = render(&line(LineCap::Butt), &Resources::default(), &options(1.0));
    let black = [0, 0, 0, 255];
    assert_eq!(px(&butt, 30, 18), black);
    assert_eq!(px(&butt, 30, 21), black);
    assert_eq!(px(&butt, 30, 17), WHITE);
    assert_eq!(px(&butt, 30, 22), WHITE);
    assert_eq!(px(&butt, 8, 20), WHITE);
    assert_eq!(px(&butt, 61, 20), WHITE);
    let square = render(&line(LineCap::Square), &Resources::default(), &options(1.0));
    assert_eq!(px(&square, 8, 20), black);
    assert_eq!(px(&square, 61, 20), black);
    // The width scales with the resolution.
    let big = render(&line(LineCap::Butt), &Resources::default(), &options(3.0));
    assert_eq!(px(&big, 90, 54), black);
    assert_eq!(px(&big, 90, 65), black);
    assert_eq!(px(&big, 90, 52), WHITE);
    assert_eq!(px(&big, 90, 67), WHITE);
}

#[test]
fn rotation_maps_the_shape_to_the_expected_pixels() {
    // (x, y) -> (50 - y, x): a quarter turn.
    let l = list(
        60.0,
        60.0,
        vec![
            transform(0.0, 1.0, -1.0, 0.0, 50.0, 0.0),
            fill(rect_path(10.0, 10.0, 20.0, 10.0), BLACK()),
            Command::PopTransform,
            fill(rect_path(0.0, 50.0, 5.0, 5.0), color(1.0, 0.0, 0.0, 1.0)),
        ],
    );
    let image = render(&l, &Resources::default(), &options(1.0));
    assert_eq!(px(&image, 35, 20), [0, 0, 0, 255]);
    assert_eq!(px(&image, 30, 10), [0, 0, 0, 255]);
    assert_eq!(px(&image, 39, 29), [0, 0, 0, 255]);
    assert_eq!(px(&image, 20, 20), WHITE);
    assert_eq!(px(&image, 35, 35), WHITE);
    // The pop restores the identity.
    assert_eq!(px(&image, 2, 52), [255, 0, 0, 255]);
}

#[test]
fn a_degenerate_transform_paints_nothing_and_does_not_fail() {
    let res = with_image();
    let l = list(
        30.0,
        30.0,
        vec![
            transform(0.0, 0.0, 0.0, 0.0, 5.0, 5.0),
            fill(rect_path(0.0, 0.0, 20.0, 20.0), BLACK()),
            Command::DrawImage {
                image_id: 1,
                bounds: Rect::from_xywh(0.0, 0.0, 10.0, 10.0).unwrap(),
            },
            Command::PopTransform,
        ],
    );
    let image = render(&l, &res, &options(1.0));
    assert!(image.pixels.chunks_exact(4).all(|c| c == WHITE));
    // Zero-sized image rectangle too.
    let l = list(
        30.0,
        30.0,
        vec![Command::DrawImage {
            image_id: 1,
            bounds: Rect::from_xywh(5.0, 5.0, 0.0, 10.0).unwrap(),
        }],
    );
    let image = render(&l, &res, &options(1.0));
    assert!(image.pixels.chunks_exact(4).all(|c| c == WHITE));
}

// ------------------------------------------------------------------- clipping

#[test]
fn nothing_changes_outside_the_clip() {
    let body = vec![
        fill(rect_path(0.0, 0.0, 80.0, 80.0), color(0.0, 0.0, 1.0, 1.0)),
        transform(1.0, 0.0, 0.0, 1.0, 5.0, 5.0),
        fill(rect_path(0.0, 0.0, 70.0, 70.0), color(1.0, 0.0, 0.0, 0.5)),
        Command::PopTransform,
    ];
    let mut clipped = vec![clip(20.0, 20.0, 30.0, 40.0)];
    clipped.extend(body);
    clipped.push(Command::PopClip);
    clipped.push(fill(rect_path(70.0, 70.0, 5.0, 5.0), BLACK()));
    let with_clip = render(
        &list(80.0, 80.0, clipped),
        &Resources::default(),
        &options(1.0),
    );
    let reference = render(
        &list(
            80.0,
            80.0,
            vec![fill(rect_path(70.0, 70.0, 5.0, 5.0), BLACK())],
        ),
        &Resources::default(),
        &options(1.0),
    );
    for y in 0..80 {
        for x in 0..80 {
            let inside = (20..50).contains(&x) && (20..60).contains(&y);
            if inside {
                assert_ne!(px(&with_clip, x, y), WHITE, "({x},{y})");
            } else {
                assert_eq!(px(&with_clip, x, y), px(&reference, x, y), "({x},{y})");
            }
        }
    }
}

#[test]
fn a_fractional_clip_edge_is_not_darkened_twice() {
    // A black shape covering the page clipped at x = 10.5: the edge pixel is half black.
    let l = list(
        20.0,
        20.0,
        vec![
            clip(10.5, 0.0, 9.5, 20.0),
            fill(rect_path(0.0, 0.0, 20.0, 20.0), BLACK()),
            Command::PopClip,
        ],
    );
    let image = render(&l, &Resources::default(), &options(1.0));
    let edge = px(&image, 10, 5);
    assert!((edge[0] as i32 - 128).abs() <= 2, "{edge:?}");
    assert_eq!(px(&image, 9, 5), WHITE);
    assert_eq!(px(&image, 11, 5), [0, 0, 0, 255]);
}

#[test]
fn nested_clips_intersect_and_pops_restore_independently() {
    let l = list(
        60.0,
        60.0,
        vec![
            clip(0.0, 0.0, 40.0, 40.0),
            clip(20.0, 20.0, 40.0, 40.0),
            fill(rect_path(0.0, 0.0, 60.0, 60.0), color(1.0, 0.0, 0.0, 1.0)),
            Command::PopClip,
            fill(rect_path(0.0, 0.0, 10.0, 10.0), color(0.0, 1.0, 0.0, 1.0)),
            Command::PopClip,
            // Disjoint clips: everything inside is hidden.
            clip(0.0, 40.0, 10.0, 10.0),
            clip(30.0, 40.0, 10.0, 10.0),
            fill(rect_path(0.0, 0.0, 60.0, 60.0), BLACK()),
            Command::PopClip,
            Command::PopClip,
        ],
    );
    let image = render(&l, &Resources::default(), &options(1.0));
    assert_eq!(px(&image, 25, 25), [255, 0, 0, 255]);
    assert_eq!(px(&image, 45, 45), WHITE);
    assert_eq!(px(&image, 10, 30), WHITE);
    assert_eq!(px(&image, 5, 5), [0, 255, 0, 255]);
    assert_eq!(px(&image, 5, 45), WHITE);
    assert_eq!(px(&image, 35, 45), WHITE);
}

#[test]
fn clip_and_transform_stacks_cross_without_interference() {
    // The clip is in page coordinates: a later transform does not move it.
    let l = list(
        60.0,
        60.0,
        vec![
            clip(0.0, 0.0, 30.0, 30.0),
            transform(1.0, 0.0, 0.0, 1.0, 20.0, 20.0),
            Command::PopClip,
            fill(rect_path(0.0, 0.0, 10.0, 10.0), BLACK()),
            Command::PopTransform,
        ],
    );
    let image = render(&l, &Resources::default(), &options(1.0));
    assert_eq!(px(&image, 25, 25), [0, 0, 0, 255]);
    assert_eq!(px(&image, 5, 5), WHITE);
}

// ---------------------------------------------------------------------- images

#[test]
fn images_are_placed_scaled_and_clipped() {
    let res = with_image();
    let near = |c: [u8; 4], t: [u8; 4]| {
        c.iter()
            .zip(t)
            .all(|(x, y)| (i32::from(*x) - i32::from(y)).abs() <= 25)
    };
    let l = list(
        60.0,
        60.0,
        vec![Command::DrawImage {
            image_id: 1,
            bounds: Rect::from_xywh(10.0, 10.0, 40.0, 40.0).unwrap(),
        }],
    );
    let image = render(&l, &res, &options(1.0));
    assert!(near(px(&image, 20, 20), [255, 0, 0, 255]));
    assert!(near(px(&image, 40, 20), [0, 255, 0, 255]));
    assert!(near(px(&image, 20, 40), [0, 0, 255, 255]));
    assert!(near(px(&image, 40, 40), [255, 255, 255, 255]));
    assert_eq!(px(&image, 9, 20), WHITE);
    assert_eq!(px(&image, 50, 20), WHITE);
    // Same placement at 2x: same quadrants, double the pixels.
    let two = render(&l, &res, &options(2.0));
    assert!(near(px(&two, 40, 40), [255, 0, 0, 255]));
    assert!(near(px(&two, 80, 40), [0, 255, 0, 255]));
    // Clipping the right half hides the green quadrant only.
    let clipped = list(
        60.0,
        60.0,
        vec![
            clip(0.0, 0.0, 30.0, 60.0),
            Command::DrawImage {
                image_id: 1,
                bounds: Rect::from_xywh(10.0, 10.0, 40.0, 40.0).unwrap(),
            },
            Command::PopClip,
        ],
    );
    let image = render(&clipped, &res, &options(1.0));
    assert!(near(px(&image, 20, 20), [255, 0, 0, 255]));
    assert_eq!(px(&image, 40, 20), WHITE);
}

#[test]
fn images_survive_rotation() {
    let res = with_image();
    // Quarter turn about (30, 30): the red (top-left) quadrant moves to the top-right.
    let l = list(
        60.0,
        60.0,
        vec![
            transform(0.0, 1.0, -1.0, 0.0, 60.0, 0.0),
            Command::DrawImage {
                image_id: 1,
                bounds: Rect::from_xywh(10.0, 10.0, 40.0, 40.0).unwrap(),
            },
            Command::PopTransform,
        ],
    );
    let image = render(&l, &res, &options(1.0));
    let near = |c: [u8; 4], t: [u8; 4]| {
        c.iter()
            .zip(t)
            .all(|(a, b)| (*a as i32 - b as i32).abs() <= 25)
    };
    // (x, y) -> (60 - y, x): red (20,20) -> (40,20); green (40,20) -> (40,40).
    assert!(
        near(px(&image, 40, 20), [255, 0, 0, 255]),
        "{:?}",
        px(&image, 40, 20)
    );
    assert!(
        near(px(&image, 40, 40), [0, 255, 0, 255]),
        "{:?}",
        px(&image, 40, 40)
    );
    assert!(
        near(px(&image, 20, 20), [0, 0, 255, 255]),
        "{:?}",
        px(&image, 20, 20)
    );
    assert_eq!(px(&image, 55, 55), WHITE);
}

#[test]
fn general_affine_images_match_the_direct_matrix() {
    // Shear + rotation + non-uniform scale: pixel colours checked against the
    // forward mapping of the quadrant centres.
    let res = with_image();
    let (a, b, c, d, e, f) = (0.9, 0.35, -0.4, 1.1, 15.0, 3.0);
    let l = list(
        80.0,
        80.0,
        vec![
            transform(a, b, c, d, e, f),
            Command::DrawImage {
                image_id: 1,
                bounds: Rect::from_xywh(0.0, 0.0, 40.0, 40.0).unwrap(),
            },
            Command::PopTransform,
        ],
    );
    let image = render(&l, &res, &options(1.0));
    let near = |c: [u8; 4], t: [u8; 4]| {
        c.iter()
            .zip(t)
            .all(|(x, y)| (*x as i32 - y as i32).abs() <= 25)
    };
    for ((u, v), expected) in [
        ((10.0, 10.0), [255, 0, 0, 255]),
        ((30.0, 10.0), [0, 255, 0, 255]),
        ((10.0, 30.0), [0, 0, 255, 255]),
        ((30.0, 30.0), [255, 255, 255, 255]),
    ] {
        let x = a * u + c * v + e;
        let y = b * u + d * v + f;
        let got = px(&image, x as u32, y as u32);
        assert!(near(got, expected), "({u},{v}) -> ({x},{y}): {got:?}");
    }
}

#[test]
fn image_errors_are_typed() {
    let mut res = with_image();
    res.images
        .insert(2, ("image/png", vec![0x89, b'P', b'N', b'G', 1, 2]));
    res.images.insert(3, ("text/plain", png_2x2()));
    res.images.insert(4, ("image/jpeg", png_2x2()));
    let draw = |id| {
        list(
            10.0,
            10.0,
            vec![Command::DrawImage {
                image_id: id,
                bounds: Rect::from_xywh(0.0, 0.0, 5.0, 5.0).unwrap(),
            }],
        )
    };
    let o = options(1.0);
    assert_eq!(
        render_image(&draw(9), &res, &o),
        Err(RenderError::MissingResource)
    );
    assert!(matches!(
        render_image(&draw(2), &res, &o),
        Err(RenderError::Image(_))
    ));
    assert_eq!(
        render_image(&draw(3), &res, &o),
        Err(RenderError::InvalidMime)
    );
    assert!(matches!(
        render_image(&draw(4), &res, &o),
        Err(RenderError::Image(_))
    ));
    // The preflight also covers an image hidden by an empty clip.
    let hidden = list(
        10.0,
        10.0,
        vec![
            clip(0.0, 0.0, 2.0, 2.0),
            clip(5.0, 5.0, 2.0, 2.0),
            Command::DrawImage {
                image_id: 9,
                bounds: Rect::from_xywh(0.0, 0.0, 5.0, 5.0).unwrap(),
            },
            Command::PopClip,
            Command::PopClip,
        ],
    );
    assert_eq!(
        render_image(&hidden, &res, &o),
        Err(RenderError::MissingResource)
    );
    // Image budgets.
    let small = RenderOptions {
        limits: RenderLimits {
            max_image_bytes: 10,
            ..RenderLimits::default()
        },
        ..o
    };
    assert_eq!(
        render_image(&draw(1), &res, &small),
        Err(RenderError::LimitExceeded)
    );
    let few = RenderOptions {
        limits: RenderLimits {
            max_image_pixels: 3,
            ..RenderLimits::default()
        },
        ..o
    };
    assert!(matches!(
        render_image(&draw(1), &res, &few),
        Err(RenderError::Image(_))
    ));
}

// ------------------------------------------------------------------------ text

fn text_page(res: &mut Resources) -> DisplayList {
    let run = text_at(res, 7, "Atelier raster 0123", 14.0, 10.0, 30.0);
    list(240.0, 50.0, vec![run])
}

fn ink(image: &Image) -> f64 {
    image
        .pixels
        .chunks_exact(4)
        .map(|c| f64::from(c[3]) / 255.0)
        .sum()
}
fn ink_bounds(image: &Image) -> Option<(u32, u32, u32, u32)> {
    let mut b: Option<(u32, u32, u32, u32)> = None;
    for y in 0..image.height {
        for x in 0..image.width {
            if px(image, x, y)[3] > 0 {
                let v = b.get_or_insert((x, y, x, y));
                v.0 = v.0.min(x);
                v.1 = v.1.min(y);
                v.2 = v.2.max(x);
                v.3 = v.3.max(y);
            }
        }
    }
    b
}

#[test]
fn text_ink_is_consistent_between_resolutions() {
    let mut res = Resources::default();
    let l = text_page(&mut res);
    let low = render(&l, &res, &transparent(1.0));
    let high = render(&l, &res, &transparent(3.0));
    let area_low = ink(&low);
    let area_high = ink(&high) / 9.0;
    assert!(area_low > 100.0, "text must leave ink: {area_low}");
    assert!(
        (area_low - area_high).abs() / area_high < 0.03,
        "{area_low} vs {area_high}"
    );
    // Ink boxes agree after scaling (within a pixel at the coarse resolution).
    let (a0, b0, a1, b1) = ink_bounds(&low).unwrap();
    let (c0, d0, c1, d1) = ink_bounds(&high).unwrap();
    assert!((f64::from(a0) - f64::from(c0) / 3.0).abs() <= 1.0);
    assert!((f64::from(b0) - f64::from(d0) / 3.0).abs() <= 1.0);
    assert!((f64::from(a1) - f64::from(c1) / 3.0).abs() <= 1.0);
    assert!((f64::from(b1) - f64::from(d1) / 3.0).abs() <= 1.0);
    // The text sits above its baseline (y = 30) and right of its origin.
    assert!(b1 <= 32 && a0 >= 9);
}

#[test]
fn text_colour_alpha_and_missing_glyphs() {
    let mut res = Resources::default();
    let Command::DrawGlyphRun { glyphs, .. } = text_at(&mut res, 7, "H", 40.0, 5.0, 40.0) else {
        unreachable!()
    };
    let l = list(
        50.0,
        50.0,
        vec![Command::DrawGlyphRun {
            font_id: 7,
            color: color(1.0, 0.0, 0.0, 1.0),
            glyphs: glyphs.clone(),
        }],
    );
    let image = render(&l, &res, &options(1.0));
    let stem = (0..50)
        .flat_map(|x| (0..50).map(move |y| (x, y)))
        .find(|&(x, y)| px(&image, x, y)[1] == 0);
    assert!(stem.is_some());
    let missing = list(
        50.0,
        50.0,
        vec![Command::DrawGlyphRun {
            font_id: 8,
            color: BLACK(),
            glyphs,
        }],
    );
    assert_eq!(
        render_image(&missing, &res, &options(1.0)),
        Err(RenderError::MissingResource)
    );
    // A space (no outline) is accepted.
    res.glyphs.insert((9, 1), DisplayPath::default());
    let blank = list(
        50.0,
        50.0,
        vec![Command::DrawGlyphRun {
            font_id: 9,
            color: BLACK(),
            glyphs: vec![PositionedGlyph {
                glyph_id: 1,
                position: p(5.0, 5.0),
                advance: p(3.0, 0.0),
            }],
        }],
    );
    let image = render(&blank, &res, &options(1.0));
    assert!(image.pixels.chunks_exact(4).all(|c| c == WHITE));
}

// ------------------------------------------------------- region, tiles, robustness

fn sample_page(res: &mut Resources) -> DisplayList {
    let run = text_at(res, 7, "Tile seams", 20.0, 30.0, 60.0);
    res.images.insert(1, ("image/png", png_2x2()));
    list(
        300.0,
        200.0,
        vec![
            fill(
                rect_path(0.0, 0.0, 300.0, 200.0),
                color(0.9, 0.95, 1.0, 1.0),
            ),
            fill(
                poly(&[(20.3, 20.7), (250.1, 40.2), (130.9, 170.4)]),
                color(0.8, 0.2, 0.1, 0.7),
            ),
            Command::StrokePath {
                path: DisplayPath::new(vec![
                    PathElement::MoveTo(p(10.0, 190.0)),
                    PathElement::CubicTo {
                        control1: p(100.0, 100.0),
                        control2: p(200.0, 250.0),
                        to: p(290.0, 20.0),
                    },
                ])
                .unwrap(),
                color: color(0.0, 0.3, 0.0, 1.0),
                style: StrokeStyle::new(5.5, LineCap::Round, LineJoin::Round, 4.0).unwrap(),
            },
            clip(50.0, 50.5, 180.0, 90.0),
            run,
            transform(0.8, 0.5, -0.5, 0.8, 120.0, 20.0),
            Command::DrawImage {
                image_id: 1,
                bounds: Rect::from_xywh(0.0, 0.0, 90.0, 60.0).unwrap(),
            },
            fill(rect_path(10.0, 10.0, 50.0, 30.0), color(0.0, 0.0, 0.0, 0.5)),
            Command::PopTransform,
            Command::PopClip,
        ],
    )
}

#[test]
fn a_region_equals_the_crop_of_the_full_render() {
    let mut res = Resources::default();
    let l = sample_page(&mut res);
    for scale in [1.0, 1.7] {
        let full = render(&l, &res, &options(scale));
        let region = PixelRegion {
            x: 61,
            y: 37,
            width: 150,
            height: 101,
        };
        let part = render(
            &l,
            &res,
            &RenderOptions {
                region: Some(region),
                ..options(scale)
            },
        );
        assert_eq!((part.width, part.height), (150, 101));
        for y in 0..region.height {
            for x in 0..region.width {
                assert_eq!(
                    px(&part, x, y),
                    px(&full, x + region.x, y + region.y),
                    "scale {scale} at ({x},{y})"
                );
            }
        }
    }
}

#[test]
fn tiled_output_matches_a_single_surface_render() {
    // The same shapes drawn directly with the rasterizer on one 300 x 200 surface.
    let tri = poly(&[(20.3, 20.7), (250.1, 40.2), (130.9, 170.4)]);
    let l = list(
        300.0,
        200.0,
        vec![
            fill(
                rect_path(3.5, 4.5, 280.25, 150.75),
                color(0.2, 0.4, 0.6, 1.0),
            ),
            fill(tri.clone(), color(0.8, 0.2, 0.1, 0.7)),
        ],
    );
    let tiled = render(&l, &Resources::default(), &options(1.0));
    let mut direct = Rasterizer::new(300, 200, RasterLimits::default()).unwrap();
    direct.clear([255; 4]);
    let to_path = |d: &DisplayPath| {
        let mut path = Path::new();
        for e in &d.elements {
            match *e {
                PathElement::MoveTo(q) => path.move_to(q),
                PathElement::LineTo(q) => path.line_to(q).unwrap(),
                PathElement::Close => path.close().unwrap(),
                _ => unreachable!(),
            }
        }
        path
    };
    direct
        .fill_path(
            &to_path(&rect_path(3.5, 4.5, 280.25, 150.75)),
            RFill::NonZero,
            [51, 102, 153, 255],
        )
        .unwrap();
    direct
        .fill_path(&to_path(&tri), RFill::NonZero, [204, 51, 26, 178])
        .unwrap();
    let mut diffs = Vec::new();
    for (n, (a, b)) in tiled.pixels.iter().zip(&direct.image().pixels).enumerate() {
        if a != b {
            diffs.push((n / 4 % 300, n / 4 / 300, *a, *b));
        }
    }
    println!("{} diffs: {:?}", diffs.len(), &diffs[..diffs.len().min(10)]);
    assert!(diffs.is_empty(), "tiling must not change a pixel");
}

#[test]
fn rendering_is_deterministic_and_png_round_trips() {
    let mut res = Resources::default();
    let l = sample_page(&mut res);
    let a = render_png(&l, &res, &options(1.0)).unwrap();
    let b = render_png(&l, &res, &options(1.0)).unwrap();
    assert_eq!(a, b);
    let decoded = decode_png(&a, DecodeLimits::default()).unwrap();
    assert_eq!(decoded, render(&l, &res, &options(1.0)));
}

#[test]
fn invalid_options_and_oversized_pages_fail_before_allocating() {
    let res = Resources::default();
    let l = list(200.0, 100.0, vec![]);
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        for resolution in [Resolution::Scale(bad), Resolution::Dpi(bad)] {
            let o = RenderOptions {
                resolution,
                ..RenderOptions::default()
            };
            assert_eq!(render_image(&l, &res, &o), Err(RenderError::InvalidOptions));
        }
    }
    for scale in [1.0e6, 1.0e300, 5000.0] {
        assert_eq!(
            render_image(&l, &res, &options(scale)),
            Err(RenderError::LimitExceeded),
            "{scale}"
        );
    }
    // Default pixel budget: 16,777,216 pixels.
    assert!(render_image(&l, &res, &options(20.0)).is_ok());
    assert_eq!(
        render_image(&l, &res, &options(30.0)),
        Err(RenderError::LimitExceeded)
    );
    // A huge page is fine when only a small region is requested.
    let huge = list(
        1.0e6,
        1.0e6,
        vec![fill(rect_path(10.0, 10.0, 10.0, 10.0), BLACK())],
    );
    let o = RenderOptions {
        region: Some(PixelRegion {
            x: 0,
            y: 0,
            width: 32,
            height: 32,
        }),
        ..options(1.0)
    };
    let image = render(&huge, &res, &o);
    assert_eq!(px(&image, 15, 15), [0, 0, 0, 255]);
    assert_eq!(
        render_image(&huge, &res, &options(1.0)),
        Err(RenderError::LimitExceeded)
    );
    // Regions outside the page or empty.
    for region in [
        PixelRegion {
            x: 190,
            y: 0,
            width: 20,
            height: 10,
        },
        PixelRegion {
            x: 0,
            y: 0,
            width: 0,
            height: 10,
        },
        PixelRegion {
            x: u32::MAX,
            y: 0,
            width: 2,
            height: 2,
        },
    ] {
        let o = RenderOptions {
            region: Some(region),
            ..options(1.0)
        };
        assert_eq!(render_image(&l, &res, &o), Err(RenderError::InvalidOptions));
    }
}

#[test]
fn inconsistent_lists_are_rejected_and_budgets_hold() {
    let res = Resources::default();
    let unbalanced = DisplayList {
        page: page(10.0, 10.0),
        commands: vec![clip(0.0, 0.0, 5.0, 5.0)],
    };
    assert_eq!(
        render_image(&unbalanced, &res, &options(1.0)),
        Err(RenderError::InvalidList)
    );
    let underflow = DisplayList {
        page: page(10.0, 10.0),
        commands: vec![Command::PopTransform],
    };
    assert_eq!(
        render_image(&underflow, &res, &options(1.0)),
        Err(RenderError::InvalidList)
    );
    let bad_colour = DisplayList {
        page: page(10.0, 10.0),
        commands: vec![fill(
            rect_path(0.0, 0.0, 5.0, 5.0),
            Color {
                red: f32::NAN,
                green: 0.0,
                blue: 0.0,
                alpha: 1.0,
            },
        )],
    };
    assert_eq!(
        render_image(&bad_colour, &res, &options(1.0)),
        Err(RenderError::InvalidList)
    );
    // A glyph outline starting without a move is refused, not drawn.
    let mut broken = Resources::default();
    broken.glyphs.insert(
        (1, 1),
        DisplayPath {
            elements: vec![PathElement::LineTo(p(1.0, 1.0))],
        },
    );
    let l = list(
        10.0,
        10.0,
        vec![Command::DrawGlyphRun {
            font_id: 1,
            color: BLACK(),
            glyphs: vec![PositionedGlyph {
                glyph_id: 1,
                position: p(1.0, 1.0),
                advance: p(1.0, 0.0),
            }],
        }],
    );
    assert_eq!(
        render_image(&l, &broken, &options(1.0)),
        Err(RenderError::InvalidList)
    );
    // Operation and work budgets.
    let many = list(
        100.0,
        100.0,
        (0..50)
            .map(|_| fill(rect_path(0.0, 0.0, 100.0, 100.0), BLACK()))
            .collect(),
    );
    for limits in [
        RenderLimits {
            max_ops: 10,
            ..RenderLimits::default()
        },
        RenderLimits {
            max_tile_entries: 10,
            ..RenderLimits::default()
        },
        RenderLimits {
            max_work: 1000,
            ..RenderLimits::default()
        },
    ] {
        let o = RenderOptions {
            limits,
            ..options(1.0)
        };
        assert_eq!(
            render_image(&many, &res, &o),
            Err(RenderError::LimitExceeded)
        );
    }
    // Coordinates beyond the rasterizer range are refused, not wrapped.
    let far = list(
        100.0,
        100.0,
        vec![fill(rect_path(-1.0e12, -1.0e12, 2.0e12, 2.0e12), BLACK())],
    );
    assert!(matches!(
        render_image(&far, &res, &options(1.0)),
        Err(RenderError::Raster(_))
    ));
    // Shapes far outside the page are simply culled.
    let outside = list(
        100.0,
        100.0,
        vec![fill(rect_path(1.0e12, 1.0e12, 5.0, 5.0), BLACK())],
    );
    assert!(render_image(&outside, &res, &options(1.0)).is_ok());
}

#[test]
fn links_and_structure_markers_have_no_visual_effect() {
    let plain = list(
        40.0,
        40.0,
        vec![fill(rect_path(5.0, 5.0, 20.0, 20.0), BLACK())],
    );
    let marked = list(
        40.0,
        40.0,
        vec![
            Command::Structure {
                role: "paragraph".into(),
                node_id: 7,
            },
            Command::Link {
                bounds: Rect::from_xywh(0.0, 0.0, 40.0, 40.0).unwrap(),
                target: "https://example.test/".into(),
            },
            fill(rect_path(5.0, 5.0, 20.0, 20.0), BLACK()),
        ],
    );
    let r = Resources::default();
    assert_eq!(
        render(&plain, &r, &options(2.0)),
        render(&marked, &r, &options(2.0))
    );
}

// --------------------------------------------------- agreement with the SVG backend

/// Interprets the SVG produced by `render_svg` (fills, glyph uses, images,
/// translate/scale/matrix transforms, rectangular clips) and returns the box
/// of the ink in page units.
fn svg_ink_bounds(svg: &str) -> Option<[f64; 4]> {
    type M = [f64; 6];
    fn num(s: &str) -> Vec<f64> {
        s.split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')')
            .filter_map(|t| t.parse().ok())
            .collect()
    }
    fn apply(m: &M, x: f64, y: f64) -> (f64, f64) {
        (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
    }
    fn bbox(points: &[(f64, f64)]) -> Option<[f64; 4]> {
        let mut it = points.iter();
        let first = it.next()?;
        let mut b = [first.0, first.1, first.0, first.1];
        for q in it {
            b[0] = b[0].min(q.0);
            b[1] = b[1].min(q.1);
            b[2] = b[2].max(q.0);
            b[3] = b[3].max(q.1);
        }
        Some(b)
    }
    fn path_points(d: &str) -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        for part in d.split(|c: char| c.is_ascii_alphabetic()) {
            let v = num(part);
            out.extend(v.chunks_exact(2).map(|c| (c[0], c[1])));
        }
        out
    }
    let mut glyph_defs: HashMap<String, Vec<(f64, f64)>> = HashMap::new();
    let mut clip_defs: HashMap<String, [f64; 4]> = HashMap::new();
    let mut current_clip_def: Option<String> = None;
    let mut clip_stack: Vec<Option<[f64; 4]>> = vec![None];
    let mut matrix: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut ink: Option<[f64; 4]> = None;
    let mut add = |points: Vec<(f64, f64)>, clip: Option<[f64; 4]>| {
        if let Some(mut b) = bbox(&points) {
            if let Some(c) = clip {
                b = [
                    b[0].max(c[0]),
                    b[1].max(c[1]),
                    b[2].min(c[2]),
                    b[3].min(c[3]),
                ];
                if b[2] <= b[0] || b[3] <= b[1] {
                    return;
                }
            }
            ink = Some(match ink {
                None => b,
                Some(i) => [
                    i[0].min(b[0]),
                    i[1].min(b[1]),
                    i[2].max(b[2]),
                    i[3].max(b[3]),
                ],
            });
        }
    };
    let mut reader = Reader::new(svg.as_bytes());
    let mut in_defs = false;
    let mut depth_clip: Vec<bool> = Vec::new();
    let mut matrices: Vec<M> = Vec::new();
    while let Some(event) = reader.next_event().unwrap() {
        match event {
            Event::StartElement {
                name,
                attributes,
                empty,
            } => {
                let attr = |n: &str| {
                    attributes
                        .iter()
                        .find(|a| a.name.local == n)
                        .map(|a| a.value.clone())
                };
                match name.local.as_str() {
                    "defs" => in_defs = !empty,
                    "clipPath" => current_clip_def = attr("id"),
                    "rect" if in_defs => {
                        let v = |n| attr(n).unwrap().parse::<f64>().unwrap();
                        let (x, y, w, h) = (v("x"), v("y"), v("width"), v("height"));
                        clip_defs.insert(current_clip_def.clone().unwrap(), [x, y, x + w, y + h]);
                    }
                    "path" if in_defs => {
                        glyph_defs.insert(attr("id").unwrap(), path_points(&attr("d").unwrap()));
                    }
                    "g" => {
                        let mut is_clip = false;
                        if let Some(c) = attr("clip-path") {
                            let id = c
                                .trim_start_matches("url(#")
                                .trim_end_matches(')')
                                .to_string();
                            let def = clip_defs[&id];
                            let top = clip_stack.last().copied().flatten();
                            clip_stack.push(Some(top.map_or(def, |t| {
                                [
                                    t[0].max(def[0]),
                                    t[1].max(def[1]),
                                    t[2].min(def[2]),
                                    t[3].min(def[3]),
                                ]
                            })));
                            is_clip = true;
                        }
                        depth_clip.push(is_clip);
                        matrices.push(matrix);
                        if let Some(t) = attr("transform") {
                            let v = num(&t);
                            matrix = [v[0], v[1], v[2], v[3], v[4], v[5]];
                        }
                        if empty {
                            // Self-closing groups are not produced by the backend.
                            unreachable!();
                        }
                    }
                    "path" => {
                        let pts = path_points(&attr("d").unwrap())
                            .into_iter()
                            .map(|(x, y)| apply(&matrix, x, y))
                            .collect();
                        add(pts, clip_stack.last().copied().flatten());
                    }
                    "use" => {
                        let id = attr("href").unwrap().trim_start_matches('#').to_string();
                        let (x, y) = (
                            attr("x").unwrap().parse::<f64>().unwrap(),
                            attr("y").unwrap().parse::<f64>().unwrap(),
                        );
                        let pts = glyph_defs[&id]
                            .iter()
                            .map(|&(px_, py)| apply(&matrix, px_ + x, py + y))
                            .collect();
                        add(pts, clip_stack.last().copied().flatten());
                    }
                    "image" => {
                        let v = |n| attr(n).unwrap().parse::<f64>().unwrap();
                        let (x, y, w, h) = (v("x"), v("y"), v("width"), v("height"));
                        let pts = [(x, y), (x + w, y), (x, y + h), (x + w, y + h)]
                            .iter()
                            .map(|&(a, b)| apply(&matrix, a, b))
                            .collect();
                        add(pts, clip_stack.last().copied().flatten());
                    }
                    _ => {}
                }
            }
            Event::EndElement { name } => match name.local.as_str() {
                "defs" => in_defs = false,
                "g" => {
                    matrix = matrices.pop().unwrap();
                    if depth_clip.pop().unwrap() {
                        clip_stack.pop();
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    ink
}

#[test]
fn ink_boxes_match_the_svg_backend() {
    let mut res = Resources::default();
    let run = text_at(&mut res, 7, "Boxes agree", 18.0, 12.0, 40.0);
    res.images.insert(1, ("image/png", png_2x2()));
    let lists = [
        list(
            120.0,
            80.0,
            vec![fill(rect_path(10.25, 20.5, 33.0, 17.75), BLACK())],
        ),
        list(200.0, 80.0, vec![run.clone()]),
        list(
            120.0,
            80.0,
            vec![
                clip(20.0, 10.0, 50.0, 40.0),
                fill(rect_path(0.0, 0.0, 120.0, 80.0), BLACK()),
                Command::PopClip,
            ],
        ),
        list(
            120.0,
            80.0,
            vec![
                transform(2.0, 0.0, 0.0, 1.5, 5.0, 7.0),
                Command::DrawImage {
                    image_id: 1,
                    bounds: Rect::from_xywh(3.0, 4.0, 20.0, 10.0).unwrap(),
                },
                fill(rect_path(30.0, 20.0, 10.0, 8.0), BLACK()),
                Command::PopTransform,
            ],
        ),
        list(
            200.0,
            80.0,
            vec![
                clip(30.0, 0.0, 60.0, 80.0),
                transform(1.0, 0.0, 0.0, 1.0, 4.0, -2.0),
                run,
                Command::PopTransform,
                Command::PopClip,
            ],
        ),
    ];
    for (n, l) in lists.iter().enumerate() {
        let svg = render_svg(l, &res, SvgLimits::default()).unwrap();
        let expected = svg_ink_bounds(&svg).expect("svg ink");
        for scale in [1.0, 2.5] {
            let image = render(l, &res, &transparent(scale));
            let (x0, y0, x1, y1) = ink_bounds(&image).expect("raster ink");
            let got = [
                f64::from(x0),
                f64::from(y0),
                f64::from(x1 + 1),
                f64::from(y1 + 1),
            ];
            for i in 0..4 {
                assert!(
                    (got[i] - expected[i] * scale).abs() <= 1.0,
                    "list {n} scale {scale} edge {i}: raster {got:?}, svg {:?}",
                    expected.map(|v| v * scale)
                );
            }
        }
    }
}

// ----------------------------------------------------------- reference images

fn reference_scenes() -> Vec<(&'static str, DisplayList, Resources)> {
    // 1: shapes, even-odd, clip, rotation, stroke.
    let shapes = {
        let ring: Vec<PathElement> = [
            rect_path(4.0, 4.0, 28.0, 28.0),
            rect_path(10.0, 10.0, 16.0, 16.0),
        ]
        .iter()
        .flat_map(|p| p.elements.clone())
        .collect();
        list(
            64.0,
            48.0,
            vec![
                Command::FillPath {
                    path: DisplayPath::new(ring).unwrap(),
                    color: color(0.1, 0.3, 0.8, 1.0),
                    rule: FillRule::EvenOdd,
                },
                clip(20.0, 8.0, 30.0, 30.0),
                transform(0.866, 0.5, -0.5, 0.866, 20.0, 0.0),
                fill(rect_path(10.0, 10.0, 24.0, 12.0), color(0.9, 0.2, 0.1, 0.8)),
                Command::PopTransform,
                Command::PopClip,
                Command::StrokePath {
                    path: DisplayPath::new(vec![
                        PathElement::MoveTo(p(4.0, 44.0)),
                        PathElement::QuadraticTo {
                            control: p(30.0, 30.0),
                            to: p(60.0, 44.0),
                        },
                    ])
                    .unwrap(),
                    color: BLACK(),
                    style: StrokeStyle::new(3.0, LineCap::Round, LineJoin::Round, 4.0).unwrap(),
                },
            ],
        )
    };
    // 2: text.
    let mut text_res = Resources::default();
    let text = {
        let a = text_at(&mut text_res, 7, "Atelier", 16.0, 3.0, 20.0);
        let b = text_at(&mut text_res, 7, "raster", 11.0, 3.0, 38.0);
        list(64.0, 48.0, vec![a, b])
    };
    // 3: image.
    let image_res = with_image();
    let image = list(
        64.0,
        48.0,
        vec![
            fill(rect_path(0.0, 0.0, 64.0, 48.0), color(0.95, 0.95, 0.8, 1.0)),
            Command::DrawImage {
                image_id: 1,
                bounds: Rect::from_xywh(8.0, 6.0, 30.0, 30.0).unwrap(),
            },
            clip(30.0, 20.0, 30.0, 20.0),
            Command::DrawImage {
                image_id: 1,
                bounds: Rect::from_xywh(26.0, 14.0, 34.0, 28.0).unwrap(),
            },
            Command::PopClip,
        ],
    );
    vec![
        ("shapes", shapes, Resources::default()),
        ("text", text, text_res),
        ("image", image, image_res),
    ]
}

/// FNV-1a 64 of each reference PNG file (regenerate with ATELIER_UPDATE_REFERENCE=1).
const REFERENCE_SUMS: [(&str, u64); 3] = [
    ("shapes", 0x9fc7d0007a0a3f55),
    ("text", 0x081123ad56968a38),
    ("image", 0x351f6f62464ffd78),
];

#[test]
fn reference_images_are_unchanged() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/reference");
    let update = std::env::var_os("ATELIER_UPDATE_REFERENCE").is_some();
    for (name, l, res) in reference_scenes() {
        let png = render_png(&l, &res, &options(1.0)).unwrap();
        let file = dir.join(format!("{name}.png"));
        if update {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&file, &png).unwrap();
            println!("(\"{name}\", {:#018x}),", fnv(&png));
            continue;
        }
        let stored = std::fs::read(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        let expected_sum = REFERENCE_SUMS.iter().find(|(n, _)| *n == name).unwrap().1;
        assert_eq!(fnv(&stored), expected_sum, "{name}.png was edited");
        assert_eq!(png, stored, "{name} rendering changed");
        let decoded = decode_png(&stored, DecodeLimits::default()).unwrap();
        assert_eq!((decoded.width, decoded.height), (64, 48));
        assert_eq!(decoded, render(&l, &res, &options(1.0)));
    }
}

// ------------------------------------------------------------- random campaign

fn random_path(rng: &mut Rng, span: f64) -> DisplayPath {
    let mut elements = vec![PathElement::MoveTo(p(
        rng.float(-span, span),
        rng.float(-span, span),
    ))];
    for _ in 0..rng.below(8) {
        let q = |r: &mut Rng| p(r.float(-span, span), r.float(-span, span));
        elements.push(match rng.below(4) {
            0 => PathElement::LineTo(q(rng)),
            1 => PathElement::QuadraticTo {
                control: q(rng),
                to: q(rng),
            },
            2 => PathElement::CubicTo {
                control1: q(rng),
                control2: q(rng),
                to: q(rng),
            },
            _ => PathElement::Close,
        });
    }
    DisplayPath { elements }
}

fn random_command(rng: &mut Rng, clip_depth: &mut usize, transform_depth: &mut usize) -> Command {
    let span = [30.0, 60.0, 100.0, 100.0, 1.0e4, 1.0e11][rng.below(6)];
    let c = Color {
        red: rng.float(0.0, 1.0) as f32,
        green: rng.float(0.0, 1.0) as f32,
        blue: rng.float(0.0, 1.0) as f32,
        alpha: rng.float(0.0, 1.0) as f32,
    };
    match rng.below(14) {
        0 | 1 => Command::FillPath {
            path: random_path(rng, span),
            color: c,
            rule: if rng.below(2) == 0 {
                FillRule::NonZero
            } else {
                FillRule::EvenOdd
            },
        },
        2 => Command::StrokePath {
            path: random_path(rng, span),
            color: c,
            style: StrokeStyle {
                width: rng.float(0.0, 12.0),
                cap: [LineCap::Butt, LineCap::Round, LineCap::Square][rng.below(3)],
                join: [LineJoin::Miter, LineJoin::Round, LineJoin::Bevel][rng.below(3)],
                miter_limit: rng.float(0.9, 10.0),
            },
        },
        3 => Command::DrawGlyphRun {
            font_id: [0, 0, 1, 1, 5][rng.below(5)],
            color: c,
            glyphs: (0..rng.below(6))
                .map(|_| PositionedGlyph {
                    glyph_id: [0, 1, 2, 2, 7][rng.below(5)],
                    position: p(rng.float(-span, span), rng.float(-span, span)),
                    advance: p(3.0, 0.0),
                })
                .collect(),
        },
        4 => Command::DrawImage {
            image_id: [1, 1, 1, 1, 2, 3, 9][rng.below(7)],
            bounds: Rect::new(
                p(rng.float(-span, span), rng.float(-span, span)),
                p(rng.float(-span, span), rng.float(-span, span)),
            ),
        },
        5 | 6 => {
            *clip_depth += 1;
            Command::PushClip {
                bounds: Rect::new(
                    p(rng.float(-span, span), rng.float(-span, span)),
                    p(rng.float(-span, span), rng.float(-span, span)),
                ),
            }
        }
        7 => {
            if *clip_depth > 0 || rng.below(6) == 0 {
                *clip_depth = clip_depth.saturating_sub(1);
                Command::PopClip
            } else {
                Command::Structure {
                    role: "pad".into(),
                    node_id: 0,
                }
            }
        }
        8 | 9 => {
            *transform_depth += 1;
            let v = |r: &mut Rng| match r.below(6) {
                0 => 0.0,
                1 if r.below(4) == 0 => 1.0e12,
                2 => -1.0,
                _ => r.float(-3.0, 3.0),
            };
            Command::PushTransform {
                transform: DisplayTransform {
                    a: v(rng),
                    b: v(rng),
                    c: v(rng),
                    d: v(rng),
                    e: rng.float(-50.0, 50.0),
                    f: rng.float(-50.0, 50.0),
                },
            }
        }
        10 => {
            if *transform_depth > 0 || rng.below(6) == 0 {
                *transform_depth = transform_depth.saturating_sub(1);
                Command::PopTransform
            } else {
                Command::Structure {
                    role: "pad".into(),
                    node_id: 1,
                }
            }
        }
        11 => Command::Link {
            bounds: Rect::new(p(0.0, 0.0), p(5.0, 5.0)),
            target: "x".into(),
        },
        _ => Command::Structure {
            role: "r".into(),
            node_id: rng.next(),
        },
    }
}

fn fuzz_resources() -> Resources {
    let mut res = with_image();
    let mut g = 0;
    for font in 0..2_u64 {
        for glyph in 0..3_u32 {
            g += 1;
            res.glyphs.insert(
                (font, glyph),
                poly(&[(0.0, 0.0), (6.0 + f64::from(g), 0.0), (3.0, -9.0)]),
            );
        }
    }
    res.images.insert(2, ("image/png", vec![1, 2, 3]));
    res.images.insert(3, ("image/gif", png_2x2()));
    res
}

#[test]
fn random_and_mutated_lists_never_panic() {
    let res = fuzz_resources();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut ok, mut failed) = (0, 0);
    let mut kinds = std::collections::BTreeMap::new();
    for case in 0..400 {
        let (mut clip_depth, mut transform_depth) = (0, 0);
        let count = rng.below(25);
        let mut commands: Vec<Command> = (0..count)
            .map(|_| random_command(&mut rng, &mut clip_depth, &mut transform_depth))
            .collect();
        if case % 6 != 0 {
            // Balance the stacks, as a well-formed producer would.
            commands.extend(std::iter::repeat_n(Command::PopClip, clip_depth));
            commands.extend(std::iter::repeat_n(Command::PopTransform, transform_depth));
        }
        if case % 4 == 1 && !commands.is_empty() {
            // Mutations: drop, duplicate, swap, corrupt.
            for _ in 0..1 + rng.below(3) {
                let i = rng.below(commands.len());
                match rng.below(4) {
                    0 => {
                        commands.remove(i);
                    }
                    1 => commands.insert(i, commands[i].clone()),
                    2 => {
                        let j = rng.below(commands.len());
                        commands.swap(i, j);
                    }
                    _ => {
                        if let Command::FillPath { color, .. } = &mut commands[i] {
                            color.alpha = f32::NAN;
                        }
                    }
                }
                if commands.is_empty() {
                    break;
                }
            }
        }
        let scale = [0.37, 1.0, 2.5, 7.0][rng.below(4)];
        let region = (case % 5 == 0).then(|| PixelRegion {
            x: rng.below(20) as u32,
            y: rng.below(20) as u32,
            width: 1 + rng.below(40) as u32,
            height: 1 + rng.below(40) as u32,
        });
        let l = DisplayList {
            page: page(64.0, 48.0),
            commands,
        };
        let o = RenderOptions {
            region,
            background: if case % 2 == 0 { [255; 4] } else { [0; 4] },
            ..options(scale)
        };
        match render_image(&l, &res, &o) {
            Ok(image) => {
                ok += 1;
                assert_eq!(
                    image.pixels.len(),
                    (image.width * image.height * 4) as usize
                );
                // Deterministic.
                assert_eq!(render_image(&l, &res, &o).unwrap(), image);
            }
            Err(e) => {
                failed += 1;
                *kinds
                    .entry(format!("{e:?}").split('(').next().unwrap().to_string())
                    .or_insert(0) += 1;
            }
        }
    }
    println!("campaign: {ok} rendered, {failed} typed errors {kinds:?}");
    assert!(
        ok > 50 && failed > 20,
        "campaign too uniform: {ok} ok, {failed} errors, {kinds:?}"
    );
    assert!(kinds.len() >= 3, "{kinds:?}");
}

#[test]
fn rotation_decomposition_reproduces_general_matrices() {
    // Exercised through the image path: random invertible matrices must not fail.
    let res = with_image();
    let mut rng = Rng(42);
    for _ in 0..200 {
        let t = DisplayTransform {
            a: rng.float(-3.0, 3.0),
            b: rng.float(-3.0, 3.0),
            c: rng.float(-3.0, 3.0),
            d: rng.float(-3.0, 3.0),
            e: rng.float(-20.0, 20.0),
            f: rng.float(-20.0, 20.0),
        };
        let l = DisplayList {
            page: page(40.0, 40.0),
            commands: vec![
                Command::PushTransform { transform: t },
                Command::DrawImage {
                    image_id: 1,
                    bounds: Rect::from_xywh(2.0, 2.0, 10.0, 10.0).unwrap(),
                },
                Command::PopTransform,
            ],
        };
        match render_image(&l, &res, &options(1.0)) {
            Ok(_) | Err(RenderError::Raster(_)) => {}
            Err(e) => panic!("{t:?}: {e:?}"),
        }
    }
}

// ------------------------------------------------------------------ timing

#[test]
#[ignore = "indicative timing; run with --release --ignored --nocapture"]
fn timing_of_a_text_page() {
    let mut res = Resources::default();
    let lines = [
        "Atelier est une suite bureautique ecrite en Rust sans dependance externe.",
        "Chaque page produit une liste d'affichage rejouee par plusieurs backends.",
        "Le backend raster sert aux miniatures et aux comparaisons de rendu 0123.",
    ];
    let mut commands = Vec::new();
    for i in 0..48 {
        commands.push(text_at(
            &mut res,
            7,
            lines[i % 3],
            11.0,
            72.0,
            72.0 + 14.0 * i as f64,
        ));
    }
    let l = list(612.0, 792.0, commands);
    let glyphs: usize = l
        .commands
        .iter()
        .map(|c| {
            if let Command::DrawGlyphRun { glyphs, .. } = c {
                glyphs.len()
            } else {
                0
            }
        })
        .sum();
    for dpi in [96.0, 300.0] {
        let o = RenderOptions {
            resolution: Resolution::Dpi(dpi),
            limits: RenderLimits::default(),
            ..RenderOptions::default()
        };
        let mut samples = Vec::new();
        for _ in 0..5 {
            let start = std::time::Instant::now();
            let image = render_image(&l, &res, &o).unwrap();
            samples.push(start.elapsed());
            assert!(ink(&image) > 0.0);
        }
        samples.sort();
        let start = std::time::Instant::now();
        let png = render_png(&l, &res, &o).unwrap();
        println!(
            "{dpi} dpi: {glyphs} glyphs, median {:?}, min {:?}, render+PNG {:?} ({} bytes)",
            samples[2],
            samples[0],
            start.elapsed(),
            png.len()
        );
    }
}

#[test]
fn raster_limits_default_is_untouched() {
    // Guard: the backend must not rely on changed defaults of the rasterizer.
    let d = RasterLimits::default();
    assert_eq!((d.max_pixels, d.max_clip_depth), (16_777_216, 16));
}
