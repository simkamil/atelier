// SPDX-License-Identifier: AGPL-3.0-or-later
//! Default Unicode 17.0.0 line break opportunities (UAX #14 revision 55).
//!
//! This computes legal opportunities only. Choosing a line for a particular width and
//! language-specific hyphenation are deliberately left to higher-level layout code.

/// Whether a line break is optional or required by a hard line break / end of text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BreakKind {
    /// A permitted line ending chosen by a higher-level line fitting algorithm.
    Allowed,
    /// A mandatory line ending (LB3–LB5), including the end of the input.
    Mandatory,
}

/// A line break opportunity at a UTF-8 byte offset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LineBreak {
    /// UTF-8 byte offset immediately after the character ending this line.
    pub offset: usize,
    /// Whether the opportunity is optional or mandatory.
    pub kind: BreakKind,
}

#[derive(Clone, Copy)]
struct Item {
    offset: usize,
    ch: char,
    class: &'static str,
    category: &'static str,
    eaw: &'static str,
    ep: bool,
    attached: bool,
}

fn is_hard(c: &str) -> bool {
    matches!(c, "BK" | "CR" | "LF" | "NL")
}
fn is_alpha(c: &str) -> bool {
    matches!(c, "AL" | "HL")
}
fn east_asian(item: &Item) -> bool {
    matches!(item.eaw, "F" | "W" | "H")
}
fn aksara(item: &Item) -> bool {
    matches!(item.class, "AK" | "AS") || item.ch == '\u{25CC}'
}

/// Default Unicode 17 line-break opportunities, in UTF-8 byte offsets.
/// Nonempty input has no initial opportunity and always ends with a mandatory one.
/// Empty input has the sole mandatory opportunity at zero. Time and memory are O(n).
pub fn line_breaks(text: &str) -> Vec<LineBreak> {
    use crate::properties::{
        east_asian_width, general_category, is_extended_pictographic, line_break_class,
    };
    let mut items = Vec::<Item>::with_capacity(text.chars().count());
    let mut bases = Vec::<Item>::with_capacity(items.capacity());
    let mut base_at = Vec::with_capacity(items.capacity());
    for (offset, ch) in text.char_indices() {
        let category = general_category(ch);
        let class = match line_break_class(ch) {
            // LB1
            "AI" | "SG" | "XX" => "AL",
            "SA" if matches!(category, "Mn" | "Mc") => "CM",
            "SA" => "AL",
            "CJ" => "NS",
            other => other,
        };
        let mark = matches!(class, "CM" | "ZWJ");
        let attached = mark
            && bases
                .last()
                .is_some_and(|base| !is_hard(base.class) && !matches!(base.class, "SP" | "ZW"));
        let item = if attached {
            // LB9: ignore this scalar in ALL subsequent rule contexts.
            Item {
                offset,
                ch,
                attached,
                ..*bases.last().unwrap()
            }
        } else if mark {
            // LB10 specifies all four properties of U+0041, not just AL.
            Item {
                offset,
                ch: 'A',
                class: "AL",
                category: "Lu",
                eaw: "Na",
                ep: false,
                attached,
            }
        } else {
            Item {
                offset,
                ch,
                class,
                category,
                eaw: east_asian_width(ch),
                ep: is_extended_pictographic(ch),
                attached,
            }
        };
        if !attached {
            bases.push(item);
        }
        // Keep the original scalar for LB8a, which precedes LB9/LB10.
        items.push(Item { ch, ..item });
        base_at.push(bases.len() - 1);
    }
    // Context summaries on the logical sequence (LB9 marks omitted), built once.
    let mut nonspace = Vec::with_capacity(bases.len());
    let mut numeric = vec![false; bases.len() + 1];
    let mut ri = vec![0usize; bases.len() + 1];
    let mut last_nonspace = None;
    for (i, item) in bases.iter().enumerate() {
        if item.class != "SP" {
            last_nonspace = Some(i);
        }
        nonspace.push(last_nonspace);
        numeric[i + 1] = item.class == "NU" || matches!(item.class, "SY" | "IS") && numeric[i];
        ri[i + 1] = if item.class == "RI" { ri[i] + 1 } else { 0 };
    }
    let mut out = Vec::new();
    for right in 1..items.len() {
        let left = right - 1;
        let a = items[left].class;
        let b = items[right].class;
        let li = base_at[left];
        let kind = if a == "CR" && b == "LF" {
            // LB5
            None
        } else if is_hard(a) {
            // LB4/LB5
            Some(BreakKind::Mandatory)
        } else if is_hard(b) || matches!(b, "SP" | "ZW") {
            // LB6/LB7
            None
        } else if nonspace[li].is_some_and(|i| bases[i].class == "ZW") {
            // LB8
            Some(BreakKind::Allowed)
        } else if items[left].ch == '\u{200D}' || items[right].attached {
            // LB8a/LB9
            None
        } else if permits(&bases, li, nonspace[li], &numeric, &ri) {
            Some(BreakKind::Allowed)
        } else {
            None
        };
        if let Some(kind) = kind {
            out.push(LineBreak {
                offset: items[right].offset,
                kind,
            });
        }
    }
    out.push(LineBreak {
        offset: text.len(),
        kind: BreakKind::Mandatory,
    }); // LB3, including empty input.
    out
}

// Early returns preserve the normative order and keep each numbered rule visible.
fn permits(
    items: &[Item],
    left: usize,
    nonspace: Option<usize>,
    numeric: &[bool],
    ri: &[usize],
) -> bool {
    let a = &items[left];
    let b = &items[left + 1];
    let ac = a.class;
    let bc = b.class;
    let before = left.checked_sub(1).map(|i| &items[i]);
    let after = items.get(left + 2);
    if ac == "WJ" || bc == "WJ" {
        return false;
    } // LB11
    if ac == "GL" {
        return false;
    } // LB12
    if bc == "GL" && !matches!(ac, "SP" | "BA" | "HY" | "HH") {
        return false;
    } // LB12a
    if matches!(bc, "CL" | "CP" | "EX" | "SY") {
        return false;
    } // LB13
    if nonspace.is_some_and(|i| items[i].class == "OP") {
        return false;
    } // LB14
    if nonspace.is_some_and(|q| {
        items[q].class == "QU"
            && items[q].category == "Pi"
            && q.checked_sub(1).is_none_or(|i| {
                matches!(
                    items[i].class,
                    "BK" | "CR" | "LF" | "NL" | "OP" | "QU" | "GL" | "SP" | "ZW"
                )
            })
    }) {
        return false;
    } // LB15a
    if bc == "QU"
        && b.category == "Pf"
        && after.is_none_or(|i| {
            matches!(
                i.class,
                "SP" | "GL"
                    | "WJ"
                    | "CL"
                    | "QU"
                    | "CP"
                    | "EX"
                    | "IS"
                    | "SY"
                    | "BK"
                    | "CR"
                    | "LF"
                    | "NL"
                    | "ZW"
            )
        })
    {
        return false;
    } // LB15b
    if ac == "SP" && bc == "IS" && after.is_some_and(|i| i.class == "NU") {
        return true;
    } // LB15c
    if bc == "IS" {
        return false;
    } // LB15d
    if bc == "NS" && nonspace.is_some_and(|i| matches!(items[i].class, "CL" | "CP")) {
        return false;
    } // LB16
    if bc == "B2" && nonspace.is_some_and(|i| items[i].class == "B2") {
        return false;
    } // LB17
    if ac == "SP" {
        return true;
    } // LB18
    if bc == "QU" && b.category != "Pi" || ac == "QU" && a.category != "Pf" {
        return false;
    } // LB19
    if bc == "QU" && (!east_asian(a) || after.is_none_or(|i| !east_asian(i)))
        || ac == "QU" && (!east_asian(b) || before.is_none_or(|i| !east_asian(i)))
    {
        return false;
    } // LB19a
    if ac == "CB" || bc == "CB" {
        return true;
    } // LB20
    if matches!(ac, "HY" | "HH")
        && is_alpha(bc)
        && before.is_none_or(|i| {
            matches!(
                i.class,
                "BK" | "CR" | "LF" | "NL" | "SP" | "ZW" | "CB" | "GL"
            )
        })
    {
        return false;
    } // LB20a
    if matches!(bc, "BA" | "HH" | "HY" | "NS") || ac == "BB" {
        return false;
    } // LB21
    if matches!(ac, "HY" | "HH") && before.is_some_and(|i| i.class == "HL") && bc != "HL" {
        return false;
    } // LB21a
    if ac == "SY" && bc == "HL" {
        return false;
    } // LB21b
    if bc == "IN" {
        return false;
    } // LB22
    if is_alpha(ac) && bc == "NU" || ac == "NU" && is_alpha(bc) {
        return false;
    } // LB23
    if ac == "PR" && matches!(bc, "ID" | "EB" | "EM")
        || matches!(ac, "ID" | "EB" | "EM") && bc == "PO"
    {
        return false;
    } // LB23a
    if matches!(ac, "PR" | "PO") && is_alpha(bc) || is_alpha(ac) && matches!(bc, "PR" | "PO") {
        return false;
    } // LB24
    let numeric_end = if matches!(ac, "CL" | "CP") {
        left
    } else {
        left + 1
    };
    let prefix_number = bc == "NU"
        || bc == "OP"
            && (after.is_some_and(|i| i.class == "NU")
                || after.is_some_and(|i| i.class == "IS")
                    && items.get(left + 3).is_some_and(|i| i.class == "NU"));
    if matches!(bc, "PO" | "PR") && numeric[numeric_end]
        || matches!(ac, "PO" | "PR") && prefix_number
        || matches!(ac, "HY" | "IS") && bc == "NU"
        || bc == "NU" && numeric[left + 1]
    {
        return false;
    } // LB25
    if ac == "JL" && matches!(bc, "JL" | "JV" | "H2" | "H3")
        || matches!(ac, "JV" | "H2") && matches!(bc, "JV" | "JT")
        || matches!(ac, "JT" | "H3") && bc == "JT"
    {
        return false;
    } // LB26
    let hangul = |c| matches!(c, "JL" | "JV" | "JT" | "H2" | "H3");
    if hangul(ac) && bc == "PO" || ac == "PR" && hangul(bc) {
        return false;
    } // LB27
    if is_alpha(ac) && is_alpha(bc) {
        return false;
    } // LB28
    if ac == "AP" && aksara(b)
        || aksara(a) && matches!(bc, "VF" | "VI")
        || ac == "VI" && before.is_some_and(aksara) && (bc == "AK" || b.ch == '\u{25CC}')
        || aksara(a) && aksara(b) && after.is_some_and(|i| i.class == "VF")
    {
        return false;
    } // LB28a
    if ac == "IS" && is_alpha(bc) {
        return false;
    } // LB29
    if (is_alpha(ac) || ac == "NU") && bc == "OP" && !east_asian(b)
        || ac == "CP" && (is_alpha(bc) || bc == "NU") && !east_asian(a)
    {
        return false;
    } // LB30
    if ac == "RI" && bc == "RI" && ri[left + 1] % 2 == 1 {
        return false;
    } // LB30a
    if bc == "EM" && (ac == "EB" || a.ep && a.category == "Cn") {
        return false;
    } // LB30b
    true // LB31
}
