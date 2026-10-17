// SPDX-License-Identifier: AGPL-3.0-or-later
//! DOCX export of the editor session (US-270 / US-307): paragraphs from the
//! text, inline pictures stored in `word/media` with their original format.

use crate::editor::EditorError;
use crate::images::ImageStore;
use atelier_fmt_ooxml::docx_image::write_new_docx_with_pictures;
use atelier_fmt_ooxml::{DocxWriteOptions, NewDocument, PageSetup};
use atelier_model::Transaction;
use atelier_text_model::inline_image::{IMAGE_OBJECT_KIND, InlineImage};
use atelier_text_model::rich::{AnchoredObject, Inline, RichOp, default_rich_document};
use atelier_unicode::segmentation::grapheme_boundaries;

/// Most paragraphs exported (a longer text is refused, not truncated).
pub(crate) const MAX_EXPORT_PARAGRAPHS: usize = 16_384;

/// Result of an export.
pub(crate) struct Exported {
    pub bytes: Vec<u8>,
    pub notices: u32,
    pub pictures: u32,
}

struct ParagraphSource {
    text: String,
    pictures: Vec<(usize, InlineImage)>,
}

/// Splits `text` at CR, LF or CRLF; picture scalars leave the text and become
/// `(offset, picture)` pairs, moved to the next grapheme boundary.
fn paragraphs(text: &str, images: &ImageStore) -> Result<Vec<ParagraphSource>, EditorError> {
    let mut out = Vec::new();
    let mut current = ParagraphSource {
        text: String::new(),
        pictures: Vec::new(),
    };
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push(core::mem::replace(
                    &mut current,
                    ParagraphSource {
                        text: String::new(),
                        pictures: Vec::new(),
                    },
                ));
                if out.len() > MAX_EXPORT_PARAGRAPHS {
                    return Err(EditorError("EDITOR_EXPORT_LIMIT"));
                }
            }
            _ => {
                if let Some(image) = images.slot_of(c).and_then(|slot| images.get(slot)) {
                    current.pictures.push((current.text.len(), image.clone()));
                } else {
                    current.text.push(c);
                }
            }
        }
    }
    out.push(current);
    if out.len() > MAX_EXPORT_PARAGRAPHS {
        return Err(EditorError("EDITOR_EXPORT_LIMIT"));
    }
    for paragraph in &mut out {
        if paragraph.pictures.is_empty() {
            continue;
        }
        let index = grapheme_boundaries(&paragraph.text);
        for (offset, _) in &mut paragraph.pictures {
            while *offset <= paragraph.text.len() && !index.is_boundary(*offset).unwrap_or(false) {
                *offset += 1;
            }
        }
    }
    Ok(out)
}

/// A4 portrait with the editor's 54 pt margins, so that a picture reduced to
/// the editor's text width keeps the same width in the exported page.
fn page() -> PageSetup {
    PageSetup {
        width: 11906,
        height: 16838,
        margin_top: 1080,
        margin_right: 1080,
        margin_bottom: 1080,
        margin_left: 1080,
        landscape: false,
    }
}

fn failed<E>(_: E) -> EditorError {
    EditorError("EDITOR_EXPORT_FAILED")
}

pub(crate) fn export_docx(text: &str, images: &ImageStore) -> Result<Exported, EditorError> {
    let sources = paragraphs(text, images)?;
    let mut d = default_rich_document();
    let section = *d
        .document
        .section_ids()
        .first()
        .ok_or(EditorError("EDITOR_EXPORT_FAILED"))?;
    let first = *d
        .document
        .paragraph_ids()
        .first()
        .ok_or(EditorError("EDITOR_EXPORT_FAILED"))?;
    let mut ops = Vec::new();
    let mut ids = vec![first];
    if !sources[0].text.is_empty() {
        ops.push(RichOp::InsertText {
            para: first,
            offset: 0,
            text: sources[0].text.clone(),
        });
    }
    for (position, source) in sources.iter().enumerate().skip(1) {
        let subtree = d.document.new_paragraph(&source.text).map_err(failed)?;
        ids.push(subtree.root);
        ops.push(RichOp::InsertBlock {
            parent: Some(section),
            position,
            subtree,
        });
    }
    let mut pictures = 0u32;
    for (para, source) in ids.iter().zip(&sources) {
        for (offset, image) in &source.pictures {
            pictures = pictures.saturating_add(1);
            ops.push(RichOp::InsertInline {
                para: *para,
                item: Inline::Anchored(AnchoredObject {
                    id: d.document.alloc_id().map_err(failed)?,
                    offset: *offset,
                    kind: IMAGE_OBJECT_KIND.into(),
                    data: image.to_object_data(),
                }),
            });
        }
    }
    let transaction = Transaction::new(ops).map_err(failed)?;
    d.document.apply(&transaction).map_err(failed)?;
    let mut input = NewDocument::from_default(&d);
    input.page = page();
    let written = write_new_docx_with_pictures(&input, &DocxWriteOptions::default())
        .map_err(|_| EditorError("EDITOR_EXPORT_FAILED"))?;
    Ok(Exported {
        bytes: written.bytes,
        notices: u32::try_from(written.notices.len()).unwrap_or(u32::MAX),
        pictures,
    })
}
