// SPDX-License-Identifier: AGPL-3.0-or-later

use atelier_image::{DecodeLimits, ImageError, decode_jpeg, decode_jpeg_with_metadata};

const FIXTURES: &[(&str, u32, u32)] = &[
    ("baseline-gray", 19, 17),
    ("baseline-444", 19, 17),
    ("baseline-422", 19, 17),
    ("baseline-420", 19, 17),
    ("progressive-444", 19, 17),
    ("progressive-422", 19, 17),
    ("progressive-420", 19, 17),
    ("baseline-cmyk", 19, 17),
    ("adobe-ycck", 19, 17),
    ("restart-420", 19, 17),
    ("baseline-noise-low", 19, 17),
    ("baseline-noise-high", 19, 17),
    ("progressive-noise", 19, 17),
    ("progressive-gray", 19, 17),
    ("progressive-restart", 19, 17),
    ("size-1x1", 1, 1),
    ("size-16x16", 16, 16),
];

#[test]
fn baseline_progressive_sampling_grayscale_cmyk_and_restart_match_pillow() {
    for &(name, width, height) in FIXTURES {
        let jpeg = fixture(name, "jpg");
        let expected = fixture(name, "rgba");
        let image = decode_jpeg(&jpeg, DecodeLimits::default())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!((image.width, image.height), (width, height), "{name}");
        assert_eq!(image.pixels.len(), expected.len(), "{name}");
        let mut sums = [0u64; 4];
        for (actual, expected) in image.pixels.chunks_exact(4).zip(expected.chunks_exact(4)) {
            for channel in 0..4 {
                sums[channel] += u64::from(actual[channel].abs_diff(expected[channel]));
            }
        }
        let pixels = u64::from(width * height);
        for (channel, sum) in sums.into_iter().enumerate() {
            let mean = sum as f64 / pixels as f64;
            assert!(
                mean <= 1.0,
                "{name} channel {channel}: mean error {mean:.4}"
            );
        }
    }
}

#[test]
fn reads_all_exif_orientations_in_both_tiff_byte_orders_without_applying_them() {
    let jpeg = fixture("baseline-444", "jpg");
    for little_endian in [true, false] {
        for orientation in 1..=8 {
            let with_exif = add_exif(&jpeg, orientation, little_endian);
            let decoded = decode_jpeg_with_metadata(&with_exif, DecodeLimits::default()).unwrap();
            assert_eq!(decoded.orientation, orientation);
            assert_eq!((decoded.image.width, decoded.image.height), (19, 17));
        }
    }
}

#[test]
fn rejects_truncation_bad_markers_bad_segments_and_over_budget_dimensions() {
    let jpeg = fixture("baseline-444", "jpg");
    for end in [0, 1, 2, 3, jpeg.len() / 2, jpeg.len() - 1] {
        assert!(
            decode_jpeg(&jpeg[..end], DecodeLimits::default()).is_err(),
            "truncation at {end}"
        );
    }
    let mut bad_marker = jpeg.clone();
    bad_marker[0] = 0;
    assert!(matches!(
        decode_jpeg(&bad_marker, DecodeLimits::default()),
        Err(ImageError::InvalidData(_))
    ));
    let mut bad_segment = jpeg.clone();
    bad_segment[4] = 0;
    bad_segment[5] = 1;
    assert!(matches!(
        decode_jpeg(&bad_segment, DecodeLimits::default()),
        Err(ImageError::InvalidData(_))
    ));
    assert_eq!(
        decode_jpeg(
            &jpeg,
            DecodeLimits {
                max_pixels: 1,
                ..DecodeLimits::default()
            }
        ),
        Err(ImageError::LimitExceeded)
    );
    assert_eq!(
        decode_jpeg(
            &jpeg,
            DecodeLimits {
                max_input_bytes: jpeg.len() - 1,
                ..DecodeLimits::default()
            }
        ),
        Err(ImageError::LimitExceeded)
    );
}

#[test]
fn malformed_exif_orientation_means_as_stored() {
    let jpeg = fixture("baseline-444", "jpg");
    let mut with_exif = add_exif(&jpeg, 1, true);
    // The orientation value is the TIFF SHORT stored inline in the APP1 IFD entry.
    let marker = with_exif.windows(6).position(|w| w == b"Exif\0\0").unwrap();
    with_exif[marker + 6 + 8 + 2 + 8] = 9;
    with_exif[marker + 6 + 8 + 2 + 9] = 0;
    let decoded = decode_jpeg_with_metadata(&with_exif, DecodeLimits::default()).unwrap();
    assert_eq!(decoded.orientation, 1);
    assert_eq!((decoded.image.width, decoded.image.height), (19, 17));
    // Orientation 0 and a bad entry type are equally ignored.
    with_exif[marker + 6 + 8 + 2 + 8] = 0;
    assert_eq!(
        decode_jpeg_with_metadata(&with_exif, DecodeLimits::default())
            .unwrap()
            .orientation,
        1
    );
}

#[test]
fn exif_maximum_offset_and_truncated_ifd_never_panic() {
    let jpeg = fixture("baseline-444", "jpg");
    let mut maximal_offset = add_exif(&jpeg, 1, true);
    let exif = maximal_offset
        .windows(6)
        .position(|w| w == b"Exif\0\0")
        .unwrap();
    maximal_offset[exif + 10..exif + 14].copy_from_slice(&u32::MAX.to_le_bytes());
    let result = std::panic::catch_unwind(|| {
        decode_jpeg_with_metadata(&maximal_offset, DecodeLimits::default())
    });
    assert_eq!(result.unwrap().unwrap().orientation, 1);

    let truncated = add_truncated_exif(&jpeg);
    let result =
        std::panic::catch_unwind(|| decode_jpeg_with_metadata(&truncated, DecodeLimits::default()));
    assert_eq!(result.unwrap().unwrap().orientation, 1);
}

#[test]
fn rejects_two_component_frames_before_scan_decode() {
    let mut jpeg = fixture("baseline-444", "jpg");
    let sof = jpeg.windows(2).position(|w| w == [0xff, 0xc0]).unwrap();
    let length = usize::from(u16::from_be_bytes([jpeg[sof + 2], jpeg[sof + 3]]));
    let payload = sof + 4;
    jpeg[payload + 5] = 2;
    jpeg.drain(payload + 6 + 2 * 3..payload + 6 + 3 * 3);
    jpeg[sof + 2..sof + 4].copy_from_slice(&((length - 3) as u16).to_be_bytes());
    assert!(matches!(
        decode_jpeg(&jpeg, DecodeLimits::default()),
        Err(ImageError::Unsupported("two-component JPEG color model"))
    ));
}

#[test]
fn rejects_incompatible_adobe_transforms_before_scans_in_either_marker_order() {
    let rgb = fixture("baseline-444", "jpg");
    for after_sof in [false, true] {
        let invalid = set_adobe_transform(&rgb, 2, after_sof);
        assert!(matches!(
            decode_jpeg(&invalid, DecodeLimits::default()),
            Err(ImageError::InvalidData(_))
        ));
    }
    let cmyk = fixture("baseline-cmyk", "jpg");
    for after_sof in [false, true] {
        let invalid = set_adobe_transform(&cmyk, 1, after_sof);
        assert!(matches!(
            decode_jpeg(&invalid, DecodeLimits::default()),
            Err(ImageError::InvalidData(_))
        ));
    }
}

#[test]
fn progressive_eob_run_must_end_before_restart_marker() {
    let valid = fixture("progressive-restart", "jpg");
    assert!(decode_jpeg(&valid, DecodeLimits::default()).is_ok());
    let invalid = include_bytes!("fixtures/jpeg/progressive-restart-eobrun-crosses-restart.jpg");
    assert!(matches!(
        decode_jpeg(invalid, DecodeLimits::default()),
        Err(ImageError::InvalidData(_))
    ));
}

#[test]
fn progressive_scans_reject_invalid_refinement_order_and_missing_components() {
    let mut wrong_refinement = fixture("progressive-444", "jpg");
    let sos = wrong_refinement
        .windows(2)
        .position(|w| w == [0xff, 0xda])
        .unwrap();
    let count = usize::from(wrong_refinement[sos + 4]);
    let ah_al = sos + 7 + 2 * count;
    wrong_refinement[ah_al] = 0x10;
    assert!(matches!(
        decode_jpeg(&wrong_refinement, DecodeLimits::default()),
        Err(ImageError::InvalidData(_))
    ));

    let missing_component = make_missing_component_scan();
    assert!(matches!(
        decode_jpeg(&missing_component, DecodeLimits::default()),
        Err(ImageError::InvalidData(_))
    ));
}

#[test]
fn truncation_and_single_byte_mutation_campaign_never_panics() {
    let jpeg = fixture("baseline-gray", "jpg");
    let limits = DecodeLimits {
        max_pixels: 1024,
        max_input_bytes: 4096,
    };
    for end in 0..jpeg.len() {
        let result = std::panic::catch_unwind(|| decode_jpeg(&jpeg[..end], limits));
        assert!(result.is_ok(), "decoder panicked on prefix {end}");
        assert!(
            result.unwrap().is_err(),
            "truncated prefix {end} was accepted"
        );
    }
    for index in 0..jpeg.len() {
        let mut mutated = jpeg.clone();
        mutated[index] ^= 1;
        let result = std::panic::catch_unwind(|| decode_jpeg(&mutated, limits));
        assert!(
            result.is_ok(),
            "decoder panicked after mutation at byte {index}"
        );
    }
}

#[test]
fn marker_count_budget_bounds_segment_work() {
    let mut jpeg = vec![0xff, 0xd8];
    for _ in 0..4097 {
        jpeg.extend_from_slice(&[0xff, 0xfe, 0, 2]);
    }
    jpeg.extend_from_slice(&[0xff, 0xd9]);
    assert_eq!(
        decode_jpeg(
            &jpeg,
            DecodeLimits {
                max_pixels: 1,
                max_input_bytes: 32_000
            }
        ),
        Err(ImageError::LimitExceeded)
    );
}

fn fixture(name: &str, ext: &str) -> Vec<u8> {
    match (name, ext) {
        ("baseline-gray", "jpg") => include_bytes!("fixtures/jpeg/baseline-gray.jpg").to_vec(),
        ("baseline-gray", "rgba") => include_bytes!("fixtures/jpeg/baseline-gray.rgba").to_vec(),
        ("baseline-444", "jpg") => include_bytes!("fixtures/jpeg/baseline-444.jpg").to_vec(),
        ("baseline-444", "rgba") => include_bytes!("fixtures/jpeg/baseline-444.rgba").to_vec(),
        ("baseline-422", "jpg") => include_bytes!("fixtures/jpeg/baseline-422.jpg").to_vec(),
        ("baseline-422", "rgba") => include_bytes!("fixtures/jpeg/baseline-422.rgba").to_vec(),
        ("baseline-420", "jpg") => include_bytes!("fixtures/jpeg/baseline-420.jpg").to_vec(),
        ("baseline-420", "rgba") => include_bytes!("fixtures/jpeg/baseline-420.rgba").to_vec(),
        ("progressive-444", "jpg") => include_bytes!("fixtures/jpeg/progressive-444.jpg").to_vec(),
        ("progressive-444", "rgba") => {
            include_bytes!("fixtures/jpeg/progressive-444.rgba").to_vec()
        }
        ("progressive-422", "jpg") => include_bytes!("fixtures/jpeg/progressive-422.jpg").to_vec(),
        ("progressive-422", "rgba") => {
            include_bytes!("fixtures/jpeg/progressive-422.rgba").to_vec()
        }
        ("progressive-420", "jpg") => include_bytes!("fixtures/jpeg/progressive-420.jpg").to_vec(),
        ("progressive-420", "rgba") => {
            include_bytes!("fixtures/jpeg/progressive-420.rgba").to_vec()
        }
        ("baseline-cmyk", "jpg") => include_bytes!("fixtures/jpeg/baseline-cmyk.jpg").to_vec(),
        ("baseline-cmyk", "rgba") => include_bytes!("fixtures/jpeg/baseline-cmyk.rgba").to_vec(),
        ("adobe-ycck", "jpg") => include_bytes!("fixtures/jpeg/adobe-ycck.jpg").to_vec(),
        ("adobe-ycck", "rgba") => include_bytes!("fixtures/jpeg/adobe-ycck.rgba").to_vec(),
        ("restart-420", "jpg") => include_bytes!("fixtures/jpeg/restart-420.jpg").to_vec(),
        ("restart-420", "rgba") => include_bytes!("fixtures/jpeg/restart-420.rgba").to_vec(),
        ("baseline-noise-low", "jpg") => {
            include_bytes!("fixtures/jpeg/baseline-noise-low.jpg").to_vec()
        }
        ("baseline-noise-low", "rgba") => {
            include_bytes!("fixtures/jpeg/baseline-noise-low.rgba").to_vec()
        }
        ("baseline-noise-high", "jpg") => {
            include_bytes!("fixtures/jpeg/baseline-noise-high.jpg").to_vec()
        }
        ("baseline-noise-high", "rgba") => {
            include_bytes!("fixtures/jpeg/baseline-noise-high.rgba").to_vec()
        }
        ("progressive-noise", "jpg") => {
            include_bytes!("fixtures/jpeg/progressive-noise.jpg").to_vec()
        }
        ("progressive-noise", "rgba") => {
            include_bytes!("fixtures/jpeg/progressive-noise.rgba").to_vec()
        }
        ("progressive-gray", "jpg") => {
            include_bytes!("fixtures/jpeg/progressive-gray.jpg").to_vec()
        }
        ("progressive-gray", "rgba") => {
            include_bytes!("fixtures/jpeg/progressive-gray.rgba").to_vec()
        }
        ("progressive-restart", "jpg") => {
            include_bytes!("fixtures/jpeg/progressive-restart.jpg").to_vec()
        }
        ("progressive-restart", "rgba") => {
            include_bytes!("fixtures/jpeg/progressive-restart.rgba").to_vec()
        }
        ("size-1x1", "jpg") => include_bytes!("fixtures/jpeg/size-1x1.jpg").to_vec(),
        ("size-1x1", "rgba") => include_bytes!("fixtures/jpeg/size-1x1.rgba").to_vec(),
        ("size-16x16", "jpg") => include_bytes!("fixtures/jpeg/size-16x16.jpg").to_vec(),
        ("size-16x16", "rgba") => include_bytes!("fixtures/jpeg/size-16x16.rgba").to_vec(),
        _ => unreachable!(),
    }
}

fn add_exif(jpeg: &[u8], orientation: u8, little_endian: bool) -> Vec<u8> {
    let mut tiff = if little_endian {
        b"II".to_vec()
    } else {
        b"MM".to_vec()
    };
    let put16 = |v: u16, le: bool| if le { v.to_le_bytes() } else { v.to_be_bytes() };
    let put32 = |v: u32, le: bool| if le { v.to_le_bytes() } else { v.to_be_bytes() };
    tiff.extend_from_slice(&put16(42, little_endian));
    tiff.extend_from_slice(&put32(8, little_endian));
    tiff.extend_from_slice(&put16(1, little_endian));
    tiff.extend_from_slice(&put16(0x0112, little_endian));
    tiff.extend_from_slice(&put16(3, little_endian));
    tiff.extend_from_slice(&put32(1, little_endian));
    tiff.extend_from_slice(&put16(u16::from(orientation), little_endian));
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&put32(0, little_endian));
    let mut payload = b"Exif\0\0".to_vec();
    payload.extend_from_slice(&tiff);
    let mut output = jpeg[..2].to_vec();
    output.extend_from_slice(&[0xff, 0xe1]);
    output.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    output.extend_from_slice(&payload);
    output.extend_from_slice(&jpeg[2..]);
    output
}

fn make_missing_component_scan() -> Vec<u8> {
    let gray = fixture("baseline-gray", "jpg");
    let mut pos = 2;
    let mut headers = Vec::new();
    let mut entropy = Vec::new();
    while pos < gray.len() {
        assert_eq!(gray[pos], 0xff);
        while gray[pos] == 0xff {
            pos += 1;
        }
        let marker = gray[pos];
        pos += 1;
        if marker == 0xd9 {
            break;
        }
        let length = u16::from_be_bytes([gray[pos], gray[pos + 1]]) as usize;
        let end = pos + length;
        if marker == 0xda {
            let mut at = end;
            while at + 1 < gray.len() && !(gray[at] == 0xff && gray[at + 1] == 0xd9) {
                at += 1;
            }
            entropy.extend_from_slice(&gray[end..at]);
            break;
        }
        if marker != 0xc0 {
            headers.extend_from_slice(&gray[pos - 2..end]);
        }
        pos = end;
    }
    let mut jpeg = vec![0xff, 0xd8];
    jpeg.extend_from_slice(&headers);
    let mut frame = vec![8, 0, 17, 0, 19, 3];
    for id in 1..=3 {
        frame.extend_from_slice(&[id, 0x11, 0]);
    }
    append_segment(&mut jpeg, 0xc0, &frame);
    append_segment(&mut jpeg, 0xda, &[1, 1, 0, 0, 63, 0]);
    jpeg.extend_from_slice(&entropy);
    jpeg.extend_from_slice(&[0xff, 0xd9]);
    jpeg
}

fn append_segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) {
    out.extend_from_slice(&[0xff, marker]);
    out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(payload);
}

fn add_truncated_exif(jpeg: &[u8]) -> Vec<u8> {
    let mut result = jpeg[..2].to_vec();
    let mut payload = b"Exif\0\0II".to_vec();
    payload.extend_from_slice(&42u16.to_le_bytes());
    payload.extend_from_slice(&8u32.to_le_bytes());
    payload.extend_from_slice(&1u16.to_le_bytes());
    append_segment(&mut result, 0xe1, &payload);
    result.extend_from_slice(&jpeg[2..]);
    result
}

fn set_adobe_transform(jpeg: &[u8], transform: u8, after_sof: bool) -> Vec<u8> {
    let mut result = jpeg.to_vec();
    let mut at = 2;
    let mut sof = None;
    while at + 4 <= result.len() && result[at] == 0xff {
        let marker = result[at + 1];
        let length = usize::from(u16::from_be_bytes([result[at + 2], result[at + 3]]));
        let end = at + 2 + length;
        if marker == 0xda {
            break;
        }
        if marker == 0xee && result.get(at + 4..at + 9) == Some(b"Adobe") {
            result.drain(at..end);
            if let Some(position) = sof.as_mut() {
                if *position > at {
                    *position -= end - at;
                }
            }
            continue;
        }
        if marker == 0xc0 || marker == 0xc2 {
            sof = Some(if after_sof { end } else { at });
        }
        at = end;
    }
    let mut app14 = Vec::new();
    append_segment(
        &mut app14,
        0xee,
        &[b"Adobe".as_slice(), &[0, 100, 0, 0, 0, 0, transform]].concat(),
    );
    result.splice(sof.unwrap()..sof.unwrap(), app14);
    result
}
