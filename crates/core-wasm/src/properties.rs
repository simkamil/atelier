// SPDX-License-Identifier: AGPL-3.0-or-later
//! Document properties of a DOCX (US-328): read, edit, and stamp on save.
//!
//! Both commands are stateless `Editor` opcodes carrying the DOCX bytes (like
//! opcode 18) and edit the OPC package only, so the rest of the file is kept
//! byte for byte. Opcodes (all `ATED`, version 1):
//! * 26 `PropertiesRead`: DOCX bytes. Response `ATPP`: `u16 1`, `u16 0`, then
//!   11 editable strings (`u32 len` + UTF-8) in [`FIELDS`] order, the 11
//!   character limits (`u32` each), 3 dates (`u8` 0 none / 1 valid W3CDTF /
//!   2 unreadable text, then a string), 5 optional strings (last modified by,
//!   revision, application, application version, template: `u8` present,
//!   string), total time (`u8` present, `u64`), 6 counters (pages, words,
//!   characters, characters with spaces, lines, paragraphs: `u8` present,
//!   `u64`) and `u32` notice count.
//! * 27 `PropertiesApply`: `u32 docx length`, DOCX bytes, then 11 fields
//!   (`u8` 1 = change, `u32 len`, UTF-8; an empty text removes the property).
//!   Response `ATPA`: `u16 1`, `u16 status`; status 0 is followed by
//!   `u32 changed fields` and the new DOCX bytes (the input untouched when
//!   nothing changed); status 1 (too long) by `u32 field index`, `u32 max`,
//!   `u32 length` and nothing is applied.
//!
//! Saving: staged export kind 3 (see `staged.rs`) carries a [`SaveStamp`] and
//! calls `update_on_save` with the word, character and paragraph counts of
//! the document. Pages and lines need a layout the export does not run, so
//! they are left as stored.

use atelier_fmt_ooxml::{
    DocumentStatistics, ImportedDocx, MAX_COMMENTS_CHARS, MAX_HYPERLINK_BASE_CHARS,
    MAX_KEYWORDS_CHARS, MAX_LANGUAGE_CHARS, MAX_LINE_CHARS, Package, PropertyChanges, PropertyDate,
    PropertyError, SaveContext, W3cDateTime,
};
use atelier_text_model::statistics::{StatisticsOptions, document};

use crate::editor::{EditorError, Writer};

pub const OP_READ: u16 = 26;
pub const OP_APPLY: u16 = 27;

const MAX_DOCX: usize = 8 * 1024 * 1024;
const MAX_RESPONSE: usize = 9 * 1024 * 1024;
const MAX_FIELD_BYTES: usize = 64 * 1024;
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

pub fn is_properties(opcode: u16) -> bool {
    opcode == OP_READ || opcode == OP_APPLY
}

/// Length check of `parse_command` for the two opcodes.
pub fn body_len_ok(opcode: u16, len: usize) -> bool {
    match opcode {
        OP_READ => len <= MAX_DOCX,
        OP_APPLY => (4..=4 + MAX_DOCX + 11 * (5 + MAX_FIELD_BYTES)).contains(&len),
        _ => false,
    }
}

pub fn handle(opcode: u16, body: &[u8]) -> Result<Vec<u8>, EditorError> {
    match opcode {
        OP_READ => read(body),
        OP_APPLY => apply(body),
        _ => Err(INVALID),
    }
}

fn open(bytes: &[u8]) -> Result<Package, EditorError> {
    Package::read(bytes).map_err(|_| EditorError("EDITOR_PROPERTIES_FAILED"))
}

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
    fn u8(&mut self) -> Result<u8, EditorError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, EditorError> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().map_err(|_| INVALID)?,
        ))
    }
    fn u32(&mut self) -> Result<u32, EditorError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().map_err(|_| INVALID)?,
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

fn put_str(out: &mut Writer, s: &str) -> Result<(), EditorError> {
    out.u32(u32::try_from(s.len()).map_err(|_| EditorError("EDITOR_LIMIT"))?);
    out.bytes(s.as_bytes());
    Ok(())
}
fn put_opt(out: &mut Writer, s: Option<&str>) -> Result<(), EditorError> {
    out.u8(u8::from(s.is_some()));
    put_str(out, s.unwrap_or(""))
}
fn put_date(out: &mut Writer, d: Option<&PropertyDate>) -> Result<(), EditorError> {
    match d {
        None => {
            out.u8(0);
            put_str(out, "")
        }
        Some(PropertyDate::Valid(v)) => {
            out.u8(1);
            put_str(out, &v.to_w3cdtf())
        }
        Some(PropertyDate::Invalid(t)) => {
            out.u8(2);
            put_str(out, t)
        }
    }
}
fn put_num(out: &mut Writer, n: Option<u64>) {
    out.u8(u8::from(n.is_some()));
    out.u64(n.unwrap_or(0));
}

fn read(body: &[u8]) -> Result<Vec<u8>, EditorError> {
    let snapshot = open(body)?.properties_snapshot();
    let (e, r) = (&snapshot.editable, &snapshot.read_only);
    let mut out = Writer::with_capacity(4096)?;
    out.bytes(b"ATPP");
    out.u16(1);
    out.u16(0);
    for s in [
        &e.title,
        &e.subject,
        &e.author,
        &e.manager,
        &e.company,
        &e.category,
        &e.keywords,
        &e.comments,
        &e.hyperlink_base,
        &e.content_status,
        &e.language,
    ] {
        put_str(&mut out, s)?;
    }
    for max in LIMITS {
        out.u32(max as u32);
    }
    for d in [&r.created, &r.modified, &r.last_printed] {
        put_date(&mut out, d.as_ref())?;
    }
    for s in [
        &r.last_modified_by,
        &r.revision,
        &r.application,
        &r.app_version,
        &r.template,
    ] {
        put_opt(&mut out, s.as_deref())?;
    }
    put_num(&mut out, r.total_time);
    let s = &r.statistics;
    for n in [
        s.pages,
        s.words,
        s.characters,
        s.characters_with_spaces,
        s.lines,
        s.paragraphs,
    ] {
        put_num(&mut out, n);
    }
    out.u32(u32::try_from(snapshot.notices.len()).unwrap_or(u32::MAX));
    out.finish(MAX_RESPONSE)
}

fn apply(body: &[u8]) -> Result<Vec<u8>, EditorError> {
    let mut c = Cursor(body);
    let docx_len = c.u32()? as usize;
    if docx_len > MAX_DOCX {
        return Err(INVALID);
    }
    let docx = c.take(docx_len)?;
    let mut values: [Option<String>; 11] = Default::default();
    for slot in &mut values {
        let present = c.u8()?;
        let len = c.u32()? as usize;
        match present {
            0 if len == 0 => {}
            1 => *slot = Some(c.text(len, MAX_FIELD_BYTES)?),
            _ => return Err(INVALID),
        }
    }
    if !c.0.is_empty() {
        return Err(INVALID);
    }
    let [
        title,
        subject,
        author,
        manager,
        company,
        category,
        keywords,
        comments,
        hyperlink_base,
        content_status,
        language,
    ] = values;
    let changes = PropertyChanges {
        title,
        subject,
        author,
        manager,
        company,
        category,
        keywords,
        comments,
        hyperlink_base,
        content_status,
        language,
    };
    let mut package = open(docx)?;
    let mut out = Writer::with_capacity(16)?;
    out.bytes(b"ATPA");
    out.u16(1);
    match package.apply_property_changes(&changes) {
        Err(PropertyError::TooLong { field, max, len }) => {
            out.u16(1);
            out.u32(FIELDS.iter().position(|f| *f == field).unwrap_or(0) as u32);
            out.u32(u32::try_from(max).unwrap_or(u32::MAX));
            out.u32(u32::try_from(len).unwrap_or(u32::MAX));
            out.finish(32)
        }
        Ok(report) => {
            out.u16(0);
            out.u32(report.changed.len() as u32);
            if report.changed.is_empty() {
                let mut full = Writer::with_capacity(12 + docx.len())?;
                full.bytes(&out.finish(16)?);
                full.bytes(docx);
                return full.finish(MAX_RESPONSE);
            }
            let written = package
                .write()
                .map_err(|_| EditorError("EDITOR_PROPERTIES_FAILED"))?;
            let mut full = Writer::with_capacity(12 + written.len())?;
            full.bytes(&out.finish(16)?);
            full.bytes(&written);
            full.finish(MAX_RESPONSE)
        }
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
