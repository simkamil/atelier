// SPDX-License-Identifier: AGPL-3.0-or-later
//! Demonstration of the full flow: display list -> `atelier-raster` -> reference PNG.
//!
//! Regenerate the reference with `ATELIER_UPDATE_REFERENCES=1 cargo test -p atelier-image-diff`.
#![forbid(unsafe_code)]

use atelier_geom::{Point, Rect};
use atelier_image_diff::{
    Config, DIFF_MARK, Image, MatchError, Tolerance, check_image_matches, load_png,
};
use atelier_raster::display_list::{
    RenderOptions, Resolution, ResourceProvider, SvgImage, render_image,
};
use atelier_render::{
    Color, Command, DisplayList, DisplayPath, FillRule, LineCap, LineJoin, PageInfo, PathElement,
    StrokeStyle,
};
use std::path::PathBuf;

struct NoResources;
impl ResourceProvider for NoResources {
    fn glyph_outline(&self, _font_id: u64, _glyph_id: u32) -> Option<&DisplayPath> {
        None
    }
    fn image(&self, _image_id: u64) -> Option<SvgImage<'_>> {
        None
    }
}

fn p(x: f64, y: f64) -> Point {
    Point::try_new(x, y).unwrap()
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> DisplayPath {
    DisplayPath::new(vec![
        PathElement::MoveTo(p(x, y)),
        PathElement::LineTo(p(x + w, y)),
        PathElement::LineTo(p(x + w, y + h)),
        PathElement::LineTo(p(x, y + h)),
        PathElement::Close,
    ])
    .unwrap()
}

/// 48x32 page: a blue rectangle, a translucent red one overlapping it and a black line.
fn page(red_width: f64) -> Image {
    let bounds = Rect::from_xywh(0.0, 0.0, 48.0, 32.0).unwrap();
    let commands = vec![
        Command::FillPath {
            path: rect(4.0, 4.0, 24.0, 16.0),
            color: Color::new(0.1, 0.3, 0.8, 1.0).unwrap(),
            rule: FillRule::NonZero,
        },
        Command::FillPath {
            path: rect(18.0, 10.0, red_width, 14.0),
            color: Color::new(0.9, 0.2, 0.1, 0.5).unwrap(),
            rule: FillRule::NonZero,
        },
        Command::StrokePath {
            path: DisplayPath::new(vec![
                PathElement::MoveTo(p(4.0, 28.0)),
                PathElement::LineTo(p(44.0, 28.0)),
            ])
            .unwrap(),
            color: Color::new(0.0, 0.0, 0.0, 1.0).unwrap(),
            style: StrokeStyle::new(2.0, LineCap::Butt, LineJoin::Miter, 4.0).unwrap(),
        },
    ];
    let list = DisplayList::new(PageInfo::new(bounds).unwrap(), commands).unwrap();
    let options = RenderOptions {
        resolution: Resolution::Scale(1.0),
        ..RenderOptions::default()
    };
    render_image(&list, &NoResources, &options).unwrap()
}

#[test]
fn rendered_page_matches_versioned_reference() {
    // Deterministic, unrotated rendering: zero tolerance.
    atelier_image_diff::assert_image_matches!(
        &page(20.0),
        "tests/reference/page.png",
        Tolerance::exact()
    );
}

#[test]
fn simulated_regression_produces_actual_and_diff_images() {
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("image-diff-demo-regression");
    let _ = std::fs::remove_dir_all(&out);
    let reference = crate_path("tests/reference/page.png");
    let config = Config {
        output_dir: Some(out.clone()),
        ..Config::default()
    };
    // The second rectangle is 4 units wider: a visible regression.
    let regressed = page(24.0);
    let error = check_image_matches(&regressed, &reference, &Tolerance::exact(), &config)
        .expect_err("regression must be detected");
    let MatchError::Mismatch {
        actual_path,
        diff_path,
        report,
        ..
    } = &error
    else {
        panic!("unexpected error: {error}");
    };
    assert_eq!(*actual_path, out.join("page.actual.png"));
    assert_eq!(*diff_path, out.join("page.diff.png"));
    assert_eq!(load_png(actual_path).unwrap(), regressed);
    let bounds = report.bounds.unwrap();
    assert_eq!((bounds.x, bounds.width), (38, 4)); // x = 18 + 20 .. 18 + 24
    let diff = load_png(diff_path).unwrap();
    let at = |x: u32, y: u32| {
        let i = ((y * diff.width + x) * 4) as usize;
        <[u8; 4]>::try_from(&diff.pixels[i..i + 4]).unwrap()
    };
    assert_eq!(at(39, 15), DIFF_MARK);
    assert_ne!(at(2, 2), DIFF_MARK);
    assert!(error.to_string().contains("page.diff.png"));
}

fn crate_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}
