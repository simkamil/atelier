// SPDX-License-Identifier: AGPL-3.0-or-later
//! Styles of the rich editor session (US-250). Extension module of F0: the
//! handlers of `ListStyles`, `ApplyStyle` and `PreviewStyle`. **Owner:
//! US-250** (with `web/styles-pane.mjs`). Contract:
//! `docs/us-240-format-protocol.md`, sections 5.4 to 5.6.
//!
//! * `ListStyles`: the paragraph and character styles of the catalog, ordered
//!   like Word's gallery (default style, `uiPriority`, name), with what a tile
//!   needs (sample of the resolved style) and the flags of the contract.
//! * `ApplyStyle`: the style reference of every paragraph the selection
//!   touches (`SetParaProps`), or the character style of the selection or of
//!   the word under the caret (`SetCharAttrs`), as one history step. Direct
//!   character **and** paragraph formatting stay: only the reference changes.
//! * `PreviewStyle`: the preview never touches the document, its history, its
//!   revision or its `modified` flag. The touched paragraphs are cloned with
//!   the style applied (`Overlay`); every layout call goes through
//!   `Overlaid`, a `LayoutContext` that serves those clones and delegates the
//!   rest to the document (`layout_doc!`). `cancel_preview` lays the same
//!   paragraphs out again from the real document; the changed pages go to
//!   `Session::pending`, reported by the next `State`. The save reads the
//!   document, never the layout: it cannot see the preview.

use std::collections::BTreeMap;

use atelier_text_model::rich::flat::{
    BlockKind, EditOutcome, FlatEdit, FlatError, FlatIndex, LayoutContext, LayoutScope,
    ListCounters, PageSetup, ParagraphPosition,
};
use atelier_text_model::rich::{
    CharAttrs, NodeId, ParaProps, Paragraph, ResolvedProperties, RichDocument, RichOp, Section,
    Style, StyleCatalog, StyleKind, Theme, Underline,
};

use super::format_char::pieces;
use super::format_state::{clean_name, word_under_caret};
use super::{DEFAULT_LAYOUT_BUDGET, Session};
use crate::rich_layout::{PageDelta, RichPageCache};
use crate::rich_protocol::{
    Command, ErrorCode, FormatStateData, HitMode, MAX_STYLE_LIST, Response, Rgb, StyleEntry,
    StyleFilter, StyleTarget, Tri,
};

/// The preview in progress, if any.
#[derive(Default)]
pub(super) struct Preview {
    pub(super) overlay: Option<Overlay>,
}

/// The paragraphs of a preview as they would be with the style, by flat index.
pub(super) struct Overlay {
    paragraphs: BTreeMap<usize, Paragraph>,
    /// Flat paragraphs `first..=last` are laid out again to show and to undo it.
    first: usize,
    last: usize,
}

impl Overlay {
    /// The edit that tells the page cache which paragraphs to lay out again.
    fn outcome(&self, session: &Session) -> Result<EditOutcome, ErrorCode> {
        let doc = &session.doc;
        let start = doc.paragraph_start(self.first)?;
        let end = session.paragraph_end(self.last)?;
        let bytes = end.saturating_sub(start);
        let count = self.last - self.first + 1;
        Ok(EditOutcome {
            revision: doc.revision(),
            start,
            removed_bytes: bytes,
            inserted_bytes: bytes,
            doc_len: doc.len(),
            first_paragraph: self.first,
            removed_paragraphs: count,
            inserted_paragraphs: count,
            layout_scope: LayoutScope::Paragraphs,
            selection: (session.anchor, session.focus),
            can_undo: doc.can_undo(),
            can_redo: doc.can_redo(),
        })
    }
}

/// The document as the layout sees it: the real one, with the paragraphs of
/// the preview substituted. Without overlay it is the document itself.
pub(super) struct Overlaid<'a> {
    pub(super) doc: &'a RichDocument,
    pub(super) overlay: Option<&'a Overlay>,
}

impl Overlaid<'_> {
    fn substitute(&self, index: usize) -> Option<&Paragraph> {
        self.overlay.and_then(|o| o.paragraphs.get(&index))
    }
}

/// `&dyn LayoutContext` of a session: the document, or the preview over it.
/// A field borrow (not a method of `Session`, which would conflict with
/// `&mut self.cache`). Every layout call of the session passes this.
macro_rules! layout_doc {
    ($session:expr) => {
        &$crate::rich_editor::styles::Overlaid {
            doc: &$session.doc,
            overlay: $session.preview.overlay.as_ref(),
        } as &dyn atelier_text_model::rich::flat::LayoutContext
    };
}
pub(super) use layout_doc;

impl FlatIndex for Overlaid<'_> {
    fn len(&self) -> usize {
        self.doc.len()
    }
    fn paragraph_count(&self) -> usize {
        self.doc.paragraph_count()
    }
    fn locate(&self, position: usize) -> Result<ParagraphPosition, FlatError> {
        self.doc.locate(position)
    }
    fn position(&self, at: ParagraphPosition) -> Result<usize, FlatError> {
        self.doc.position(at)
    }
    fn check_position(&self, position: usize) -> Result<(), FlatError> {
        self.doc.check_position(position)
    }
    fn text(&self, start: usize, end: usize) -> Result<String, FlatError> {
        self.doc.text(start, end)
    }
    fn snap_back(&self, position: usize) -> Result<usize, FlatError> {
        self.doc.snap_back(position)
    }
    fn prev_grapheme(&self, position: usize) -> Result<Option<usize>, FlatError> {
        self.doc.prev_grapheme(position)
    }
    fn next_grapheme(&self, position: usize) -> Result<Option<usize>, FlatError> {
        self.doc.next_grapheme(position)
    }
    fn prev_word(&self, position: usize) -> Result<Option<usize>, FlatError> {
        self.doc.prev_word(position)
    }
    fn next_word(&self, position: usize) -> Result<Option<usize>, FlatError> {
        self.doc.next_word(position)
    }
    fn word_at(&self, position: usize) -> Result<Option<(usize, usize)>, FlatError> {
        self.doc.word_at(position)
    }
}

impl LayoutContext for Overlaid<'_> {
    fn catalog(&self) -> &StyleCatalog {
        LayoutContext::catalog(self.doc)
    }
    fn theme(&self) -> &Theme {
        LayoutContext::theme(self.doc)
    }
    fn paragraph(&self, index: usize) -> Option<&Paragraph> {
        self.substitute(index)
            .or_else(|| LayoutContext::paragraph(self.doc, index))
    }
    fn block_kind(&self, index: usize) -> Result<BlockKind, FlatError> {
        self.doc.block_kind(index)
    }
    fn section_of(&self, index: usize) -> Result<(usize, &Section), FlatError> {
        self.doc.section_of(index)
    }
    fn resolved(&self, index: usize) -> Result<ResolvedProperties, FlatError> {
        match self.substitute(index) {
            Some(p) => self.doc.resolved_for(index, p, None),
            None => self.doc.resolved(index),
        }
    }
    fn resolved_run(&self, index: usize, direct: &CharAttrs) -> Result<CharAttrs, FlatError> {
        match self.substitute(index) {
            Some(p) => self
                .doc
                .resolved_for(index, p, Some(direct))
                .map(|r| r.character),
            None => self.doc.resolved_run(index, direct),
        }
    }
    fn page_setup(&self, section: usize) -> Result<PageSetup, FlatError> {
        self.doc.page_setup(section)
    }
    fn object_resource(&self, object: NodeId) -> Option<&[u8]> {
        self.doc.object_resource(object)
    }
    fn list_counters(&self, index: usize) -> Result<Option<ListCounters>, FlatError> {
        self.doc.list_counters(index)
    }
    fn paragraph_start(&self, index: usize) -> Result<usize, FlatError> {
        self.doc.paragraph_start(index)
    }
}

/// True for the commands that end a preview before they run. Reading
/// commands and the layout pump keep it: the pages they send show the preview.
pub(super) fn cancels_preview(command: &Command) -> bool {
    !matches!(
        command,
        Command::PreviewStyle { .. }
            | Command::Pages { .. }
            | Command::Layout { .. }
            | Command::TextWindow { .. }
            | Command::MarkSaved { .. }
            | Command::AccessibleWindow { .. }
            | Command::SelectionRects { .. }
            | Command::ImageBitmap { .. }
            | Command::Resync
            | Command::Close
            | Command::Save { .. }
            | Command::Search { .. }
            | Command::SearchRects { .. }
            | Command::Statistics
            | Command::PropertiesRead
            | Command::FormatState
            | Command::ListStyles { .. }
            | Command::ListFonts
            | Command::FormatMarks { .. }
            | Command::HitTest {
                mode: HitMode::Probe,
                ..
            }
    )
}

/// Hook of `Session::run` and of `PreviewStyle`: ends the preview in progress
/// by laying its paragraphs out again from the real document. The pages that
/// change are carried to the next `State`. Idempotent.
pub(super) fn cancel_preview(session: &mut Session) {
    let Some(overlay) = session.preview.overlay.take() else {
        return;
    };
    // The paragraphs of the overlay always exist (it ends before any edit);
    // should they not, the cache is rebuilt from the document.
    match overlay.outcome(session) {
        Ok(outcome) => {
            let delta = session.cache.apply_edit(layout_doc!(session), &outcome);
            session.carry(delta);
        }
        Err(_) => session.rebuild_cache(),
    }
}

/// A style of the catalog of `kind`, or `RICH_UNKNOWN_STYLE`.
fn known_style<'a>(
    doc: &'a RichDocument,
    id: &str,
    target: StyleTarget,
) -> Result<&'a Style, ErrorCode> {
    let kind = match target {
        StyleTarget::Paragraph => StyleKind::Paragraph,
        StyleTarget::Character => StyleKind::Character,
    };
    LayoutContext::catalog(doc)
        .style(id)
        .filter(|style| style.kind == kind)
        .ok_or(ErrorCode::UnknownStyle)
}

fn rgb(color: atelier_text_model::rich::Color) -> Rgb {
    Rgb {
        r: color.r,
        g: color.g,
        b: color.b,
    }
}

impl Session {
    /// The styles the selection is in: `(paragraph, character)` identifiers.
    /// A paragraph without style reference is in the default style.
    fn current_styles(&mut self) -> (Option<String>, Option<String>) {
        let state: Option<Box<FormatStateData>> = match self.format_state() {
            Ok(Response::FormatState(state)) => Some(state),
            _ => None,
        };
        let Some(state) = state else {
            return (None, None);
        };
        let paragraph = match &state.paragraph.style {
            Tri::Value(id) => Some(id.clone()),
            Tri::Unset => LayoutContext::catalog(&self.doc)
                .default_style(StyleKind::Paragraph)
                .map(|s| s.id.clone()),
            Tri::Mixed => None,
        };
        let character = match &state.chars.style {
            Tri::Value(id) => Some(id.clone()),
            _ => None,
        };
        (paragraph, character)
    }

    /// `ListStyles`: see the module documentation. With `used_only`, the
    /// styles the document uses, the current ones and the default one.
    pub(super) fn list_styles(
        &mut self,
        filter: StyleFilter,
        used_only: bool,
    ) -> Result<Response, ErrorCode> {
        let (used_paragraph, used_character) = self.doc.used_style_ids();
        let (current_paragraph, current_character) = self.current_styles();
        let catalog = LayoutContext::catalog(&self.doc);
        let mut chosen: Vec<&Style> = catalog
            .styles()
            .filter(|style| {
                !matches!(
                    (style.kind, filter),
                    (StyleKind::Table, _)
                        | (StyleKind::Paragraph, StyleFilter::Character)
                        | (StyleKind::Character, StyleFilter::Paragraph)
                )
            })
            .filter(|style| clean_name(&style.id).as_deref() == Some(style.id.as_str()))
            .collect();
        let is_used = |style: &Style| match style.kind {
            StyleKind::Character => used_character.contains(&style.id),
            _ => used_paragraph.contains(&style.id),
        };
        let is_current = |style: &Style| match style.kind {
            StyleKind::Character => current_character.as_deref() == Some(style.id.as_str()),
            _ => current_paragraph.as_deref() == Some(style.id.as_str()),
        };
        if used_only {
            chosen.retain(|s| is_used(s) || is_current(s) || s.is_default);
        }
        // The order of Word's galleries: the default style, then `uiPriority`,
        // then the name; paragraph styles before character styles.
        chosen.sort_by_cached_key(|s| {
            (
                s.kind == StyleKind::Character,
                !s.is_default,
                s.ui_priority.unwrap_or(99),
                s.display_name().to_lowercase(),
                s.id.clone(),
            )
        });
        chosen.truncate(MAX_STYLE_LIST);
        let mut styles = Vec::with_capacity(chosen.len());
        for style in chosen {
            let resolved = self.doc.resolved_style(&style.id, style.kind);
            let character = resolved
                .as_ref()
                .map(|r| r.character.clone())
                .unwrap_or_default();
            let paragraph = resolved.map(|r| r.paragraph).unwrap_or_default();
            let flags = u8::from(style.is_builtin())
                | u8::from(is_used(style)) << 1
                | u8::from(is_current(style)) << 2
                | u8::from(style.is_default) << 3
                | u8::from(style.quick) << 4;
            let half_points = character
                .size_half_points
                .and_then(|h| u16::try_from(h).ok())
                .filter(|h| (2..=3276).contains(h))
                .unwrap_or(0);
            styles.push(StyleEntry {
                id: style.id.clone(),
                name: style
                    .name
                    .as_deref()
                    .and_then(clean_name)
                    .filter(|n| *n != style.id)
                    .unwrap_or_default(),
                based_on: style
                    .based_on
                    .as_deref()
                    .and_then(clean_name)
                    .unwrap_or_default(),
                target: if style.kind == StyleKind::Character {
                    StyleTarget::Character
                } else {
                    StyleTarget::Paragraph
                },
                flags,
                heading_level: style.heading_level(),
                font: character
                    .font
                    .as_deref()
                    .and_then(clean_name)
                    .unwrap_or_default(),
                half_points,
                char_flags: u8::from(character.bold == Some(true))
                    | u8::from(character.italic == Some(true)) << 1
                    | u8::from(character.strike == Some(true)) << 2,
                underline: match character.underline {
                    None | Some(Underline::None) => 0,
                    Some(Underline::Single) => 1,
                    Some(Underline::Double) => 2,
                    Some(Underline::Thick) => 3,
                    Some(Underline::Dotted) => 4,
                    Some(Underline::Dashed) => 5,
                    Some(Underline::Wave) => 6,
                },
                color: character.color.map(rgb),
                alignment: match paragraph.alignment {
                    None | Some(atelier_text_model::rich::Alignment::Left) => 0,
                    Some(atelier_text_model::rich::Alignment::Center) => 1,
                    Some(atelier_text_model::rich::Alignment::Right) => 2,
                    Some(atelier_text_model::rich::Alignment::Justify) => 3,
                },
            });
        }
        Ok(Response::StyleList {
            revision: self.revision(),
            styles,
        })
    }

    /// The range a character style targets: the selection, else the word
    /// under the caret (`None`: the caret is outside a word).
    fn style_range(&self) -> Option<(usize, usize)> {
        let (low, high) = self.selection_range();
        if low != high {
            Some((low, high))
        } else {
            word_under_caret(&self.doc, low)
        }
    }

    /// `ApplyStyle`: one history step, one revision; nothing recorded when the
    /// style is already applied to everything the command targets.
    pub(super) fn apply_style(
        &mut self,
        base_revision: u64,
        command_id: u32,
        target: StyleTarget,
        id: &str,
    ) -> Result<Response, ErrorCode> {
        self.begin_format(base_revision, command_id)?;
        if target == StyleTarget::Paragraph && id.is_empty() {
            return Err(ErrorCode::UnknownStyle);
        }
        if !id.is_empty() || target == StyleTarget::Paragraph {
            known_style(&self.doc, id, target)?;
        }
        let mut delta = PageDelta::default();
        match target {
            StyleTarget::Paragraph => {
                let touched = self.touched_paragraphs()?;
                let mut ops = Vec::new();
                for t in &touched {
                    let Some(p) = LayoutContext::paragraph(&self.doc, t.index) else {
                        continue;
                    };
                    if p.props.style.as_deref() == Some(id) {
                        continue;
                    }
                    let start = self.doc.paragraph_start(t.index)?;
                    let (para, _) = self.doc.node_position(start)?;
                    ops.push(RichOp::SetParaProps {
                        para,
                        set: ParaProps {
                            style: Some(id.to_owned()),
                            ..ParaProps::default()
                        },
                        clear: ParaProps::default(),
                    });
                }
                if !ops.is_empty() {
                    let before = self.doc.revision();
                    // A style can carry numbering: the whole layout is redone.
                    let outcome = self.doc.apply_format(
                        ops,
                        (self.anchor, self.focus),
                        LayoutScope::All,
                        false,
                    )?;
                    if outcome.revision != before {
                        delta = self.cache.apply_edit(layout_doc!(self), &outcome);
                    }
                }
            }
            StyleTarget::Character => {
                let (set, clear) = char_style_change(id);
                match self.style_range() {
                    Some((from, to)) => {
                        let targets = pieces(&self.doc, from, to)?;
                        if let (Some(a), Some(b)) = (targets.first(), targets.last()) {
                            let first = a.index;
                            let ids = self.doc.flat_node_ids(first, b.index - first + 1);
                            let ops: Vec<RichOp> = targets
                                .iter()
                                .filter(|p| {
                                    // Already the style of the whole stretch.
                                    p.direct.char_style.as_deref()
                                        != if id.is_empty() { None } else { Some(id) }
                                })
                                .map(|p| RichOp::SetCharAttrs {
                                    para: ids[p.index - first],
                                    start: p.from,
                                    end: p.to,
                                    set: set.clone(),
                                    clear: clear.clone(),
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
                                    delta = self.cache.apply_edit(layout_doc!(self), &outcome);
                                }
                            }
                        }
                    }
                    None => {
                        // A caret outside a word: the style of the next typed text.
                        self.typing.set.char_style = set.char_style.clone();
                        self.typing.clear.char_style = clear.char_style.clone();
                        self.typing.armed =
                            !self.typing.set.is_empty() || !self.typing.clear.is_empty();
                    }
                }
            }
        }
        self.note_command(command_id);
        self.finish(delta, DEFAULT_LAYOUT_BUDGET, true)
    }

    /// `PreviewStyle`: shows (replacing any preview) or cancels the preview.
    pub(super) fn preview_style(
        &mut self,
        target: StyleTarget,
        show: bool,
        id: &str,
    ) -> Result<Response, ErrorCode> {
        self.deny_formatting()?;
        if show {
            known_style(&self.doc, id, target)?;
        }
        // `show` replaces the preview in progress; `cancel` is idempotent.
        cancel_preview(self);
        let mut delta = PageDelta::default();
        if show {
            if let Some(overlay) = self.build_overlay(target, id)? {
                let outcome = overlay.outcome(self)?;
                self.preview.overlay = Some(overlay);
                delta = self.cache.apply_edit(layout_doc!(self), &outcome);
            }
        }
        self.finish(delta, DEFAULT_LAYOUT_BUDGET, false)
    }

    /// The paragraphs the style would change, clones with it applied; `None`
    /// when it would change nothing.
    fn build_overlay(&self, target: StyleTarget, id: &str) -> Result<Option<Overlay>, ErrorCode> {
        let mut paragraphs = BTreeMap::new();
        match target {
            StyleTarget::Paragraph => {
                for t in self.touched_paragraphs()? {
                    let Some(p) = LayoutContext::paragraph(&self.doc, t.index) else {
                        continue;
                    };
                    if p.props.style.as_deref() != Some(id) {
                        let mut clone = p.clone();
                        clone.props.style = Some(id.to_owned());
                        paragraphs.insert(t.index, clone);
                    }
                }
            }
            StyleTarget::Character => {
                let (set, clear) = char_style_change(id);
                if let Some((from, to)) = self.style_range() {
                    for piece in pieces(&self.doc, from, to)? {
                        if piece.direct.char_style.as_deref() == Some(id) {
                            continue;
                        }
                        let Some(original) = LayoutContext::paragraph(&self.doc, piece.index)
                        else {
                            continue;
                        };
                        let clone = paragraphs
                            .entry(piece.index)
                            .or_insert_with(|| original.clone());
                        clone
                            .set_char_attrs(piece.from, piece.to, &set, &clear)
                            .map_err(|_| ErrorCode::Limit)?;
                    }
                }
            }
        }
        let (Some(first), Some(last)) = (
            paragraphs.keys().next().copied(),
            paragraphs.keys().next_back().copied(),
        ) else {
            return Ok(None);
        };
        Ok(Some(Overlay {
            paragraphs,
            first,
            last,
        }))
    }
}

/// `(set, clear)` of the character style `id` (empty: remove the style).
fn char_style_change(id: &str) -> (CharAttrs, CharAttrs) {
    let mut set = CharAttrs::default();
    let mut clear = CharAttrs::default();
    if id.is_empty() {
        clear.char_style = Some("x".into());
    } else {
        set.char_style = Some(id.to_owned());
    }
    (set, clear)
}
