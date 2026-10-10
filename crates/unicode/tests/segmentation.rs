// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_unicode::segmentation::{OffsetError, grapheme_boundaries, word_boundaries};

fn check_corpus(path: &str, word: bool) {
    let source = std::fs::read_to_string(path).unwrap();
    for (line_no, raw) in source.lines().enumerate() {
        let line = raw.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let mut text = String::new();
        let mut expected = vec![0usize];
        let mut offset = 0;
        for token in line.split_whitespace() {
            match token {
                "÷" => {
                    if expected.last().copied() != Some(offset) {
                        expected.push(offset);
                    }
                }
                "×" => {}
                hex => {
                    let scalar = u32::from_str_radix(hex, 16).unwrap();
                    let c = char::from_u32(scalar).unwrap();
                    text.push(c);
                    offset += c.len_utf8();
                }
            }
        }
        if expected.last().copied() != Some(offset) {
            expected.push(offset);
        }
        let actual = if word {
            word_boundaries(&text)
        } else {
            grapheme_boundaries(&text)
        };
        assert_eq!(
            actual.boundaries(),
            expected,
            "{} line {}: {line}",
            path,
            line_no + 1
        );
    }
}

#[test]
fn official_unicode_17_grapheme_break_test() {
    let root = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/ucd/17.0.0/auxiliary/GraphemeBreakTest.txt"
    );
    check_corpus(root, false);
}

#[test]
fn official_unicode_17_word_break_test() {
    let root = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/ucd/17.0.0/auxiliary/WordBreakTest.txt"
    );
    check_corpus(root, true);
}

#[test]
fn navigation_and_double_ended_iteration_obey_byte_offsets() {
    let index = grapheme_boundaries("a\u{0301}🙂b");
    assert_eq!(index.boundaries(), [0, 3, 7, 8]);
    assert_eq!(index.floor_boundary(1), Ok(0));
    assert_eq!(index.ceil_boundary(1), Ok(3));
    assert_eq!(index.previous_boundary(7), Ok(Some(3)));
    assert_eq!(index.next_boundary(3), Ok(Some(7)));
    assert_eq!(
        index.forward_from(1).unwrap().collect::<Vec<_>>(),
        [0..3, 3..7, 7..8]
    );
    assert_eq!(
        index.backward_from(1).unwrap().collect::<Vec<_>>(),
        std::iter::once(0..3).collect::<Vec<_>>()
    );
    let mut segments = index.segments();
    assert_eq!(segments.next(), Some(0..3));
    assert_eq!(segments.next_back(), Some(7..8));
    assert_eq!(segments.next_back(), Some(3..7));
    assert_eq!(segments.next(), None);
    assert_eq!(
        index.ceil_boundary(4),
        Err(OffsetError::NotCharBoundary { offset: 4 })
    );
    assert_eq!(
        index.next_boundary(9),
        Err(OffsetError::OutOfBounds { offset: 9, len: 8 })
    );
    assert_eq!(grapheme_boundaries("").boundaries(), [0]);
    assert_eq!(grapheme_boundaries("").segments().count(), 0);
}

#[test]
fn targeted_context_and_long_runs() {
    for text in [
        "क्‍क",
        "👩🏽‍🚀",
        "🇫🇷🇨🇦",
        "a\u{0301}\u{0301}\u{0301}",
        "a\u{200D}\u{0301}b",
    ] {
        let index = grapheme_boundaries(text);
        assert_eq!(index.segments().map(|r| &text[r]).collect::<String>(), text);
        assert_eq!(
            index.segments().rev().collect::<Vec<_>>(),
            index
                .segments()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
        );
    }
    let long = format!(
        "{}{}{}",
        "🇫🇷".repeat(3000),
        "\u{0301}".repeat(3000),
        "x".repeat(3000)
    );
    assert_eq!(
        grapheme_boundaries(&long)
            .segments()
            .map(|r| &long[r])
            .collect::<String>(),
        long
    );
    let ignored = format!("a{}b", "\u{200E}".repeat(3000));
    assert_eq!(
        word_boundaries(&ignored)
            .segments()
            .map(|r| &ignored[r])
            .collect::<String>(),
        ignored
    );

    let regional_indicators = "🇫🇷".repeat(3000);
    let word_index = word_boundaries(&regional_indicators);
    assert_eq!(word_index.segments().count(), 3000);
    assert_eq!(
        word_index
            .segments()
            .map(|r| &regional_indicators[r])
            .collect::<String>(),
        regional_indicators
    );
}
