// SPDX-License-Identifier: AGPL-3.0-or-later

use atelier_compress::{Level, compress_zlib};
use atelier_image::{DecodeLimits, ImageError, decode_png};

const SIG: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut covered = kind.to_vec();
    covered.extend_from_slice(data);
    out.extend_from_slice(&atelier_compress::crc32(&covered).to_be_bytes());
}

fn png(
    width: u32,
    height: u32,
    depth: u8,
    color: u8,
    interlace: u8,
    chunks: &[(&[u8; 4], &[u8])],
) -> Vec<u8> {
    let mut out = SIG.to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[depth, color, 0, 0, interlace]);
    chunk(&mut out, b"IHDR", &ihdr);
    for (kind, data) in chunks {
        chunk(&mut out, kind, data);
    }
    chunk(&mut out, b"IEND", &[]);
    out
}

fn zlib(raw: &[u8]) -> Vec<u8> {
    let mut compressed = vec![0; raw.len() * 2 + 128];
    let n = compress_zlib(raw, Level::Normal, &mut compressed).unwrap();
    compressed.truncate(n);
    compressed
}

#[test]
fn decodes_all_legal_color_depth_combinations_to_rgba8() {
    let cases = [
        (0, 1, vec![0b1000_0000], [255, 255, 255, 255]),
        (0, 2, vec![0b0100_0000], [85, 85, 85, 255]),
        (0, 4, vec![0x30], [51, 51, 51, 255]),
        (0, 8, vec![71], [71, 71, 71, 255]),
        (0, 16, vec![0x12, 0x34], [18, 18, 18, 255]),
        (2, 8, vec![17, 81, 201], [17, 81, 201, 255]),
        (
            2,
            16,
            vec![0x12, 0x34, 0x80, 0, 0xff, 0xff],
            [18, 128, 255, 255],
        ),
        (4, 8, vec![25, 128], [25, 25, 25, 128]),
        (4, 16, vec![0, 255, 0x80, 0], [1, 1, 1, 128]),
        (6, 8, vec![1, 80, 180, 200], [1, 80, 180, 200]),
        (
            6,
            16,
            vec![0, 255, 0x80, 0, 0xff, 0xff, 0, 0],
            [1, 128, 255, 0],
        ),
    ];
    for (color, depth, row, expected) in cases {
        let mut raw = vec![0];
        raw.extend_from_slice(&row);
        let stream = zlib(&raw);
        let image = decode_png(
            &png(1, 1, depth, color, 0, &[(b"IDAT", &stream)]),
            DecodeLimits::default(),
        )
        .unwrap();
        assert_eq!(image.pixels, expected, "color={color}, depth={depth}");
    }
    for depth in [1, 2, 4, 8] {
        let row = vec![0];
        let mut bytes = Vec::new();
        chunk(&mut bytes, b"PLTE", &[9, 33, 77]);
        let stream = zlib(&[&[0][..], &row].concat());
        chunk(&mut bytes, b"IDAT", &stream);
        chunk(&mut bytes, b"IEND", &[]);
        let mut full = SIG.to_vec();
        let ihdr = [0, 0, 0, 1, 0, 0, 0, 1, depth, 3, 0, 0, 0];
        chunk(&mut full, b"IHDR", &ihdr);
        full.extend_from_slice(&bytes);
        assert_eq!(
            decode_png(&full, DecodeLimits::default()).unwrap().pixels,
            [9, 33, 77, 255]
        );
    }
}

#[test]
fn applies_transparency_before_reducing_sample_precision() {
    let gray_raw = [0, 0xab, 0xcd];
    let idat = zlib(&gray_raw);
    let chunks: [(&[u8; 4], &[u8]); 2] = [(b"tRNS", &[0xab, 0xcd]), (b"IDAT", &idat)];
    assert_eq!(
        decode_png(&png(1, 1, 16, 0, 0, &chunks), DecodeLimits::default())
            .unwrap()
            .pixels,
        [171, 171, 171, 0]
    );

    let rgb_raw = [0, 1, 2, 3];
    let idat = zlib(&rgb_raw);
    let chunks: [(&[u8; 4], &[u8]); 2] = [(b"tRNS", &[0, 1, 0, 2, 0, 3]), (b"IDAT", &idat)];
    assert_eq!(
        decode_png(&png(1, 1, 8, 2, 0, &chunks), DecodeLimits::default())
            .unwrap()
            .pixels,
        [1, 2, 3, 0]
    );
}

fn filtered_row(input: &[u8], prev: &[u8], bpp: usize, filter: u8) -> Vec<u8> {
    let mut row = vec![filter];
    for (i, &x) in input.iter().enumerate() {
        let a = if i >= bpp { input[i - bpp] } else { 0 };
        let b = prev.get(i).copied().unwrap_or(0);
        let c = if i >= bpp {
            prev.get(i - bpp).copied().unwrap_or(0)
        } else {
            0
        };
        let pred = match filter {
            0 => 0,
            1 => a,
            2 => b,
            3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
            4 => {
                let p = i32::from(a) + i32::from(b) - i32::from(c);
                let pa = (p - i32::from(a)).abs();
                let pb = (p - i32::from(b)).abs();
                let pc = (p - i32::from(c)).abs();
                if pa <= pb && pa <= pc {
                    a
                } else if pb <= pc {
                    b
                } else {
                    c
                }
            }
            _ => unreachable!(),
        };
        row.push(x.wrapping_sub(pred));
    }
    row
}

#[test]
fn reverses_all_filters_and_adam7_passes() {
    let width = 9usize;
    let height = 7usize;
    let pixels: Vec<[u8; 4]> = (0..width * height)
        .map(|i| [i as u8, (i * 17) as u8, (255 - i) as u8, (i * 31) as u8])
        .collect();
    let mut raw = Vec::new();
    let mut prev = vec![0; width * 4];
    for y in 0..height {
        let row: Vec<u8> = pixels[y * width..(y + 1) * width]
            .iter()
            .flat_map(|p| p.iter().copied())
            .collect();
        raw.extend(filtered_row(&row, &prev, 4, (y % 5) as u8));
        prev = row;
    }
    let stream = zlib(&raw);
    let image = decode_png(
        &png(width as u32, height as u32, 8, 6, 0, &[(b"IDAT", &stream)]),
        DecodeLimits::default(),
    )
    .unwrap();
    let expected: Vec<u8> = pixels.iter().flat_map(|p| p.iter().copied()).collect();
    assert_eq!(image.pixels, expected);

    let passes = [
        (0, 0, 8, 8),
        (4, 0, 8, 8),
        (0, 4, 4, 8),
        (2, 0, 4, 4),
        (0, 2, 2, 4),
        (1, 0, 2, 2),
        (0, 1, 1, 2),
    ];
    let mut raw = Vec::new();
    for (pass_no, &(x0, y0, dx, dy)) in passes.iter().enumerate() {
        let xs: Vec<usize> = (x0..width).step_by(dx).collect();
        let ys: Vec<usize> = (y0..height).step_by(dy).collect();
        let mut prev = vec![0; xs.len() * 4];
        for (row_no, &y) in ys.iter().enumerate() {
            let row: Vec<u8> = xs
                .iter()
                .flat_map(|&x| pixels[y * width + x].iter().copied())
                .collect();
            raw.extend(filtered_row(&row, &prev, 4, ((pass_no + row_no) % 5) as u8));
            prev = row;
        }
    }
    let stream = zlib(&raw);
    let image = decode_png(
        &png(width as u32, height as u32, 8, 6, 1, &[(b"IDAT", &stream)]),
        DecodeLimits::default(),
    )
    .unwrap();
    assert_eq!(image.pixels, expected);
}

#[test]
fn validates_crc_order_zlib_trailing_data_and_limits() {
    let stream = zlib(&[0, 1]);
    let valid = png(1, 1, 8, 0, 0, &[(b"IDAT", &stream)]);
    let mut bad_crc = valid.clone();
    let crc_at = bad_crc.len() - 4;
    bad_crc[crc_at] ^= 1;
    assert!(matches!(
        decode_png(&bad_crc, DecodeLimits::default()),
        Err(ImageError::InvalidData("chunk CRC mismatch"))
    ));
    let mut trailing = valid.clone();
    trailing.push(0);
    assert!(decode_png(&trailing, DecodeLimits::default()).is_err());
    let mut zlib_extra = stream.clone();
    zlib_extra.push(0);
    assert!(
        decode_png(
            &png(1, 1, 8, 0, 0, &[(b"IDAT", &zlib_extra)]),
            DecodeLimits::default()
        )
        .is_err()
    );
    assert_eq!(
        decode_png(
            &valid,
            DecodeLimits {
                max_pixels: 0,
                max_input_bytes: usize::MAX
            }
        ),
        Err(ImageError::LimitExceeded)
    );
    assert_eq!(
        decode_png(
            &valid,
            DecodeLimits {
                max_pixels: 1,
                max_input_bytes: valid.len() - 1
            }
        ),
        Err(ImageError::LimitExceeded)
    );
    let bad_order = {
        let mut x = SIG.to_vec();
        chunk(&mut x, b"IDAT", &stream);
        x
    };
    assert!(decode_png(&bad_order, DecodeLimits::default()).is_err());

    let midpoint = stream.len() / 2;
    let split = png(
        1,
        1,
        8,
        0,
        0,
        &[
            (b"IDAT", &stream[..midpoint]),
            (b"IDAT", &stream[midpoint..]),
        ],
    );
    assert_eq!(
        decode_png(&split, DecodeLimits::default()).unwrap().pixels,
        [1, 1, 1, 255]
    );

    let mut ancillary = png(1, 1, 8, 0, 0, &[(b"tEXt", b"k\0v"), (b"IDAT", &stream)]);
    // The text chunk is covered by CRC even though its contents are ignored.
    let text = ancillary.windows(3).position(|w| w == b"k\0v").unwrap();
    ancillary[text] ^= 1;
    assert!(matches!(
        decode_png(&ancillary, DecodeLimits::default()),
        Err(ImageError::InvalidData("chunk CRC mismatch"))
    ));
}

#[test]
fn rejects_dimensions_and_chunk_lengths_above_png_31_bit_bounds() {
    let limits = DecodeLimits {
        max_pixels: u64::MAX,
        max_input_bytes: usize::MAX,
    };
    for (width, height) in [(0x8000_0000, 1), (1, 0x8000_0000)] {
        let input = png(width, height, 8, 0, 0, &[]);
        assert_eq!(
            decode_png(&input, limits),
            Err(ImageError::InvalidData("PNG dimensions exceed 2^31-1"))
        );
    }

    let mut input = png(1, 1, 8, 0, 0, &[]);
    input[8..12].copy_from_slice(&0x8000_0000u32.to_be_bytes());
    assert_eq!(
        decode_png(&input, limits),
        Err(ImageError::InvalidData("PNG chunk length exceeds 2^31-1"))
    );
}

#[test]
fn rejects_bad_palette_transparency_unknown_critical_and_filter() {
    let stream = zlib(&[0, 1]);
    let mut x = SIG.to_vec();
    let ihdr = [0, 0, 0, 1, 0, 0, 0, 1, 1, 3, 0, 0, 0];
    chunk(&mut x, b"IHDR", &ihdr);
    chunk(&mut x, b"PLTE", &[1, 2, 3]);
    chunk(&mut x, b"tRNS", &[1, 2]);
    chunk(&mut x, b"IDAT", &stream);
    chunk(&mut x, b"IEND", &[]);
    assert!(decode_png(&x, DecodeLimits::default()).is_err());

    let mut x = SIG.to_vec();
    chunk(&mut x, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]);
    chunk(&mut x, b"ABCD", &[]);
    chunk(&mut x, b"IDAT", &stream);
    chunk(&mut x, b"IEND", &[]);
    assert!(matches!(
        decode_png(&x, DecodeLimits::default()),
        Err(ImageError::Unsupported(_))
    ));
    let bad_filter = zlib(&[5, 1]);
    assert!(
        decode_png(
            &png(1, 1, 8, 0, 0, &[(b"IDAT", &bad_filter)]),
            DecodeLimits::default()
        )
        .is_err()
    );
}
