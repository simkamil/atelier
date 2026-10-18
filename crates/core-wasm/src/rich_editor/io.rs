// SPDX-License-Identifier: AGPL-3.0-or-later
//! Origin, read-only state, saving and pictures of the rich editor session
//! (US-218, tranche R3b). Layout of the messages: `rich_protocol/io.rs`.

use atelier_fmt_ooxml::docx_image::{write_docx_with_pictures, write_new_docx_with_pictures};
use atelier_fmt_ooxml::{
    AcceptError, DocxVariant, DocxWriteOptions, ImportedDocx, NewDocument, RevisionReport,
    WriteNotice, WrittenDocx, accept_all_revisions,
};
use atelier_model::Transaction;
use atelier_text_model::inline_image::IMAGE_OBJECT_KIND;
use atelier_text_model::plain_io::{
    FileFormat, LineEndingChoice, LossCategory, SaveOptions, TextIoError, Unmappable,
    formatting_loss, rich_to_text, save_text,
};
use atelier_text_model::rich::flat::FlatEdit;
use atelier_text_model::rich::{RichDocument, RichOp, default_rich_document};

use super::Session;
use crate::images::{self, ImageInsertError};
use crate::rich_layout::{PageDelta, RichPageCache};
use crate::rich_protocol::{
    self as proto, DEFAULT_LAYOUT_BUDGET, ErrorCode, MAX_NOTICE_BYTES, MAX_NOTICES, Notice,
    Response, SaveFormat, notice,
};

/// Distinct pictures one document may receive through `InsertImage` (US-270).
const MAX_IMAGES: usize = 512;
/// Encoded bytes of the pictures one document may receive (US-270).
const MAX_IMAGE_STORE_BYTES: usize = 32 * 1024 * 1024;

/// How a text file was found, to give it back as it was.
pub(super) struct TextOrigin {
    pub format: FileFormat,
    /// Several kinds of line break: saved with the dominant one.
    pub mixed_breaks: bool,
    /// Malformed sequences were replaced by U+FFFD when opening.
    pub replaced_on_open: bool,
}

/// Where the document comes from, and what its file holds that the model
/// does not.
pub(super) enum Source {
    /// A new document: UTF-8 without BOM and CRLF as text, the default
    /// profile as DOCX.
    New,
    Text(TextOrigin),
    /// A DOCX: the import keeps the package (parts the model does not hold,
    /// the skeleton, the baseline the writer compares with).
    Docx(Box<ImportedDocx>),
}

pub(super) struct Origin {
    pub source: Source,
    /// Every edit command answers `RICH_READ_ONLY`.
    pub read_only: bool,
    /// The source had tracked changes or comments.
    pub has_revisions: bool,
    /// Pictures inserted by `InsertImage` (count, encoded bytes).
    pub images: (usize, usize),
}

impl Origin {
    pub(super) fn new(source: Source) -> Self {
        Self {
            source,
            read_only: false,
            has_revisions: false,
            images: (0, 0),
        }
    }
}

fn clip(text: String) -> String {
    let mut text = text;
    if text.len() > MAX_NOTICE_BYTES {
        let mut end = MAX_NOTICE_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

fn capped(mut notices: Vec<Notice>) -> Vec<Notice> {
    if notices.len() > MAX_NOTICES {
        let more = notices.len() - (MAX_NOTICES - 1);
        notices.truncate(MAX_NOTICES - 1);
        notices.push(Notice {
            code: notice::MORE,
            text: more.to_string(),
        });
    }
    notices
}

fn write_notice_code(notice: &WriteNotice) -> u16 {
    let index = match notice {
        WriteNotice::MacrosRemoved { .. } => 0,
        WriteNotice::ParagraphRebuilt { .. } => 1,
        WriteNotice::EnvelopeDropped { .. } => 2,
        WriteNotice::AlternateContentFlattened => 3,
        WriteNotice::BlocksReordered => 4,
        WriteNotice::HighlightApproximated { .. } => 5,
        WriteNotice::HighlightColorApproximated { .. } => 6,
        WriteNotice::UnsupportedCharacter { .. } => 7,
        WriteNotice::MergeStructureChanged { .. } => 8,
        WriteNotice::ThemeNotWritten => 9,
        WriteNotice::HeaderFooterNotWritten => 10,
        WriteNotice::ListDefinitionMissing { .. } => 11,
        WriteNotice::MoreNotices { .. } => 12,
        #[allow(unreachable_patterns)]
        _ => 0xff,
    };
    notice::WRITE_BASE + index
}

fn loss_code(category: LossCategory) -> u16 {
    let index = match category {
        LossCategory::CharacterFormatting => 0,
        LossCategory::ParagraphFormatting => 1,
        LossCategory::Styles => 2,
        LossCategory::Tables => 3,
        LossCategory::Objects => 4,
        LossCategory::Fields => 5,
        LossCategory::Links => 6,
        LossCategory::Bookmarks => 7,
        LossCategory::HeadersFooters => 8,
        LossCategory::Lists => 9,
        LossCategory::SectionBreaks => 10,
    };
    notice::LOSS_BASE + index
}

fn variant_of(format: SaveFormat) -> DocxVariant {
    match format {
        SaveFormat::Docm => DocxVariant::MacroDocument,
        SaveFormat::Dotx => DocxVariant::Template,
        _ => DocxVariant::Document,
    }
}

fn revision_notices(report: &RevisionReport) -> Vec<Notice> {
    let mut out = Vec::new();
    let accepted = report.insertions + report.deletions + report.moves + report.property_changes;
    if accepted > 0 {
        out.push(Notice {
            code: notice::REVISIONS_ACCEPTED,
            text: format!(
                "insertions={} deletions={} moves={} property_changes={}",
                report.insertions, report.deletions, report.moves, report.property_changes
            ),
        });
    }
    if report.comments > 0 {
        out.push(Notice {
            code: notice::COMMENTS_REMOVED,
            text: report.comments.to_string(),
        });
    }
    if report.rows_removed > 0 {
        out.push(Notice {
            code: notice::ROWS_REMOVED,
            text: report.rows_removed.to_string(),
        });
    }
    out
}

/// Kinds of anchored objects that stand for tracked deletions, moves and
/// comments in the model of an imported DOCX.
fn is_revision_mark(kind: &str) -> bool {
    matches!(
        kind,
        "w:del"
            | "w:moveFrom"
            | "w:moveFromRangeStart"
            | "w:moveFromRangeEnd"
            | "w:moveToRangeStart"
            | "w:moveToRangeEnd"
            | "w:commentRangeStart"
            | "w:commentRangeEnd"
            | "w:commentReference"
    )
}

/// Removes from `doc` the marks of deleted text, moves and comments, leaving
/// the text "all changes accepted" that the import already holds.
pub(super) fn show_accepted(doc: &mut RichDocument) -> Result<(), ErrorCode> {
    let mut ops = Vec::new();
    for para in doc.paragraph_ids() {
        let Ok(paragraph) = doc.paragraph(para) else {
            continue;
        };
        for object in paragraph
            .anchored
            .iter()
            .filter(|o| is_revision_mark(&o.kind))
        {
            ops.push(RichOp::RemoveInline {
                para,
                id: object.id,
            });
        }
    }
    if ops.is_empty() {
        return Ok(());
    }
    let transaction = Transaction::new(ops).map_err(|_| ErrorCode::OpenFailed)?;
    doc.apply(&transaction)
        .map(|_| ())
        .map_err(|_| ErrorCode::OpenFailed)
}

impl Session {
    pub(super) fn deny_read_only(&self) -> Result<(), ErrorCode> {
        if self.origin.read_only {
            Err(ErrorCode::ReadOnly)
        } else {
            Ok(())
        }
    }

    /// `Save`: the document as a file, with what the format does not keep.
    pub(super) fn save(&mut self, format: SaveFormat) -> Result<Response, ErrorCode> {
        let (bytes, notices) = if format == SaveFormat::Txt {
            self.save_text()?
        } else {
            self.save_docx(format)?
        };
        if bytes.len() > proto::MAX_SAVE_BYTES {
            return Err(ErrorCode::Limit);
        }
        Ok(Response::SaveData {
            revision: self.revision(),
            bytes,
            notices: capped(notices),
        })
    }

    fn save_text(&self) -> Result<(Vec<u8>, Vec<Notice>), ErrorCode> {
        let text = rich_to_text(&self.doc);
        let mut notices = Vec::new();
        let options = match &self.origin.source {
            Source::Text(origin) => {
                if origin.mixed_breaks {
                    notices.push(Notice {
                        code: notice::LINE_BREAKS_NORMALISED,
                        text: "mixed line breaks saved with the dominant style".into(),
                    });
                }
                if origin.replaced_on_open {
                    notices.push(Notice {
                        code: notice::OPEN_REPLACED,
                        text: "malformed sequences were replaced when the file was opened".into(),
                    });
                }
                SaveOptions {
                    unmappable: Unmappable::Replace,
                    ..SaveOptions::as_opened(origin.format)
                }
            }
            _ => SaveOptions {
                line_ending: LineEndingChoice::CrLf,
                unmappable: Unmappable::Replace,
                ..SaveOptions::default()
            },
        };
        let saved = save_text(&text, &options).map_err(|error| match error {
            TextIoError::TooLarge { .. } | TextIoError::AllocationFailed => ErrorCode::Limit,
            _ => ErrorCode::SaveFailed,
        })?;
        for (category, count) in formatting_loss(&self.doc).categories() {
            notices.push(Notice {
                code: loss_code(category),
                text: clip(format!("{}={count}", category.catalog_key())),
            });
        }
        if saved.replaced > 0 {
            notices.push(Notice {
                code: notice::REPLACED_CHARACTERS,
                text: saved.replaced.to_string(),
            });
        }
        Ok((saved.bytes, notices))
    }

    fn save_docx(&mut self, format: SaveFormat) -> Result<(Vec<u8>, Vec<Notice>), ErrorCode> {
        let modified = self.revision() != 0 || self.origin.images.0 > 0;
        let mut options = DocxWriteOptions {
            variant: Some(variant_of(format)),
            ..DocxWriteOptions::default()
        };
        let failed = |_| ErrorCode::SaveFailed;
        let written: WrittenDocx = match &mut self.origin.source {
            Source::Docx(imported) if self.origin.read_only => {
                // A read-only session has no edit: the import still holds
                // the whole model, the package comes back as it was.
                write_docx_with_pictures(imported, &options).map_err(failed)?
            }
            Source::Docx(imported) => {
                // The writer compares the model with the baseline kept by the
                // import: it gets a working copy of the session's document,
                // never the session's own (the history stays).
                options.regenerate_all = modified;
                imported.document = self.doc.clone();
                let mut result = write_docx_with_pictures(imported, &options);
                if result.is_err() && options.regenerate_all {
                    options.regenerate_all = false;
                    imported.document = self.doc.clone();
                    result = write_docx_with_pictures(imported, &options);
                }
                imported.document = RichDocument::default();
                result.map_err(failed)?
            }
            _ => {
                let defaults = default_rich_document();
                let mut input = NewDocument::from_default(&defaults);
                input.document = &self.doc;
                write_new_docx_with_pictures(&input, &options).map_err(failed)?
            }
        };
        let notices = written
            .notices
            .iter()
            .map(|n| Notice {
                code: write_notice_code(n),
                text: clip(format!("{n:?}")),
            })
            .collect();
        Ok((written.bytes, notices))
    }

    /// `MakeEditableCopy`: the document of the source file as a DOCX where
    /// every tracked change is accepted and the comments are gone, with a
    /// notice for what was done. The session does not change: the client
    /// opens the bytes as a new document (`Open`), which is editable.
    pub(super) fn editable_copy(&mut self) -> Result<Response, ErrorCode> {
        // A document that is not read-only is its own editable copy: what is
        // saved includes the edits (a read-only session never has any).
        let Source::Docx(imported) = &self.origin.source else {
            return self.save(SaveFormat::Docx);
        };
        if !self.origin.read_only {
            return self.save(SaveFormat::Docx);
        }
        let accepted = accept_all_revisions(&imported.package).map_err(|error| match error {
            AcceptError::Unsupported(_) => ErrorCode::RevisionsUnsupported,
            _ => ErrorCode::SaveFailed,
        })?;
        let bytes = accepted
            .package
            .write()
            .map_err(|_| ErrorCode::SaveFailed)?;
        if bytes.len() > proto::MAX_SAVE_BYTES {
            return Err(ErrorCode::Limit);
        }
        Ok(Response::SaveData {
            revision: self.revision(),
            bytes,
            notices: capped(revision_notices(&accepted.report)),
        })
    }

    /// `InsertImage`: the picture replaces the selection, at the caret.
    pub(super) fn insert_image(&mut self, alt: &str, bytes: &[u8]) -> Result<Response, ErrorCode> {
        self.deny_read_only()?;
        let image = images::analyze(bytes)
            .map_err(|error| match error {
                ImageInsertError::TooLarge | ImageInsertError::StoreFull => ErrorCode::Limit,
                ImageInsertError::Unsupported | ImageInsertError::Corrupt => {
                    ErrorCode::ImageInvalid
                }
            })?
            .with_alt(alt);
        let (count, total) = self.origin.images;
        if count >= MAX_IMAGES || total.saturating_add(bytes.len()) > MAX_IMAGE_STORE_BYTES {
            return Err(ErrorCode::Limit);
        }
        let data = image.to_object_data();
        let (start, end) = self.selection_range();
        let mut delta = PageDelta::default();
        let mut at = start;
        if start != end {
            let outcome = self.doc.replace_range(start, end, "")?;
            let more = self.cache.apply_edit(&self.doc, &outcome);
            super::merge(&mut delta, more);
            at = self.snap(start);
        }
        let outcome = match self.doc.insert_object(at, IMAGE_OBJECT_KIND, data) {
            Ok(outcome) => outcome,
            Err(error) => {
                if start != end {
                    // The deletion stays applied (it is one undo step): the
                    // client learns it from the next State.
                    self.select(at, at, false, None);
                    self.carry(delta);
                    self.resync = true;
                }
                return Err(error.into());
            }
        };
        let more = self.cache.apply_edit(&self.doc, &outcome);
        super::merge(&mut delta, more);
        let (anchor, focus) = outcome.selection;
        self.select(self.snap(anchor), self.snap(focus), false, None);
        self.origin.images = (count + 1, total + bytes.len());
        self.finish(delta, DEFAULT_LAYOUT_BUDGET, true)
    }
}
