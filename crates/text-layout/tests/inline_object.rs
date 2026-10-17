// SPDX-License-Identifier: AGPL-3.0-or-later
//! Inline object geometry (US-270): natural size, reduction, line box, EXIF.

use atelier_text_layout::inline_object::{fit, line_box, natural_size};

#[test]
fn natural_size_uses_96_dpi_and_the_exif_orientation() {
    assert_eq!(natural_size(400, 200, 1), (300.0, 150.0));
    assert_eq!(natural_size(400, 200, 3), (300.0, 150.0));
    assert_eq!(natural_size(400, 200, 6), (150.0, 300.0));
    assert_eq!(natural_size(400, 200, 8), (150.0, 300.0));
}

#[test]
fn a_picture_that_fits_is_left_alone_and_never_enlarged() {
    assert_eq!(fit(100.0, 50.0, 400.0, 700.0), Some((100.0, 50.0)));
}

#[test]
fn a_wide_picture_shrinks_proportionally_to_the_text_width() {
    let (w, h) = fit(1000.0, 500.0, 487.276, 733.89).unwrap();
    assert!((w - 487.276).abs() < 1e-9);
    assert!((h - 243.638).abs() < 1e-9);
    assert!((w / h - 2.0).abs() < 1e-9);
}

#[test]
fn a_tall_picture_shrinks_to_the_height_limit() {
    let (w, h) = fit(100.0, 2000.0, 487.0, 700.0).unwrap();
    assert!((h - 700.0).abs() < 1e-9);
    assert!((w - 35.0).abs() < 1e-9);
}

#[test]
fn invalid_geometry_is_rejected_without_panic() {
    assert_eq!(fit(0.0, 10.0, 100.0, 100.0), None);
    assert_eq!(fit(f64::NAN, 10.0, 100.0, 100.0), None);
    assert_eq!(fit(10.0, 10.0, -1.0, 100.0), None);
    assert_eq!(fit(10.0, f64::INFINITY, 1.0, 1.0), None);
}

#[test]
fn line_box_grows_with_the_tallest_object_and_keeps_the_descent() {
    let text_only = line_box(11.0, 3.0, []);
    assert_eq!((text_only.ascent, text_only.height()), (11.0, 14.0));
    let small = line_box(11.0, 3.0, [8.0]);
    assert_eq!(small.ascent, 11.0);
    let tall = line_box(11.0, 3.0, [8.0, 120.0, 40.0]);
    assert_eq!(
        (tall.ascent, tall.descent, tall.height()),
        (120.0, 3.0, 123.0)
    );
    assert_eq!(line_box(11.0, 3.0, [f64::NAN]).ascent, 11.0);
}
