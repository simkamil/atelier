// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_unicode::case::{
    caseless_eq, to_case_folded, to_lowercase, to_titlecase, to_uppercase,
};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

type Mapping = BTreeMap<u32, Vec<u32>>;

fn hex_seq(field: &str) -> Vec<u32> {
    field
        .split_whitespace()
        .map(|h| u32::from_str_radix(h, 16).unwrap())
        .collect()
}

fn text(seq: &[u32]) -> String {
    seq.iter().map(|&n| char::from_u32(n).unwrap()).collect()
}

struct Official {
    upper: Mapping,
    lower: Mapping,
    title: Mapping,
    fold: Mapping,
}

fn official() -> Official {
    let mut o = Official {
        upper: Mapping::new(),
        lower: Mapping::new(),
        title: Mapping::new(),
        fold: Mapping::new(),
    };
    // Simple mappings from UnicodeData.txt (fields 12, 13, 14).
    for line in include_str!("../../../data/ucd/17.0.0/UnicodeData.txt").lines() {
        let f: Vec<&str> = line.split(';').collect();
        let n = u32::from_str_radix(f[0], 16).unwrap();
        for (idx, map) in [(12, &mut o.upper), (13, &mut o.lower), (14, &mut o.title)] {
            if !f[idx].is_empty() {
                map.insert(n, hex_seq(f[idx]));
            }
        }
    }
    // Unconditional full mappings from SpecialCasing.txt override the simple ones.
    for raw in include_str!("../../../data/ucd/17.0.0/SpecialCasing.txt").lines() {
        let line = raw.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(';').map(str::trim).collect();
        // Fields: code; lower; title; upper; [condition;]. Conditional rows are skipped.
        if f.len() > 5 && !f[4].is_empty() {
            continue;
        }
        let n = u32::from_str_radix(f[0], 16).unwrap();
        o.lower.insert(n, hex_seq(f[1]));
        o.title.insert(n, hex_seq(f[2]));
        o.upper.insert(n, hex_seq(f[3]));
    }
    // Statuses C and F of CaseFolding.txt.
    for raw in include_str!("../../../data/ucd/17.0.0/CaseFolding.txt").lines() {
        let line = raw.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(';').map(str::trim).collect();
        if f[1] == "C" || f[1] == "F" {
            o.fold
                .insert(u32::from_str_radix(f[0], 16).unwrap(), hex_seq(f[2]));
        }
    }
    o
}

fn expected(map: &Mapping, n: u32) -> String {
    match map.get(&n) {
        Some(seq) => text(seq),
        None => char::from_u32(n).unwrap().to_string(),
    }
}

#[test]
fn every_scalar_matches_the_official_case_data() {
    let o = official();
    assert!(o.upper.len() > 1000 && o.lower.len() > 1000 && o.fold.len() > 1000);
    let mut checked = 0usize;
    let (mut all, mut all_upper, mut all_lower, mut all_fold) =
        (String::new(), String::new(), String::new(), String::new());
    for n in 0..=0x10FFFFu32 {
        let Some(c) = char::from_u32(n) else { continue };
        let s = c.to_string();
        assert_eq!(to_uppercase(&s), expected(&o.upper, n), "upper U+{n:04X}");
        assert_eq!(to_lowercase(&s), expected(&o.lower, n), "lower U+{n:04X}");
        assert_eq!(to_titlecase(&s), expected(&o.title, n), "title U+{n:04X}");
        assert_eq!(to_case_folded(&s), expected(&o.fold, n), "fold U+{n:04X}");
        all.push(c);
        all_upper.push_str(&expected(&o.upper, n));
        all_lower.push_str(&expected(&o.lower, n));
        all_fold.push_str(&expected(&o.fold, n));
        checked += 1;
    }
    assert_eq!(checked, 0x110000 - 0x800);
    // Whole-string conversion agrees with scalar-wise conversion, except for context rules:
    // the final-sigma rule only turns U+03A3 into U+03C2, so compare after undoing it.
    assert_eq!(to_uppercase(&all), all_upper);
    assert_eq!(to_case_folded(&all), all_fold);
    assert_eq!(
        to_lowercase(&all).replace('\u{03C2}', "\u{03C3}"),
        all_lower.replace('\u{03C2}', "\u{03C3}")
    );
}

#[test]
fn targeted_cases() {
    assert_eq!(to_uppercase("stra\u{00DF}e"), "STRASSE");
    assert_eq!(to_titlecase("\u{00DF}"), "Ss");
    assert_eq!(to_case_folded("stra\u{00DF}e"), "strasse");
    assert_eq!(to_uppercase("\u{0149}"), "\u{02BC}N");
    assert_eq!(to_case_folded("\u{0149}"), "\u{02BC}n");
    assert_eq!(to_titlecase("\u{0149}"), "\u{02BC}N");
    // Dz digraphs: upper, lower and title forms differ.
    assert_eq!(to_uppercase("\u{01C5}"), "\u{01C4}");
    assert_eq!(to_lowercase("\u{01C5}"), "\u{01C6}");
    assert_eq!(to_titlecase("\u{01C6}"), "\u{01C5}");
    assert_eq!(to_case_folded("\u{01C5}"), "\u{01C6}");
    // Dotted capital I lowercases to i + combining dot above, and folds likewise.
    assert_eq!(to_lowercase("\u{0130}"), "i\u{0307}");
    assert_eq!(to_case_folded("\u{0130}"), "i\u{0307}");
    assert_eq!(to_uppercase("i"), "I");
    // Final sigma.
    assert_eq!(to_lowercase("\u{0391}\u{03A3}"), "\u{03B1}\u{03C2}");
    assert_eq!(
        to_lowercase("\u{0391}\u{03A3}\u{0391}"),
        "\u{03B1}\u{03C3}\u{03B1}"
    );
    assert_eq!(to_lowercase("\u{03A3}"), "\u{03C3}");
    assert_eq!(to_lowercase("\u{0391}.\u{03A3}"), "\u{03B1}.\u{03C2}");
    assert_eq!(
        to_lowercase("\u{0391}\u{03A3}.\u{0391}"),
        "\u{03B1}\u{03C3}.\u{03B1}"
    );
    assert_eq!(to_lowercase("\u{0391}\u{03A3} "), "\u{03B1}\u{03C2} ");
    assert_eq!(
        to_lowercase("\u{0391}\u{03A3}\u{0301}"),
        "\u{03B1}\u{03C2}\u{0301}"
    );
    assert_eq!(to_lowercase("1\u{03A3}"), "1\u{03C3}");
    // Titlecase works per word.
    assert_eq!(to_titlecase("hello wORLD"), "Hello World");
    assert_eq!(to_titlecase("don't STOP"), "Don't Stop");
    assert_eq!(to_titlecase("\u{01C6}ungla"), "\u{01C5}ungla");
    assert_eq!(
        to_titlecase("\u{0391}\u{03A3} \u{0391}\u{03A3}"),
        "\u{0391}\u{03C2} \u{0391}\u{03C2}"
    );
    assert_eq!(to_titlecase("123abc"), "123Abc");
    assert_eq!(to_titlecase(""), "");
    // Caseless matching.
    assert!(caseless_eq("Stra\u{00DF}e", "STRASSE"));
    assert!(caseless_eq("\u{00C9}cole", "e\u{0301}COLE"));
    assert!(!caseless_eq("abc", "abd"));
}

#[test]
fn long_inputs_stay_linear() {
    let start = Instant::now();
    // Many sigmas separated by case-ignorable runs, and one long ignorable tail.
    let pattern = "\u{0391}'\u{03A3}'.\u{03A3}\u{0301}";
    let big: String = pattern.repeat(100_000);
    let lower = to_lowercase(&big);
    assert_eq!(lower.chars().count(), big.chars().count());
    let sigma_tail = format!("\u{0391}\u{03A3}{}", "'".repeat(500_000));
    assert!(to_lowercase(&sigma_tail).contains('\u{03C2}'));
    let mut many = String::from("\u{0391}");
    many.push_str(&"\u{03A3}".repeat(300_000));
    assert_eq!(to_lowercase(&many).chars().count(), 300_001);
    assert_eq!(
        to_titlecase(&"ab \u{00DF}".repeat(100_000)).chars().count(),
        5 * 100_000
    );
    assert_eq!(to_uppercase(&"\u{00DF}".repeat(200_000)).len(), 400_000);
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(20), "took {elapsed:?}");
}
