// SPDX-License-Identifier: AGPL-3.0-or-later
//! Text encoding conversion for the encodings used by Atelier.

#![forbid(unsafe_code)]

pub mod properties;

mod tables;

/// A supported byte encoding. UTF-16 byte order is explicit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    Windows1252,
    Iso8859_1,
    Cp437,
    MacRoman,
}

/// Failure to represent a character in a single-byte encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EncodeError {
    /// The Unicode scalar that could not be encoded.
    pub character: char,
    /// Byte offset of the character in the input UTF-8 string.
    pub utf8_position: usize,
    /// The requested output encoding.
    pub encoding: Encoding,
}

impl core::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "character U+{:04X} at UTF-8 byte {} is not representable in {:?}",
            self.character as u32, self.utf8_position, self.encoding
        )
    }
}

impl std::error::Error for EncodeError {}

/// Detects an encoding by BOM, then valid UTF-8, then Windows-1252.
pub fn detect_encoding(bytes: &[u8]) -> Encoding {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Encoding::Utf8;
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return Encoding::Utf16Le;
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return Encoding::Utf16Be;
    }
    if core::str::from_utf8(bytes).is_ok() {
        Encoding::Utf8
    } else {
        Encoding::Windows1252
    }
}

/// Decodes bytes, replacing malformed sequences with U+FFFD.
/// A leading BOM is removed only when it matches `encoding`.
pub fn decode(bytes: &[u8], encoding: Encoding) -> String {
    let bytes = match encoding {
        Encoding::Utf8 if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) => &bytes[3..],
        Encoding::Utf16Le if bytes.starts_with(&[0xFF, 0xFE]) => &bytes[2..],
        Encoding::Utf16Be if bytes.starts_with(&[0xFE, 0xFF]) => &bytes[2..],
        _ => bytes,
    };
    match encoding {
        Encoding::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        Encoding::Utf16Le | Encoding::Utf16Be => decode_utf16(bytes, encoding == Encoding::Utf16Le),
        Encoding::Windows1252 => decode_single_byte(bytes, &tables::WINDOWS_1252),
        Encoding::Iso8859_1 => bytes
            .iter()
            .map(|&b| char::from_u32(b as u32).unwrap_or('\u{FFFD}'))
            .collect(),
        Encoding::Cp437 => decode_single_byte(bytes, &tables::CP437),
        Encoding::MacRoman => decode_single_byte(bytes, &tables::MAC_ROMAN),
    }
}

fn decode_single_byte(bytes: &[u8], table: &[u32; 256]) -> String {
    bytes
        .iter()
        .map(|&b| char::from_u32(table[b as usize]).unwrap_or('\u{FFFD}'))
        .collect()
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> String {
    let mut units = Vec::with_capacity(bytes.len().div_ceil(2));
    let mut chunks = bytes.chunks_exact(2);
    for pair in &mut chunks {
        units.push(if little_endian {
            u16::from_le_bytes([pair[0], pair[1]])
        } else {
            u16::from_be_bytes([pair[0], pair[1]])
        });
    }
    let mut result: String = char::decode_utf16(units)
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect();
    if !chunks.remainder().is_empty() {
        result.push('\u{FFFD}');
    }
    result
}

/// Encodes without adding a BOM. Single-byte failures report the input UTF-8 byte offset.
pub fn encode(text: &str, encoding: Encoding) -> Result<Vec<u8>, EncodeError> {
    match encoding {
        Encoding::Utf8 => Ok(text.as_bytes().to_vec()),
        Encoding::Utf16Le => Ok(text.encode_utf16().flat_map(u16::to_le_bytes).collect()),
        Encoding::Utf16Be => Ok(text.encode_utf16().flat_map(u16::to_be_bytes).collect()),
        _ => {
            let table: &[u32; 256] = match encoding {
                Encoding::Windows1252 => &tables::WINDOWS_1252,
                Encoding::Iso8859_1 => &tables::ISO_8859_1,
                Encoding::Cp437 => &tables::CP437,
                Encoding::MacRoman => &tables::MAC_ROMAN,
                _ => unreachable!(),
            };
            let mut output = Vec::with_capacity(text.len());
            for (position, character) in text.char_indices() {
                let codepoint = character as u32;
                let byte = table.iter().position(|&mapped| mapped == codepoint);
                match byte {
                    Some(value) => output.push(value as u8),
                    None => {
                        return Err(EncodeError {
                            character,
                            utf8_position: position,
                            encoding,
                        });
                    }
                }
            }
            Ok(output)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_priority_and_fallback() {
        assert_eq!(detect_encoding(b""), Encoding::Utf8);
        assert_eq!(detect_encoding(b"ASCII"), Encoding::Utf8);
        assert_eq!(detect_encoding("é🙂".as_bytes()), Encoding::Utf8);
        assert_eq!(detect_encoding(b"\xEF\xBB\xBFx"), Encoding::Utf8);
        assert_eq!(detect_encoding(b"\xFF\xFEx\0"), Encoding::Utf16Le);
        assert_eq!(detect_encoding(b"\xFE\xFF\0x"), Encoding::Utf16Be);
        assert_eq!(detect_encoding(b"\xC3("), Encoding::Windows1252);
    }

    #[test]
    fn bom_handling_is_initial_and_encoding_specific() {
        assert_eq!(
            decode(b"\xEF\xBB\xBFx\xEF\xBB\xBF", Encoding::Utf8),
            "x\u{FEFF}"
        );
        assert_eq!(
            decode(b"\xFF\xFEa\0\xFF\xFE", Encoding::Utf16Le),
            "a\u{FEFF}"
        );
        assert_eq!(decode(b"\xEF\xBB\xBF", Encoding::Windows1252), "ï»¿");
        assert_eq!(decode(b"\xFF\xFE", Encoding::Utf16Be), "\u{FFFE}");
    }

    #[test]
    fn malformed_sequences_are_replaced_without_losing_neighbors() {
        assert_eq!(
            decode(b"a\xF0\x28\x8C\x28z", Encoding::Utf8),
            "a\u{FFFD}(\u{FFFD}(z"
        );
        assert_eq!(
            decode(&[0x00, 0xD8, 0x41, 0x00], Encoding::Utf16Le),
            "\u{FFFD}A"
        );
        assert_eq!(decode(&[0x00, 0xDC], Encoding::Utf16Le), "\u{FFFD}");
        assert_eq!(decode(&[0x61, 0x00, 0xFF], Encoding::Utf16Le), "a\u{FFFD}");
        assert_eq!(decode(&[0x81], Encoding::Windows1252), "\u{FFFD}");
    }

    #[test]
    fn historical_tables_cover_all_bytes_and_round_trip_defined_values() {
        assert_eq!(
            mapping_fingerprint(&tables::WINDOWS_1252),
            0x9AE1_4D09_8B17_B49E
        );
        assert_eq!(mapping_fingerprint(&tables::CP437), 0x8770_36DB_04EE_6E19);
        assert_eq!(
            mapping_fingerprint(&tables::MAC_ROMAN),
            0x019A_3AD4_A72D_2BAD
        );
        assert_eq!(
            mapping_fingerprint(&tables::ISO_8859_1),
            0x8084_B7F6_C938_AF25
        );

        for encoding in [
            Encoding::Windows1252,
            Encoding::Cp437,
            Encoding::MacRoman,
            Encoding::Iso8859_1,
        ] {
            for byte in 0u8..=255 {
                let decoded = decode(&[byte], encoding);
                assert_eq!(decoded.chars().count(), 1, "{encoding:?} byte {byte:02X}");
                if decoded != "\u{FFFD}" {
                    assert_eq!(
                        encode(&decoded, encoding).unwrap(),
                        vec![byte],
                        "{encoding:?} byte {byte:02X}"
                    );
                }
            }
        }
        assert_eq!(decode(&[0x80], Encoding::Windows1252), "€");
        assert_eq!(decode(&[0x80], Encoding::Iso8859_1), "\u{0080}");
        assert_eq!(decode(&[0x82], Encoding::Cp437), "é");
        assert_eq!(decode(&[0x8E], Encoding::MacRoman), "é");
    }

    fn mapping_fingerprint(table: &[u32; 256]) -> u64 {
        let mut hash = 0xCBF2_9CE4_8422_2325_u64;
        for value in table {
            for byte in value.to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        hash
    }

    #[test]
    fn encoding_reports_character_and_utf8_byte_offset() {
        let error = encode("A🙂", Encoding::Windows1252).unwrap_err();
        assert_eq!(error.character, '🙂');
        assert_eq!(error.utf8_position, 1);
        assert_eq!(error.encoding, Encoding::Windows1252);
        let error = encode("é🙂", Encoding::Windows1252).unwrap_err();
        assert_eq!(error.character, '🙂');
        assert_eq!(error.utf8_position, 2);
        assert_eq!(encode("aé", Encoding::Iso8859_1).unwrap(), [b'a', 0xE9]);
    }

    #[test]
    fn utf_encodings_round_trip_without_bom() {
        let input = "Atelier — café 🙂";
        for encoding in [Encoding::Utf8, Encoding::Utf16Le, Encoding::Utf16Be] {
            let bytes = encode(input, encoding).unwrap();
            assert_eq!(decode(&bytes, encoding), input);
        }
        assert_eq!(
            decode(&[0xFF, 0xFE, 0x3D, 0xD8, 0x42, 0xDE], Encoding::Utf16Le),
            "🙂"
        );
        assert_eq!(
            decode(&[0xFE, 0xFF, 0xD8, 0x3D, 0xDE, 0x42], Encoding::Utf16Be),
            "🙂"
        );
        assert!(
            !encode("x", Encoding::Utf8)
                .unwrap()
                .starts_with(&[0xEF, 0xBB, 0xBF])
        );
    }
}
