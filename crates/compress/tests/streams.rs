// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_compress::{
    Format, InflateError, InflateState, Level, Status, adler32, compress, compress_zlib, crc32,
};

fn inflate_stream(
    format: Format,
    bytes: &[u8],
    piece: usize,
    out_piece: usize,
    limit: u64,
) -> Result<Vec<u8>, InflateError> {
    let mut state = InflateState::new(format, limit);
    let mut decoded = Vec::new();
    let mut pos = 0;
    loop {
        let end = (pos + piece).min(bytes.len());
        let input = &bytes[pos..end];
        let mut out = vec![0u8; out_piece];
        let finish = end == bytes.len();
        let p = state.push(input, &mut out, finish)?;
        pos += p.consumed;
        decoded.extend_from_slice(&out[..p.produced]);
        match p.status {
            Status::Done => return Ok(decoded),
            Status::NeedOutput => {}
            Status::NeedInput => {
                if finish && pos == bytes.len() {
                    return Err(InflateError::UnexpectedEof);
                }
                if p.consumed == 0 && p.produced == 0 && pos < bytes.len() {
                    continue;
                }
            }
        }
        assert!(pos <= bytes.len());
    }
}

#[test]
fn independent_rfc_vectors_decode_incrementally() {
    let cases: [(&str, Format); 7] = [
        ("stored", Format::Deflate),
        ("fixed", Format::Deflate),
        ("dynamic", Format::Deflate),
        ("zlib", Format::Zlib),
        ("empty", Format::Zlib),
        ("empty-deflate", Format::Deflate),
        ("window-32768", Format::Deflate),
    ];
    for (name, format) in cases {
        let packed = std::fs::read(format!(
            "{}/tests/vectors/{name}.bin",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let expected = std::fs::read(format!(
            "{}/tests/vectors/{name}.out",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        if format == Format::Deflate {
            let expected_type = match name {
                "stored" | "window-32768" => 0,
                "fixed" => 1,
                "dynamic" => 2,
                _ => 1,
            };
            assert_eq!((packed[0] >> 1) & 3, expected_type);
        }
        assert_eq!(
            inflate_stream(format, &packed, 1, 1, expected.len() as u64).unwrap(),
            expected,
            "{name}"
        );
        assert_eq!(
            inflate_stream(
                format,
                &packed,
                packed.len().max(1),
                7,
                expected.len() as u64
            )
            .unwrap(),
            expected,
            "{name} monolithic"
        );
    }
}

#[test]
fn malformed_truncated_and_limit_errors_are_typed() {
    let stored = [0x01, 0x03, 0x00, 0xfc, 0xff, b'a', b'b', b'c'];
    assert_eq!(
        inflate_stream(Format::Deflate, &stored, 2, 1, 2),
        Err(InflateError::OutputLimit)
    );
    let mut bounded = InflateState::new(Format::Deflate, 2);
    let mut output = [0xaa; 3];
    assert_eq!(
        bounded.push(&stored, &mut output, true),
        Err(InflateError::OutputLimit)
    );
    assert_eq!(output, [b'a', b'b', 0xaa]);
    assert!(matches!(
        bounded.push(&[], &mut output, true),
        Err(InflateError::InvalidData(_))
    ));
    assert_eq!(
        inflate_stream(Format::Deflate, &stored[..7], 1, 1, 10),
        Err(InflateError::UnexpectedEof)
    );
    let bad_zlib = [0x78, 0x9c, 0x03, 0x00, 0, 0, 0, 0];
    assert!(matches!(
        inflate_stream(Format::Zlib, &bad_zlib, 1, 1, 20),
        Err(InflateError::InvalidData(_))
    ));
    let missing_checksum = include_bytes!("vectors/empty.bin");
    assert_eq!(
        inflate_stream(
            Format::Zlib,
            &missing_checksum[..missing_checksum.len() - 1],
            1,
            1,
            0
        ),
        Err(InflateError::UnexpectedEof)
    );
    let bad_header = [0x00, 0x00];
    assert!(matches!(
        inflate_stream(Format::Zlib, &bad_header, 1, 1, 20),
        Err(InflateError::InvalidData(_))
    ));
    for bad in [
        include_bytes!("vectors/incomplete-code-tree.bin").as_slice(),
        include_bytes!("vectors/incomplete-literal-tree.bin").as_slice(),
        include_bytes!("vectors/incomplete-distance-tree.bin").as_slice(),
    ] {
        assert!(matches!(
            inflate_stream(Format::Deflate, bad, bad.len(), 1, 100),
            Err(InflateError::InvalidData(_))
        ));
    }
    let bad_distance = include_bytes!("vectors/distance-before-history.bin");
    assert!(matches!(
        inflate_stream(Format::Deflate, bad_distance, bad_distance.len(), 8, 100),
        Err(InflateError::InvalidData(_))
    ));
}

#[test]
fn encoder_roundtrips_deterministic_inputs_and_formats() {
    let root = format!("{}/tests/corpus", env!("CARGO_MANIFEST_DIR"));
    let mut inputs = Vec::new();
    for n in [
        0usize, 1, 2, 3, 255, 256, 257, 258, 32767, 32768, 32769, 65537,
    ] {
        let mut x = 0x1234_5678u32;
        let mut b = Vec::with_capacity(n);
        for _ in 0..n {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            b.push(x as u8);
        }
        inputs.push(b);
    }
    let mut window_case = Vec::with_capacity(32768 + 258);
    for seed in 1..1000u32 {
        window_case.clear();
        let mut x = seed;
        for _ in 0..32768 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            window_case.push(x as u8);
        }
        let first = &window_case[..3];
        if !window_case[1..].windows(3).any(|triplet| triplet == first) {
            break;
        }
    }
    assert!(
        !window_case[1..]
            .windows(3)
            .any(|triplet| triplet == &window_case[..3])
    );
    let prefix = window_case[..258].to_vec();
    window_case.extend_from_slice(&prefix);
    inputs.push(window_case);
    for name in [
        "repetitive.txt",
        "mixed-utf8.txt",
        "binary-repeat.bin",
        "incompressible.bin",
    ] {
        inputs.push(std::fs::read(format!("{root}/{name}")).unwrap());
    }
    for input in inputs {
        for level in [Level::Fast, Level::Normal] {
            let mut packed = vec![0; input.len() * 2 + 1024];
            let n = compress(&input, level, &mut packed).unwrap();
            packed.truncate(n);
            assert_eq!(
                inflate_stream(Format::Deflate, &packed, 13, 11, input.len() as u64)
                    .unwrap_or_else(|e| panic!(
                        "raw level={level:?} len={} data error={e:?}",
                        input.len()
                    )),
                input
            );
            let mut wrapped = vec![0; input.len() * 2 + 1030];
            let n = compress_zlib(&input, level, &mut wrapped).unwrap();
            wrapped.truncate(n);
            assert_eq!(
                inflate_stream(Format::Zlib, &wrapped, 17, 9, input.len() as u64).unwrap(),
                input
            );
        }
    }
}

#[test]
fn checksums_and_output_capacity_are_explicit() {
    assert_eq!(adler32(b"Wikipedia"), 0x11e60398);
    assert_eq!(crc32(b"123456789"), 0xcbf43926);
    assert!(compress(b"some payload", Level::Normal, &mut []).is_err());
}

#[test]
fn encoder_matches_independently_validated_wire_fixtures() {
    // These deterministic wire bytes were accepted by Python stdlib zlib.
    // Regeneration requires the explicit --update-wire-fixtures oracle mode;
    // normal CI compares exact known-valid bytes without invoking this inflater.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    for name in [
        "repetitive.txt",
        "mixed-utf8.txt",
        "binary-repeat.bin",
        "incompressible.bin",
    ] {
        let input = std::fs::read(root.join("corpus").join(name)).unwrap();
        for (level, label) in [(Level::Fast, "fast"), (Level::Normal, "normal")] {
            for wrapped in [false, true] {
                let mut output = vec![0; input.len() * 2 + 8192];
                let len = if wrapped {
                    compress_zlib(&input, level, &mut output)
                } else {
                    compress(&input, level, &mut output)
                }
                .unwrap();
                output.truncate(len);
                let format = if wrapped { "zlib" } else { "raw" };
                let expected = std::fs::read(
                    root.join("encoder-wire")
                        .join(format!("{name}-{label}-{format}.bin")),
                )
                .unwrap();
                assert_eq!(
                    output, expected,
                    "wire format changed: {name}/{label}/{format}"
                );
            }
        }
    }
}
