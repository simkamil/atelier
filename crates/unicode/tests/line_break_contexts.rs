// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_unicode::line_break::line_breaks;
use atelier_unicode::properties::line_break_class;

#[test]
fn long_space_numeric_and_regional_runs_have_exact_opportunities() {
    let text = "a ".repeat(50_000);
    assert_eq!(
        line_breaks(&text)
            .into_iter()
            .map(|b| b.offset)
            .collect::<Vec<_>>(),
        (1..=50_000).map(|i| i * 2).collect::<Vec<_>>()
    );
    let text = "🇫🇷".repeat(10_000);
    assert_eq!(
        line_breaks(&text)
            .into_iter()
            .map(|b| b.offset)
            .collect::<Vec<_>>(),
        (1..=10_000).map(|i| i * 8).collect::<Vec<_>>()
    );
    for text in [
        "1,234.56".repeat(10_000),
        "\u{11013}\u{0308}\u{3099}\u{11046}".repeat(10_000),
    ] {
        assert_eq!(
            line_breaks(&text)
                .into_iter()
                .map(|b| b.offset)
                .collect::<Vec<_>>(),
            [text.len()]
        );
    }
}

#[test]
fn official_boundaries_survive_lb9_marks_in_all_rule_contexts() {
    let mut cases = 0;
    for (line_number, line) in include_str!("../../../data/ucd/17.0.0/auxiliary/LineBreakTest.txt")
        .lines()
        .enumerate()
    {
        let body = line.split('#').next().unwrap().trim();
        if body.is_empty() {
            continue;
        }
        let mut text = String::new();
        let mut expected = Vec::new();
        for token in body.split_whitespace() {
            match token {
                "÷" => expected.push(text.len()),
                "×" => (),
                hex => {
                    let ch = char::from_u32(u32::from_str_radix(hex, 16).unwrap()).unwrap();
                    text.push(ch);
                    // UAX14 LB9 ignores these CM in subsequent rules. Use marks
                    // with different EAW properties to expose raw-scalar lookups.
                    // Keep literal ZWJ adjacent to its successor for earlier LB8a.
                    if !matches!(
                        line_break_class(ch),
                        "BK" | "CR" | "LF" | "NL" | "SP" | "ZW" | "ZWJ"
                    ) {
                        text.push('\u{0308}');
                        text.push('\u{3099}');
                    }
                }
            }
        }
        let actual: Vec<_> = line_breaks(&text).into_iter().map(|b| b.offset).collect();
        assert_eq!(
            actual,
            expected,
            "LB9 context derived from official line {}",
            line_number + 1
        );
        cases += 1;
    }
    assert_eq!(cases, 19_338);
}
