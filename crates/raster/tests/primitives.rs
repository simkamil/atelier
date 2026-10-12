// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_geom::{AffineTransform as Matrix, Path, Point, Rect};
use atelier_image::{DecodeLimits, Image, decode_png};
use atelier_raster::{
    FillRule, LineCap, LineJoin, RasterError, RasterLimits, Rasterizer, StrokeStyle,
};

fn point(x: f64, y: f64) -> Point {
    Point::try_new(x, y).unwrap()
}
fn polygon(vertices: &[(f64, f64)], close: bool) -> Path {
    let mut path = Path::new();
    for (i, &(x, y)) in vertices.iter().enumerate() {
        if i == 0 {
            path.move_to(point(x, y));
        } else {
            path.line_to(point(x, y)).unwrap();
        }
    }
    if close && !vertices.is_empty() {
        path.close().unwrap();
    }
    path
}
fn rect(x: f64, y: f64, w: f64, h: f64) -> Path {
    polygon(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h)], true)
}
fn surface(w: u32, h: u32) -> Rasterizer {
    Rasterizer::new(w, h, RasterLimits::default()).unwrap()
}
fn pixel(r: &Rasterizer, x: usize, y: usize) -> [u8; 4] {
    r.image().pixels[(y * r.image().width as usize + x) * 4..][..4]
        .try_into()
        .unwrap()
}
const RED: [u8; 4] = [255, 0, 0, 255];

#[test]
fn integer_rectangles_and_fractional_coverage_have_analytic_pixels() {
    let mut r = surface(4, 3);
    r.fill_path(&rect(1.0, 0.5, 2.0, 1.0), FillRule::NonZero, RED)
        .unwrap();
    for y in 0..3 {
        for x in 0..4 {
            let expected = if (1..3).contains(&x) && y < 2 {
                [255, 0, 0, 128]
            } else {
                [0; 4]
            };
            assert_eq!(pixel(&r, x, y), expected, "{x},{y}");
        }
    }
}

#[test]
fn holes_winding_and_implicit_closure() {
    let mut path = rect(0.0, 0.0, 5.0, 5.0);
    path.move_to(point(1.0, 1.0));
    path.line_to(point(4.0, 1.0)).unwrap();
    path.line_to(point(4.0, 4.0)).unwrap();
    path.line_to(point(1.0, 4.0)).unwrap();
    let mut even = surface(5, 5);
    let mut nonzero = surface(5, 5);
    even.fill_path(&path, FillRule::EvenOdd, RED).unwrap();
    nonzero.fill_path(&path, FillRule::NonZero, RED).unwrap();
    assert_eq!(pixel(&even, 2, 2), [0; 4]);
    assert_eq!(pixel(&nonzero, 2, 2), RED);
    assert_eq!(pixel(&even, 0, 2), RED);
    let mut opposite = rect(0.0, 0.0, 5.0, 5.0);
    opposite.move_to(point(1.0, 1.0));
    opposite.line_to(point(1.0, 4.0)).unwrap();
    opposite.line_to(point(4.0, 4.0)).unwrap();
    opposite.line_to(point(4.0, 1.0)).unwrap();
    let mut r = surface(5, 5);
    r.fill_path(&opposite, FillRule::NonZero, RED).unwrap();
    assert_eq!(pixel(&r, 2, 2), [0; 4]);
}

#[test]
fn diagonal_matches_independent_point_in_triangle_oracle() {
    let mut r = surface(7, 7);
    let p = polygon(&[(0.0, 0.0), (7.0, 0.0), (0.0, 7.0)], true);
    r.fill_path(&p, FillRule::NonZero, RED).unwrap();
    for y in 0..7 {
        for x in 0..7 {
            let hits = (0..8)
                .flat_map(|sy| (0..8).map(move |sx| (sx, sy)))
                .filter(|&(sx, sy)| 8 * (x + y) + sx + sy + 1 < 56)
                .count();
            let alpha = (255.0 * hits as f64 / 64.0).round() as u8;
            assert_eq!(pixel(&r, x, y)[3], alpha, "{x},{y}");
        }
    }
}

#[test]
fn source_over_keeps_straight_colors() {
    let mut r = surface(1, 1);
    let p = rect(0.0, 0.0, 1.0, 1.0);
    r.fill_path(&p, FillRule::NonZero, [255, 0, 0, 128])
        .unwrap();
    r.fill_path(&p, FillRule::NonZero, [0, 0, 255, 128])
        .unwrap();
    assert_eq!(pixel(&r, 0, 0), [85, 0, 170, 192]);
    r.fill_path(&p, FillRule::NonZero, [0, 255, 0, 0]).unwrap();
    assert_eq!(pixel(&r, 0, 0), [85, 0, 170, 192]);
}

#[test]
fn clips_intersect_sample_bits_and_restore_stack() {
    let left = rect(0.0, 0.0, 0.5, 1.0);
    let right = rect(0.5, 0.0, 0.5, 1.0);
    let mut r = surface(1, 1);
    r.push_clip_path(&left, FillRule::NonZero).unwrap();
    r.fill_path(&left, FillRule::NonZero, RED).unwrap();
    assert_eq!(pixel(&r, 0, 0), [255, 0, 0, 128]);
    r.clear([0; 4]);
    r.push_clip_path(&right, FillRule::NonZero).unwrap();
    r.fill_path(&rect(0.0, 0.0, 1.0, 1.0), FillRule::NonZero, RED)
        .unwrap();
    assert_eq!(pixel(&r, 0, 0), [0; 4]);
    r.pop_clip().unwrap();
    r.pop_clip().unwrap();
    r.fill_path(&right, FillRule::NonZero, RED).unwrap();
    assert_eq!(pixel(&r, 0, 0), [255, 0, 0, 128]);
    assert_eq!(r.pop_clip(), Err(RasterError::ClipStackUnderflow));
}

#[test]
fn rectangle_clip_and_curved_path_clip() {
    let mut p = Path::new();
    p.move_to(point(0.0, 0.0));
    p.quadratic_to(point(4.0, 0.0), point(4.0, 4.0)).unwrap();
    p.cubic_to(point(0.0, 4.0), point(0.0, 4.0), point(0.0, 0.0))
        .unwrap();
    p.close().unwrap();
    let mut r = surface(4, 4);
    r.push_clip_rect(Rect::from_xywh(1.0, 1.0, 2.0, 2.0).unwrap())
        .unwrap();
    r.push_clip_path(&p, FillRule::NonZero).unwrap();
    r.fill_path(&rect(0.0, 0.0, 4.0, 4.0), FillRule::NonZero, RED)
        .unwrap();
    assert_eq!(pixel(&r, 0, 0), [0; 4]);
    assert!(pixel(&r, 2, 2)[3] > 0);
    assert_eq!(pixel(&r, 3, 3), [0; 4]);
}

#[test]
fn stroke_caps_have_distinct_analytic_extents_and_no_alpha_seams() {
    let path = polygon(&[(2.0, 2.0), (4.0, 2.0)], false);
    for cap in [LineCap::Butt, LineCap::Square, LineCap::Round] {
        let mut r = surface(6, 4);
        r.stroke_path(
            &path,
            StrokeStyle {
                width: 2.0,
                cap,
                ..StrokeStyle::default()
            },
            [255, 0, 0, 128],
        )
        .unwrap();
        assert_eq!(pixel(&r, 2, 1), [255, 0, 0, 128]);
        let a = pixel(&r, 1, 1)[3];
        match cap {
            LineCap::Butt => assert_eq!(a, 0),
            LineCap::Square => assert_eq!(a, 128),
            LineCap::Round => assert!(a > 0 && a < 128),
        }
        assert!(r.image().pixels.chunks_exact(4).all(|p| p[3] <= 128));
    }
}

#[test]
fn stroke_joins_miter_limit_and_closed_paths() {
    let path = polygon(&[(1.0, 3.0), (3.0, 3.0), (3.0, 5.0)], false);
    let mut alphas = Vec::new();
    for join in [LineJoin::Miter, LineJoin::Bevel, LineJoin::Round] {
        let mut r = surface(6, 6);
        r.stroke_path(
            &path,
            StrokeStyle {
                width: 2.0,
                join,
                ..StrokeStyle::default()
            },
            RED,
        )
        .unwrap();
        alphas.push(pixel(&r, 3, 2)[3]);
    }
    assert_eq!(alphas[0], 255);
    assert!(alphas[1] < alphas[2] && alphas[2] < alphas[0]);
    let mut r = surface(6, 6);
    r.stroke_path(
        &path,
        StrokeStyle {
            width: 2.0,
            miter_limit: 1.0,
            ..StrokeStyle::default()
        },
        RED,
    )
    .unwrap();
    assert_eq!(pixel(&r, 3, 2)[3], alphas[1]);
    let mut r = surface(6, 6);
    r.stroke_path(
        &rect(1.0, 1.0, 4.0, 4.0),
        StrokeStyle {
            width: 1.0,
            cap: LineCap::Round,
            ..StrokeStyle::default()
        },
        RED,
    )
    .unwrap();
    assert_eq!(pixel(&r, 3, 3), [0; 4]);
    assert!(pixel(&r, 1, 1)[3] > 0);
}

#[test]
fn point_strokes_and_degenerate_paths_are_defined() {
    let mut r = surface(4, 4);
    let empty = Path::new();
    r.fill_path(&empty, FillRule::NonZero, RED).unwrap();
    let path = polygon(&[(2.0, 2.0), (2.0, 2.0)], false);
    r.fill_path(&path, FillRule::NonZero, RED).unwrap();
    assert!(r.image().pixels.iter().all(|&v| v == 0));
    r.stroke_path(
        &path,
        StrokeStyle {
            width: 2.0,
            cap: LineCap::Round,
            ..StrokeStyle::default()
        },
        RED,
    )
    .unwrap();
    assert!(pixel(&r, 2, 2)[3] > 0);
    let before = r.image().clone();
    r.stroke_path(
        &path,
        StrokeStyle {
            width: 0.0,
            ..StrokeStyle::default()
        },
        RED,
    )
    .unwrap();
    assert_eq!(r.image(), &before);
}

#[test]
fn images_identity_translation_rotation_and_bilinear_alpha() {
    let source = Image {
        width: 2,
        height: 1,
        pixels: vec![255, 0, 0, 255, 0, 0, 255, 255],
    };
    let mut r = surface(2, 1);
    r.draw_image(&source, Matrix::identity()).unwrap();
    assert_eq!(r.image(), &source);
    let mut r = surface(4, 2);
    r.draw_image(&source, Matrix::translation(1.0, 1.0).unwrap())
        .unwrap();
    assert_eq!(pixel(&r, 1, 1), [255, 0, 0, 255]);
    assert_eq!(pixel(&r, 2, 1), [0, 0, 255, 255]);
    let rotate = Matrix::rotation(core::f64::consts::FRAC_PI_2)
        .unwrap()
        .then(Matrix::translation(1.0, 0.0).unwrap())
        .unwrap();
    let mut r = surface(1, 2);
    r.draw_image(&source, rotate).unwrap();
    assert_eq!(pixel(&r, 0, 0), [255, 0, 0, 255]);
    assert_eq!(pixel(&r, 0, 1), [0, 0, 255, 255]);
    let source = Image {
        width: 2,
        height: 1,
        pixels: vec![255, 0, 0, 0, 0, 255, 0, 255],
    };
    let mut r = surface(3, 1);
    r.draw_image(&source, Matrix::scale(1.5, 1.0).unwrap())
        .unwrap();
    assert_eq!(pixel(&r, 1, 0), [0, 255, 0, 128]);
}

#[test]
fn images_respect_fractional_coverage_and_clip() {
    let source = Image {
        width: 1,
        height: 1,
        pixels: RED.to_vec(),
    };
    let mut r = surface(2, 1);
    r.push_clip_rect(Rect::from_xywh(0.5, 0.0, 1.0, 1.0).unwrap())
        .unwrap();
    r.draw_image(&source, Matrix::translation(0.5, 0.0).unwrap())
        .unwrap();
    assert_eq!(pixel(&r, 0, 0), [255, 0, 0, 128]);
    assert_eq!(pixel(&r, 1, 0), [255, 0, 0, 128]);
}

#[test]
fn png_roundtrip_preserves_all_pixels() {
    let mut r = surface(12, 9);
    r.fill_path(
        &polygon(&[(1.0, 0.0), (10.0, 8.0), (0.0, 8.0)], true),
        FillRule::EvenOdd,
        [43, 177, 92, 157],
    )
    .unwrap();
    let bytes = r.encode_png(DecodeLimits::default()).unwrap();
    assert_eq!(
        decode_png(&bytes, DecodeLimits::default()).unwrap(),
        *r.image()
    );
}

#[test]
fn invalid_inputs_and_budgets_are_atomic() {
    assert!(matches!(
        Rasterizer::new(0, 1, RasterLimits::default()),
        Err(RasterError::InvalidInput)
    ));
    assert!(matches!(
        Rasterizer::new(u32::MAX, u32::MAX, RasterLimits::default()),
        Err(RasterError::LimitExceeded)
    ));
    let mut r = surface(3, 3);
    r.clear([2, 3, 4, 5]);
    let before = r.image().clone();
    let bad = Image {
        width: 2,
        height: 2,
        pixels: vec![0; 3],
    };
    assert_eq!(
        r.draw_image(&bad, Matrix::identity()),
        Err(RasterError::InvalidInput)
    );
    let source = Image {
        width: 1,
        height: 1,
        pixels: RED.to_vec(),
    };
    assert!(
        r.draw_image(&source, Matrix::scale(0.0, 1.0).unwrap())
            .is_err()
    );
    assert!(
        r.stroke_path(
            &rect(0.0, 0.0, 1.0, 1.0),
            StrokeStyle {
                width: f64::NAN,
                ..StrokeStyle::default()
            },
            RED
        )
        .is_err()
    );
    assert_eq!(r.image(), &before);
    for limits in [
        RasterLimits {
            max_work: 1,
            ..RasterLimits::default()
        },
        RasterLimits {
            max_edges: 2,
            ..RasterLimits::default()
        },
        RasterLimits {
            max_mask_bytes: 1,
            ..RasterLimits::default()
        },
    ] {
        let mut r = Rasterizer::new(3, 3, limits).unwrap();
        r.clear([2, 3, 4, 5]);
        assert_eq!(
            r.fill_path(&rect(0.0, 0.0, 3.0, 3.0), FillRule::NonZero, RED),
            Err(RasterError::LimitExceeded)
        );
        assert_eq!(r.image(), &before);
    }
    let mut r = Rasterizer::new(
        3,
        3,
        RasterLimits {
            max_clip_depth: 1,
            ..RasterLimits::default()
        },
    )
    .unwrap();
    r.push_clip_rect(Rect::from_xywh(1.0, 1.0, 1.0, 1.0).unwrap())
        .unwrap();
    assert_eq!(
        r.push_clip_rect(Rect::from_xywh(0.0, 0.0, 3.0, 3.0).unwrap()),
        Err(RasterError::LimitExceeded)
    );
    r.fill_path(&rect(0.0, 0.0, 3.0, 3.0), FillRule::NonZero, RED)
        .unwrap();
    assert_eq!(pixel(&r, 0, 0), [0; 4]);
    assert_eq!(pixel(&r, 1, 1), RED);
}
