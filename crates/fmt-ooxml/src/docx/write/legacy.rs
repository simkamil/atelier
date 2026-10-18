// SPDX-License-Identifier: AGPL-3.0-or-later
//! The text of the source, rebuilt from a paragraph of the model (US-204).
//!
//! The model keeps line, page and column breaks and symbols as inline items
//! outside the paragraph text, while the preserved skeleton of a `w:p`
//! (`PreservedElement::range`) counts one `\n` for each break and the
//! character of each symbol in the text. [`legacy`] inserts them back, so the
//! writer can compare the current paragraph with the one of the import, and
//! the skeleton with both, exactly as it did when the text held them. The
//! inverse of [`super::super::inline::strip`], deterministic: two equal
//! paragraphs give equal source texts.

use atelier_text_model::rich::{BreakKind, CharRun, InlineBreak, InlineSymbol, NodeId, Paragraph};
use std::borrow::Cow;

/// A break or a symbol standing in the source text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum LegacyItem {
    Break(BreakKind),
    Symbol { font: String, code: u32 },
}

/// The paragraph with the characters of its breaks and symbols in the text
/// (and runs and object offsets shifted to match), and where they are.
pub(super) struct Legacy<'a> {
    pub paragraph: Cow<'a, Paragraph>,
    /// `(offset in the source text, item)`, sorted.
    pub items: Vec<(usize, LegacyItem)>,
}

enum Kind<'a> {
    Anchored,
    Break(&'a InlineBreak),
    Symbol(&'a InlineSymbol),
}

fn width(kind: &Kind<'_>) -> usize {
    match kind {
        Kind::Anchored => 0,
        Kind::Break(_) => 1,
        Kind::Symbol(s) => char::from_u32(s.code).map_or(0, char::len_utf8),
    }
}

pub(super) fn legacy(p: &Paragraph) -> Legacy<'_> {
    if p.breaks.is_empty() && p.symbols.is_empty() {
        return Legacy {
            paragraph: Cow::Borrowed(p),
            items: Vec::new(),
        };
    }
    let mut order: Vec<(usize, NodeId, Kind<'_>)> = Vec::new();
    order.extend(p.anchored.iter().map(|x| (x.offset, x.id, Kind::Anchored)));
    order.extend(p.breaks.iter().map(|x| (x.offset, x.id, Kind::Break(x))));
    order.extend(p.symbols.iter().map(|x| (x.offset, x.id, Kind::Symbol(x))));
    order.sort_by_key(|(offset, id, _)| (*offset, *id));
    // Bytes of the items before each entry of `order`, and the offsets.
    let mut before = Vec::with_capacity(order.len());
    let mut total = 0;
    for (_, _, kind) in &order {
        before.push(total);
        total += width(kind);
    }
    // Bytes of the items located strictly before a text offset.
    let before_offset = |offset: usize| -> usize {
        let i = order.partition_point(|(o, _, _)| *o < offset);
        before.get(i).copied().unwrap_or(total)
    };
    let mut text = String::with_capacity(p.text.len() + total);
    let mut items = Vec::new();
    let mut at = 0;
    let mut anchored = p.anchored.clone();
    for (offset, id, kind) in &order {
        let offset = (*offset).clamp(at, p.text.len());
        text.push_str(p.text.get(at..offset).unwrap_or(""));
        at = offset;
        let start = text.len();
        match kind {
            Kind::Anchored => {
                if let Some(a) = anchored.iter_mut().find(|a| a.id == *id) {
                    a.offset = start;
                }
            }
            Kind::Break(b) => {
                text.push('\n');
                items.push((start, LegacyItem::Break(b.kind)));
            }
            Kind::Symbol(s) => {
                if let Some(c) = char::from_u32(s.code) {
                    text.push(c);
                }
                items.push((
                    start,
                    LegacyItem::Symbol {
                        font: s.font.clone(),
                        code: s.code,
                    },
                ));
            }
        }
    }
    text.push_str(p.text.get(at..).unwrap_or(""));
    // Items at the boundary between two runs go with the run that follows.
    let runs: Vec<CharRun> = p
        .runs
        .iter()
        .map(|r| CharRun {
            start: r.start + before_offset(r.start),
            end: r.end + before_offset(r.end),
            attrs: r.attrs.clone(),
        })
        .collect();
    Legacy {
        paragraph: Cow::Owned(Paragraph {
            text,
            runs,
            anchored,
            breaks: Vec::new(),
            symbols: Vec::new(),
            ..p.clone()
        }),
        items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atelier_text_model::rich::{AnchoredObject, CharAttrs, RichDocument};

    #[test]
    fn items_come_back_into_the_text_in_order() {
        let mut doc = RichDocument::new();
        let (a, b, s) = (
            doc.alloc_id().unwrap(),
            doc.alloc_id().unwrap(),
            doc.alloc_id().unwrap(),
        );
        let bold = CharAttrs {
            bold: Some(true),
            ..CharAttrs::default()
        };
        let mut p = Paragraph {
            text: "abcde".into(),
            runs: vec![CharRun {
                start: 1,
                end: 4,
                attrs: bold,
            }],
            ..Paragraph::default()
        };
        p.anchored.push(AnchoredObject {
            id: a,
            offset: 2,
            kind: "w:drawing".into(),
            data: vec![],
        });
        p.breaks.push(InlineBreak {
            id: b,
            offset: 2,
            kind: BreakKind::Page,
        });
        p.symbols.push(InlineSymbol {
            id: s,
            offset: 4,
            font: "Symbol".into(),
            code: 0xf0b7,
        });
        let l = legacy(&p);
        // "ab" object, `\n`, "cd", symbol, "e"
        assert_eq!(l.paragraph.text, "ab\ncd\u{f0b7}e");
        assert_eq!(l.paragraph.anchored[0].offset, 2);
        assert_eq!(l.items[0], (2, LegacyItem::Break(BreakKind::Page)));
        assert_eq!(l.items[1].0, 5);
        // The run [1, 4) covers the break inside it and ends before the symbol.
        assert_eq!((l.paragraph.runs[0].start, l.paragraph.runs[0].end), (1, 5));
        let plain = Paragraph::with_text("x");
        assert!(matches!(legacy(&plain).paragraph, Cow::Borrowed(_)));
    }
}
