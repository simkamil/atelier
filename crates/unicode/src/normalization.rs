// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unicode 17.0.0 canonical normalization forms NFD and NFC.
//!
//! Inputs are any valid UTF-8 strings; functions never panic. Decomposition, canonical ordering
//! (counting sort per long run of non-starters, so no quadratic behaviour on long runs of
//! combining marks) and composition each run in linear time. Hangul syllables are algorithmic.

use crate::properties::{canonical_combining_class, canonical_composition, decomposition};

const S_BASE: u32 = 0xAC00;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const L_COUNT: u32 = 19;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = V_COUNT * T_COUNT;
const S_COUNT: u32 = L_COUNT * N_COUNT;

/// Scalars below this value have no decomposition and are starters (U+00C0 is the first
/// canonical decomposition).
const FIRST_SPECIAL: u32 = 0x00C0;
/// Scalars below this value are NFC_Quick_Check=Yes starters (U+0300 is the first combining mark).
const FIRST_NFC_NOT_YES: u32 = 0x0300;

fn ccc(c: char) -> u8 {
    if (c as u32) < FIRST_NFC_NOT_YES {
        0
    } else {
        canonical_combining_class(c)
    }
}

fn is_hangul_syllable(c: char) -> bool {
    (S_BASE..S_BASE + S_COUNT).contains(&(c as u32))
}

fn push_char(out: &mut Vec<char>, n: u32) {
    if let Some(c) = char::from_u32(n) {
        out.push(c);
    }
}

/// Appends the full canonical decomposition of `c`.
fn decompose_into(c: char, out: &mut Vec<char>) {
    let n = c as u32;
    if n < FIRST_SPECIAL {
        out.push(c);
    } else if is_hangul_syllable(c) {
        let s = n - S_BASE;
        push_char(out, L_BASE + s / N_COUNT);
        push_char(out, V_BASE + (s % N_COUNT) / T_COUNT);
        if s % T_COUNT != 0 {
            push_char(out, T_BASE + s % T_COUNT);
        }
    } else if let Some((None, sequence)) = decomposition(c) {
        // Mappings are short and nesting is shallow, so recursion depth is bounded.
        for d in sequence.iter().filter_map(|&m| char::from_u32(m)) {
            decompose_into(d, out);
        }
    } else {
        out.push(c);
    }
}

/// Canonical ordering of one run of non-starters (stable by combining class).
fn order_run(run: &mut [char]) {
    if run.windows(2).all(|w| ccc(w[0]) <= ccc(w[1])) {
        return;
    }
    if run.len() <= 32 {
        for i in 1..run.len() {
            let mut j = i;
            while j > 0 && ccc(run[j - 1]) > ccc(run[j]) {
                run.swap(j - 1, j);
                j -= 1;
            }
        }
        return;
    }
    let mut starts = [0usize; 257];
    for &c in run.iter() {
        starts[ccc(c) as usize + 1] += 1;
    }
    for i in 1..257 {
        starts[i] += starts[i - 1];
    }
    let mut sorted = vec!['\0'; run.len()];
    for &c in run.iter() {
        let slot = &mut starts[ccc(c) as usize];
        sorted[*slot] = c;
        *slot += 1;
    }
    run.copy_from_slice(&sorted);
}

fn decompose_ordered(text: &str) -> Vec<char> {
    let mut out = Vec::with_capacity(text.len());
    let mut run_start = 0;
    for c in text.chars() {
        let before = out.len();
        decompose_into(c, &mut out);
        // A starter closes the pending run of non-starters.
        if ccc(out[before]) == 0 {
            order_run(&mut out[run_start..before]);
            run_start = before;
        }
    }
    let end = out.len();
    order_run(&mut out[run_start..end]);
    out
}

/// Hangul or table composition of a starter and a following scalar.
fn compose_pair(first: char, second: char) -> Option<char> {
    let (a, b) = (first as u32, second as u32);
    if (L_BASE..L_BASE + L_COUNT).contains(&a) && (V_BASE..V_BASE + V_COUNT).contains(&b) {
        return char::from_u32(S_BASE + ((a - L_BASE) * V_COUNT + (b - V_BASE)) * T_COUNT);
    }
    if is_hangul_syllable(first)
        && (a - S_BASE) % T_COUNT == 0
        && (T_BASE + 1..T_BASE + T_COUNT).contains(&b)
    {
        return char::from_u32(a + (b - T_BASE));
    }
    canonical_composition(first, second)
}

fn compose(decomposed: &[char]) -> String {
    let mut out: Vec<char> = Vec::with_capacity(decomposed.len());
    let mut starter: Option<usize> = None;
    // Combining class of the last scalar kept after the starter, if any.
    let mut last: Option<u8> = None;
    for &c in decomposed {
        let class = ccc(c);
        if let Some(s) = starter {
            let blocked = last.is_some_and(|l| l >= class);
            if !blocked && let Some(composite) = compose_pair(out[s], c) {
                out[s] = composite;
                continue;
            }
        }
        if class == 0 {
            starter = Some(out.len());
            last = None;
        } else {
            last = Some(class);
        }
        out.push(c);
    }
    out.into_iter().collect()
}

/// Canonical decomposition (Normalization Form D).
pub fn nfd(text: &str) -> String {
    if text.chars().all(|c| (c as u32) < FIRST_SPECIAL) {
        return text.to_owned();
    }
    decompose_ordered(text).into_iter().collect()
}

/// Canonical decomposition followed by canonical composition (Normalization Form C).
pub fn nfc(text: &str) -> String {
    if text.chars().all(|c| (c as u32) < FIRST_NFC_NOT_YES) {
        return text.to_owned();
    }
    compose(&decompose_ordered(text))
}

/// Exact test that `text` is already in NFD; allocation-free.
pub fn is_nfd(text: &str) -> bool {
    let mut previous = 0u8;
    for c in text.chars() {
        if (c as u32) < FIRST_SPECIAL {
            previous = 0;
            continue;
        }
        let class = ccc(c);
        if (class != 0 && previous > class)
            || is_hangul_syllable(c)
            || matches!(decomposition(c), Some((None, _)))
        {
            return false;
        }
        previous = class;
    }
    true
}

/// Exact test that `text` is already in NFC. Strings below U+0300 are answered without
/// allocating; others are compared with their normalized form.
pub fn is_nfc(text: &str) -> bool {
    text.chars().all(|c| (c as u32) < FIRST_NFC_NOT_YES) || nfc(text) == text
}
