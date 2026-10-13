// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fields ([MS-DOC] 2.8.25 `Plcffld`, 2.9.82 `Fld`): the field marks 0x13
//! (begin), 0x14 (separator) and 0x15 (end) found in the text are checked
//! against the `Plcffld` of their story, and the field code is analysed.
//! `PAGE`, `NUMPAGES`, `DATE` and `TIME` become model fields, `HYPERLINK`
//! becomes a model link over the result, any other field keeps its result as
//! text and its instruction in the model field.

use super::{DocLimits, rd_u32};
use atelier_text_model::rich::{LinkTarget, NodeId};

/// Kind of field recognised from the code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldKind {
    Page,
    NumPages,
    Date,
    Time,
    Hyperlink,
    Other,
}

/// What became of a field.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedField {
    pub paragraph: NodeId,
    pub kind: FieldKind,
    /// The field code as written (trimmed, control characters removed).
    pub code: String,
    /// `Fld.flt` of the begin mark when the `Plcffld` lists it.
    pub field_type: Option<u8>,
    /// Model field (for every kind except `Hyperlink`).
    pub field: Option<NodeId>,
    /// Model link (for `Hyperlink`).
    pub link: Option<NodeId>,
}

/// Marks of one `Plcffld`: `(cp, mark, second byte)`.
#[derive(Clone, Debug, Default)]
pub(crate) struct FldMarks {
    marks: Vec<(usize, u8, u8)>,
}

impl FldMarks {
    pub fn parse(table: &[u8], fc: usize, lcb: usize, limits: &DocLimits) -> FldMarks {
        let Some(data) = fc.checked_add(lcb).and_then(|e| table.get(fc..e)) else {
            return FldMarks::default();
        };
        if data.len() < 4 {
            return FldMarks::default();
        }
        let n = ((data.len() - 4) / 6).min(limits.max_fields);
        let base = (n + 1) * 4;
        let marks = (0..n)
            .map(|i| {
                let cp = rd_u32(data, i * 4).unwrap_or(0) as usize;
                let (a, b) = (data[base + 2 * i], data[base + 2 * i + 1]);
                (cp, a & 0x1F, b)
            })
            .collect();
        FldMarks { marks }
    }

    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    /// The `Fld` of the mark at `cp` (relative to the story).
    pub fn at(&self, cp: usize) -> Option<(u8, u8)> {
        let i = self.marks.partition_point(|m| m.0 < cp);
        self.marks.get(i).filter(|m| m.0 == cp).map(|m| (m.1, m.2))
    }
}

pub(crate) fn kind_of(code: &str) -> FieldKind {
    let name: String = code
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase();
    match name.as_str() {
        "PAGE" => FieldKind::Page,
        "NUMPAGES" => FieldKind::NumPages,
        "DATE" => FieldKind::Date,
        "TIME" => FieldKind::Time,
        "HYPERLINK" => FieldKind::Hyperlink,
        _ => FieldKind::Other,
    }
}

/// Instruction stored in the model field.
pub(crate) fn instruction_of(kind: FieldKind, code: &str) -> String {
    match kind {
        FieldKind::Page => "PAGE".to_owned(),
        FieldKind::NumPages => "NUMPAGES".to_owned(),
        FieldKind::Date => "DATE".to_owned(),
        FieldKind::Time => "TIME".to_owned(),
        _ => code.to_owned(),
    }
}

/// Splits a field code in words; double quotes group, `\"` is a literal quote.
fn words(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut quoted, mut any) = (false, false);
    let mut chars = code.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            '\\' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Target and tooltip of a `HYPERLINK` code: `HYPERLINK "url"`,
/// `HYPERLINK \l "bookmark"`, `HYPERLINK "file" \l "bookmark"`, `\o "tip"`.
pub(crate) fn hyperlink(code: &str) -> Option<(LinkTarget, Option<String>)> {
    let w = words(code);
    let mut it = w.iter().skip(1);
    let (mut file, mut mark, mut tip) = (None, None, None);
    while let Some(word) = it.next() {
        match word.to_ascii_lowercase().as_str() {
            "\\l" => mark = it.next().cloned(),
            "\\o" => tip = it.next().cloned(),
            "\\t" => {
                it.next();
            }
            s if s.starts_with('\\') => {}
            _ => {
                if file.is_none() {
                    file = Some(word.clone());
                }
            }
        }
    }
    let target = match (
        file.filter(|f| !f.is_empty()),
        mark.filter(|m| !m.is_empty()),
    ) {
        (Some(f), Some(m)) => LinkTarget::External(format!("{f}#{m}")),
        (Some(f), None) => LinkTarget::External(f),
        (None, Some(m)) => LinkTarget::Bookmark(m),
        (None, None) => return None,
    };
    Some((target, tip.filter(|t| !t.is_empty())))
}
