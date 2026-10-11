// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_unicode::normalization::{is_nfc, is_nfd, nfc, nfd};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

fn decode(field: &str) -> String {
    field
        .split_whitespace()
        .map(|h| char::from_u32(u32::from_str_radix(h, 16).unwrap()).unwrap())
        .collect()
}

#[test]
fn normalization_test_txt_nfc_and_nfd_are_fully_conformant() {
    let source = include_str!("../../../data/ucd/17.0.0/NormalizationTest.txt");
    let (mut lines, mut checks) = (0usize, 0usize);
    let mut part = 0u32;
    let mut listed = BTreeSet::new();
    for (number, raw) in source.lines().enumerate() {
        if let Some(rest) = raw.strip_prefix("@Part") {
            part = rest[..1].parse().unwrap();
            continue;
        }
        let line = raw.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<String> = line.split(';').take(5).map(decode).collect();
        assert_eq!(f.len(), 5, "line {}", number + 1);
        if part == 1 {
            let mut chars = f[0].chars();
            if let (Some(c), None) = (chars.next(), chars.next()) {
                listed.insert(c);
            }
        }
        lines += 1;
        // NFC: c2 == NFC(c1) == NFC(c2) == NFC(c3); c4 == NFC(c4) == NFC(c5).
        for (src, want) in [(0, 1), (1, 1), (2, 1), (3, 3), (4, 3)] {
            assert_eq!(
                nfc(&f[src]),
                f[want],
                "NFC line {} col {}",
                number + 1,
                src + 1
            );
            checks += 1;
        }
        // NFD: c3 == NFD(c1) == NFD(c2) == NFD(c3); c5 == NFD(c4) == NFD(c5).
        for (src, want) in [(0, 2), (1, 2), (2, 2), (3, 4), (4, 4)] {
            assert_eq!(
                nfd(&f[src]),
                f[want],
                "NFD line {} col {}",
                number + 1,
                src + 1
            );
            checks += 1;
        }
        // The quick predicates agree with the transformation.
        for s in &f {
            assert_eq!(is_nfc(s), nfc(s) == *s);
            assert_eq!(is_nfd(s), nfd(s) == *s);
        }
    }
    assert!(lines > 20_000, "only {lines} test lines parsed");
    assert!(listed.len() > 10_000);
    // Every scalar not listed in Part 1 is invariant under NFC and NFD.
    let mut invariants = 0usize;
    for n in 0..=0x10FFFFu32 {
        let Some(c) = char::from_u32(n) else { continue };
        if listed.contains(&c) {
            continue;
        }
        let s = c.to_string();
        assert_eq!(nfc(&s), s, "U+{n:04X} NFC");
        assert_eq!(nfd(&s), s, "U+{n:04X} NFD");
        assert!(is_nfc(&s) && is_nfd(&s));
        invariants += 1;
    }
    println!(
        "NormalizationTest: {lines} lines, {checks} NFC/NFD checks, {invariants} invariant scalars"
    );
}

#[test]
fn predicates_are_exact_on_every_scalar() {
    for n in 0..=0x10FFFFu32 {
        let Some(c) = char::from_u32(n) else { continue };
        let s = c.to_string();
        assert_eq!(is_nfd(&s), nfd(&s) == s, "U+{n:04X}");
        assert_eq!(is_nfc(&s), nfc(&s) == s, "U+{n:04X}");
    }
}

#[test]
fn targeted_examples() {
    assert_eq!(nfd("\u{00E9}"), "e\u{0301}");
    assert_eq!(nfc("e\u{0301}"), "\u{00E9}");
    // Canonical reordering: dot below (220) before acute (230).
    assert_eq!(nfd("a\u{0301}\u{0323}"), "a\u{0323}\u{0301}");
    assert_eq!(nfc("a\u{0301}\u{0323}"), "\u{1EA1}\u{0301}");
    // Hangul is algorithmic.
    assert_eq!(nfd("\u{AC01}"), "\u{1100}\u{1161}\u{11A8}");
    assert_eq!(nfc("\u{1100}\u{1161}\u{11A8}"), "\u{AC01}");
    assert_eq!(nfc("\u{AC00}\u{11A8}"), "\u{AC01}");
    // Composition exclusions and singletons do not recompose.
    assert_eq!(nfc("\u{0958}"), "\u{0915}\u{093C}");
    assert_eq!(nfc("\u{212B}"), "\u{00C5}");
    // Compatibility characters are untouched by canonical forms.
    assert_eq!(nfc("\u{FB01}"), "\u{FB01}");
    assert!(is_nfc("") && is_nfd("") && is_nfc("abc") && is_nfd("abc"));
    assert!(!is_nfc("e\u{0301}") && is_nfd("e\u{0301}"));
    assert!(!is_nfd("\u{00E9}") && is_nfc("\u{00E9}"));
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

#[test]
fn random_strings_never_panic_and_satisfy_normalization_laws() {
    let pool: Vec<char> = "aeAÉ\u{00C5}\u{212B}\u{0301}\u{0300}\u{0323}\u{0316}\u{0327}\u{0345}\u{093C}\u{0915}\u{0958}\u{1100}\u{1161}\u{11A8}\u{AC00}\u{AC01}\u{FB01}\u{1D15E}\u{1D157}\u{1D165}\u{0000}\u{10FFFF}ß"
        .chars()
        .collect();
    let mut rng = Lcg(0x5EED);
    for _ in 0..5000 {
        let len = (rng.next() % 24) as usize;
        let text: String = (0..len)
            .map(|_| pool[(rng.next() as usize) % pool.len()])
            .collect();
        let (c, d) = (nfc(&text), nfd(&text));
        assert_eq!(nfc(&c), c);
        assert_eq!(nfd(&d), d);
        assert_eq!(nfd(&c), d);
        assert_eq!(nfc(&d), c);
        assert!(is_nfc(&c) && is_nfd(&d));
    }
}

#[test]
fn long_runs_of_combining_marks_are_not_quadratic() {
    // Marks of descending combining class force a full reorder of one huge run.
    let marks = [
        '\u{0345}', '\u{0301}', '\u{0316}', '\u{0327}', '\u{0300}', '\u{0323}',
    ];
    let n = 600_000;
    let mut text = String::from("a");
    text.extend((0..n).map(|i| marks[i % marks.len()]));
    let start = Instant::now();
    let d = nfd(&text);
    let c = nfc(&text);
    assert!(is_nfd(&d));
    assert_eq!(d.chars().count(), n + 1);
    assert_eq!(nfd(&c), d);
    // Same mark repeated, plus alternation of starters and marks.
    let same: String = std::iter::once('e')
        .chain(std::iter::repeat_n('\u{0301}', n))
        .collect();
    assert_eq!(nfc(&same).chars().count(), n);
    let alternating: String = (0..n).flat_map(|_| ['e', '\u{0301}']).collect();
    assert_eq!(nfc(&alternating).chars().count(), n);
    assert_eq!(nfd(&alternating).chars().count(), 2 * n);
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(20), "took {elapsed:?}");
}
