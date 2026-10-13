// SPDX-License-Identifier: AGPL-3.0-or-later
//! Property tests of the text-file round trips (US-316), with a fixed-seed
//! generator: save then reopen gives the same text; open then save "as opened"
//! gives the same bytes; arbitrary bytes never panic.

use atelier_text_model::plain_io::*;
use atelier_unicode::Encoding;

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0x5851_f42d_4c95_7f2d)
    }
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

/// Characters every encoding of the test can write.
const NARROW: &[&str] = &[
    "a", "Z", " ", "\t", "é", "à", "Ç", "ï", "€", "œ", "“", "”", "…", "ÿ", "\u{A0}", "0", ".",
    "\u{1}", "\u{C}",
];
/// Characters only the Unicode encodings can write.
const WIDE: &[&str] = &[
    "日本",
    "😀",
    "🇫🇷",
    "e\u{301}",
    "\u{FEFF}",
    "\u{FFFD}",
    "ß",
    "\u{10FFFF}",
];

fn random_text(rng: &mut Rng, wide: bool) -> String {
    let mut text = String::new();
    for _ in 0..rng.below(60) {
        match rng.below(8) {
            0 | 1 => text.push('\n'),
            2 if wide => text.push_str(WIDE[rng.below(WIDE.len())]),
            _ => text.push_str(NARROW[rng.below(NARROW.len())]),
        }
    }
    // Saved without BOM, a leading U+FEFF would be read back as a BOM.
    if text.starts_with('\u{FEFF}') {
        text.remove(0);
    }
    text
}

const SEEDS: u64 = 300;

#[test]
fn save_then_reopen_and_as_opened_round_trips() {
    use SaveEncoding::*;
    let targets = [
        (Utf8, Encoding::Utf8, false, true),
        (Utf8Bom, Encoding::Utf8, true, true),
        (Utf16Le, Encoding::Utf16Le, true, true),
        (Utf16Be, Encoding::Utf16Be, true, true),
        (Windows1252, Encoding::Windows1252, false, false),
    ];
    let mut cases = 0;
    for seed in 0..SEEDS {
        let mut rng = Rng::new(seed);
        for (save_encoding, encoding, bom, wide) in targets {
            let text = random_text(&mut rng, wide);
            let breaks = text.matches('\n').count();
            for (choice, eol) in [
                (LineEndingChoice::Lf, Eol::Lf),
                (LineEndingChoice::CrLf, Eol::CrLf),
            ] {
                let options = SaveOptions {
                    encoding: save_encoding,
                    line_ending: choice,
                    ..SaveOptions::default()
                };
                let saved = save_text(&text, &options).unwrap();
                assert_eq!(
                    (saved.encoding, saved.bom, saved.eol, saved.replaced),
                    (encoding, bom, eol, 0)
                );

                // Reopen with the encoding imposed: same text.
                let opened = open_text(
                    &saved.bytes,
                    &OpenOptions {
                        encoding: Some(encoding),
                        ..OpenOptions::default()
                    },
                )
                .unwrap();
                assert_eq!(
                    opened.text, text,
                    "seed {seed} {save_encoding:?} {choice:?}"
                );
                assert_eq!(opened.bom, bom);
                assert_eq!(opened.invalid_sequences, 0);
                let counts = opened.line_endings;
                match eol {
                    Eol::Lf => assert_eq!((counts.lf, counts.crlf, counts.cr), (breaks, 0, 0)),
                    _ => assert_eq!((counts.lf, counts.crlf, counts.cr), (0, breaks, 0)),
                }

                // Open (imposed) then save "as opened": same bytes.
                let again =
                    save_text(&opened.text, &SaveOptions::as_opened(opened.format())).unwrap();
                assert_eq!(
                    again.bytes, saved.bytes,
                    "seed {seed} {save_encoding:?} {choice:?}"
                );

                // Same through the automatic detection whenever it picks the
                // same encoding (it may legitimately not, for short texts).
                let auto = open_text(&saved.bytes, &OpenOptions::default()).unwrap();
                if auto.encoding == encoding {
                    assert_eq!(auto.text, text);
                    let back =
                        save_text(&auto.text, &SaveOptions::as_opened(auto.format())).unwrap();
                    assert_eq!(back.bytes, saved.bytes);
                }
                // The rich document target gives the same bytes back too.
                let doc = opened.to_rich_document(1000).unwrap();
                let from_rich =
                    save_rich_document(&doc, &SaveOptions::as_opened(opened.format())).unwrap();
                assert_eq!(from_rich.bytes, saved.bytes, "rich, seed {seed}");
                cases += 1;
            }
        }
    }
    assert_eq!(cases, SEEDS as usize * 10);
    eprintln!("round-trip property: {cases} cases");
}

#[test]
fn arbitrary_bytes_never_panic_and_valid_ones_round_trip() {
    let mut cases = 0;
    let mut exact = 0;
    for seed in 0..SEEDS {
        let mut rng = Rng::new(seed ^ 0xABCD);
        let len = rng.below(200);
        // A mix of fully random bytes and text-like bytes.
        let bytes: Vec<u8> = (0..len)
            .map(|_| match rng.below(4) {
                0 => rng.below(256) as u8,
                1 => b"\r\n\0 \t"[rng.below(5)],
                2 => [0xFF, 0xFE, 0xEF, 0xBB, 0xBF, 0xD8, 0xDC, 0x80, 0x81][rng.below(9)],
                _ => b'a' + rng.below(26) as u8,
            })
            .collect();
        for encoding in std::iter::once(None).chain(MANUAL_ENCODINGS.map(Some)) {
            let options = OpenOptions {
                encoding,
                ..OpenOptions::default()
            };
            let opened = open_text(&bytes, &options).unwrap();
            assert!(!opened.text.contains('\r'));
            let doc = opened.to_rich_document(10_000).unwrap();
            assert_eq!(
                atelier_text_model::plain_io::rich_to_text(&doc),
                opened.text
            );
            // Replaced sequences (U+FFFD) cannot go back to a single-byte
            // encoding: `?` is written instead of failing.
            let replacing = SaveOptions {
                unmappable: Unmappable::Replace,
                ..SaveOptions::as_opened(opened.format())
            };
            let saved = save_text(&opened.text, &replacing).unwrap();
            if opened.round_trip_is_exact() {
                assert_eq!(saved.bytes, bytes, "seed {seed} {encoding:?}");
                exact += 1;
            }
            // Every other save target must also work or fail cleanly.
            for target in SAVE_ENCODINGS {
                let options = SaveOptions {
                    encoding: target,
                    ..SaveOptions::default()
                };
                let _ = save_text(&opened.text, &options);
            }
            cases += 1;
        }
    }
    eprintln!("arbitrary bytes: {cases} cases, {exact} exact round trips");
    assert!(exact > 0);
}

#[test]
fn large_inputs_are_handled_in_one_pass() {
    // 8 MiB of mixed text through every step: a smoke test of linear cost.
    let line = "Éléphant à l'été, œuvre « complète » €\n";
    let text = line.repeat(8 * 1024 * 1024 / line.len());
    let saved = save_text(
        &text,
        &SaveOptions {
            encoding: SaveEncoding::Utf16Le,
            line_ending: LineEndingChoice::CrLf,
            ..SaveOptions::default()
        },
    )
    .unwrap();
    let opened = open_text(&saved.bytes, &OpenOptions::default()).unwrap();
    assert_eq!(opened.text, text);
    assert_eq!(opened.encoding, Encoding::Utf16Le);
}
