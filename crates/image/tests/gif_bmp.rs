// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_image::{DecodeLimits, ImageError, decode_bmp, decode_gif};

fn gif(
    width: u16,
    height: u16,
    left: u16,
    top: u16,
    interlace: bool,
    gce: bool,
    indices: &[u8],
) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"GIF89a");
    b.extend_from_slice(&width.to_le_bytes());
    b.extend_from_slice(&height.to_le_bytes());
    b.extend_from_slice(&[0xf7, 0, 0]);
    for i in 0..256u16 {
        b.extend_from_slice(&[i as u8, 0, 0]);
    }
    if gce {
        b.extend_from_slice(&[0x21, 0xf9, 4, 1, 0, 0, 0, 0]);
    }
    b.push(0x2c);
    b.extend_from_slice(&left.to_le_bytes());
    b.extend_from_slice(&top.to_le_bytes());
    b.extend_from_slice(
        &(if interlace {
            1u16
        } else {
            indices.len() as u16
        })
        .to_le_bytes(),
    );
    b.extend_from_slice(
        &(if interlace {
            indices.len() as u16
        } else {
            1u16
        })
        .to_le_bytes(),
    );
    b.push(if interlace { 0x40 } else { 0 });
    b.push(8);
    let mut bits = Vec::new();
    let mut acc = 0u32;
    let mut nbits = 0;
    for code in std::iter::once(256u16)
        .chain(indices.iter().map(|v| *v as u16))
        .chain(std::iter::once(257))
    {
        acc |= u32::from(code) << nbits;
        nbits += 9;
        while nbits >= 8 {
            bits.push(acc as u8);
            acc >>= 8;
            nbits -= 8;
        }
    }
    if nbits > 0 {
        bits.push(acc as u8)
    }
    b.push(bits.len() as u8);
    b.extend(bits);
    b.push(0);
    b.push(0x3b);
    b
}

fn bmp_header(w: i32, h: i32, bpp: u16, compression: u32, dib: u32, pixel_offset: u32) -> Vec<u8> {
    let mut b = vec![0u8; pixel_offset as usize];
    b[0..2].copy_from_slice(b"BM");
    b[2..6].copy_from_slice(&(pixel_offset + 16).to_le_bytes());
    b[10..14].copy_from_slice(&pixel_offset.to_le_bytes());
    b[14..18].copy_from_slice(&dib.to_le_bytes());
    b[18..22].copy_from_slice(&w.to_le_bytes());
    b[22..26].copy_from_slice(&h.to_le_bytes());
    b[26..28].copy_from_slice(&1u16.to_le_bytes());
    b[28..30].copy_from_slice(&bpp.to_le_bytes());
    b[30..34].copy_from_slice(&compression.to_le_bytes());
    b
}
fn set_file_size(b: &mut [u8]) {
    let n = b.len() as u32;
    b[2..6].copy_from_slice(&n.to_le_bytes());
}
fn set_colors_used(b: &mut [u8], n: u32) {
    b[46..50].copy_from_slice(&n.to_le_bytes());
}
fn palette(b: &mut Vec<u8>, colors: &[[u8; 3]]) {
    for c in colors {
        b.extend_from_slice(&[c[2], c[1], c[0], 0]);
    }
}

#[test]
fn gif_decodes_first_frame_and_interlace_order() {
    let indices = [1, 2, 3, 4];
    let data = gif(1, 4, 0, 0, true, false, &indices);
    let image = decode_gif(&data, DecodeLimits::default()).unwrap();
    assert_eq!((image.width, image.height), (1, 4));
    assert_eq!(
        image.pixels,
        vec![1, 0, 0, 255, 3, 0, 0, 255, 2, 0, 0, 255, 4, 0, 0, 255]
    );
}
#[test]
fn gif_offset_transparency_and_background() {
    let data = gif(2, 1, 1, 0, false, true, &[0]);
    let image = decode_gif(&data, DecodeLimits::default()).unwrap();
    assert_eq!(image.pixels, vec![0, 0, 0, 0, 0, 0, 0, 0]);
    let opaque = gif(2, 1, 1, 0, false, false, &[7]);
    let image = decode_gif(&opaque, DecodeLimits::default()).unwrap();
    assert_eq!(image.pixels, vec![0, 0, 0, 255, 7, 0, 0, 255]);
}
#[test]
fn gif_malformed_and_limits_are_errors() {
    let data = gif(1, 1, 0, 0, false, false, &[1]);
    assert!(decode_gif(&data[..data.len() - 3], DecodeLimits::default()).is_err());
    let limits = DecodeLimits {
        max_pixels: 0,
        ..DecodeLimits::default()
    };
    assert_eq!(decode_gif(&data, limits), Err(ImageError::LimitExceeded));
}

#[test]
fn bmp_rgb_24_bottom_up_and_top_down() {
    let mut b = bmp_header(2, 2, 24, 0, 40, 54);
    b.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0]);
    set_file_size(&mut b);
    let im = decode_bmp(&b, DecodeLimits::default()).unwrap();
    assert_eq!(
        im.pixels,
        vec![
            255, 0, 0, 255, 255, 255, 255, 255, 0, 0, 255, 255, 0, 255, 0, 255
        ]
    );
    let mut t = bmp_header(1, -1, 24, 0, 40, 54);
    t.extend_from_slice(&[30, 20, 10, 0]);
    set_file_size(&mut t);
    assert_eq!(
        decode_bmp(&t, DecodeLimits::default()).unwrap().pixels,
        vec![10, 20, 30, 255]
    );
}
#[test]
fn bmp_indexed_depths_and_palette_indices() {
    for (bpp, row, pixel) in [
        (1, vec![0x80, 0, 0, 0], vec![255, 0, 0, 255]),
        (4, vec![0x10, 0, 0, 0], vec![255, 0, 0, 255]),
        (8, vec![1, 0, 0, 0], vec![255, 0, 0, 255]),
    ] {
        let off = 54 + 2 * 4;
        let mut b = bmp_header(1, 1, bpp, 0, 40, off);
        b.truncate(54);
        set_colors_used(&mut b, 2);
        palette(&mut b, &[[0, 0, 0], [255, 0, 0]]);
        b.extend(row);
        set_file_size(&mut b);
        assert_eq!(
            decode_bmp(&b, DecodeLimits::default()).unwrap().pixels,
            pixel
        );
    }
}
#[test]
fn bmp_bitfields_preserve_explicit_alpha() {
    let mut b = bmp_header(1, 1, 32, 3, 56, 70);
    for (i, m) in [0x00ff0000u32, 0x0000ff00, 0x000000ff, 0xff000000]
        .iter()
        .enumerate()
    {
        let o = 54 + i * 4;
        b[o..o + 4].copy_from_slice(&m.to_le_bytes());
    }
    b.extend_from_slice(&[0x10, 0x20, 0x40, 0x80]);
    set_file_size(&mut b);
    assert_eq!(
        decode_bmp(&b, DecodeLimits::default()).unwrap().pixels,
        vec![0x40, 0x20, 0x10, 0x80]
    );
}
#[test]
fn bmp_rle8_runs_and_rle4_absolute_padding() {
    let colors = [
        [0, 0, 0],
        [255, 0, 0],
        [0, 255, 0],
        [0, 0, 255],
        [255, 255, 0],
        [255, 0, 255],
    ];
    let mut b = bmp_header(3, 1, 8, 1, 40, 54 + 6 * 4);
    b.truncate(54);
    set_colors_used(&mut b, 6);
    palette(&mut b, &colors);
    b.extend_from_slice(&[2, 1, 1, 2, 0, 1]);
    b[34..38].copy_from_slice(&6u32.to_le_bytes());
    set_file_size(&mut b);
    assert_eq!(
        decode_bmp(&b, DecodeLimits::default()).unwrap().pixels,
        vec![255, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255]
    );
    b[34..38].copy_from_slice(&7u32.to_le_bytes());
    assert_eq!(
        decode_bmp(&b, DecodeLimits::default()),
        Err(ImageError::Truncated)
    );
    let mut c = bmp_header(5, 1, 4, 2, 40, 54 + 6 * 4);
    c.truncate(54);
    set_colors_used(&mut c, 6);
    palette(&mut c, &colors);
    c.extend_from_slice(&[0, 5, 0x12, 0x34, 0x50, 0, 0, 1]);
    c[34..38].copy_from_slice(&8u32.to_le_bytes());
    set_file_size(&mut c);
    let im = decode_bmp(&c, DecodeLimits::default()).unwrap();
    assert_eq!(
        &im.pixels[..20],
        &[
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255, 255, 0, 255, 255
        ]
    );
}
#[test]
fn bmp_truncation_incompatible_modes_and_limits_fail_cleanly() {
    let mut b = bmp_header(1, 1, 24, 0, 40, 54);
    b.extend_from_slice(&[0, 0, 0, 0]);
    set_file_size(&mut b);
    assert!(decode_bmp(&b, DecodeLimits::default()).is_ok());
    b.truncate(55);
    assert_eq!(
        decode_bmp(&b, DecodeLimits::default()),
        Err(ImageError::Truncated)
    );
    let mut r = bmp_header(1, 1, 24, 1, 40, 54);
    set_file_size(&mut r);
    assert!(matches!(
        decode_bmp(&r, DecodeLimits::default()),
        Err(ImageError::InvalidData(_))
    ));
    let d = bmp_header(2, 2, 24, 0, 40, 54);
    assert_eq!(
        decode_bmp(&d, DecodeLimits::default()),
        Err(ImageError::Truncated)
    );
}

fn gif_minimum_code_size_two() -> Vec<u8> {
    let mut b = b"GIF89a\x01\0\x01\0\x80\0\0\0\0\0\xff\0\0".to_vec();
    b.extend_from_slice(&[0x2c, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x4c, 0x01, 0, 0x3b]);
    b
}

#[test]
fn gif_lzw_minimum_code_size_two_decodes_literal_stream() {
    let image = decode_gif(&gif_minimum_code_size_two(), DecodeLimits::default()).unwrap();
    assert_eq!(image.pixels, vec![255, 0, 0, 255]);
}

#[test]
fn decoder_truncations_never_panic() {
    let gif = gif_minimum_code_size_two();
    for end in 0..gif.len() {
        let result = std::panic::catch_unwind(|| decode_gif(&gif[..end], DecodeLimits::default()));
        assert!(result.is_ok(), "GIF parser panicked on prefix {end}");
    }
    let mut bmp = bmp_header(1, 1, 24, 0, 40, 54);
    bmp.extend_from_slice(&[30, 20, 10, 0]);
    set_file_size(&mut bmp);
    for end in 0..bmp.len() {
        let result = std::panic::catch_unwind(|| decode_bmp(&bmp[..end], DecodeLimits::default()));
        assert!(result.is_ok(), "BMP parser panicked on prefix {end}");
    }
}
#[test]
fn bmp_16_and_32_bit_rgb_use_expected_opaque_conventions() {
    let mut bmp16 = bmp_header(1, 1, 16, 0, 40, 54);
    bmp16.extend_from_slice(&[0x00, 0x7c, 0, 0]);
    set_file_size(&mut bmp16);
    assert_eq!(
        decode_bmp(&bmp16, DecodeLimits::default()).unwrap().pixels,
        vec![255, 0, 0, 255]
    );

    let mut bmp32 = bmp_header(1, 1, 32, 0, 40, 54);
    bmp32.extend_from_slice(&[0x11, 0x22, 0x33, 0x00]);
    set_file_size(&mut bmp32);
    assert_eq!(
        decode_bmp(&bmp32, DecodeLimits::default()).unwrap().pixels,
        vec![0x33, 0x22, 0x11, 255]
    );
}

#[test]
fn bmp_rle_delta_and_end_of_line_place_bottom_up_pixels() {
    let mut b = bmp_header(3, 2, 8, 1, 40, 66);
    b.truncate(54);
    set_colors_used(&mut b, 3);
    palette(&mut b, &[[0, 0, 0], [255, 0, 0], [0, 255, 0]]);
    b.extend_from_slice(&[1, 1, 0, 2, 1, 0, 1, 2, 0, 0, 3, 1, 0, 1]);
    b[34..38].copy_from_slice(&14u32.to_le_bytes());
    set_file_size(&mut b);
    assert_eq!(
        decode_bmp(&b, DecodeLimits::default()).unwrap().pixels,
        vec![
            255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 0, 255, 0, 255,
            0, 255
        ]
    );
}
#[test]
fn gif_plain_text_consumes_gce_but_comment_and_application_do_not() {
    fn insert_after_gce(data: &[u8], extension: &[u8]) -> Vec<u8> {
        let start = data.windows(2).position(|w| w == [0x21, 0xf9]).unwrap();
        let at = start + 8;
        let mut out = data[..at].to_vec();
        out.extend_from_slice(extension);
        out.extend_from_slice(&data[at..]);
        out
    }

    let base = gif(1, 1, 0, 0, false, true, &[0]);
    let plain_text = [0x21, 0x01, 12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let image = decode_gif(
        &insert_after_gce(&base, &plain_text),
        DecodeLimits::default(),
    )
    .unwrap();
    assert_eq!(image.pixels, vec![0, 0, 0, 255]);

    let comment = [0x21, 0xfe, 1, b'x', 0];
    let image = decode_gif(&insert_after_gce(&base, &comment), DecodeLimits::default()).unwrap();
    assert_eq!(image.pixels, vec![0, 0, 0, 0]);

    let application = [
        0x21, 0xff, 11, b'A', b'P', b'P', b'I', b'D', b'0', b'0', b'1', b'2', b'3', b'4', 0,
    ];
    let image = decode_gif(
        &insert_after_gce(&base, &application),
        DecodeLimits::default(),
    )
    .unwrap();
    assert_eq!(image.pixels, vec![0, 0, 0, 0]);

    let bad_plain_text = [0x21, 0x01, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    assert!(
        decode_gif(
            &insert_after_gce(&base, &bad_plain_text),
            DecodeLimits::default(),
        )
        .is_err()
    );
}
