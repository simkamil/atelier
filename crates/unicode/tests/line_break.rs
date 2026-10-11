// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_unicode::line_break::{BreakKind, line_breaks};
use atelier_unicode::properties::line_break_class;

#[test]
fn unicode_17_line_break_test_corpus() {
    let data = include_str!("../../../data/ucd/17.0.0/auxiliary/LineBreakTest.txt");
    let mut cases = 0usize;
    for (line_no, line) in data.lines().enumerate() {
        let content = line.split('#').next().unwrap().trim();
        if content.is_empty() {
            continue;
        }
        let mut chars = Vec::new();
        let mut expected_offsets = Vec::new();
        let mut marker = None;
        let mut byte_offset = 0;
        for token in content.split_whitespace() {
            match token {
                "÷" | "×" => marker = Some(token == "÷"),
                hex => {
                    if marker.take().unwrap_or(false) {
                        expected_offsets.push(byte_offset);
                    }
                    let scalar = u32::from_str_radix(hex, 16).unwrap();
                    let ch = char::from_u32(scalar).unwrap();
                    byte_offset += ch.len_utf8();
                    chars.push(ch);
                }
            }
        }
        if marker == Some(true) {
            expected_offsets.push(byte_offset);
        }
        let text: String = chars.iter().collect();
        let expected: Vec<_> = expected_offsets
            .into_iter()
            .filter(|&offset| offset != 0)
            .collect();
        let actual = line_breaks(&text);
        let actual_offsets: Vec<_> = actual.iter().map(|br| br.offset).collect();
        assert_eq!(
            actual_offsets,
            expected,
            "LineBreakTest line {}: {content}",
            line_no + 1
        );
        for br in actual {
            let should_be_mandatory = br.offset == text.len()
                || chars
                    .iter()
                    .scan(0usize, |offset, ch| {
                        let start = *offset;
                        *offset += ch.len_utf8();
                        Some((start, ch))
                    })
                    .find(|(start, ch)| start + ch.len_utf8() == br.offset)
                    .is_some_and(|(_, ch)| {
                        matches!(line_break_class(*ch), "BK" | "CR" | "LF" | "NL")
                    });
            assert_eq!(
                br.kind == BreakKind::Mandatory,
                should_be_mandatory,
                "mandatory kind at line {} offset {}",
                line_no + 1,
                br.offset
            );
        }
        cases += 1;
    }
    assert_eq!(cases, 19_338);
}

#[test]
fn mandatory_controls_crlf_spaces_and_utf8_offsets() {
    let breaks = line_breaks("a\r\nb\u{000B}c\u{000C}d\u{0085}e\u{2028}f\u{2029}g");
    assert_eq!(
        breaks
            .iter()
            .filter(|b| b.kind == BreakKind::Mandatory)
            .count(),
        7
    );
    assert!(!breaks.iter().any(|b| b.offset == 2)); // CR × LF
    assert_eq!(
        line_breaks("")[0],
        atelier_unicode::line_break::LineBreak {
            offset: 0,
            kind: BreakKind::Mandatory
        }
    );
    for text in [
        "a b",
        "x\u{00A0}y",
        "x\u{202F}y",
        "a\u{2060}b",
        "a\u{200B}b",
        "👩\u{200D}💻",
    ] {
        let breaks = line_breaks(text);
        assert!(breaks.windows(2).all(|w| w[0].offset < w[1].offset));
        assert!(breaks.iter().all(|b| text.is_char_boundary(b.offset)));
        assert_eq!(breaks.last().unwrap().offset, text.len());
        assert_eq!(breaks.last().unwrap().kind, BreakKind::Mandatory);
    }
}

#[test]
fn long_context_runs_are_linear_and_ordered() {
    let text = format!(
        "{}{}{}{}",
        "a ".repeat(50_000),
        "🇫🇷".repeat(10_000),
        "1,234.56".repeat(10_000),
        "क्".repeat(20_000)
    );
    let breaks = line_breaks(&text);
    assert!(breaks.windows(2).all(|w| w[0].offset < w[1].offset));
    assert_eq!(breaks.last().unwrap().offset, text.len());
}
