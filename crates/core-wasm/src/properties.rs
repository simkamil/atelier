// SPDX-License-Identifier: AGPL-3.0-or-later
//! Document properties of a DOCX (US-328): read, edit, and stamp on save.
//!
//! Reading and editing the properties is done by the rich editor (`rich_editor`,
//! commands `PropertiesRead` and `PropertiesApply`). This module keeps the field
//! table and limits they share, and the save stamp. The stateless opcodes 26 and
//! 27 of the plain-text editor were removed in R8.
//!
//! Saving: staged export kind 3 (see `staged.rs`) carries a [`SaveStamp`] and
//! calls `update_on_save` with the word, character and paragraph counts of
//! the document. Pages and lines need a layout the export does not run, so
//! they are left as stored.

use atelier_fmt_ooxml::{
    DocumentStatistics, ImportedDocx, MAX_COMMENTS_CHARS, MAX_HYPERLINK_BASE_CHARS,
    MAX_KEYWORDS_CHARS, MAX_LANGUAGE_CHARS, MAX_LINE_CHARS, SaveContext, W3cDateTime,
};
use atelier_text_model::statistics::{StatisticsOptions, document};

use crate::editor::EditorError;

const MAX_STAMP_TEXT: usize = 1024;
const INVALID: EditorError = EditorError("EDITOR_INVALID_COMMAND");

/// Field order of the wire format (the names `PropertyError::TooLong` uses).
pub const FIELDS: [&str; 11] = [
    "title",
    "subject",
    "author",
    "manager",
    "company",
    "category",
    "keywords",
    "comments",
    "hyperlink_base",
    "content_status",
    "language",
];
pub const LIMITS: [usize; 11] = [
    MAX_LINE_CHARS,
    MAX_LINE_CHARS,
    MAX_LINE_CHARS,
    MAX_LINE_CHARS,
    MAX_LINE_CHARS,
    MAX_LINE_CHARS,
    MAX_KEYWORDS_CHARS,
    MAX_COMMENTS_CHARS,
    MAX_HYPERLINK_BASE_CHARS,
    MAX_LINE_CHARS,
    MAX_LANGUAGE_CHARS,
];

struct Cursor<'a>(&'a [u8]);
impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], EditorError> {
        if n > self.0.len() {
            return Err(INVALID);
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Ok(head)
    }
    fn u16(&mut self) -> Result<u16, EditorError> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().map_err(|_| INVALID)?,
        ))
    }
    fn i64(&mut self) -> Result<i64, EditorError> {
        Ok(i64::from_le_bytes(
            self.take(8)?.try_into().map_err(|_| INVALID)?,
        ))
    }
    fn text(&mut self, len: usize, max: usize) -> Result<String, EditorError> {
        if len > max {
            return Err(INVALID);
        }
        core::str::from_utf8(self.take(len)?)
            .map(str::to_owned)
            .map_err(|_| EditorError("EDITOR_INVALID_UTF8"))
    }
}

/// What the host supplies when it saves.
pub struct SaveStamp {
    pub unix_seconds: i64,
    pub user: Option<String>,
    pub app_version: Option<String>,
}

/// Parses `i64 unix seconds`, `u16 + user`, `u16 + app version` and returns the
/// DOCX bytes that follow.
pub fn parse_stamp(input: &[u8]) -> Result<(SaveStamp, &[u8]), EditorError> {
    let mut c = Cursor(input);
    let unix_seconds = c.i64()?;
    W3cDateTime::from_unix_seconds(unix_seconds).ok_or(INVALID)?;
    let len = usize::from(c.u16()?);
    let user = c.text(len, MAX_STAMP_TEXT)?;
    let len = usize::from(c.u16()?);
    let app_version = c.text(len, MAX_STAMP_TEXT)?;
    let non_empty = |s: String| (!s.is_empty()).then_some(s);
    Ok((
        SaveStamp {
            unix_seconds,
            user: non_empty(user),
            app_version: non_empty(app_version),
        },
        c.0,
    ))
}

/// Calls `update_on_save` on the package of an imported DOCX.
pub fn stamp(imported: &mut ImportedDocx, stamp: &SaveStamp) -> Result<(), EditorError> {
    let context = context(&imported.document, stamp)?;
    imported.package.update_on_save(&context);
    Ok(())
}

/// The values `update_on_save` takes, from the document (statistics) and the
/// caller's stamp (time, user, version).
pub fn context(
    document_model: &atelier_text_model::rich::RichDocument,
    stamp: &SaveStamp,
) -> Result<SaveContext, EditorError> {
    let now = W3cDateTime::from_unix_seconds(stamp.unix_seconds).ok_or(INVALID)?;
    let counts = document(document_model, &StatisticsOptions::default());
    let n = |v: usize| Some(v as u64);
    Ok(SaveContext {
        now,
        user: stamp.user.clone(),
        application: Some("Atelier".to_owned()),
        app_version: stamp.app_version.clone(),
        statistics: Some(DocumentStatistics {
            pages: None,
            words: n(counts.words),
            characters: n(counts.characters),
            characters_with_spaces: n(counts.characters_with_spaces),
            lines: None,
            paragraphs: n(counts.paragraphs),
        }),
    })
}
