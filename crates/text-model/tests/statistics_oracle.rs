// SPDX-License-Identifier: AGPL-3.0-or-later
//! Comparison with the counts written by an application (US-343).
//!
//! Dumps produced by `tools/extract-statistics-oracle.py` hold the body text
//! of a document and the statistics its producer stored in it. The committed
//! dumps in `tests/statistics-data` come from the LibreOffice ODT corpus of
//! `tests/odt-reference` (producer: LibreOffice, NOT Microsoft Word): the test
//! pins the measured differences, each explained in
//! `docs/us-343-statistics.md`.
//!
//! Word-saved documents are checked locally: set `ATELIER_STATS_ORACLE_DIR`
//! to a directory of dumps and the same comparison runs on them, without the
//! pinned expectations, with a 0.5 % tolerance on every counter.

use atelier_text_model::statistics::{CharClass, CountingRules, Counts, classify, count_paragraph};
use std::path::Path;

struct Dump {
    name: String,
    producer: String,
    expected: Counts,
    /// `(tag, text)`: B body, H header/footer, N note body.
    paragraphs: Vec<(char, String)>,
}

fn unescape(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('v') => out.push('\u{b}'),
            Some(o) => out.push(o),
            None => out.push('\\'),
        }
    }
    out
}

fn load(path: &Path) -> Dump {
    let text = std::fs::read_to_string(path).unwrap();
    let mut lines = text.lines();
    let producer = lines
        .next()
        .unwrap()
        .trim_start_matches("@producer ")
        .to_owned();
    let mut expected = Counts::default();
    for kv in lines
        .next()
        .unwrap()
        .trim_start_matches("@expected ")
        .split(' ')
    {
        let (k, v) = kv.split_once('=').unwrap();
        let v: usize = v.parse().unwrap_or(0);
        match k {
            "words" => expected.words = v,
            "chars" => expected.characters = v,
            "chars_spaces" => expected.characters_with_spaces = v,
            "paragraphs" => expected.paragraphs = v,
            _ => {}
        }
    }
    Dump {
        name: path.file_name().unwrap().to_string_lossy().into_owned(),
        producer,
        expected,
        paragraphs: lines
            .map(|l| (l.chars().next().unwrap(), unescape(&l[2..])))
            .collect(),
    }
}

/// The counting rules documented for Word, on the body only.
fn measure_word(dump: &Dump) -> Counts {
    let rules = CountingRules::default();
    dump.paragraphs
        .iter()
        .filter(|(tag, _)| *tag == 'B')
        .map(|(_, p)| count_paragraph(p, &rules))
        .sum()
}

/// Tokens made only of em dashes (U+2014): a word for Word, not for
/// LibreOffice. The only such token in the corpus; a lone "." or "-" is not
/// covered by the measurement.
fn em_dash_tokens(text: &str) -> usize {
    text.split(|c: char| classify(c) == CharClass::Separator)
        .filter(|t| !t.is_empty() && t.chars().all(|c| c == '\u{2014}'))
        .count()
}

/// Our counters with the scope and the one word rule of LibreOffice: body,
/// headers, footers and notes counted, em-dash tokens not words.
fn measure_libreoffice_profile(dump: &Dump) -> Counts {
    let rules = CountingRules::default();
    dump.paragraphs
        .iter()
        .map(|(_, p)| {
            let mut c = count_paragraph(p, &rules);
            c.words -= em_dash_tokens(p);
            c
        })
        .sum()
}

fn dumps(dir: &Path) -> Vec<Dump> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".stats.txt"))
        .collect();
    paths.sort();
    paths.iter().map(|p| load(p)).collect()
}

fn gap(ours: usize, theirs: usize) -> f64 {
    if theirs == 0 {
        if ours == 0 { 0.0 } else { 100.0 }
    } else {
        (ours as f64 - theirs as f64).abs() * 100.0 / theirs as f64
    }
}

fn tuple(c: Counts) -> [usize; 4] {
    [
        c.words,
        c.characters,
        c.characters_with_spaces,
        c.paragraphs,
    ]
}

fn libreoffice_corpus() -> Vec<Dump> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/statistics-data");
    let all = dumps(&dir);
    assert!(all.len() >= 14, "{} dumps", all.len());
    for d in &all {
        assert!(d.producer.starts_with("LibreOffice"), "{}", d.producer);
    }
    all
}

/// With LibreOffice's scope and its word rule, our counters are LibreOffice's,
/// except where LibreOffice counts generated list labels (`lists.odt`: 10
/// labels, 16 characters), which are not text of the document.
const LIST_LABELS: (&str, [usize; 4], [usize; 4]) =
    ("lists.odt.stats.txt", [19, 96, 104, 11], [29, 112, 120, 11]);

#[test]
fn libreoffice_profile_reproduces_libreoffice_counts() {
    let corpus = libreoffice_corpus();
    let mut differences = Vec::new();
    for d in &corpus {
        let ours = tuple(measure_libreoffice_profile(d));
        let theirs = tuple(d.expected);
        if ours != theirs {
            differences.push((d.name.as_str(), ours, theirs));
        }
    }
    assert_eq!(differences, vec![LIST_LABELS]);
}

/// Differences between the Word rules (body only, dashes are words) and
/// LibreOffice, as `[words, characters, with spaces, paragraphs]` ours minus
/// LibreOffice's, for the documents that differ.
const WORD_VS_LIBREOFFICE: &[(&str, [i64; 4])] = &[
    // LibreOffice counts the note body and the citation mark.
    ("fields-notes.odt.stats.txt", [-4, -16, -19, -1]),
    // LibreOffice counts the header and footer (7 words, 26 characters, 2
    // paragraphs) and ignores 112 isolated em dashes (a word for Word).
    ("header-footer.odt.stats.txt", [105, -26, -31, -2]),
    // LibreOffice counts the 10 generated list labels.
    ("lists.odt.stats.txt", [-10, -16, -16, 0]),
    // LibreOffice ignores isolated em dashes: 300, 8 and 1.
    ("multi-page.odt.stats.txt", [300, 0, 0, 0]),
    ("para-format.odt.stats.txt", [8, 0, 0, 0]),
    ("text-accents.odt.stats.txt", [1, 0, 0, 0]),
];

#[test]
fn word_profile_differences_with_libreoffice_are_the_documented_ones() {
    let mut report = String::new();
    let mut differences: Vec<(String, [i64; 4])> = Vec::new();
    for d in &libreoffice_corpus() {
        let (ours, theirs) = (tuple(measure_word(d)), tuple(d.expected));
        report += &format!("{:30} word-rules {ours:?} LibreOffice {theirs:?}\n", d.name);
        if ours != theirs {
            let delta = core::array::from_fn(|i| ours[i] as i64 - theirs[i] as i64);
            differences.push((d.name.clone(), delta));
        }
    }
    eprintln!("{report}");
    let expected: Vec<(String, [i64; 4])> = WORD_VS_LIBREOFFICE
        .iter()
        .map(|(n, d)| ((*n).to_owned(), *d))
        .collect();
    assert_eq!(differences, expected);
}

#[test]
fn local_oracle_directory_is_within_tolerance() {
    let Ok(dir) = std::env::var("ATELIER_STATS_ORACLE_DIR") else {
        eprintln!("ATELIER_STATS_ORACLE_DIR not set: local oracle check skipped");
        return;
    };
    let mut worst: f64 = 0.0;
    let mut failures = Vec::new();
    for d in dumps(Path::new(&dir)) {
        let m = measure_word(&d);
        let pairs = [
            ("words", m.words, d.expected.words),
            ("chars", m.characters, d.expected.characters),
            (
                "chars+spaces",
                m.characters_with_spaces,
                d.expected.characters_with_spaces,
            ),
            ("paragraphs", m.paragraphs, d.expected.paragraphs),
        ];
        for (what, ours, theirs) in pairs {
            let g = gap(ours, theirs);
            worst = worst.max(g);
            eprintln!(
                "{:32} [{}] {what}: ours {ours} theirs {theirs} ({g:.2} %)",
                d.name, d.producer
            );
            if g > 0.5 {
                failures.push(format!("{} {what}: {ours} vs {theirs}", d.name));
            }
        }
    }
    eprintln!("worst gap {worst:.2} %");
    assert!(failures.is_empty(), "{failures:#?}");
}
