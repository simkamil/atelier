// SPDX-License-Identifier: AGPL-3.0-or-later
//! From a model paragraph to the text the line layout shapes (US-216).
//!
//! The model keeps line, page and column breaks, symbols and anchored objects
//! outside `Paragraph::text` (three flat bytes each). The line layout works on
//! text, so [`prepare`] builds the text it shapes:
//!
//! - a line break is a `\n` (the layout consumes it as a forced break);
//! - a page break is a `\n` too, and [`page_break_lines`] turns the line that
//!   follows it into a manual page break of the pagination; a column break is
//!   handled as a page break while there are no columns;
//! - a symbol is its character, drawn with the symbol's font when a registered
//!   font has it, otherwise with the font of the run (the Symbol and Wingdings
//!   private-use code points `F000..F0FF` then stand for their Latin-1 low
//!   byte);
//! - anchored objects take no room.
//!
//! Every item keeps its place in flat positions through a [`Mark`], which
//! the caret code uses to convert between flat and laid-out offsets. The preview
//! and the editor page cache both call this module.

use atelier_font::manager::{FontManager, FontRequest, FontStyle};
use atelier_text_layout::ParagraphLayout;
use atelier_text_model::rich::flat::OBJECT_LEN;
use atelier_text_model::rich::{BreakKind, CharAttrs, CharRun, NodeId, Paragraph};

/// One inline item: where it is in the flat text and in the laid-out text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Mark {
    /// Offset of the item in the paragraph's flat text (three bytes).
    pub flat: usize,
    /// Offset of its text in the laid-out text.
    pub layout: usize,
    /// Bytes it takes in the laid-out text (zero for an object).
    pub len: usize,
}

/// The paragraph to shape and how its items map into it.
#[derive(Debug, Clone)]
pub(crate) struct Prepared {
    /// Text and runs to lay out (properties are the caller's).
    pub paragraph: Paragraph,
    pub marks: Vec<Mark>,
    /// Laid-out offsets just after a page or column break.
    pub page_breaks: Vec<usize>,
}

enum Kind {
    Object,
    Line,
    Page,
    Symbol(String, u32),
}

fn symbol_char(font: &str, code: u32, fonts: &FontManager<'_>) -> (char, bool) {
    let wanted = char::from_u32(code).unwrap_or('\u{fffd}');
    let native = fonts
        .resolve_for_char(&FontRequest::new([font], 400, FontStyle::Normal), wanted)
        .is_ok_and(|face| face.family.eq_ignore_ascii_case(font));
    if native {
        return (wanted, true);
    }
    let low = if (0xf000..=0xf0ff).contains(&code) {
        char::from_u32(code - 0xf000).filter(|c| !c.is_control())
    } else {
        None
    };
    (low.unwrap_or(wanted), false)
}

/// Builds the text to lay out. `with_runs` is false for plain text.
pub(crate) fn prepare(source: &Paragraph, fonts: &FontManager<'_>, with_runs: bool) -> Prepared {
    let mut items: Vec<(usize, NodeId, Kind)> = Vec::new();
    items.extend(
        source
            .anchored
            .iter()
            .map(|x| (x.offset, x.id, Kind::Object)),
    );
    items.extend(source.breaks.iter().map(|x| {
        let kind = match x.kind {
            BreakKind::Line => Kind::Line,
            BreakKind::Page | BreakKind::Column => Kind::Page,
        };
        (x.offset, x.id, kind)
    }));
    items.extend(
        source
            .symbols
            .iter()
            .map(|x| (x.offset, x.id, Kind::Symbol(x.font.clone(), x.code))),
    );
    items.sort_by_key(|(offset, id, _)| (*offset, *id));
    let runs: &[CharRun] = if with_runs { &source.runs } else { &[] };

    let mut text = String::with_capacity(source.text.len() + items.len());
    let mut out_runs: Vec<CharRun> = Vec::new();
    let mut marks = Vec::with_capacity(items.len());
    let mut page_breaks = Vec::new();
    let mut at = 0usize;
    let piece = |text: &mut String, out_runs: &mut Vec<CharRun>, a: usize, b: usize| {
        let base = text.len();
        text.push_str(source.text.get(a..b).unwrap_or(""));
        for run in runs {
            let (s, e) = (run.start.max(a), run.end.min(b));
            if s < e {
                out_runs.push(CharRun {
                    start: base + s - a,
                    end: base + e - a,
                    attrs: run.attrs.clone(),
                });
            }
        }
    };
    for (k, (offset, _, kind)) in items.iter().enumerate() {
        let offset = (*offset).min(source.text.len());
        if offset > at && source.text.is_char_boundary(offset) {
            piece(&mut text, &mut out_runs, at, offset);
            at = offset;
        }
        let layout = text.len();
        let len = match kind {
            Kind::Object => 0,
            Kind::Line => {
                text.push('\n');
                1
            }
            Kind::Page => {
                text.push('\n');
                page_breaks.push(layout + 1);
                1
            }
            Kind::Symbol(font, code) => {
                let (c, native) = symbol_char(font, *code, fonts);
                text.push(c);
                let covering = runs.iter().find(|r| r.start < offset && offset < r.end);
                if covering.is_some() || native {
                    let mut attrs: CharAttrs =
                        covering.map(|r| r.attrs.clone()).unwrap_or_default();
                    if native {
                        attrs.font = Some(font.clone());
                    }
                    out_runs.push(CharRun {
                        start: layout,
                        end: layout + c.len_utf8(),
                        attrs,
                    });
                }
                c.len_utf8()
            }
        };
        marks.push(Mark {
            flat: offset + OBJECT_LEN * k,
            layout,
            len,
        });
    }
    piece(&mut text, &mut out_runs, at, source.text.len());
    let mut merged: Vec<CharRun> = Vec::with_capacity(out_runs.len());
    for run in out_runs {
        match merged.last_mut() {
            Some(last) if last.end == run.start && last.attrs == run.attrs => last.end = run.end,
            _ => merged.push(run),
        }
    }
    Prepared {
        paragraph: Paragraph {
            text,
            runs: merged,
            props: source.props.clone(),
            ..Paragraph::default()
        },
        marks,
        page_breaks,
    }
}

/// Indices of the lines that begin a new page: the line starting at each of
/// the laid-out offsets `after` (just past a page or column break).
pub(crate) fn page_break_lines(layout: &ParagraphLayout, after: &[usize]) -> Vec<usize> {
    let mut lines: Vec<usize> = after
        .iter()
        .filter_map(|offset| {
            layout
                .lines
                .iter()
                .position(|line| line.source.start == *offset)
        })
        .filter(|index| *index > 0)
        .collect();
    lines.sort_unstable();
    lines.dedup();
    lines
}
