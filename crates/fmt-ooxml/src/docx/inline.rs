// SPDX-License-Identifier: AGPL-3.0-or-later
//! Breaks and symbols as inline items of the model (US-204).
//!
//! The reader of a `w:p` builds the paragraph text of the source: a `\n` for
//! each `w:br` or `w:cr`, the character of each `w:sym`. That text is what
//! the preserved skeleton ranges refer to. [`strip`] turns it into the text of
//! the model: the characters of the breaks and the symbols leave the text and
//! stay as `Inline::Break` and `Inline::Symbol` items (3 bytes each in the flat
//! text), and every other offset follows. The writer applies the opposite
//! mapping ([`super::write::legacy`]) to find the source text again.

use atelier_text_model::rich::{CharRun, Paragraph};
use atelier_unicode::segmentation::grapheme_boundaries;

/// Offsets of the source text mapped to the model text: the bytes that the
/// items took out of the text are subtracted.
pub(super) struct OffsetMap {
    /// Removed `(start, end)` ranges of the source text, sorted, disjoint.
    spans: Vec<(usize, usize)>,
    /// Bytes removed before each span.
    before: Vec<usize>,
    /// Bytes removed in all.
    total: usize,
}

impl OffsetMap {
    fn new(spans: Vec<(usize, usize)>) -> Self {
        let mut before = Vec::with_capacity(spans.len());
        let mut total = 0;
        for (s, e) in &spans {
            before.push(total);
            total += e - s;
        }
        Self {
            spans,
            before,
            total,
        }
    }

    /// Offset in the model text of an offset of the source text; an offset
    /// inside a removed range maps to its start.
    pub fn map(&self, offset: usize) -> usize {
        let i = self.spans.partition_point(|(_, e)| *e <= offset);
        let mut removed = self.before.get(i).copied().unwrap_or(self.total);
        if let Some((s, _)) = self.spans.get(i) {
            if *s < offset {
                removed += offset - s;
            }
        }
        offset - removed
    }
}

/// Moves the items of `paragraph` (whose offsets are those of the source text)
/// out of its text. Returns the map for the metadata kept next to it.
pub(super) fn strip(paragraph: &mut Paragraph) -> OffsetMap {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    spans.extend(paragraph.breaks.iter().map(|b| (b.offset, b.offset + 1)));
    spans.extend(paragraph.symbols.iter().map(|s| {
        let width = char::from_u32(s.code).map_or(0, char::len_utf8);
        (s.offset, s.offset + width)
    }));
    spans.sort_unstable();
    spans.retain(|(s, e)| e > s && *e <= paragraph.text.len());
    let map = OffsetMap::new(spans);
    if map.spans.is_empty() {
        return map;
    }
    let mut text = String::with_capacity(paragraph.text.len());
    let mut at = 0;
    for (s, e) in &map.spans {
        if let Some(piece) = paragraph.text.get(at..*s) {
            text.push_str(piece);
        }
        at = *e;
    }
    text.push_str(paragraph.text.get(at..).unwrap_or(""));
    for b in &mut paragraph.breaks {
        b.offset = map.map(b.offset);
    }
    for s in &mut paragraph.symbols {
        s.offset = map.map(s.offset);
    }
    for a in &mut paragraph.anchored {
        a.offset = map.map(a.offset);
    }
    let runs: Vec<CharRun> = std::mem::take(&mut paragraph.runs)
        .into_iter()
        .map(|r| CharRun {
            start: map.map(r.start),
            end: map.map(r.end),
            attrs: r.attrs,
        })
        .collect();
    paragraph.text = text;
    paragraph.runs = snap_runs(&paragraph.text, runs);
    paragraph.breaks.sort_by_key(|x| (x.offset, x.id));
    paragraph.symbols.sort_by_key(|x| (x.offset, x.id));
    paragraph.anchored.sort_by_key(|x| (x.offset, x.id));
    map
}

/// Run boundaries moved to grapheme boundaries of `text` (starts back, ends
/// forward), empty runs dropped, neighbours with equal attributes merged.
fn snap_runs(text: &str, runs: Vec<CharRun>) -> Vec<CharRun> {
    let bounds = grapheme_boundaries(text);
    let mut out: Vec<CharRun> = Vec::with_capacity(runs.len());
    let mut previous_end = 0;
    for run in runs {
        let start = bounds
            .floor_boundary(run.start.min(text.len()))
            .unwrap_or(0)
            .max(previous_end);
        let end = bounds
            .ceil_boundary(run.end.min(text.len()))
            .unwrap_or(text.len())
            .max(start);
        previous_end = end;
        if start >= end || run.attrs.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.end == start && last.attrs == run.attrs => last.end = end,
            _ => out.push(CharRun {
                start,
                end,
                attrs: run.attrs,
            }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use atelier_text_model::rich::{BreakKind, CharAttrs, InlineBreak, InlineSymbol, RichDocument};

    #[test]
    fn items_leave_the_text_and_offsets_follow() {
        let mut doc = RichDocument::new();
        let (b, s) = (doc.alloc_id().unwrap(), doc.alloc_id().unwrap());
        // "ab" `\n` "cd" U+F0B7 "e"
        let mut p = Paragraph {
            text: "ab\ncd\u{f0b7}e".into(),
            runs: vec![CharRun {
                start: 1,
                end: 6,
                attrs: CharAttrs {
                    bold: Some(true),
                    ..CharAttrs::default()
                },
            }],
            ..Paragraph::default()
        };
        p.breaks.push(InlineBreak {
            id: b,
            offset: 2,
            kind: BreakKind::Line,
        });
        p.symbols.push(InlineSymbol {
            id: s,
            offset: 5,
            font: "Symbol".into(),
            code: 0xf0b7,
        });
        let map = strip(&mut p);
        assert_eq!(p.text, "abcde");
        assert_eq!((p.breaks[0].offset, p.symbols[0].offset), (2, 4));
        assert_eq!((p.runs[0].start, p.runs[0].end), (1, 4));
        assert_eq!([0, 2, 3, 5, 6, 8].map(|o| map.map(o)), [0, 2, 2, 4, 4, 4]);
        p.validate_local().unwrap();
    }
}
