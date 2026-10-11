// SPDX-License-Identifier: AGPL-3.0-or-later
//! Language-independent Unicode 17.0.0 case conversion and case folding on strings.
//!
//! All functions accept any valid UTF-8 string, never panic and run in time linear in the input
//! length. Full (multi-scalar) mappings such as `ß` -> `SS` are applied, together with the
//! language-independent conditional `Final_Sigma` rule of `SpecialCasing.txt`. Language-dependent
//! rules (`tr`, `az`, `lt`) are not applied.

use crate::normalization::nfd;
use crate::properties::{
    case_fold as fold_mapping, full_lowercase, full_titlecase, full_uppercase, is_case_ignorable,
    is_cased,
};
use crate::segmentation::word_boundaries;

const CAPITAL_SIGMA: char = '\u{03A3}';
const FINAL_SIGMA: char = '\u{03C2}';

fn push_mapping(out: &mut String, c: char, mapping: Option<&'static [u32]>) {
    match mapping {
        Some(seq) => out.extend(seq.iter().filter_map(|&n| char::from_u32(n))),
        None => out.push(c),
    }
}

/// `Final_Sigma` condition for the scalar starting at byte offset `at` of `text`.
fn is_final_sigma(text: &str, at: usize) -> bool {
    let before = text[..at]
        .chars()
        .rev()
        .find(|&c| !is_case_ignorable(c))
        .is_some_and(is_cased);
    if !before {
        return false;
    }
    let after = text[at + CAPITAL_SIGMA.len_utf8()..]
        .chars()
        .find(|&c| !is_case_ignorable(c))
        .is_some_and(is_cased);
    !after
}

fn push_lower(out: &mut String, text: &str, at: usize, c: char) {
    if c == CAPITAL_SIGMA && is_final_sigma(text, at) {
        out.push(FINAL_SIGMA);
    } else {
        push_mapping(out, c, full_lowercase(c));
    }
}

/// Full uppercase conversion (`ß` -> `SS`, `ŉ` -> `ʼN`).
pub fn to_uppercase(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        push_mapping(&mut out, c, full_uppercase(c));
    }
    out
}

/// Full lowercase conversion, including the final-sigma rule (`ΑΣ` -> `ας`).
pub fn to_lowercase(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (at, c) in text.char_indices() {
        push_lower(&mut out, text, at, c);
    }
    out
}

/// Titlecase conversion: in each UAX #29 word, the first cased scalar takes its full titlecase
/// mapping (`ǆ` -> `ǅ`, `ß` -> `Ss`) and the following scalars are lowercased.
pub fn to_titlecase(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let words = word_boundaries(text);
    for range in words.segments() {
        let mut seen_cased = false;
        for (offset, c) in text[range.clone()].char_indices() {
            if seen_cased {
                push_lower(&mut out, text, range.start + offset, c);
            } else if is_cased(c) {
                seen_cased = true;
                push_mapping(&mut out, c, full_titlecase(c));
            } else {
                out.push(c);
            }
        }
    }
    out
}

/// Default full case folding (`CaseFolding.txt` statuses C and F), for caseless matching.
pub fn to_case_folded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        push_mapping(&mut out, c, fold_mapping(c));
    }
    out
}

/// Canonical caseless matching (Unicode D145): `NFD(fold(NFD(x)))` is equal for both strings.
pub fn caseless_eq(a: &str, b: &str) -> bool {
    nfd(&to_case_folded(&nfd(a))) == nfd(&to_case_folded(&nfd(b)))
}
