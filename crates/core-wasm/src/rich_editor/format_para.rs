// SPDX-License-Identifier: AGPL-3.0-or-later
//! Paragraph formatting of the rich editor session (US-243). Extension module
//! of F0: the handler of `ApplyParaFormat` and of `FormatMarks`. **Owner:
//! US-243** (with `web/format-para.mjs`). Contract:
//! `docs/us-240-format-protocol.md`, sections 4.2, 5.3 and 5.8.
//!
//! `ApplyParaFormat` targets every paragraph the selection touches (the one a
//! selection ends at the very start of excepted; a caret touches its own
//! paragraph; opaque blocks are skipped), builds one `SetParaProps` per
//! paragraph and applies them with `RichDocument::apply_format`: one history
//! step, one revision. Shading and borders need model fields first (task
//! US-243-M1 of the audit in the contract): a command that carries one is
//! refused whole with `RICH_FORMAT_NOT_IMPLEMENTED`.
//!
//! `FormatMarks` reads the layout only (`PageCache::format_marks`), so it is
//! allowed on a read-only or plain-text document.

use atelier_text_model::rich::flat::{FlatIndex, LayoutContext, LayoutScope};
use atelier_text_model::rich::{Alignment, LineRule, LineSpacing, ParaProps, RichOp};

use super::styles::layout_doc;
use super::{DEFAULT_LAYOUT_BUDGET, Session};
use crate::rich_layout::{PageDelta, RichPageCache};
use crate::rich_protocol::{
    EditOp, ErrorCode, MAX_FORMAT_MARKS, MAX_TWIPS, ParaAttr, ParaEdit, ParaFormat, ParaValue,
    Response,
};

/// Default tab stop: one step of "increase / decrease indent", twips.
const INDENT_STEP: i32 = 720;
/// Space added by "add space before / after", twips (12 pt).
const SPACE_STEP: u32 = 240;

/// The alignment of a wire number (0 left, 1 center, 2 right, 3 justify).
fn alignment(number: u8) -> Option<Alignment> {
    Some(match number {
        0 => Alignment::Left,
        1 => Alignment::Center,
        2 => Alignment::Right,
        3 => Alignment::Justify,
        _ => return None,
    })
}

/// Next (`up`) or previous multiple of the step, from `from`, within `0..=MAX_TWIPS`.
fn indent_step(from: i32, up: bool) -> i32 {
    let stepped = if up {
        (from.div_euclid(INDENT_STEP) + 1) * INDENT_STEP
    } else if from.rem_euclid(INDENT_STEP) == 0 {
        from - INDENT_STEP
    } else {
        from.div_euclid(INDENT_STEP) * INDENT_STEP
    };
    stepped.clamp(0, MAX_TWIPS)
}

/// What the paragraphs touched resolve to, before the command.
pub(super) struct Touched {
    pub(super) index: usize,
    pub(super) resolved: ParaProps,
}

/// `set` and `clear` of one paragraph for the edits of the command.
fn props_for(
    edits: &[ParaEdit],
    touched: &Touched,
    space_target: &SpaceTarget,
) -> Option<(ParaProps, ParaProps)> {
    let mut set = ParaProps::default();
    let mut clear = ParaProps::default();
    let marker = ParaProps {
        alignment: Some(Alignment::Left),
        indent_left: Some(0),
        indent_right: Some(0),
        indent_first_line: Some(0),
        space_before: Some(0),
        space_after: Some(0),
        line_spacing: Some(LineSpacing {
            rule: LineRule::Auto,
            value: 240,
        }),
        ..ParaProps::default()
    };
    let resolved = &touched.resolved;
    for edit in edits {
        match edit.op {
            EditOp::Clear => match edit.attr {
                ParaAttr::Alignment => clear.alignment = marker.alignment,
                ParaAttr::IndentLeft => clear.indent_left = marker.indent_left,
                ParaAttr::IndentRight => clear.indent_right = marker.indent_right,
                ParaAttr::FirstLine => clear.indent_first_line = marker.indent_first_line,
                ParaAttr::SpaceBefore => clear.space_before = marker.space_before,
                ParaAttr::SpaceAfter => clear.space_after = marker.space_after,
                ParaAttr::LineSpacing => clear.line_spacing = marker.line_spacing,
                ParaAttr::Shading | ParaAttr::Borders => return None,
            },
            EditOp::Set => match (edit.attr, edit.value.as_ref()?) {
                (ParaAttr::Alignment, ParaValue::Choice(n)) => set.alignment = Some(alignment(*n)?),
                (ParaAttr::IndentLeft, ParaValue::Twips(v)) => set.indent_left = Some(*v),
                (ParaAttr::IndentRight, ParaValue::Twips(v)) => set.indent_right = Some(*v),
                (ParaAttr::FirstLine, ParaValue::Twips(v)) => set.indent_first_line = Some(*v),
                (ParaAttr::SpaceBefore, ParaValue::Length(v)) => set.space_before = Some(*v),
                (ParaAttr::SpaceAfter, ParaValue::Length(v)) => set.space_after = Some(*v),
                (ParaAttr::LineSpacing, ParaValue::Line { rule, value }) => {
                    set.line_spacing = Some(LineSpacing {
                        rule: match rule {
                            0 => LineRule::Auto,
                            1 => LineRule::Exact,
                            2 => LineRule::AtLeast,
                            _ => return None,
                        },
                        value: *value,
                    });
                }
                _ => return None,
            },
            EditOp::Toggle => match edit.attr {
                ParaAttr::SpaceBefore => set.space_before = Some(space_target.before),
                ParaAttr::SpaceAfter => set.space_after = Some(space_target.after),
                _ => return None,
            },
            EditOp::StepUp | EditOp::StepDown if edit.attr == ParaAttr::IndentLeft => {
                set.indent_left = Some(indent_step(
                    resolved.indent_left.unwrap_or(0),
                    edit.op == EditOp::StepUp,
                ));
            }
            _ => return None,
        }
    }
    Some((set, clear))
}

/// The value a "toggle" of the spacing writes to every touched paragraph:
/// 12 pt when none has any, else 0 (the rule of Word, from the resolved values).
struct SpaceTarget {
    before: u32,
    after: u32,
}

impl Session {
    /// Paragraphs the selection touches, with their resolved properties.
    pub(super) fn touched_paragraphs(&self) -> Result<Vec<Touched>, ErrorCode> {
        let (low, high) = self.selection_range();
        let doc = &self.doc;
        let first = doc.locate(low)?;
        let last = doc.locate(high)?;
        let mut last_index = last.paragraph;
        // A selection that ends at the very start of a paragraph does not touch it.
        if low != high && last_index > first.paragraph && last.offset == 0 {
            last_index -= 1;
        }
        let mut out = Vec::new();
        for index in first.paragraph..=last_index {
            let Some(p) = LayoutContext::paragraph(doc, index) else {
                continue; // an opaque block
            };
            let resolved = match doc.resolved(index) {
                Ok(r) => r.paragraph,
                Err(_) => p.props.clone(),
            };
            out.push(Touched { index, resolved });
        }
        Ok(out)
    }

    /// `FormatMarks`: spaces, tabs, no-break spaces, breaks and paragraph ends
    /// of the pages `first..first + count`, from the layout. A page not laid
    /// out is `RICH_NOT_READY` (the client waits for a `Layout`), never a
    /// partial list; `RICH_LIMIT` when the pages hold more marks than a reply
    /// carries (the client asks for fewer pages).
    pub(super) fn format_marks(&mut self, first: u32, count: u32) -> Result<Response, ErrorCode> {
        let page_count = self.cache.page_count();
        if first >= page_count {
            return Err(ErrorCode::PageRange);
        }
        let end = first.saturating_add(count).min(page_count);
        let marks = self
            .cache
            .format_marks(layout_doc!(self), first, end, MAX_FORMAT_MARKS)?;
        if marks.len() > MAX_FORMAT_MARKS {
            return Err(ErrorCode::Limit);
        }
        Ok(Response::FormatMarks {
            revision: self.revision(),
            marks,
        })
    }

    /// `ApplyParaFormat`: the edits on every paragraph the selection touches,
    /// as one history step (one Ctrl+Z).
    pub(super) fn apply_para_format(&mut self, format: ParaFormat) -> Result<Response, ErrorCode> {
        self.begin_format(format.base_revision, format.command_id)?;
        // Shading and borders: no model field yet (US-243-M1).
        if format
            .edits
            .iter()
            .any(|e| matches!(e.attr, ParaAttr::Shading | ParaAttr::Borders))
        {
            return Err(ErrorCode::FormatNotImplemented);
        }
        let touched = self.touched_paragraphs()?;
        let all_zero = |value: fn(&ParaProps) -> Option<u32>| {
            touched.iter().all(|t| value(&t.resolved).unwrap_or(0) == 0)
        };
        let space_target = SpaceTarget {
            before: if all_zero(|p| p.space_before) {
                SPACE_STEP
            } else {
                0
            },
            after: if all_zero(|p| p.space_after) {
                SPACE_STEP
            } else {
                0
            },
        };
        let mut ops = Vec::with_capacity(touched.len());
        for t in &touched {
            let (set, clear) =
                props_for(&format.edits, t, &space_target).ok_or(ErrorCode::InvalidBody)?;
            let start = self.doc.paragraph_start(t.index)?;
            let (para, _) = self.doc.node_position(start)?;
            ops.push(RichOp::SetParaProps { para, set, clear });
        }
        let before = self.revision();
        let outcome = self.doc.apply_format(
            ops,
            (self.anchor, self.focus),
            LayoutScope::Paragraphs,
            false,
        )?;
        let delta = if outcome.revision == before {
            PageDelta::default()
        } else {
            self.cache.apply_edit(layout_doc!(self), &outcome)
        };
        self.note_command(format.command_id);
        self.finish(delta, DEFAULT_LAYOUT_BUDGET, true)
    }
}
