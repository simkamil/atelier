// SPDX-License-Identifier: AGPL-3.0-or-later
//! Character formatting of the rich editor session (US-240). Extension module
//! of F0: the handler of `ApplyCharFormat` and the two hooks of typing
//! attributes. **Owner: US-240** (with `web/format-char.mjs`); nothing else of
//! the engine has to change. Contract: `docs/us-240-format-protocol.md`.
//!
//! * `apply_char_format`: the target range (selection, else the word under the
//!   caret, else the typing attributes), the `SetCharAttrs` operations per
//!   stretch of uniform attributes, the `Toggle` and size-step rules of the
//!   contract, then `self.cache.apply_edit` and `self.finish` like
//!   `Session::edit`, all through `RichDocument::apply_format` (one history
//!   step, one revision).
//! * `Typing`: the attributes of the next typed text.
//! * `after_text_edit`: applies the typing attributes to the text just
//!   inserted, joined with the insertion (`join_previous`).

use atelier_text_model::rich::flat::{
    EditOutcome, FlatEdit, FlatIndex, LayoutContext, LayoutScope,
};
use atelier_text_model::rich::{CharAttrs, Color, RichDocument, RichOp, Underline, VerticalAlign};

use super::format_state::word_under_caret;
use super::{DEFAULT_LAYOUT_BUDGET, Session};
use crate::rich_layout::{PageDelta, RichPageCache};
use crate::rich_preview::{LOADED_FONT_BYTES, LOADED_FONT_FACES};
use crate::rich_protocol::{
    CharAttr, CharEdit, CharFormat, CharValue, EditOp, ErrorCode, FormatScope, MAX_HALF_POINTS,
    MAX_STATE_RUNS, MIN_HALF_POINTS, Response, size_step,
};

/// Font files already given to the engine, kept for the life of the module so
/// that a reopened document does not leak its fonts again (`FontManager`
/// borrows `'static` bytes).
static LOADED_FONTS: std::sync::Mutex<Vec<&'static [u8]>> = std::sync::Mutex::new(Vec::new());

/// The `'static` copy of a font file (`None` when it is no font or the budget is spent).
fn intern_font(bytes: Vec<u8>) -> Option<&'static [u8]> {
    atelier_font::FontFace::parse_collection(&bytes, 0).ok()?;
    let mut known = LOADED_FONTS.lock().ok()?;
    if let Some(found) = known.iter().find(|font| ***font == *bytes) {
        return Some(*found);
    }
    let total: usize = known.iter().map(|font| font.len()).sum();
    if known.len() >= LOADED_FONT_FACES || total + bytes.len() > LOADED_FONT_BYTES {
        return None;
    }
    let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    known.push(leaked);
    Some(leaked)
}

/// Size used when the cascade gives none: 11 pt.
const DEFAULT_HALF_POINTS: u16 = 22;

/// Attributes of the next typed text when the caret is not in a word:
/// `clear` then `set` over what the text would inherit. Forgotten at every
/// selection change (`Session::select`), kept by consecutive typing
/// (`after_text_edit`).
#[derive(Default)]
pub(super) struct Typing {
    pub(super) set: CharAttrs,
    pub(super) clear: CharAttrs,
    /// Pending: `FormatState` shows it (`typing` flag) and the next insertion applies it.
    pub(super) armed: bool,
}

impl Typing {
    /// Called by `Session::select`: any selection change drops the attributes.
    pub(super) fn forget(&mut self) {
        *self = Self::default();
    }
}

/// One stretch of one paragraph with uniform direct attributes.
struct Piece {
    /// Flat paragraph index.
    index: usize,
    /// Text offsets (bytes of the paragraph text).
    from: usize,
    to: usize,
    direct: CharAttrs,
    resolved: CharAttrs,
}

type Change = (CharAttrs, CharAttrs);

fn resolve(doc: &RichDocument, index: usize, direct: &CharAttrs) -> CharAttrs {
    doc.resolved_run(index, direct)
        .unwrap_or_else(|_| direct.clone())
}

/// The pieces of the flat range `low..high` (the paragraphs it touches; one
/// that the range ends at the very start of is not touched), in order.
fn pieces(doc: &RichDocument, low: usize, high: usize) -> Result<Vec<Piece>, ErrorCode> {
    let first = doc.locate(low)?;
    let last = doc.locate(high)?;
    let mut last_index = last.paragraph;
    if last_index > first.paragraph && last.offset == 0 {
        last_index -= 1;
    }
    let mut out = Vec::new();
    for index in first.paragraph..=last_index {
        let Some(p) = LayoutContext::paragraph(doc, index) else {
            continue;
        };
        let flat_len = doc.paragraph_flat(index)?.len();
        let start_flat = if index == first.paragraph {
            first.offset
        } else {
            0
        };
        let end_flat = if index == last.paragraph {
            last.offset
        } else {
            flat_len
        };
        let from = doc.flat_to_text(index, start_flat)?;
        let to = doc.flat_to_text(index, end_flat)?;
        let push = |start: usize, end: usize, direct: CharAttrs, out: &mut Vec<Piece>| {
            if end > start {
                let resolved = resolve(doc, index, &direct);
                out.push(Piece {
                    index,
                    from: start,
                    to: end,
                    direct,
                    resolved,
                });
            }
        };
        let mut at = from;
        for run in p.runs.iter().filter(|r| r.end > from && r.start < to) {
            if run.start > at {
                push(at, run.start, CharAttrs::default(), &mut out);
            }
            let start = run.start.max(from);
            let end = run.end.min(to);
            push(start, end, run.attrs.clone(), &mut out);
            at = end;
        }
        if at < to {
            push(at, to, CharAttrs::default(), &mut out);
        }
        if out.len() > MAX_STATE_RUNS {
            return Err(ErrorCode::Limit);
        }
    }
    Ok(out)
}

/// A value an edit writes on a piece.
#[derive(Clone)]
enum Val {
    Clear,
    Bool(bool),
    Underline(Underline),
    Align(VerticalAlign),
    Size(u32),
    Font(String),
    Color(Color),
}

/// A `CharAttrs` whose only defined property is `attr` (a mask for `clear`).
fn mask(attr: CharAttr) -> CharAttrs {
    let black = Color { r: 0, g: 0, b: 0 };
    let mut m = CharAttrs::default();
    match attr {
        CharAttr::Font => m.font = Some("x".into()),
        CharAttr::Size => m.size_half_points = Some(1),
        CharAttr::Bold => m.bold = Some(true),
        CharAttr::Italic => m.italic = Some(true),
        CharAttr::Underline => m.underline = Some(Underline::Single),
        CharAttr::Strike => m.strike = Some(true),
        CharAttr::VerticalAlign => m.vertical_align = Some(VerticalAlign::Superscript),
        CharAttr::Color => m.color = Some(black),
        CharAttr::Highlight => m.highlight = Some(black),
    }
    m
}

/// Writes `value` of `attr` into the `(set, clear)` pair of a piece, replacing
/// what an earlier edit of the command put there.
fn put(change: &mut Change, attr: CharAttr, value: Val) {
    let (set, clear) = change;
    set.clear(&mask(attr));
    clear.clear(&mask(attr));
    match (attr, value) {
        (_, Val::Clear) => clear.overlay(&mask(attr)),
        (CharAttr::Bold, Val::Bool(v)) => set.bold = Some(v),
        (CharAttr::Italic, Val::Bool(v)) => set.italic = Some(v),
        (CharAttr::Strike, Val::Bool(v)) => set.strike = Some(v),
        (CharAttr::Underline, Val::Underline(v)) => set.underline = Some(v),
        (CharAttr::VerticalAlign, Val::Align(v)) => set.vertical_align = Some(v),
        (CharAttr::Size, Val::Size(v)) => set.size_half_points = Some(v),
        (CharAttr::Font, Val::Font(v)) => set.font = Some(v),
        (CharAttr::Color, Val::Color(v)) => set.color = Some(v),
        (CharAttr::Highlight, Val::Color(v)) => set.highlight = Some(v),
        _ => {}
    }
}

fn underline_of(n: u8) -> Underline {
    match n {
        1 => Underline::Single,
        2 => Underline::Double,
        3 => Underline::Thick,
        4 => Underline::Dotted,
        5 => Underline::Dashed,
        6 => Underline::Wave,
        _ => Underline::None,
    }
}

fn align_of(n: u8) -> VerticalAlign {
    match n {
        1 => VerticalAlign::Superscript,
        2 => VerticalAlign::Subscript,
        _ => VerticalAlign::Baseline,
    }
}

fn flag(attrs: &CharAttrs, attr: CharAttr) -> bool {
    match attr {
        CharAttr::Bold => attrs.bold.unwrap_or(false),
        CharAttr::Italic => attrs.italic.unwrap_or(false),
        CharAttr::Strike => attrs.strike.unwrap_or(false),
        _ => false,
    }
}

fn underline_is(attrs: &CharAttrs, target: Underline) -> bool {
    attrs.underline.unwrap_or(Underline::None) == target
}

fn align_is(attrs: &CharAttrs, target: VerticalAlign) -> bool {
    attrs.vertical_align.unwrap_or(VerticalAlign::Baseline) == target
}

/// Computes what `edit` writes on each piece of `targets` (one pair per piece,
/// accumulated over the edits of the command).
fn compute(
    doc: &RichDocument,
    targets: &[Piece],
    edit: &CharEdit,
    out: &mut [Change],
) -> Result<(), ErrorCode> {
    let attr = edit.attr;
    // What a piece resolves to without its own direct value of `attr`.
    let base = |piece: &Piece| -> CharAttrs {
        let mut direct = piece.direct.clone();
        direct.clear(&mask(attr));
        resolve(doc, piece.index, &direct)
    };
    match (edit.op, edit.value.as_ref()) {
        (EditOp::Clear, _) => {
            for change in out.iter_mut() {
                put(change, attr, Val::Clear);
            }
        }
        (EditOp::Set, Some(value)) => {
            let val = match (attr, value) {
                (CharAttr::Font, CharValue::Text(t)) => Val::Font(t.clone()),
                (CharAttr::Size, CharValue::Half(v)) => Val::Size(u32::from(*v)),
                (CharAttr::Bold | CharAttr::Italic | CharAttr::Strike, CharValue::Flag(v)) => {
                    Val::Bool(*v)
                }
                (CharAttr::Underline, CharValue::Choice(n)) => Val::Underline(underline_of(*n)),
                (CharAttr::VerticalAlign, CharValue::Choice(n)) => Val::Align(align_of(*n)),
                (CharAttr::Color | CharAttr::Highlight, CharValue::Color(c)) => Val::Color(Color {
                    r: c.r,
                    g: c.g,
                    b: c.b,
                }),
                _ => return Err(ErrorCode::InvalidBody),
            };
            for change in out.iter_mut() {
                put(change, attr, val.clone());
            }
        }
        (EditOp::Toggle, value) => match attr {
            CharAttr::Bold | CharAttr::Italic | CharAttr::Strike => {
                let all = targets.iter().all(|p| flag(&p.resolved, attr));
                for (piece, change) in targets.iter().zip(out.iter_mut()) {
                    if all {
                        let explicit = flag(&base(piece), attr);
                        put(
                            change,
                            attr,
                            if explicit {
                                Val::Bool(false)
                            } else {
                                Val::Clear
                            },
                        );
                    } else if !flag(&piece.resolved, attr) {
                        put(change, attr, Val::Bool(true));
                    }
                }
            }
            CharAttr::Underline => {
                let Some(CharValue::Choice(n)) = value else {
                    return Err(ErrorCode::InvalidBody);
                };
                let target = underline_of(*n);
                let all = targets.iter().all(|p| underline_is(&p.resolved, target));
                for (piece, change) in targets.iter().zip(out.iter_mut()) {
                    if !all {
                        if !underline_is(&piece.resolved, target) {
                            put(change, attr, Val::Underline(target));
                        }
                    } else if underline_is(&base(piece), Underline::None) {
                        put(change, attr, Val::Clear);
                    } else {
                        put(change, attr, Val::Underline(Underline::None));
                    }
                }
            }
            CharAttr::VerticalAlign => {
                let Some(CharValue::Choice(n)) = value else {
                    return Err(ErrorCode::InvalidBody);
                };
                let target = align_of(*n);
                let all = targets.iter().all(|p| align_is(&p.resolved, target));
                for (piece, change) in targets.iter().zip(out.iter_mut()) {
                    if !all {
                        if !align_is(&piece.resolved, target) {
                            put(change, attr, Val::Align(target));
                        }
                    } else if align_is(&base(piece), VerticalAlign::Baseline) {
                        put(change, attr, Val::Clear);
                    } else {
                        put(change, attr, Val::Align(VerticalAlign::Baseline));
                    }
                }
            }
            _ => return Err(ErrorCode::InvalidBody),
        },
        (
            op @ (EditOp::StepUp | EditOp::StepDown | EditOp::StepUpPoint | EditOp::StepDownPoint),
            _,
        ) if attr == CharAttr::Size => {
            for (piece, change) in targets.iter().zip(out.iter_mut()) {
                let half = piece
                    .resolved
                    .size_half_points
                    .map_or(DEFAULT_HALF_POINTS, |v| {
                        v.clamp(u32::from(MIN_HALF_POINTS), u32::from(MAX_HALF_POINTS)) as u16
                    });
                let next = match op {
                    EditOp::StepUp => size_step(half, true),
                    EditOp::StepDown => size_step(half, false),
                    EditOp::StepUpPoint => half.saturating_add(2).min(MAX_HALF_POINTS),
                    _ => half.saturating_sub(2).max(MIN_HALF_POINTS),
                };
                put(change, attr, Val::Size(u32::from(next)));
            }
        }
        _ => return Err(ErrorCode::InvalidBody),
    }
    Ok(())
}

/// Merges the `(set, clear)` pair of a command into the typing attributes.
/// `inherited` is what the next text would carry without any typing attribute:
/// a value equal to it needs no entry, and nothing is cleared that it lacks.
fn merge_typing(typing: &mut Typing, (set, clear): &Change, inherited: &CharAttrs) {
    typing.set.clear(clear);
    typing.clear.overlay(clear);
    typing.set.overlay(set);
    // A property that is set needs no clearing marker.
    typing.clear.clear(set);
    macro_rules! prune {
        ($($f:ident),*) => {$(
            if typing.set.$f == inherited.$f { typing.set.$f = None; }
            if inherited.$f.is_none() { typing.clear.$f = None; }
        )*};
    }
    prune!(
        bold,
        italic,
        underline,
        strike,
        vertical_align,
        font,
        size_half_points,
        color,
        highlight
    );
    typing.armed = !typing.set.is_empty() || !typing.clear.is_empty();
}

/// Merges the outcome of the insertion and that of its formatting (which spans
/// the same bytes: formatting changes no length) into the one the layout gets.
fn merge_outcomes(insert: EditOutcome, format: &EditOutcome) -> EditOutcome {
    if format.removed_bytes == 0 && format.inserted_bytes == 0 {
        return insert;
    }
    let post_end = (insert.start + insert.inserted_bytes).max(format.start + format.inserted_bytes);
    let pre_end = post_end - insert.inserted_bytes + insert.removed_bytes;
    let start = insert.start.min(format.start);
    let post_end_p = (insert.first_paragraph + insert.inserted_paragraphs)
        .max(format.first_paragraph + format.inserted_paragraphs);
    let pre_end_p = post_end_p - insert.inserted_paragraphs + insert.removed_paragraphs;
    let first = insert.first_paragraph.min(format.first_paragraph);
    EditOutcome {
        start,
        removed_bytes: pre_end - start,
        inserted_bytes: post_end - start,
        first_paragraph: first,
        removed_paragraphs: pre_end_p - first,
        inserted_paragraphs: post_end_p - first,
        layout_scope: if format.layout_scope == LayoutScope::All {
            LayoutScope::All
        } else {
            insert.layout_scope
        },
        ..insert
    }
}

/// Hook of `Session::edit`, run after the text edit and before the selection
/// is set: applies the typing attributes to the inserted text and returns the
/// outcome the layout must be given. It cannot fail: the insertion is done, so
/// a formatting that fails leaves the text unformatted.
pub(super) fn after_text_edit(
    session: &mut Session,
    _start: usize,
    _inserted: usize,
    outcome: EditOutcome,
) -> EditOutcome {
    if !session.typing.armed || outcome.inserted_bytes == 0 {
        return outcome;
    }
    let (low, high) = (outcome.start, outcome.start + outcome.inserted_bytes);
    let Ok(targets) = pieces(&session.doc, low, high) else {
        return outcome;
    };
    let (Some(a), Some(b)) = (targets.first(), targets.last()) else {
        return outcome;
    };
    let first = a.index;
    let ids = session.doc.flat_node_ids(first, b.index - first + 1);
    let ops: Vec<RichOp> = targets
        .iter()
        .map(|p| RichOp::SetCharAttrs {
            para: ids[p.index - first],
            start: p.from,
            end: p.to,
            set: session.typing.set.clone(),
            clear: session.typing.clear.clone(),
        })
        .collect();
    match session
        .doc
        .apply_format(ops, outcome.selection, LayoutScope::Paragraphs, true)
    {
        Ok(format) => merge_outcomes(outcome, &format),
        Err(_) => outcome,
    }
}

impl Session {
    /// `ApplyCharFormat` (US-240).
    pub(super) fn apply_char_format(&mut self, format: CharFormat) -> Result<Response, ErrorCode> {
        self.begin_format(format.base_revision, format.command_id)?;
        let (low, high) = self.selection_range();
        let range = if low != high {
            Some((low, high))
        } else if format.scope == FormatScope::WordOrTyping {
            word_under_caret(&self.doc, low)
        } else {
            None
        };
        let Some((from, to)) = range else {
            return self.arm_typing(&format);
        };
        let targets = pieces(&self.doc, from, to)?;
        let mut changes = vec![Change::default(); targets.len()];
        for edit in &format.edits {
            compute(&self.doc, &targets, edit, &mut changes)?;
        }
        let mut delta = PageDelta::default();
        if let (Some(a), Some(b)) = (targets.first(), targets.last()) {
            let first = a.index;
            let ids = self.doc.flat_node_ids(first, b.index - first + 1);
            let ops: Vec<RichOp> = targets
                .iter()
                .zip(changes)
                .filter(|(_, (set, clear))| !set.is_empty() || !clear.is_empty())
                .map(|(p, (set, clear))| RichOp::SetCharAttrs {
                    para: ids[p.index - first],
                    start: p.from,
                    end: p.to,
                    set,
                    clear,
                })
                .collect();
            if !ops.is_empty() {
                let before = self.doc.revision();
                let outcome = self.doc.apply_format(
                    ops,
                    (self.anchor, self.focus),
                    LayoutScope::Paragraphs,
                    false,
                )?;
                if outcome.revision != before {
                    delta = self.cache.apply_edit(&self.doc, &outcome);
                }
            }
        }
        self.note_command(format.command_id);
        self.finish(delta, DEFAULT_LAYOUT_BUDGET, true)
    }

    /// A caret outside a word: the edits become attributes of the next typed
    /// text (`FormatState` shows them, the next insertion applies them).
    fn arm_typing(&mut self, format: &CharFormat) -> Result<Response, ErrorCode> {
        let (low, _) = self.selection_range();
        let doc = &self.doc;
        let located = doc.locate(low)?;
        // The character before the caret (else the one after it) with the
        // attributes already pending: what the next text would be.
        let mut direct = CharAttrs::default();
        if let Some(p) = LayoutContext::paragraph(doc, located.paragraph) {
            let at = doc.flat_to_text(located.paragraph, located.offset)?;
            let probe = if at > 0 {
                p.text[..at].char_indices().next_back().map(|(i, _)| i)
            } else if !p.text.is_empty() {
                Some(0)
            } else {
                None
            };
            if let Some(i) = probe {
                direct = p.attrs_at(i);
            }
        }
        let inherited = direct.clone();
        direct.clear(&self.typing.clear);
        direct.overlay(&self.typing.set);
        let resolved = resolve(doc, located.paragraph, &direct);
        let targets = [Piece {
            index: located.paragraph,
            from: 0,
            to: 0,
            direct,
            resolved,
        }];
        let mut changes = [Change::default()];
        for edit in &format.edits {
            compute(&self.doc, &targets, edit, &mut changes)?;
        }
        merge_typing(&mut self.typing, &changes[0], &inherited);
        self.note_command(format.command_id);
        self.finish(PageDelta::default(), DEFAULT_LAYOUT_BUDGET, true)
    }

    /// `LoadFonts` (T240-F): the font files the client fetched on demand for a
    /// family the document uses. The engine registers them (it never carries
    /// them in the WASM) and lays the document out again with the faces; the
    /// substitute reported by `FormatState` and `ListFonts` follows. A layout
    /// change, not a document one: no revision, no history, allowed in a
    /// read-only document.
    pub(super) fn load_fonts(&mut self, fonts: Vec<Vec<u8>>) -> Result<Response, ErrorCode> {
        let mut faces = Vec::with_capacity(fonts.len());
        for bytes in fonts {
            faces.push(intern_font(bytes).ok_or(ErrorCode::InvalidBody)?);
        }
        let delta = self.cache.add_fonts(&self.doc, &faces);
        self.fonts_memo = None;
        self.finish(delta, DEFAULT_LAYOUT_BUDGET, true)
    }
}
