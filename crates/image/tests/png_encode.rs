// SPDX-License-Identifier: AGPL-3.0-or-later

use atelier_compress::{Format, InflateState, crc32};
use atelier_image::{
    DecodeLimits, Image, ImageError, PngColor, decode_png, encode_png, encode_png_rgba,
};
use std::path::Path;

/// Fixed-seed xorshift64 so every run sees the same "random" images.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| (self.next() >> 24) as u8).collect()
    }
}

fn limits() -> DecodeLimits {
    DecodeLimits::default()
}

fn rgba_round_trip(width: u32, height: u32, pixels: Vec<u8>) {
    let png = encode_png(width, height, PngColor::Rgba8, &pixels, limits()).unwrap();
    let image = decode_png(&png, limits()).unwrap();
    assert_eq!((image.width, image.height), (width, height));
    assert_eq!(image.pixels, pixels, "{width}x{height}");
}

fn gray_round_trip(width: u32, height: u32, gray: Vec<u8>) {
    let png = encode_png(width, height, PngColor::Gray8, &gray, limits()).unwrap();
    let image = decode_png(&png, limits()).unwrap();
    let expected: Vec<u8> = gray.iter().flat_map(|&g| [g, g, g, 255]).collect();
    assert_eq!(image.pixels, expected, "{width}x{height} gray");
}

/// Chunks as (type, payload), checking signature, order and every CRC.
fn chunks(png: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let mut pos = 8;
    let mut out = Vec::new();
    while pos < png.len() {
        let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = png[pos + 4..pos + 8].try_into().unwrap();
        let payload = png[pos + 8..pos + 8 + len].to_vec();
        let crc = u32::from_be_bytes(png[pos + 8 + len..pos + 12 + len].try_into().unwrap());
        assert_eq!(crc, crc32(&png[pos + 4..pos + 8 + len]), "CRC of {kind:?}");
        out.push((kind, payload));
        pos += 12 + len;
    }
    out
}

/// Filter byte chosen for each row, read back from the zlib stream.
fn filter_types(png: &[u8], row_bytes: usize, height: usize) -> Vec<u8> {
    let idat: Vec<u8> = chunks(png)
        .into_iter()
        .filter(|(k, _)| k == b"IDAT")
        .flat_map(|(_, p)| p)
        .collect();
    let size = (row_bytes + 1) * height;
    let mut raw = vec![0u8; size];
    let mut state = InflateState::new(Format::Zlib, size as u64);
    let progress = state.push(&idat, &mut raw, true).unwrap();
    assert_eq!(progress.produced, size);
    (0..height).map(|y| raw[y * (row_bytes + 1)]).collect()
}

#[test]
fn round_trips_tiny_odd_and_wide_sizes() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for (w, h) in [(1, 1), (1, 9), (9, 1), (3, 5), (7, 13), (31, 3), (257, 2)] {
        rgba_round_trip(w, h, rng.bytes(w as usize * h as usize * 4));
        gray_round_trip(w, h, rng.bytes(w as usize * h as usize));
    }
}

#[test]
fn round_trips_gradients_and_transparency() {
    let (w, h) = (64u32, 48u32);
    let mut gradient = Vec::new();
    for y in 0..h {
        for x in 0..w {
            gradient.extend_from_slice(&[(x * 4) as u8, (y * 5) as u8, (x + y) as u8, 255]);
        }
    }
    rgba_round_trip(w, h, gradient);

    let mut alpha = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let a = ((x + y) * 255 / (w + h - 2)) as u8;
            alpha.extend_from_slice(&[200, 30, 90, a]);
        }
    }
    rgba_round_trip(w, h, alpha);
    // Fully transparent pixels keep their hidden color bytes.
    rgba_round_trip(2, 1, vec![9, 8, 7, 0, 1, 2, 3, 0]);

    gray_round_trip(w, h, (0..w * h).map(|i| (i / 12) as u8).collect());
}

#[test]
fn large_random_image_spans_several_idat_chunks() {
    let mut rng = Rng(42);
    let (w, h) = (300u32, 300u32);
    let pixels = rng.bytes(w as usize * h as usize * 4);
    let png = encode_png(w, h, PngColor::Rgba8, &pixels, limits()).unwrap();
    let parsed = chunks(&png);
    let idats = parsed.iter().filter(|(k, _)| k == b"IDAT").count();
    assert!(idats > 1, "expected several IDAT chunks, got {idats}");
    assert_eq!(&parsed[0].0, b"IHDR");
    assert_eq!(parsed[0].1[8..], [8, 6, 0, 0, 0]);
    assert_eq!(&parsed.last().unwrap().0, b"IEND");
    assert_eq!(decode_png(&png, limits()).unwrap().pixels, pixels);
}

#[test]
fn encodes_gray_as_color_type_zero_and_rgba_as_six() {
    let gray = encode_png(2, 2, PngColor::Gray8, &[1, 2, 3, 4], limits()).unwrap();
    assert_eq!(chunks(&gray)[0].1[8..], [8, 0, 0, 0, 0]);
    let rgba = encode_png_rgba(
        &Image {
            width: 1,
            height: 1,
            pixels: vec![1, 2, 3, 4],
        },
        limits(),
    )
    .unwrap();
    assert_eq!(chunks(&rgba)[0].1[8..], [8, 6, 0, 0, 0]);
}

#[test]
fn output_is_deterministic() {
    let pixels = Rng(7).bytes(40 * 30 * 4);
    let a = encode_png(40, 30, PngColor::Rgba8, &pixels, limits()).unwrap();
    let b = encode_png(40, 30, PngColor::Rgba8, &pixels, limits()).unwrap();
    assert_eq!(a, b);
}

#[test]
fn adaptive_choice_picks_none_sub_up_and_average_where_they_win() {
    let mut rng = Rng(1234);
    let w = 24usize;
    let r0 = rng.bytes(w);

    // None: a row of tiny values beats every predictor.
    let none: Vec<u8> = (0..w).map(|i| (i % 2) as u8).collect();
    let png = encode_png(w as u32, 1, PngColor::Gray8, &none, limits()).unwrap();
    assert_eq!(filter_types(&png, w, 1), [0]);

    // Sub: a linear ramp on the first row (prior row is zero).
    let sub: Vec<u8> = (0..w).map(|i| (100 + 2 * i) as u8).collect();
    let png = encode_png(w as u32, 1, PngColor::Gray8, &sub, limits()).unwrap();
    assert_eq!(filter_types(&png, w, 1), [1]);

    // Up: second row repeats a random first row.
    let mut up = r0.clone();
    up.extend_from_slice(&r0);
    let png = encode_png(w as u32, 2, PngColor::Gray8, &up, limits()).unwrap();
    assert_eq!(filter_types(&png, w, 2)[1], 2);

    // Average: second row equals floor((left + up) / 2) everywhere.
    let mut avg = r0.clone();
    let mut left = 0u8;
    for &above in &r0 {
        left = ((u16::from(left) + u16::from(above)) / 2) as u8;
        avg.push(left);
    }
    let png = encode_png(w as u32, 2, PngColor::Gray8, &avg, limits()).unwrap();
    assert_eq!(filter_types(&png, w, 2)[1], 3);
}

#[test]
fn rejects_invalid_dimensions_and_buffers() {
    let e = |w, h, len| encode_png(w, h, PngColor::Rgba8, &vec![0; len], limits());
    assert!(matches!(e(0, 1, 0), Err(ImageError::InvalidData(_))));
    assert!(matches!(e(1, 0, 0), Err(ImageError::InvalidData(_))));
    assert!(matches!(e(2, 2, 15), Err(ImageError::InvalidData(_))));
    assert!(matches!(e(2, 2, 17), Err(ImageError::InvalidData(_))));
    assert!(matches!(e(1, 1, 0), Err(ImageError::InvalidData(_))));
    let gray = encode_png(2, 2, PngColor::Gray8, &[0; 16], limits());
    assert!(matches!(gray, Err(ImageError::InvalidData(_))));
}

#[test]
fn rejects_dimensions_beyond_decoder_limits_without_allocating() {
    for (w, h) in [
        (u32::MAX, u32::MAX),
        (0x8000_0000, 1),
        (1, 0x8000_0000),
        (5000, 5000),
    ] {
        let r = encode_png(w, h, PngColor::Rgba8, &[], limits());
        assert!(r.is_err(), "{w}x{h}");
    }
    let tight = DecodeLimits {
        max_pixels: 4,
        max_input_bytes: 1 << 20,
    };
    assert_eq!(
        encode_png(3, 2, PngColor::Gray8, &[0; 6], tight),
        Err(ImageError::LimitExceeded)
    );
    assert!(encode_png(2, 2, PngColor::Gray8, &[0; 4], tight).is_ok());
    let small_output = DecodeLimits {
        max_pixels: 1 << 20,
        max_input_bytes: 40,
    };
    assert_eq!(
        encode_png(8, 8, PngColor::Rgba8, &[0; 256], small_output),
        Err(ImageError::LimitExceeded)
    );
}

#[test]
fn output_limit_accepts_exact_png_size_and_rejects_one_byte_less() {
    let pixels = [17, 34, 51, 255];
    let generous = DecodeLimits {
        max_pixels: 1,
        max_input_bytes: 1 << 20,
    };
    let png = encode_png(1, 1, PngColor::Rgba8, &pixels, generous).unwrap();
    assert!(png.len() > pixels.len());

    let exact = DecodeLimits {
        max_pixels: 1,
        max_input_bytes: png.len(),
    };
    let encoded = encode_png(1, 1, PngColor::Rgba8, &pixels, exact).unwrap();
    assert_eq!(encoded, png);
    assert_eq!(decode_png(&encoded, exact).unwrap().pixels, pixels);

    let one_byte_short = DecodeLimits {
        max_pixels: 1,
        max_input_bytes: png.len() - 1,
    };
    assert_eq!(
        encode_png(1, 1, PngColor::Rgba8, &pixels, one_byte_short),
        Err(ImageError::LimitExceeded)
    );
}

#[test]
fn reference_corpus_decodes_and_re_encodes_losslessly() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/image-reference");
    let mut checked = 0;
    for line in include_str!("../../../tests/image-reference/cases.tsv").lines() {
        let fields: Vec<_> = line.split('\t').collect();
        if fields[0] != "png" || fields[4] == "-" {
            continue;
        }
        let bytes = std::fs::read(root.join(fields[1])).unwrap();
        let image = decode_png(&bytes, limits()).unwrap();
        let png = encode_png_rgba(&image, limits()).unwrap();
        assert_eq!(decode_png(&png, limits()).unwrap(), image, "{}", fields[1]);
        let gray_source = image
            .pixels
            .chunks_exact(4)
            .all(|p| p[0] == p[1] && p[1] == p[2] && p[3] == 255);
        if gray_source {
            let gray: Vec<u8> = image.pixels.chunks_exact(4).map(|p| p[0]).collect();
            let png =
                encode_png(image.width, image.height, PngColor::Gray8, &gray, limits()).unwrap();
            assert_eq!(decode_png(&png, limits()).unwrap(), image, "{}", fields[1]);
        }
        checked += 1;
    }
    assert_eq!(checked, 162);
}
