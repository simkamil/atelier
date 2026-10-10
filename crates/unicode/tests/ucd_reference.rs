// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_unicode::properties as p;
use std::fmt::{self, Write};
use std::fs;
use std::path::PathBuf;

const OFFSET: u64 = 14_695_981_039_346_656_037;
const PRIME: u64 = 1_099_511_628_211;

struct Hash(u64);
impl Hash {
    fn new() -> Self {
        Self(OFFSET)
    }
    fn bytes(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ u64::from(*b)).wrapping_mul(PRIME);
        }
    }
    fn field(&mut self, value: impl fmt::Display) {
        writeln!(self, "{value}").unwrap();
    }
    fn sequence(&mut self, seq: Option<&[u32]>) {
        if let Some(seq) = seq {
            for (i, cp) in seq.iter().enumerate() {
                if i != 0 {
                    self.bytes(b",");
                }
                write!(self, "{cp:X}").unwrap();
            }
        } else {
            self.bytes(b"~");
        }
        self.bytes(b"\n");
    }
}
impl Write for Hash {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.bytes(s.as_bytes());
        Ok(())
    }
}

fn fingerprint_fields() -> Vec<(&'static str, Hash)> {
    let labels = [
        "general_category",
        "ccc",
        "line_break",
        "grapheme_break",
        "word_break",
        "indic_conjunct_break",
        "emoji",
        "emoji_presentation",
        "emoji_modifier",
        "emoji_modifier_base",
        "emoji_component",
        "extended_pictographic",
        "cased",
        "case_ignorable",
        "white_space",
        "name",
        "simple_uppercase",
        "simple_lowercase",
        "simple_titlecase",
        "full_uppercase",
        "full_lowercase",
        "full_titlecase",
        "case_fold",
        "simple_case_fold",
        "turkic_case_fold",
        "case_fold_turkic",
        "decomposition",
        "composition_exclusion",
        "composition",
    ];
    labels
        .into_iter()
        .map(|label| (label, Hash::new()))
        .collect()
}

fn fingerprint_scalar(n: u32, hashes: &mut [(&'static str, Hash)]) {
    let c = char::from_u32(n).unwrap();
    let mut i = 0;
    hashes[i].1.field(p::general_category(c));
    i += 1;
    hashes[i].1.field(p::canonical_combining_class(c));
    i += 1;
    hashes[i].1.field(p::line_break_class(c));
    i += 1;
    hashes[i].1.field(p::grapheme_cluster_break_property(c));
    i += 1;
    hashes[i].1.field(p::word_break_property(c));
    i += 1;
    hashes[i].1.field(p::indic_conjunct_break_property(c));
    i += 1;
    for yes in [
        p::is_emoji(c),
        p::is_emoji_presentation(c),
        p::is_emoji_modifier(c),
        p::is_emoji_modifier_base(c),
        p::is_emoji_component(c),
        p::is_extended_pictographic(c),
        p::is_cased(c),
        p::is_case_ignorable(c),
        p::is_white_space(c),
    ] {
        hashes[i].1.field(if yes { "Y" } else { "N" });
        i += 1;
    }
    if let Some(name) = p::name(c) {
        hashes[i].1.field(name);
    } else {
        hashes[i].1.field("~");
    }
    i += 1;
    for seq in [
        p::simple_uppercase(c),
        p::simple_lowercase(c),
        p::simple_titlecase(c),
        p::full_uppercase(c),
        p::full_lowercase(c),
        p::full_titlecase(c),
        p::case_fold(c),
        p::simple_case_fold(c),
        p::turkic_case_fold(c),
        p::case_fold_turkic(c),
    ] {
        hashes[i].1.sequence(seq);
        i += 1;
    }
    if let Some((tag, seq)) = p::decomposition(c) {
        write!(hashes[i].1, "{}:", tag.unwrap_or("")).unwrap();
        for (j, cp) in seq.iter().enumerate() {
            if j != 0 {
                hashes[i].1.bytes(b",");
            }
            write!(hashes[i].1, "{cp:X}").unwrap();
        }
        hashes[i].1.bytes(b"\n");
    } else {
        hashes[i].1.field("~");
    }
    i += 1;
    hashes[i].1.field(if p::is_composition_exclusion(c) {
        "Y"
    } else {
        "N"
    });
    i += 1;
    let composition = p::decomposition(c)
        .filter(|(tag, seq)| tag.is_none() && seq.len() == 2)
        .and_then(|(_, seq)| {
            p::canonical_composition(char::from_u32(seq[0])?, char::from_u32(seq[1])?)
        });
    if let Some(cp) = composition {
        writeln!(hashes[i].1, "{:X}", cp as u32).unwrap();
    } else {
        hashes[i].1.field("~");
    }
}

#[test]
fn exhaustive_unicode_17_reference_fingerprints() {
    let mut hashes = fingerprint_fields();
    for n in 0..=0x10_FFFF {
        if char::from_u32(n).is_some() {
            fingerprint_scalar(n, &mut hashes);
        }
    }
    let mut conditional = Hash::new();
    for rule in p::conditional_casing_rules() {
        write!(conditional, "{:X}\t{}\t", rule.code_point, rule.condition).unwrap();
        for (idx, seq) in [rule.lower, rule.title, rule.upper].into_iter().enumerate() {
            if idx != 0 {
                conditional.bytes(b"\t");
            }
            for (i, cp) in seq.iter().enumerate() {
                if i != 0 {
                    conditional.bytes(b",");
                }
                write!(conditional, "{cp:X}").unwrap();
            }
        }
        conditional.bytes(b"\n");
    }
    hashes.push(("conditional", conditional));
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/unicode-reference/checksums.tsv");
    let text = fs::read_to_string(fixture).expect("reference checksum fixture");
    let expected: Vec<_> = text
        .lines()
        .map(|line| {
            let (label, sum) = line.split_once('\t').expect("label and checksum");
            (
                label.to_owned(),
                u64::from_str_radix(sum, 16).expect("hex checksum"),
            )
        })
        .collect();
    assert_eq!(expected.len(), hashes.len(), "reference field count");
    for ((label, hash), (expected_label, expected_hash)) in hashes.iter().zip(expected) {
        assert_eq!(*label, expected_label, "reference field order");
        assert_eq!(hash.0, expected_hash, "fingerprint for {label}");
    }
}
