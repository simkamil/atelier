// SPDX-License-Identifier: AGPL-3.0-or-later
//! A retained, independently editable DOCX model with bounded undo and redo.

use atelier_fmt_ooxml::{ImportedBreak, ImportedDocx, ImportedRunStyle, ImportedSymbol};
use atelier_model::{EditKind, History, HistoryError, Transaction};
use atelier_text_model::rich::{CharAttrs, Node, NodeId, RichDocument, RichOp, Subtree};
use atelier_unicode::segmentation::grapheme_boundaries;
use core::fmt;
use core::mem::size_of;

use crate::rich_preview::{self, PreviewError, RichPreview};

const DEFAULT_HISTORY_BUDGET: usize = 4 * 1024 * 1024;
const MAX_HISTORY_BUDGET: usize = 4 * 1024 * 1024;
const HISTORY_ENTRY_OVERHEAD: usize = 256;

/// A caret or selection in one paragraph. Offsets are UTF-8 byte offsets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RichSelection {
    pub paragraph: NodeId,
    pub anchor: usize,
    pub focus: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SessionSnapshot {
    selection: RichSelection,
    breaks: Vec<ImportedBreak>,
    symbols: Vec<ImportedSymbol>,
    run_styles: Vec<ImportedRunStyle>,
}

#[derive(Clone, Copy)]
struct TextEditRange {
    paragraph: NodeId,
    start: usize,
    end: usize,
    inserted_len: usize,
}

struct MetadataView<'a> {
    breaks: &'a [ImportedBreak],
    symbols: &'a [ImportedSymbol],
    run_styles: &'a [ImportedRunStyle],
}

/// Errors returned by the retained rich editing session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RichSessionError {
    StaleRevision { expected: u64, actual: u64 },
    InvalidSelection,
    InvalidText,
    Limit,
    Model(String),
    Preview(String),
}

impl fmt::Display for RichSessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleRevision { expected, actual } => {
                write!(
                    f,
                    "stale rich revision {expected}; current revision is {actual}"
                )
            }
            Self::InvalidSelection => f.write_str("invalid rich selection"),
            Self::InvalidText => f.write_str("invalid replacement text"),
            Self::Limit => f.write_str("rich session limit exceeded"),
            Self::Model(message) | Self::Preview(message) => f.write_str(message),
        }
    }
}
impl std::error::Error for RichSessionError {}

/// A DOCX import retained with its model, package, formatting metadata and history.
pub struct RichSession {
    imported: ImportedDocx,
    history: History<RichOp, SessionSnapshot>,
    selection: RichSelection,
    revision: u64,
}

impl RichSession {
    pub fn open(bytes: &[u8]) -> Result<Self, RichSessionError> {
        Self::open_with_history_budget(bytes, DEFAULT_HISTORY_BUDGET)
    }

    /// Opens a retained DOCX with a caller-selected logical history budget.
    /// The supported budget is capped at four MiB.
    pub fn open_with_history_budget(
        bytes: &[u8],
        budget_bytes: usize,
    ) -> Result<Self, RichSessionError> {
        if budget_bytes > MAX_HISTORY_BUDGET {
            return Err(RichSessionError::Limit);
        }
        let imported = rich_preview::import_preview_docx(bytes).map_err(map_preview_error)?;
        let paragraph = imported
            .paragraph_ids()
            .first()
            .copied()
            .ok_or(RichSessionError::InvalidSelection)?;
        let selection = RichSelection {
            paragraph,
            anchor: 0,
            focus: 0,
        };
        let snapshot = snapshot(&imported, selection);
        let mut session = Self {
            imported,
            history: History::new(snapshot, budget_bytes),
            selection,
            revision: 0,
        };
        // Fail opening the session if its initial model cannot be rendered.
        session.preview()?;
        Ok(session)
    }

    pub fn imported(&self) -> &ImportedDocx {
        &self.imported
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn selection(&self) -> RichSelection {
        self.selection
    }

    pub fn can_undo(&self) -> bool {
        self.history.undo_len() != 0
    }

    pub fn can_redo(&self) -> bool {
        self.history.redo_len() != 0
    }

    pub fn preview(&mut self) -> Result<RichPreview, RichSessionError> {
        rich_preview::render_imported_docx(&mut self.imported).map_err(map_preview_error)
    }

    pub fn select(
        &mut self,
        expected_revision: u64,
        selection: RichSelection,
    ) -> Result<(), RichSessionError> {
        self.check_revision(expected_revision)?;
        validate_selection(&self.imported.document, selection)?;
        self.history
            .move_cursor(snapshot(&self.imported, selection));
        self.selection = selection;
        Ok(())
    }

    pub fn replace(
        &mut self,
        expected_revision: u64,
        para: NodeId,
        start: usize,
        end: usize,
        text: &str,
    ) -> Result<RichPreview, RichSessionError> {
        self.check_revision(expected_revision)?;
        if text.len() > rich_preview::MAX_TEXT_BYTES {
            return Err(RichSessionError::Limit);
        }
        validate_text(text)?;
        let source = self
            .imported
            .document
            .paragraph(para)
            .map_err(|_| RichSessionError::InvalidSelection)?;
        validate_range(&source.text, start, end)?;
        let paragraph_ids = self.imported.paragraph_ids();
        let total_bytes =
            paragraph_ids
                .iter()
                .enumerate()
                .try_fold(0usize, |total, (index, id)| {
                    let paragraph_bytes = self
                        .imported
                        .document
                        .paragraph(*id)
                        .map_err(|_| RichSessionError::InvalidSelection)?
                        .text
                        .len();
                    total
                        .checked_add(paragraph_bytes)
                        .and_then(|n| n.checked_add(usize::from(index > 0)))
                        .ok_or(RichSessionError::Limit)
                })?;
        let projected_bytes = total_bytes
            .checked_sub(end - start)
            .and_then(|n| n.checked_add(text.len()))
            .ok_or(RichSessionError::Limit)?;
        if projected_bytes > rich_preview::MAX_TEXT_BYTES {
            return Err(RichSessionError::Limit);
        }
        let inserted_end = start
            .checked_add(text.len())
            .ok_or(RichSessionError::Limit)?;
        let no_op = source.text.get(start..end) == Some(text);
        let next_revision = if no_op {
            None
        } else {
            Some(
                self.revision
                    .checked_add(1)
                    .ok_or(RichSessionError::Limit)?,
            )
        };

        let before_paragraph = source.clone();
        let inherited_attrs = inherited_direct_attrs(source, start);
        let inherited_style = inherited_style(&self.imported.run_styles, para, &source.text, start);
        let after_selection = RichSelection {
            paragraph: para,
            anchor: inserted_end,
            focus: inserted_end,
        };

        if no_op {
            let preview = self.preview()?;
            self.history
                .move_cursor(snapshot(&self.imported, after_selection));
            self.selection = after_selection;
            return Ok(preview);
        }

        let mut candidate_document = self.imported.document.clone();
        let mut candidate_breaks = self.imported.breaks.clone();
        let mut candidate_symbols = self.imported.symbols.clone();
        let mut candidate_run_styles = self.imported.run_styles.clone();
        remap_metadata(
            TextEditRange {
                paragraph: para,
                start,
                end,
                inserted_len: text.len(),
            },
            inherited_style,
            &mut candidate_breaks,
            &mut candidate_symbols,
            &mut candidate_run_styles,
        );

        let mut operations = Vec::with_capacity(3);
        if start < end {
            operations.push(RichOp::DeleteText { para, start, end });
        }
        if !text.is_empty() {
            operations.push(RichOp::InsertText {
                para,
                offset: start,
                text: text.to_owned(),
            });
            if !inherited_attrs.is_empty() {
                operations.push(RichOp::SetCharAttrs {
                    para,
                    start,
                    end: inserted_end,
                    set: inherited_attrs.clone(),
                    clear: CharAttrs::default(),
                });
            }
        }
        let transaction =
            Transaction::new(operations).map_err(|e| RichSessionError::Model(e.to_string()))?;
        let candidate_selection = SessionSnapshot {
            selection: after_selection,
            breaks: candidate_breaks.clone(),
            symbols: candidate_symbols.clone(),
            run_styles: candidate_run_styles.clone(),
        };
        let after_paragraph =
            edited_paragraph(&before_paragraph, start, end, text, &inherited_attrs)?;
        let cost_bound = history_cost_bound(
            &self.imported.document,
            para,
            &after_paragraph,
            MetadataView {
                breaks: &self.imported.breaks,
                symbols: &self.imported.symbols,
                run_styles: &self.imported.run_styles,
            },
            MetadataView {
                breaks: &candidate_breaks,
                symbols: &candidate_symbols,
                run_styles: &candidate_run_styles,
            },
        )?;
        let mut candidate_history = self.history.clone();
        candidate_history
            .apply(
                &mut candidate_document,
                transaction,
                candidate_selection,
                self.revision,
                EditKind::Other,
                cost_bound,
            )
            .map_err(map_history_error)?;

        let rendered = self.render_candidate(
            candidate_document,
            candidate_breaks,
            candidate_symbols,
            candidate_run_styles,
        );
        match rendered {
            Ok(preview) => {
                self.history = candidate_history;
                self.selection = after_selection;
                self.revision = next_revision.expect("non-noop edit has next revision");
                Ok(preview)
            }
            Err(error) => Err(error),
        }
    }

    pub fn undo(&mut self, expected_revision: u64) -> Result<RichPreview, RichSessionError> {
        self.navigate_history(expected_revision, false)
    }

    pub fn redo(&mut self, expected_revision: u64) -> Result<RichPreview, RichSessionError> {
        self.navigate_history(expected_revision, true)
    }

    fn navigate_history(
        &mut self,
        expected_revision: u64,
        redo: bool,
    ) -> Result<RichPreview, RichSessionError> {
        self.check_revision(expected_revision)?;
        let has_entry = if redo {
            self.can_redo()
        } else {
            self.can_undo()
        };
        if !has_entry {
            return self.preview();
        }
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or(RichSessionError::Limit)?;
        let mut candidate_document = self.imported.document.clone();
        let mut candidate_history = self.history.clone();
        let changed = if redo {
            candidate_history.redo(&mut candidate_document)
        } else {
            candidate_history.undo(&mut candidate_document)
        }
        .map_err(map_history_error)?;
        if !changed {
            return self.preview();
        }
        let restored = candidate_history.selection().clone();
        let rendered = self.render_candidate(
            candidate_document,
            restored.breaks.clone(),
            restored.symbols.clone(),
            restored.run_styles.clone(),
        )?;
        self.history = candidate_history;
        self.selection = restored.selection;
        self.revision = next_revision;
        Ok(rendered)
    }

    fn render_candidate(
        &mut self,
        document: RichDocument,
        breaks: Vec<ImportedBreak>,
        symbols: Vec<ImportedSymbol>,
        run_styles: Vec<ImportedRunStyle>,
    ) -> Result<RichPreview, RichSessionError> {
        let old_document = core::mem::replace(&mut self.imported.document, document);
        let old_breaks = core::mem::replace(&mut self.imported.breaks, breaks);
        let old_symbols = core::mem::replace(&mut self.imported.symbols, symbols);
        let old_run_styles = core::mem::replace(&mut self.imported.run_styles, run_styles);
        match rich_preview::render_imported_docx(&mut self.imported) {
            Ok(preview) => Ok(preview),
            Err(error) => {
                self.imported.document = old_document;
                self.imported.breaks = old_breaks;
                self.imported.symbols = old_symbols;
                self.imported.run_styles = old_run_styles;
                Err(map_preview_error(error))
            }
        }
    }

    fn check_revision(&self, expected: u64) -> Result<(), RichSessionError> {
        if expected == self.revision {
            Ok(())
        } else {
            Err(RichSessionError::StaleRevision {
                expected,
                actual: self.revision,
            })
        }
    }
}

fn snapshot(imported: &ImportedDocx, selection: RichSelection) -> SessionSnapshot {
    SessionSnapshot {
        selection,
        breaks: imported.breaks.clone(),
        symbols: imported.symbols.clone(),
        run_styles: imported.run_styles.clone(),
    }
}

fn validate_selection(
    document: &RichDocument,
    selection: RichSelection,
) -> Result<(), RichSessionError> {
    let paragraph = document
        .paragraph(selection.paragraph)
        .map_err(|_| RichSessionError::InvalidSelection)?;
    validate_offset(&paragraph.text, selection.anchor)?;
    validate_offset(&paragraph.text, selection.focus)
}

fn validate_range(text: &str, start: usize, end: usize) -> Result<(), RichSessionError> {
    if start > end {
        return Err(RichSessionError::InvalidSelection);
    }
    validate_offset(text, start)?;
    validate_offset(text, end)
}

fn validate_offset(text: &str, offset: usize) -> Result<(), RichSessionError> {
    grapheme_boundaries(text)
        .is_boundary(offset)
        .map_err(|_| RichSessionError::InvalidSelection)?
        .then_some(())
        .ok_or(RichSessionError::InvalidSelection)
}

fn validate_text(text: &str) -> Result<(), RichSessionError> {
    if text.contains(['\r', '\n'])
        || text.chars().any(|c| {
            (c <= '\u{0008}')
                || ('\u{000b}'..='\u{001f}').contains(&c)
                || ('\u{007f}'..='\u{009f}').contains(&c)
                || matches!(c, '\u{2028}' | '\u{2029}' | '\u{feff}')
        })
    {
        return Err(RichSessionError::InvalidText);
    }
    Ok(())
}

fn inherited_direct_attrs(
    paragraph: &atelier_text_model::rich::Paragraph,
    offset: usize,
) -> CharAttrs {
    if paragraph.text.is_empty() {
        return CharAttrs::default();
    }
    let source = if offset == 0 {
        0
    } else {
        grapheme_boundaries(&paragraph.text)
            .boundaries()
            .iter()
            .copied()
            .take_while(|boundary| *boundary < offset)
            .last()
            .unwrap_or(0)
    };
    paragraph.attrs_at(source)
}

fn inherited_style(
    styles: &[ImportedRunStyle],
    paragraph: NodeId,
    text: &str,
    offset: usize,
) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let source = if offset == 0 {
        0
    } else {
        grapheme_boundaries(text)
            .boundaries()
            .iter()
            .copied()
            .take_while(|boundary| *boundary < offset)
            .last()
            .unwrap_or(0)
    };
    styles
        .iter()
        .find(|run| run.paragraph == paragraph && run.start <= source && source < run.end)
        .and_then(|run| run.style_id.clone())
}

fn remap_metadata(
    edit: TextEditRange,
    inherited: Option<String>,
    breaks: &mut Vec<ImportedBreak>,
    symbols: &mut Vec<ImportedSymbol>,
    styles: &mut Vec<ImportedRunStyle>,
) {
    let TextEditRange {
        paragraph,
        start,
        end,
        inserted_len,
    } = edit;
    let deleted = end - start;
    if deleted != 0 {
        breaks.retain_mut(|item| {
            if item.paragraph != paragraph {
                return true;
            }
            if start <= item.offset && item.offset < end {
                return false;
            }
            if item.offset >= end {
                item.offset -= deleted;
            }
            true
        });
        symbols.retain_mut(|item| {
            if item.paragraph != paragraph {
                return true;
            }
            if start <= item.offset && item.offset < end {
                return false;
            }
            if item.offset >= end {
                item.offset -= deleted;
            }
            true
        });
        styles.retain_mut(|item| {
            if item.paragraph != paragraph {
                return true;
            }
            let map = |position: usize| {
                if position <= start {
                    position
                } else if position >= end {
                    position - deleted
                } else {
                    start
                }
            };
            item.start = map(item.start);
            item.end = map(item.end);
            item.start < item.end
        });
    }
    if inserted_len != 0 {
        for item in breaks.iter_mut().filter(|item| item.paragraph == paragraph) {
            if item.offset >= start {
                item.offset += inserted_len;
            }
        }
        for item in symbols
            .iter_mut()
            .filter(|item| item.paragraph == paragraph)
        {
            if item.offset >= start {
                item.offset += inserted_len;
            }
        }
        let mut output = Vec::with_capacity(styles.len() + 1);
        for item in styles.drain(..) {
            if item.paragraph != paragraph || item.end <= start {
                output.push(item);
            } else if item.start >= start {
                output.push(ImportedRunStyle {
                    start: item.start + inserted_len,
                    end: item.end + inserted_len,
                    ..item
                });
            } else {
                let mut left = item.clone();
                left.end = start;
                if left.start < left.end {
                    output.push(left);
                }
                let mut right = item;
                right.start = start + inserted_len;
                right.end += inserted_len;
                if right.start < right.end {
                    output.push(right);
                }
            }
        }
        if let Some(style_id) = inherited {
            output.push(ImportedRunStyle {
                paragraph,
                start,
                end: start + inserted_len,
                style_id: Some(style_id),
            });
        }
        output.sort_by_key(|item| (item.paragraph, item.start, item.end));
        let mut normalized: Vec<ImportedRunStyle> = Vec::with_capacity(output.len());
        for item in output {
            if let Some(last) = normalized.last_mut() {
                if last.paragraph == item.paragraph
                    && last.end == item.start
                    && last.style_id == item.style_id
                {
                    last.end = item.end;
                    continue;
                }
            }
            normalized.push(item);
        }
        *styles = normalized;
    }
}

fn edited_paragraph(
    before: &atelier_text_model::rich::Paragraph,
    start: usize,
    end: usize,
    text: &str,
    attrs: &CharAttrs,
) -> Result<atelier_text_model::rich::Paragraph, RichSessionError> {
    let mut after = before.clone();
    if start < end {
        after
            .delete_text(start, end)
            .map_err(|e| RichSessionError::Model(e.to_string()))?;
    }
    if !text.is_empty() {
        after
            .insert_text(start, text)
            .map_err(|e| RichSessionError::Model(e.to_string()))?;
        if !attrs.is_empty() {
            after
                .set_char_attrs(start, start + text.len(), attrs, &CharAttrs::default())
                .map_err(|e| RichSessionError::Model(e.to_string()))?;
        }
    }
    Ok(after)
}

fn history_cost_bound(
    before_document: &RichDocument,
    paragraph: NodeId,
    after: &atelier_text_model::rich::Paragraph,
    before_metadata: MetadataView<'_>,
    after_metadata: MetadataView<'_>,
) -> Result<usize, RichSessionError> {
    let before = before_document
        .paragraph(paragraph)
        .map_err(|e| RichSessionError::Model(e.to_string()))?;
    let model_snapshot_bytes = subtree_snapshot_cost(before)?
        .checked_add(subtree_snapshot_cost(after)?)
        .ok_or(RichSessionError::Limit)?;
    let metadata_snapshot_bytes = metadata_cost(before_metadata)?
        .checked_add(metadata_cost(after_metadata)?)
        .ok_or(RichSessionError::Limit)?;
    model_snapshot_bytes
        .checked_mul(2)
        .and_then(|n| n.checked_add(metadata_snapshot_bytes))
        .and_then(|n| n.checked_add(size_of::<RichOp>().checked_mul(4)?))
        .and_then(|n| n.checked_add(size_of::<SessionSnapshot>().checked_mul(2)?))
        .and_then(|n| n.checked_add(HISTORY_ENTRY_OVERHEAD))
        .ok_or(RichSessionError::Limit)
}

fn subtree_snapshot_cost(
    paragraph: &atelier_text_model::rich::Paragraph,
) -> Result<usize, RichSessionError> {
    paragraph_cost(paragraph)?
        .checked_add(size_of::<RichOp>())
        .and_then(|n| n.checked_add(size_of::<Subtree>()))
        .and_then(|n| n.checked_add(size_of::<(NodeId, Node)>()))
        .and_then(|n| n.checked_add(64))
        .ok_or(RichSessionError::Limit)
}

fn paragraph_cost(
    paragraph: &atelier_text_model::rich::Paragraph,
) -> Result<usize, RichSessionError> {
    let mut size = size_of::<atelier_text_model::rich::Paragraph>()
        .checked_add(paragraph.text.len())
        .and_then(|n| {
            n.checked_add(
                paragraph
                    .runs
                    .len()
                    .checked_mul(size_of::<atelier_text_model::rich::CharRun>())?,
            )
        })
        .and_then(|n| {
            n.checked_add(
                paragraph
                    .anchored
                    .len()
                    .checked_mul(size_of::<atelier_text_model::rich::AnchoredObject>())?,
            )
        })
        .and_then(|n| {
            n.checked_add(
                paragraph
                    .fields
                    .len()
                    .checked_mul(size_of::<atelier_text_model::rich::Field>())?,
            )
        })
        .and_then(|n| {
            n.checked_add(
                paragraph
                    .bookmarks
                    .len()
                    .checked_mul(size_of::<atelier_text_model::rich::Bookmark>())?,
            )
        })
        .and_then(|n| {
            n.checked_add(
                paragraph
                    .links
                    .len()
                    .checked_mul(size_of::<atelier_text_model::rich::Link>())?,
            )
        })
        .ok_or(RichSessionError::Limit)?;
    for run in &paragraph.runs {
        size = size
            .checked_add(char_attrs_cost(&run.attrs)?)
            .ok_or(RichSessionError::Limit)?;
    }
    size = size
        .checked_add(paragraph.props.style.as_ref().map_or(0, String::len))
        .and_then(|n| n.checked_add(opaque_attrs_cost(&paragraph.props.opaque).ok()?))
        .ok_or(RichSessionError::Limit)?;
    for item in &paragraph.anchored {
        size = size
            .checked_add(item.kind.len())
            .and_then(|n| n.checked_add(item.data.len()))
            .ok_or(RichSessionError::Limit)?;
    }
    for item in &paragraph.fields {
        size = size
            .checked_add(item.instruction.len())
            .ok_or(RichSessionError::Limit)?;
    }
    for item in &paragraph.bookmarks {
        size = size
            .checked_add(item.name.len())
            .ok_or(RichSessionError::Limit)?;
    }
    for item in &paragraph.links {
        let target = match &item.target {
            atelier_text_model::rich::LinkTarget::External(value)
            | atelier_text_model::rich::LinkTarget::Bookmark(value) => value.len(),
        };
        size = size
            .checked_add(target)
            .and_then(|n| n.checked_add(item.tooltip.as_ref().map_or(0, String::len)))
            .ok_or(RichSessionError::Limit)?;
    }
    Ok(size)
}

fn char_attrs_cost(attrs: &CharAttrs) -> Result<usize, RichSessionError> {
    attrs
        .font
        .as_ref()
        .map_or(0, String::len)
        .checked_add(opaque_attrs_cost(&attrs.opaque)?)
        .ok_or(RichSessionError::Limit)
}

fn opaque_attrs_cost(
    attrs: &[atelier_text_model::rich::OpaqueAttr],
) -> Result<usize, RichSessionError> {
    attrs.iter().try_fold(
        attrs
            .len()
            .checked_mul(size_of::<atelier_text_model::rich::OpaqueAttr>())
            .ok_or(RichSessionError::Limit)?,
        |size, attr| {
            size.checked_add(attr.namespace.len())
                .and_then(|n| n.checked_add(attr.name.len()))
                .and_then(|n| n.checked_add(attr.value.len()))
                .ok_or(RichSessionError::Limit)
        },
    )
}

fn metadata_cost(metadata: MetadataView<'_>) -> Result<usize, RichSessionError> {
    let mut size = metadata
        .breaks
        .len()
        .checked_mul(size_of::<ImportedBreak>())
        .and_then(|n| {
            n.checked_add(
                metadata
                    .symbols
                    .len()
                    .checked_mul(size_of::<ImportedSymbol>())?,
            )
        })
        .and_then(|n| {
            n.checked_add(
                metadata
                    .run_styles
                    .len()
                    .checked_mul(size_of::<ImportedRunStyle>())?,
            )
        })
        .ok_or(RichSessionError::Limit)?;
    for item in metadata.breaks {
        size = size
            .checked_add(item.kind.len())
            .and_then(|n| n.checked_add(item.break_type.as_ref().map_or(0, String::len)))
            .ok_or(RichSessionError::Limit)?;
    }
    for item in metadata.symbols {
        size = size
            .checked_add(item.font.len())
            .and_then(|n| n.checked_add(item.character_code.len()))
            .ok_or(RichSessionError::Limit)?;
    }
    for item in metadata.run_styles {
        size = size
            .checked_add(item.style_id.as_ref().map_or(0, String::len))
            .ok_or(RichSessionError::Limit)?;
    }
    Ok(size)
}

fn map_history_error(error: HistoryError<atelier_text_model::rich::RichError>) -> RichSessionError {
    match error {
        HistoryError::BudgetExceeded { .. } | HistoryError::CostOverflow => RichSessionError::Limit,
        HistoryError::StorageReservationFailed => RichSessionError::Limit,
        HistoryError::Transaction { source, .. } => RichSessionError::Model(source.to_string()),
    }
}

fn map_preview_error(error: PreviewError) -> RichSessionError {
    match error {
        PreviewError::InputTooLarge | PreviewError::Limit(_) => RichSessionError::Limit,
        other => RichSessionError::Preview(other.to_string()),
    }
}
