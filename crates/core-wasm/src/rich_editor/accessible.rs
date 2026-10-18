// SPDX-License-Identifier: AGPL-3.0-or-later
//! `AccessibleWindow`: the paragraphs of a flat range with their role, list
//! label and objects, for the accessible DOM mirror (US-209).
//!
//! Limits of the model, documented in `docs/us-217-rich-editor.md`: the model
//! holds no numbering definitions, so a list label is the decimal counter of
//! the paragraph followed by a dot; anchored objects carry no alternative
//! text, so the value of an object inline is empty.

use atelier_text_model::rich::flat::{BlockKind, FlatIndex, LayoutContext};
use atelier_text_model::rich::{AnchoredObject, BreakKind, NodeId, RichDocument};

use crate::rich_protocol::{
    AccessibleInline, AccessibleItem, ErrorCode, MAX_ACCESSIBLE_ITEMS, inline_kind, role,
};

/// Heading level (1-9) of a style id: `atelier.heading.N`, `HeadingN`,
/// `Heading N`, `TitreN` (case-insensitive).
fn heading_level(id: &str) -> Option<u8> {
    let id = id.to_ascii_lowercase();
    let rest = id
        .strip_prefix("atelier.heading.")
        .or_else(|| id.strip_prefix("heading"))
        .or_else(|| id.strip_prefix("titre"))?;
    let level: u8 = rest.trim_start().parse().ok()?;
    (1..=9).contains(&level).then_some(level)
}

/// Looks for a heading level in the style of the paragraph and the styles it
/// is based on.
fn style_heading(doc: &RichDocument, index: usize) -> Option<u8> {
    let mut id = LayoutContext::paragraph(doc, index)?.props.style.clone()?;
    for _ in 0..8 {
        if let Some(level) = heading_level(&id) {
            return Some(level);
        }
        id = doc.catalog().style(&id)?.based_on.clone()?;
    }
    None
}

/// The inline item behind each U+FFFC of the paragraph, in text order (items
/// at one text offset are ordered by id, as in the flat text).
fn inlines(doc: &RichDocument, index: usize, text: &str) -> Vec<AccessibleInline> {
    let mut items: Vec<(usize, NodeId, u8, String)> = Vec::new();
    if let Some(p) = LayoutContext::paragraph(doc, index) {
        let object = |o: &AnchoredObject| (o.offset, o.id, inline_kind::OBJECT, String::new());
        items.extend(p.anchored.iter().map(object));
        items.extend(p.breaks.iter().map(|b| {
            let kind = match b.kind {
                BreakKind::Page | BreakKind::Column => inline_kind::PAGE_BREAK,
                _ => inline_kind::LINE_BREAK,
            };
            (b.offset, b.id, kind, String::new())
        }));
        items.extend(p.symbols.iter().map(|s| {
            let shown = if (0xF000..=0xF0FF).contains(&s.code) {
                Some(char::from(s.code as u8))
            } else {
                char::from_u32(s.code)
            };
            (
                s.offset,
                s.id,
                inline_kind::SYMBOL,
                shown.map(String::from).unwrap_or_default(),
            )
        }));
        items.sort_by_key(|(offset, id, _, _)| (*offset, *id));
    }
    text.match_indices('\u{FFFC}')
        .take(usize::from(u16::MAX))
        .enumerate()
        .map(|(n, (offset, _))| {
            // An opaque block has no items: its single mark is an object.
            let (kind, value) = items
                .get(n)
                .map_or((inline_kind::OBJECT, String::new()), |i| (i.2, i.3.clone()));
            AccessibleInline {
                offset: offset as u64,
                kind,
                value,
            }
        })
        .collect()
}

fn item(doc: &RichDocument, index: usize) -> Result<AccessibleItem, ErrorCode> {
    let start = doc.paragraph_start(index)?;
    let text = doc.paragraph_flat(index)?.into_owned();
    let (mut role, mut level, mut label) = (role::PARAGRAPH, 0u8, String::new());
    match doc.block_kind(index)? {
        BlockKind::Opaque => role = role::OPAQUE,
        BlockKind::TableCell { .. } => role = role::TABLE_CELL,
        BlockKind::Body => {
            let list = doc.list_counters(index)?;
            if let Some(heading) = style_heading(doc, index) {
                role = role::HEADING;
                level = heading;
            } else if list.is_some() {
                role = role::LIST_ITEM;
            }
            if let Some(counters) = list {
                let list_level = usize::from(counters.level).min(counters.before.len() - 1);
                if role == role::LIST_ITEM {
                    level = list_level as u8;
                }
                let number = counters.before[list_level].saturating_add(1);
                label = format!("{number}.");
            }
        }
    }
    let inlines = inlines(doc, index, &text);
    Ok(AccessibleItem {
        para_start: start as u64,
        para_end: (start + text.len()) as u64,
        role,
        level,
        label,
        text,
        inlines,
    })
}

/// Items for the paragraphs that intersect `[start, end)`: the paragraph that
/// holds `start`, up to the one that holds the last byte before `end`; at most
/// `MAX_ACCESSIBLE_ITEMS` (the client continues after the last `para_end`).
pub(super) fn window(
    doc: &RichDocument,
    start: usize,
    end: usize,
) -> Result<Vec<AccessibleItem>, ErrorCode> {
    crate::rich_protocol::check_range(doc.len() as u64, start as u64, end as u64)?;
    doc.check_position(start)?;
    doc.check_position(end)?;
    let first = doc.locate(start)?.paragraph;
    let last = if end > start {
        doc.locate(end - 1)?.paragraph
    } else {
        first
    };
    (first..=last)
        .take(MAX_ACCESSIBLE_ITEMS)
        .map(|index| item(doc, index))
        .collect()
}
