// SPDX-License-Identifier: AGPL-3.0-or-later
//! Inline picture model (US-270): signatures, EXIF geometry, object codec and
//! an undoable insertion through the rich-model transaction path.

mod common;

use atelier_text_model::inline_image::*;
use atelier_text_model::rich::{AnchoredObject, Inline, RichOp};
use common::*;
use std::sync::Arc;

#[test]
fn signatures_are_recognised_without_decoding() {
    assert_eq!(
        ImageFormat::sniff(b"\x89PNG\r\n\x1a\n...."),
        Ok(ImageFormat::Png)
    );
    assert_eq!(
        ImageFormat::sniff(&[0xff, 0xd8, 0xff, 0xe0]),
        Ok(ImageFormat::Jpeg)
    );
    assert_eq!(ImageFormat::sniff(b"GIF89a.."), Ok(ImageFormat::Gif));
    assert_eq!(ImageFormat::sniff(b"BM......"), Ok(ImageFormat::Bmp));
    assert_eq!(
        ImageFormat::sniff(b"RIFF\0\0\0\0WEBPVP8 "),
        Err(SniffError::Unsupported("WebP"))
    );
    assert_eq!(
        ImageFormat::sniff(b"II*\0\x08\0\0\0"),
        Err(SniffError::Unsupported("TIFF"))
    );
    assert_eq!(ImageFormat::sniff(b""), Err(SniffError::Unknown));
    assert_eq!(ImageFormat::sniff(b"hello"), Err(SniffError::Unknown));
}

#[test]
fn orientation_swaps_dimensions_for_five_to_eight_only() {
    for o in 1..=4u8 {
        assert_eq!(oriented_size(30, 10, o), (30, 10), "orientation {o}");
    }
    for o in 5..=8u8 {
        assert_eq!(oriented_size(30, 10, o), (10, 30), "orientation {o}");
    }
}

#[test]
fn oriented_source_is_a_bijection_for_every_orientation() {
    // A 3x2 stored image; every oriented pixel must map to a distinct stored pixel.
    for o in 1..=8u8 {
        let (ow, oh) = oriented_size(3, 2, o);
        let mut seen = std::collections::BTreeSet::new();
        for y in 0..oh {
            for x in 0..ow {
                let (sx, sy) = oriented_source(3, 2, o, x, y);
                assert!(sx < 3 && sy < 2, "orientation {o} maps outside");
                assert!(seen.insert((sx, sy)), "orientation {o} is not injective");
            }
        }
        assert_eq!(seen.len(), 6);
    }
}

#[test]
fn oriented_source_matches_the_exif_definitions() {
    // Stored 3x2: top row a b c, bottom row d e f -> index (x, y).
    // Orientation 6 (rotate 90 clockwise): oriented top-left shows stored bottom-left.
    assert_eq!(oriented_source(3, 2, 6, 0, 0), (0, 1));
    assert_eq!(oriented_source(3, 2, 6, 1, 0), (0, 0));
    // Orientation 8 (rotate 90 counter-clockwise): oriented top-left shows stored top-right.
    assert_eq!(oriented_source(3, 2, 8, 0, 0), (2, 0));
    assert_eq!(oriented_source(3, 2, 3, 0, 0), (2, 1));
    assert_eq!(oriented_source(3, 2, 2, 0, 0), (2, 0));
    assert_eq!(oriented_source(3, 2, 4, 0, 0), (0, 1));
    assert_eq!(oriented_source(3, 2, 5, 1, 0), (0, 1));
    assert_eq!(oriented_source(3, 2, 7, 0, 0), (2, 1));
}

#[test]
fn constructor_and_codec_reject_bad_values_and_round_trip() {
    let bytes: Arc<[u8]> = Arc::from(&b"\x89PNG\r\n\x1a\nxyz"[..]);
    assert_eq!(
        InlineImage::new(ImageFormat::Png, bytes.clone(), 0, 4, 1),
        Err(ImageModelError::EmptyDimensions)
    );
    assert_eq!(
        InlineImage::new(ImageFormat::Png, bytes.clone(), 4, 4, 9),
        Err(ImageModelError::InvalidOrientation)
    );
    let image = InlineImage::new(ImageFormat::Jpeg, bytes, 40, 20, 6).unwrap();
    assert_eq!(image.oriented_size(), (20, 40));
    let data = image.to_object_data();
    assert_eq!(InlineImage::from_object_data(&data), Ok(image));
    for cut in 0..16 {
        assert!(InlineImage::from_object_data(&data[..cut]).is_err());
    }
    let mut bad = data.clone();
    bad[4] = 99;
    assert!(InlineImage::from_object_data(&bad).is_err());
}

#[test]
fn image_scalars_stay_inside_the_reserved_range() {
    assert_eq!(image_char(IMAGE_CHAR_BASE, 0), Some('\u{100000}'));
    assert!(is_image_char('\u{100005}'));
    assert!(!is_image_char('a'));
    assert!(!is_image_char('\u{fffc}'));
    assert_eq!(image_char(IMAGE_CHAR_BASE, IMAGE_CHAR_COUNT), None);
    assert_eq!(image_char(0x10_fffd, 1), None);
    assert_eq!(image_char(0, 0), None);
}

#[test]
fn rich_model_inserts_a_picture_by_an_undoable_transaction() {
    let (mut doc, p) = with_text("avant apres");
    let image =
        InlineImage::new(ImageFormat::Gif, Arc::from(&b"GIF89a-----"[..]), 8, 4, 1).unwrap();
    let id = doc.alloc_id().unwrap();
    // run_checked applies the operation, then verifies that undo restores the
    // document exactly and that redo reproduces the insertion.
    run_checked(
        &mut doc,
        RichOp::InsertInline {
            para: p,
            item: Inline::Anchored(AnchoredObject {
                id,
                offset: 6,
                kind: IMAGE_OBJECT_KIND.into(),
                data: image.to_object_data(),
            }),
        },
    );
    let para = doc.paragraph(p).unwrap();
    let stored = para.anchored.iter().find(|a| a.id == id).unwrap();
    assert_eq!(InlineImage::from_object_data(&stored.data), Ok(image));
    assert_eq!(para.text, "avant apres");
}

#[test]
fn alternative_text_is_cleaned_bounded_and_survives_the_codec() {
    let bytes: Arc<[u8]> = Arc::from(&b"\x89PNG\r\n\x1a\nxyz"[..]);
    let image = InlineImage::new(ImageFormat::Png, bytes.clone(), 4, 4, 1)
        .unwrap()
        .with_alt("  Plan\nde l\u{e9}tage\t ");
    assert_eq!(image.alt, "Plan de l\u{e9}tage");
    let round = InlineImage::from_object_data(&image.to_object_data()).unwrap();
    assert_eq!(round, image);
    assert_eq!(round.alt, "Plan de l\u{e9}tage");
    let long = InlineImage::new(ImageFormat::Png, bytes, 4, 4, 1)
        .unwrap()
        .with_alt(&"\u{e9}".repeat(1000));
    assert!(long.alt.len() <= MAX_ALT_BYTES && long.alt.chars().all(|c| c == '\u{e9}'));
    // Inconsistent alt length is refused, not read out of bounds.
    let mut bad = image.to_object_data();
    bad[6] = 255;
    assert!(InlineImage::from_object_data(&bad).is_err());
}
