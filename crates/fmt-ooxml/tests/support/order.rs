// SPDX-License-Identifier: AGPL-3.0-or-later
//! Schema order of the property children of a written part (US-307): the
//! most frequent reason for Word to refuse a file.
#![allow(dead_code)]

use super::{C, W};
use atelier_fmt_ooxml::{para_mark_rpr_rank, ppr_rank, rpr_rank, sect_pr_rank};

const TBL_PR: &[&str] = &[
    "tblStyle",
    "tblpPr",
    "tblOverlap",
    "bidiVisual",
    "tblStyleRowBandSize",
    "tblStyleColBandSize",
    "tblW",
    "jc",
    "tblCellSpacing",
    "tblInd",
    "tblBorders",
    "shd",
    "tblLayout",
    "tblCellMar",
    "tblLook",
    "tblCaption",
    "tblDescription",
    "tblPrChange",
];
const TC_PR: &[&str] = &[
    "cnfStyle",
    "tcW",
    "gridSpan",
    "hMerge",
    "vMerge",
    "tcBorders",
    "shd",
    "noWrap",
    "tcMar",
    "textDirection",
    "tcFitText",
    "vAlign",
    "hideMark",
    "headers",
    "cellIns",
    "cellDel",
    "cellMerge",
    "tcPrChange",
];
/// `w:trPr` is a choice in the schema (any order); listed for completeness.
const TR_PR_NOTE: &str = "trPr children may come in any order";

type Ranked = (fn(&str) -> usize, usize);

fn local(name: &str) -> &str {
    name.rsplit('}').next().unwrap_or(name)
}

fn is_w(name: &str) -> bool {
    name.starts_with(&format!("{{{W}}}"))
}

fn walk(c: &C, parent: &str, out: &mut Vec<String>) {
    let C::E(name, _, kids) = c else { return };
    let me = local(name);
    let w_kids: Vec<&str> = kids
        .iter()
        .filter_map(|k| match k {
            C::E(n, _, _) if is_w(n) => Some(local(n)),
            _ => None,
        })
        .collect();
    if is_w(name) {
        let ranked: Option<Ranked> = match me {
            "rPr" if parent == "pPr" => Some((para_mark_rpr_rank, para_mark_rpr_rank("zzz"))),
            "rPr" => Some((rpr_rank, rpr_rank("zzz"))),
            "pPr" => Some((ppr_rank, ppr_rank("zzz"))),
            "sectPr" => Some((sect_pr_rank, sect_pr_rank("zzz"))),
            _ => None,
        };
        let ranks: Vec<usize> = match (ranked, me) {
            (Some((f, unknown)), _) => w_kids
                .iter()
                .map(|n| f(n))
                .filter(|r| *r != unknown)
                .collect(),
            (None, "tblPr") => w_kids
                .iter()
                .filter_map(|n| TBL_PR.iter().position(|x| x == n))
                .collect(),
            (None, "tcPr") => w_kids
                .iter()
                .filter_map(|n| TC_PR.iter().position(|x| x == n))
                .collect(),
            _ => Vec::new(),
        };
        if ranks.windows(2).any(|w| w[0] > w[1]) {
            out.push(format!("{me}: children out of order: {w_kids:?}"));
        }
    }
    for k in kids {
        walk(k, me, out);
    }
}

/// Problems of order in a normalized part (empty when the order is right).
pub fn order_problems(c: &C) -> Vec<String> {
    let _ = TR_PR_NOTE;
    let mut out = Vec::new();
    walk(c, "", &mut out);
    out
}
