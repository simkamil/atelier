// SPDX-License-Identifier: AGPL-3.0-or-later
//! Origin, read-only state, saving and pictures of the rich editor session
//! (US-218, tranche R3b). Layout of the messages: `rich_protocol/io.rs`.

use std::collections::BTreeMap;

use atelier_fmt_ooxml::docx_image::{write_docx_with_pictures, write_new_docx_with_pictures};
use atelier_fmt_ooxml::{
    AcceptError, DocxVariant, DocxWriteOptions, ImportedDocx, NewDocument, RevisionReport,
    StyleDecl, WriteNotice, WrittenDocx, accept_all_revisions,
};
use atelier_model::Transaction;
use atelier_text_model::inline_image::IMAGE_OBJECT_KIND;
use atelier_text_model::plain_io::{
    Eol, FileFormat, LineEndingChoice, LossCategory, SaveOptions, TextIoError, Unmappable,
    formatting_loss, rich_to_text, save_text,
};
use atelier_text_model::rich::flat::FlatEdit;
use atelier_text_model::rich::{NodeId, RichDocument, RichOp, default_rich_document};
use atelier_unicode::encode;

use super::Session;
use super::styles::layout_doc;
use crate::images::{self, ImageInsertError};
use crate::properties::{self, SaveStamp};
use crate::rich_layout::{PageDelta, RichPageCache};
use crate::rich_protocol::{
    self as proto, DEFAULT_LAYOUT_BUDGET, ErrorCode, MAX_NOTICE_BYTES, MAX_NOTICES, Notice,
    Response, SaveFormat, Stamp, notice,
};

/// Distinct pictures one document may receive through `InsertImage` (US-270).
const MAX_IMAGES: usize = 512;
/// Encoded bytes of the pictures one document may receive (US-270).
const MAX_IMAGE_STORE_BYTES: usize = 32 * 1024 * 1024;

/// How a text file was found, to give it back as it was.
pub(super) struct TextOrigin {
    pub format: FileFormat,
    /// Line break that follows each paragraph of a file with several kinds of
    /// break (empty otherwise: the dominant style is used everywhere). A
    /// paragraph created since the open has no entry and takes the dominant
    /// style; a merge keeps the break of the first paragraph.
    pub eols: BTreeMap<NodeId, Eol>,
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
    /// Pictures inserted by `InsertImage` (count, encoded bytes). An upper
    /// bound: an undo does not give a picture back, so a document that has
    /// held 512 pictures stays full for the session (the limits protect the
    /// engine, they are not a count of what the document shows).
    pub images: (usize, usize),
    /// Some parts of the DOCX could not be read for tracked changes.
    pub unreadable: bool,
    /// Document properties of a document without a DOCX package (R7b-2).
    pub props: super::find::Props,
}

impl Origin {
    pub(super) fn new(source: Source) -> Self {
        Self {
            source,
            read_only: false,
            has_revisions: false,
            images: (0, 0),
            unreadable: false,
            props: Default::default(),
        }
    }
}

fn not_scanned() -> Notice {
    Notice {
        code: notice::PARTS_NOT_SCANNED,
        text: "some parts could not be read for tracked changes and comments".into(),
    }
}

fn text_error(error: TextIoError) -> ErrorCode {
    match error {
        TextIoError::TooLarge { .. } | TextIoError::AllocationFailed => ErrorCode::Limit,
        _ => ErrorCode::SaveFailed,
    }
}

/// The line break that follows each line of a file, in order.
pub(super) fn line_breaks(
    bytes: &[u8],
    opened: &atelier_text_model::plain_io::OpenedText,
) -> Vec<Eol> {
    let decoded = atelier_unicode::decode(bytes, opened.encoding);
    let mut chars = decoded.chars().peekable();
    let mut out = Vec::new();
    while let Some(c) = chars.next() {
        match c {
            '\r' if chars.peek() == Some(&'\n') => {
                chars.next();
                out.push(Eol::CrLf);
            }
            '\r' => out.push(Eol::Cr),
            '\n' => out.push(Eol::Lf),
            _ => {}
        }
    }
    out
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
    if report.unreadable > 0 {
        out.push(not_scanned());
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
    pub(super) fn save(
        &mut self,
        format: SaveFormat,
        stamp: &Stamp,
    ) -> Result<Response, ErrorCode> {
        self.origin.props.note_save(format != SaveFormat::Txt);
        let (bytes, notices) = if format == SaveFormat::Txt {
            self.save_text()?
        } else {
            self.save_docx(format, stamp)?
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
        let per_line = match &self.origin.source {
            Source::Text(origin) if !origin.eols.is_empty() => self.save_per_line(origin, &text)?,
            _ => None,
        };
        let (bytes, replaced) = match per_line {
            Some(done) => done,
            None => {
                let saved = save_text(&text, &options).map_err(text_error)?;
                (saved.bytes, saved.replaced)
            }
        };
        for (category, count) in formatting_loss(&self.doc).categories() {
            notices.push(Notice {
                code: loss_code(category),
                text: clip(format!("{}={count}", category.catalog_key())),
            });
        }
        if replaced > 0 {
            notices.push(Notice {
                code: notice::REPLACED_CHARACTERS,
                text: replaced.to_string(),
            });
        }
        Ok((bytes, notices))
    }

    /// A text file with several kinds of line break: every line keeps the break
    /// it had. `None` when the lines no longer match the paragraphs.
    fn save_per_line(
        &self,
        origin: &TextOrigin,
        text: &str,
    ) -> Result<Option<(Vec<u8>, usize)>, ErrorCode> {
        let ids = self.doc.paragraph_ids();
        let lines: Vec<&str> = text.split('\n').collect();
        if lines.len() != ids.len() {
            return Ok(None);
        }
        let (mut out, mut replaced) = (Vec::new(), 0usize);
        for (index, line) in lines.iter().enumerate() {
            let mut origin_format = origin.format;
            // Only the first line carries the BOM.
            origin_format.bom &= index == 0;
            let options = SaveOptions {
                unmappable: Unmappable::Replace,
                line_ending: LineEndingChoice::Lf,
                ..SaveOptions::as_opened(origin_format)
            };
            let saved = save_text(line, &options).map_err(text_error)?;
            out.extend_from_slice(&saved.bytes);
            replaced += saved.replaced;
            if index + 1 < lines.len() {
                let eol = origin
                    .eols
                    .get(&ids[index])
                    .copied()
                    .unwrap_or(origin.format.eol);
                out.extend_from_slice(
                    &encode(eol.as_str(), origin.format.encoding)
                        .map_err(|_| ErrorCode::SaveFailed)?,
                );
            }
        }
        Ok(Some((out, replaced)))
    }

    /// The `SaveStamp` of the client's time, user and version.
    fn save_stamp(stamp: &Stamp) -> Result<SaveStamp, ErrorCode> {
        let non_empty = |s: &str| (!s.is_empty()).then(|| s.to_owned());
        let unix_seconds =
            i64::try_from(stamp.now_ms / 1000).map_err(|_| ErrorCode::InvalidBody)?;
        // A time the date type cannot hold is a malformed request.
        atelier_fmt_ooxml::W3cDateTime::from_unix_seconds(unix_seconds)
            .ok_or(ErrorCode::InvalidBody)?;
        Ok(SaveStamp {
            unix_seconds,
            user: non_empty(&stamp.user),
            app_version: non_empty(&stamp.version),
        })
    }

    fn save_docx(
        &mut self,
        format: SaveFormat,
        stamp: &Stamp,
    ) -> Result<(Vec<u8>, Vec<Notice>), ErrorCode> {
        let stamp = Self::save_stamp(stamp)?;
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
                // Built-in styles instantiated by `ApplyStyle`: written when the document uses them.
                let (used_paragraph, used_character) = self.doc.used_style_ids();
                for style in &self.added_styles {
                    if used_paragraph.contains(&style.id) || used_character.contains(&style.id) {
                        imported
                            .styles
                            .set_style(style.clone())
                            .map_err(|_| ErrorCode::SaveFailed)?;
                        options.added_styles.push(StyleDecl {
                            id: style.id.clone(),
                            name: style.display_name().to_owned(),
                            next: None,
                        });
                    }
                }
                imported.document = self.doc.clone();
                properties::stamp(imported, &stamp).map_err(|_| ErrorCode::SaveFailed)?;
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
                options.save = Some(
                    properties::context(&self.doc, &stamp).map_err(|_| ErrorCode::SaveFailed)?,
                );
                let mut written = write_new_docx_with_pictures(&input, &options).map_err(failed)?;
                // The properties set by the user live outside the model.
                written.bytes = self
                    .origin
                    .props
                    .rewrite(std::mem::take(&mut written.bytes))?;
                written
            }
        };
        let mut notices: Vec<Notice> = written
            .notices
            .iter()
            .map(|n| Notice {
                code: write_notice_code(n),
                text: clip(format!("{n:?}")),
            })
            .collect();
        if self.origin.unreadable {
            notices.push(not_scanned());
        }
        Ok((written.bytes, notices))
    }

    /// `MakeEditableCopy`: the document of the source file as a DOCX where
    /// every tracked change is accepted and the comments are gone, with a
    /// notice for what was done. The session does not change: the client
    /// opens the bytes as a new document (`Open`), which is editable.
    pub(super) fn editable_copy(&mut self, stamp: &Stamp) -> Result<Response, ErrorCode> {
        // A document that is not read-only is its own editable copy: what is
        // saved includes the edits (a read-only session never has any).
        let Source::Docx(imported) = &self.origin.source else {
            return self.save(SaveFormat::Docx, stamp);
        };
        if !self.origin.read_only {
            return self.save(SaveFormat::Docx, stamp);
        }
        let stamp = Self::save_stamp(stamp)?;
        let context = properties::context(&self.doc, &stamp).map_err(|_| ErrorCode::SaveFailed)?;
        let mut accepted =
            accept_all_revisions(&imported.package).map_err(|error| match error {
                AcceptError::Unsupported(_) => ErrorCode::RevisionsUnsupported,
                _ => ErrorCode::SaveFailed,
            })?;
        // The statistics of app.xml follow the accepted text (the model shows it).
        accepted.package.update_on_save(&context);
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
                ImageInsertError::TooLarge => ErrorCode::Limit,
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
            let more = self.cache.apply_edit(layout_doc!(self), &outcome);
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
        let more = self.cache.apply_edit(layout_doc!(self), &outcome);
        super::merge(&mut delta, more);
        let (anchor, focus) = outcome.selection;
        self.select(self.snap(anchor), self.snap(focus), false, None);
        self.origin.images = (count + 1, total + bytes.len());
        self.finish(delta, DEFAULT_LAYOUT_BUDGET, true)
    }
}
