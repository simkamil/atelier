// SPDX-License-Identifier: AGPL-3.0-or-later
//! Folding, key construction and linear matching for one paragraph (US-340).
//!
//! A paragraph is cut into extended grapheme clusters. Each cluster is turned
//! into a *key* (NFD, optional full case folding, optional removal of
//! combining marks, optional no-break-space unification). The keys are
//! concatenated into one byte string and a table `key position -> original
//! cluster` is kept for every cluster whose key is not empty. The query is
//! transformed the same way and searched with Knuth-Morris-Pratt; a raw hit is
//! kept only if it starts and ends exactly on the keys of two clusters, which
//! is what guarantees that a reported range never splits a grapheme.

use atelier_unicode::case::to_case_folded;
use atelier_unicode::normalization::{nfc, nfd};
use atelier_unicode::properties::general_category;
use atelier_unicode::segmentation::{BoundaryIndex, grapheme_boundaries, word_boundaries};
use core::ops::Range;

const NO_BREAK_SPACE: char = '\u{a0}';
const NARROW_NO_BREAK_SPACE: char = '\u{202f}';

/// The options that change how text is compared.
#[derive(Clone, Copy, Debug)]
pub(super) struct Flags {
    pub match_case: bool,
    pub whole_word: bool,
    pub ignore_accents: bool,
    pub match_nbsp: bool,
}

/// Appends the key of one grapheme cluster.
fn push_key(out: &mut Vec<u8>, cluster: &str, flags: Flags) {
    if cluster.is_ascii() {
        out.extend(cluster.bytes().map(|b| {
            if flags.match_case {
                b
            } else {
                b.to_ascii_lowercase()
            }
        }));
        return;
    }
    let mut s = nfd(cluster);
    if !flags.match_case {
        s = nfd(&to_case_folded(&s));
    }
    for c in s.chars() {
        if flags.ignore_accents && general_category(c) == "Mn" {
            continue;
        }
        let c = if !flags.match_nbsp && (c == NO_BREAK_SPACE || c == NARROW_NO_BREAK_SPACE) {
            ' '
        } else {
            c
        };
        let mut buf = [0u8; 4];
        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    }
}

/// A compiled query.
pub(super) struct Matcher {
    key: Vec<u8>,
    prefix: Vec<usize>,
    flags: Flags,
}

impl Matcher {
    pub fn new(query: &str, flags: Flags) -> Self {
        let query = nfc(query);
        let mut key = Vec::new();
        let index = grapheme_boundaries(&query);
        for range in index.segments() {
            push_key(&mut key, &query[range], flags);
        }
        let mut prefix = vec![0usize; key.len()];
        let mut k = 0;
        for i in 1..key.len() {
            while k > 0 && key[i] != key[k] {
                k = prefix[k - 1];
            }
            if key[i] == key[k] {
                k += 1;
            }
            prefix[i] = k;
        }
        Self { key, prefix, flags }
    }

    /// A query without any comparable content never matches.
    pub fn is_empty(&self) -> bool {
        self.key.is_empty()
    }

    pub fn analyze<'a>(&self, text: &'a str) -> Analysis<'a> {
        let graphemes = grapheme_boundaries(text);
        let mut key = Vec::with_capacity(text.len());
        let mut units = Vec::new();
        for range in graphemes.segments() {
            let before = key.len();
            push_key(&mut key, &text[range.clone()], self.flags);
            if key.len() > before {
                units.push(Unit {
                    key_start: before,
                    start: range.start,
                    end: range.end,
                });
            }
        }
        Analysis {
            key,
            units,
            graphemes,
        }
    }

    /// Non-overlapping occurrences, leftmost first, as byte ranges of `text`.
    pub fn find(&self, text: &str, analysis: &Analysis<'_>) -> Vec<Range<usize>> {
        let m = self.key.len();
        let mut out = Vec::new();
        if m == 0 || analysis.key.len() < m {
            return out;
        }
        let units = &analysis.units;
        let key_end = |u: usize| units.get(u + 1).map_or(analysis.key.len(), |n| n.key_start);
        let (mut first, mut last) = (0usize, 0usize);
        let mut words: Option<Vec<bool>> = None;
        let mut resume = 0usize;
        let mut k = 0usize;
        for (i, &b) in analysis.key.iter().enumerate() {
            while k > 0 && (k == m || b != self.key[k]) {
                k = self.prefix[k - 1];
            }
            if b == self.key[k] {
                k += 1;
            }
            if k < m {
                continue;
            }
            let (s, e) = (i + 1 - m, i + 1);
            if s < resume {
                continue;
            }
            while first < units.len() && units[first].key_start < s {
                first += 1;
            }
            if first >= units.len() || units[first].key_start != s {
                continue;
            }
            last = last.max(first);
            while last < units.len() && key_end(last) < e {
                last += 1;
            }
            if last >= units.len() || key_end(last) != e {
                continue;
            }
            let range = units[first].start..units[last].end;
            if self.flags.whole_word {
                let bits = words.get_or_insert_with(|| {
                    let mut bits = vec![false; text.len() + 1];
                    for &b in word_boundaries(text).boundaries() {
                        bits[b] = true;
                    }
                    bits
                });
                if !(bits[range.start] && bits[range.end]) {
                    continue;
                }
            }
            resume = e;
            out.push(range);
        }
        out
    }
}

/// One grapheme cluster with a non-empty key.
struct Unit {
    key_start: usize,
    start: usize,
    end: usize,
}

/// The folded form of one paragraph.
pub(super) struct Analysis<'a> {
    key: Vec<u8>,
    units: Vec<Unit>,
    graphemes: BoundaryIndex<'a>,
}

impl Analysis<'_> {
    /// Grapheme boundaries of the original text, sorted, with `0` and the length.
    pub fn boundaries(&self) -> &[usize] {
        self.graphemes.boundaries()
    }
}
