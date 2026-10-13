// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-328: document properties (`docProps/core.xml` and `docProps/app.xml`).
//!
//! The package layer already reads and writes the two parts; this module adds
//! what a Properties dialog and a save command need: the fields the first
//! version lacked, W3CDTF dates without a clock, a snapshot / change API with
//! Word's rules, the save-time update, a part writer that edits the original
//! bytes in place (so a change to one field changes one element), and a
//! neutral mapping to the PDF information dictionary.
//!
//! Nothing here reads a clock or any system API: the caller supplies "now".

use crate::{
    APP_NS, AppProperties, CORE_NS, CoreProperties, DC_NS, DCTERMS_NS, OpcError, Package, XmlLimits,
};
use std::fmt;

const XSI_NS: &str = "http://www.w3.org/2001/XMLSchema-instance";
const DCMITYPE_NS: &str = "http://purl.org/dc/dcmitype/";
const VT_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes";

/// Longest single-line field (title, subject, author, manager, company,
/// category, content status), in characters.
pub const MAX_LINE_CHARS: usize = 255;
/// Longest keywords string, in characters.
pub const MAX_KEYWORDS_CHARS: usize = 1024;
/// Longest comments string, in characters.
pub const MAX_COMMENTS_CHARS: usize = 4096;
/// Longest hyperlink base, in characters.
pub const MAX_HYPERLINK_BASE_CHARS: usize = 2048;
/// Longest language tag, in characters.
pub const MAX_LANGUAGE_CHARS: usize = 35;
/// Longest date text the parser looks at, in bytes.
const MAX_DATE_BYTES: usize = 64;

// ---------------------------------------------------------------------------
// Dates
// ---------------------------------------------------------------------------

/// How much of a W3CDTF value is present, and so how it is written back.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatePrecision {
    Year,
    Month,
    Day,
    Minute,
    Second,
    /// Seconds with this many fraction digits (1 to 9).
    Fraction(u8),
}

/// A W3CDTF date or date-time, without any clock or time-zone database.
///
/// Missing parts of a coarse value are 1 (month, day) or 0 (time). `offset_minutes`
/// is `None` when the text carried no zone designator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct W3cDateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub nanosecond: u32,
    /// Offset from UTC in minutes, east positive.
    pub offset_minutes: Option<i16>,
    pub precision: DatePrecision,
}

/// Why a date text was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DateError(pub &'static str);
impl fmt::Display for DateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid W3CDTF date: {}", self.0)
    }
}
impl std::error::Error for DateError {}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
fn days_in_month(y: i64, m: u8) -> u8 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
    }
}
/// Days since 1970-01-01 of a proleptic Gregorian date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
fn civil_from_days(z: i64) -> (i64, u8, u8) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn digits(b: &[u8], pos: &mut usize, n: usize) -> Option<u32> {
    let slice = b.get(*pos..pos.checked_add(n)?)?;
    let mut v = 0u32;
    for &c in slice {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v * 10 + u32::from(c - b'0');
    }
    *pos += n;
    Some(v)
}

impl W3cDateTime {
    /// A UTC date-time with second precision, validated.
    pub fn utc(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> Option<Self> {
        let d = Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
            nanosecond: 0,
            offset_minutes: Some(0),
            precision: DatePrecision::Second,
        };
        d.is_valid().then_some(d)
    }

    fn is_valid(&self) -> bool {
        (1..=9999).contains(&self.year)
            && (1..=12).contains(&self.month)
            && self.day >= 1
            && self.day <= days_in_month(i64::from(self.year), self.month)
            && self.hour < 24
            && self.minute < 60
            && self.second < 60
            && self.nanosecond < 1_000_000_000
            && self.offset_minutes.is_none_or(|o| o.abs() < 24 * 60)
    }

    /// Seconds since 1970-01-01T00:00:00Z (a missing offset counts as UTC).
    pub fn unix_seconds(&self) -> i64 {
        let days = days_from_civil(
            i64::from(self.year),
            i64::from(self.month),
            i64::from(self.day),
        );
        days * 86_400
            + i64::from(self.hour) * 3600
            + i64::from(self.minute) * 60
            + i64::from(self.second)
            - i64::from(self.offset_minutes.unwrap_or(0)) * 60
    }

    /// A UTC value with second precision from a Unix time; `None` outside
    /// years 1 to 9999.
    pub fn from_unix_seconds(secs: i64) -> Option<Self> {
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        let (y, m, d) = civil_from_days(days);
        let year = u16::try_from(y).ok()?;
        Self::utc(
            year,
            m,
            d,
            (rem / 3600) as u8,
            (rem % 3600 / 60) as u8,
            (rem % 60) as u8,
        )
    }

    /// The same instant in UTC (offset `Some(0)`), keeping the precision and
    /// the fraction. `None` outside years 1 to 9999.
    pub fn to_utc(&self) -> Option<Self> {
        let mut u = Self::from_unix_seconds(self.unix_seconds())?;
        u.nanosecond = self.nanosecond;
        u.precision = self.precision;
        Some(u)
    }

    /// Parses the common W3CDTF forms: `YYYY`, `YYYY-MM`, `YYYY-MM-DD`,
    /// `YYYY-MM-DDThh:mm`, `...:ss`, `...:ss.s+`, each time form with `Z`,
    /// `+hh:mm`, `-hh:mm`, `+hhmm`, or no zone at all (tolerated, offset
    /// `None`). Surrounding ASCII whitespace is ignored.
    pub fn parse(text: &str) -> Result<Self, DateError> {
        let t = text.trim_matches(|c: char| c.is_ascii_whitespace());
        if t.is_empty() {
            return Err(DateError("empty"));
        }
        if t.len() > MAX_DATE_BYTES {
            return Err(DateError("too long"));
        }
        let b = t.as_bytes();
        let mut pos = 0usize;
        let bad = DateError("malformed");
        let mut d = Self {
            year: digits(b, &mut pos, 4).ok_or(bad)? as u16,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            nanosecond: 0,
            offset_minutes: None,
            precision: DatePrecision::Year,
        };
        if pos < b.len() {
            if b[pos] != b'-' {
                return Err(bad);
            }
            pos += 1;
            d.month = digits(b, &mut pos, 2).ok_or(bad)? as u8;
            d.precision = DatePrecision::Month;
            if pos < b.len() {
                if b[pos] != b'-' {
                    return Err(bad);
                }
                pos += 1;
                d.day = digits(b, &mut pos, 2).ok_or(bad)? as u8;
                d.precision = DatePrecision::Day;
                if pos < b.len() {
                    if b[pos] != b'T' {
                        return Err(bad);
                    }
                    pos += 1;
                    d.hour = digits(b, &mut pos, 2).ok_or(bad)? as u8;
                    if b.get(pos) != Some(&b':') {
                        return Err(bad);
                    }
                    pos += 1;
                    d.minute = digits(b, &mut pos, 2).ok_or(bad)? as u8;
                    d.precision = DatePrecision::Minute;
                    if b.get(pos) == Some(&b':') {
                        pos += 1;
                        d.second = digits(b, &mut pos, 2).ok_or(bad)? as u8;
                        d.precision = DatePrecision::Second;
                        if b.get(pos) == Some(&b'.') {
                            pos += 1;
                            let start = pos;
                            let mut nanos = 0u32;
                            while let Some(c) = b.get(pos).filter(|c| c.is_ascii_digit()) {
                                if pos - start < 9 {
                                    nanos = nanos * 10 + u32::from(c - b'0');
                                }
                                pos += 1;
                            }
                            let n = pos - start;
                            if n == 0 {
                                return Err(bad);
                            }
                            let kept = n.min(9);
                            d.nanosecond = nanos * 10u32.pow((9 - kept) as u32);
                            d.precision = DatePrecision::Fraction(kept as u8);
                        }
                    }
                    match b.get(pos) {
                        None => {}
                        Some(b'Z') => {
                            pos += 1;
                            d.offset_minutes = Some(0);
                        }
                        Some(&s @ (b'+' | b'-')) => {
                            pos += 1;
                            let hh = digits(b, &mut pos, 2).ok_or(bad)? as i16;
                            if b.get(pos) == Some(&b':') {
                                pos += 1;
                            }
                            let mm = digits(b, &mut pos, 2).ok_or(bad)? as i16;
                            if mm > 59 {
                                return Err(DateError("offset"));
                            }
                            let total = hh * 60 + mm;
                            d.offset_minutes = Some(if s == b'-' { -total } else { total });
                        }
                        Some(_) => return Err(bad),
                    }
                }
            }
        }
        if pos != b.len() {
            return Err(bad);
        }
        if !d.is_valid() {
            return Err(DateError("out of range"));
        }
        Ok(d)
    }

    /// The W3CDTF text, honouring the precision (`Z` for a zero offset, no
    /// designator when the offset is unknown).
    pub fn to_w3cdtf(&self) -> String {
        let mut s = format!("{:04}", self.year);
        if self.precision == DatePrecision::Year {
            return s;
        }
        s.push_str(&format!("-{:02}", self.month));
        if self.precision == DatePrecision::Month {
            return s;
        }
        s.push_str(&format!("-{:02}", self.day));
        if self.precision == DatePrecision::Day {
            return s;
        }
        s.push_str(&format!("T{:02}:{:02}", self.hour, self.minute));
        if self.precision != DatePrecision::Minute {
            s.push_str(&format!(":{:02}", self.second));
            if let DatePrecision::Fraction(n) = self.precision {
                let frac = format!("{:09}", self.nanosecond);
                s.push('.');
                s.push_str(&frac[..usize::from(n.clamp(1, 9))]);
            }
        }
        match self.offset_minutes {
            None => {}
            Some(0) => s.push('Z'),
            Some(o) => {
                let a = o.unsigned_abs();
                s.push_str(&format!(
                    "{}{:02}:{:02}",
                    if o < 0 { '-' } else { '+' },
                    a / 60,
                    a % 60
                ));
            }
        }
        s
    }
}

/// A date property as found in the file: a valid date, or the text that could
/// not be parsed (kept verbatim and never rewritten).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PropertyDate {
    Valid(W3cDateTime),
    Invalid(String),
}
impl PropertyDate {
    pub fn from_text(text: &str) -> Self {
        match W3cDateTime::parse(text) {
            Ok(d) => Self::Valid(d),
            Err(_) => Self::Invalid(text.to_owned()),
        }
    }
}

// ---------------------------------------------------------------------------
// Public value types
// ---------------------------------------------------------------------------

/// Counters stored in `app.xml`. `None` means "not present" when read and
/// "leave as is" when given to the save update.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DocumentStatistics {
    pub pages: Option<u64>,
    pub words: Option<u64>,
    pub characters: Option<u64>,
    pub characters_with_spaces: Option<u64>,
    pub lines: Option<u64>,
    pub paragraphs: Option<u64>,
}

/// The fields the Properties dialog edits. An empty string means "absent".
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EditableProperties {
    pub title: String,
    pub subject: String,
    /// `dc:creator`.
    pub author: String,
    pub manager: String,
    pub company: String,
    pub category: String,
    pub keywords: String,
    /// `dc:description`.
    pub comments: String,
    pub hyperlink_base: String,
    pub content_status: String,
    pub language: String,
}

/// What the dialog shows but does not edit.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReadOnlyProperties {
    pub created: Option<PropertyDate>,
    pub modified: Option<PropertyDate>,
    pub last_printed: Option<PropertyDate>,
    pub last_modified_by: Option<String>,
    /// The raw `cp:revision` text.
    pub revision: Option<String>,
    pub identifier: Option<String>,
    pub version: Option<String>,
    pub application: Option<String>,
    pub app_version: Option<String>,
    pub template: Option<String>,
    /// `TotalTime`, in minutes.
    pub total_time: Option<u64>,
    pub doc_security: Option<u64>,
    pub statistics: DocumentStatistics,
}

/// A tolerated oddity of the file, reported instead of an error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PropertyNotice {
    /// A date that is not W3CDTF; the text is kept as is.
    InvalidDate { field: &'static str, text: String },
    /// A counter or revision that is not a decimal number.
    NonNumeric { field: &'static str, text: String },
    /// The same element appears several times; the last one is read.
    DuplicateElement {
        part: &'static str,
        element: &'static str,
        count: usize,
    },
    /// The part exists but no package relationship points to it; it was read
    /// anyway and will be linked if properties are written.
    UnreferencedPart { part: &'static str },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PropertiesSnapshot {
    pub editable: EditableProperties,
    pub read_only: ReadOnlyProperties,
    pub notices: Vec<PropertyNotice>,
}

/// Changes to apply: `None` leaves a field alone, `Some("")` removes the
/// element from the file, any other text sets it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PropertyChanges {
    pub title: Option<String>,
    pub subject: Option<String>,
    pub author: Option<String>,
    pub manager: Option<String>,
    pub company: Option<String>,
    pub category: Option<String>,
    pub keywords: Option<String>,
    pub comments: Option<String>,
    pub hyperlink_base: Option<String>,
    pub content_status: Option<String>,
    pub language: Option<String>,
}
impl PropertyChanges {
    /// The changes that turn `old` into `new` (what the dialog sends on OK).
    pub fn between(old: &EditableProperties, new: &EditableProperties) -> Self {
        let d = |o: &String, n: &String| (o != n).then(|| n.clone());
        Self {
            title: d(&old.title, &new.title),
            subject: d(&old.subject, &new.subject),
            author: d(&old.author, &new.author),
            manager: d(&old.manager, &new.manager),
            company: d(&old.company, &new.company),
            category: d(&old.category, &new.category),
            keywords: d(&old.keywords, &new.keywords),
            comments: d(&old.comments, &new.comments),
            hyperlink_base: d(&old.hyperlink_base, &new.hyperlink_base),
            content_status: d(&old.content_status, &new.content_status),
            language: d(&old.language, &new.language),
        }
    }
}

/// A change refused; nothing was applied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PropertyError {
    TooLong {
        field: &'static str,
        max: usize,
        len: usize,
    },
}
impl fmt::Display for PropertyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong { field, max, len } => {
                write!(
                    f,
                    "property {field} is {len} characters, the limit is {max}"
                )
            }
        }
    }
}
impl std::error::Error for PropertyError {}

/// What `apply_property_changes` did.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ApplyReport {
    /// Fields whose stored value changed.
    pub changed: Vec<&'static str>,
    /// Fields whose text was cleaned (control characters dropped, line breaks
    /// replaced) before storing.
    pub sanitized: Vec<&'static str>,
}

/// Values the caller supplies when saving.
#[derive(Clone, Debug)]
pub struct SaveContext {
    pub now: W3cDateTime,
    /// Current user: becomes `cp:lastModifiedBy`, and `dc:creator` if absent.
    pub user: Option<String>,
    pub application: Option<String>,
    pub app_version: Option<String>,
    pub statistics: Option<DocumentStatistics>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SaveReport {
    pub revision: String,
    /// The old revision was absent or not a number and restarted at 1.
    pub revision_restarted: bool,
    pub created_set: bool,
    pub creator_set: bool,
}

/// Neutral date for the PDF information dictionary; same fields as
/// `atelier_fmt_pdf::PdfDate`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PdfDateFields {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub utc_offset_minutes: i16,
}

/// Neutral metadata for the PDF writer; the fields are those of
/// `atelier_fmt_pdf::PdfInfo` that a document supplies.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PdfMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub creation_date: Option<PdfDateFields>,
    pub modification_date: Option<PdfDateFields>,
}

/// `cp:` fields that `CoreProperties` does not carry.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CoreExtra {
    pub last_printed: Option<String>,
}
/// `app.xml` fields that `AppProperties` does not carry.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AppExtra {
    pub total_time: Option<String>,
    pub characters_with_spaces: Option<String>,
    pub doc_security: Option<String>,
    pub hyperlink_base: Option<String>,
}

// ---------------------------------------------------------------------------
// Text rules
// ---------------------------------------------------------------------------

fn xml_char_ok(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r') || (c >= ' ' && c != '\u{FFFE}' && c != '\u{FFFF}')
}

fn esc_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#13;"),
            c if xml_char_ok(c) => out.push(c),
            _ => {}
        }
    }
    out
}

/// Cleans and bounds user text. Single-line fields turn tab, CR and LF into a
/// space; multi-line fields turn CR and CRLF into LF. Characters XML cannot
/// carry are dropped. Returns the text and whether it differs from the input.
fn clean_field(
    field: &'static str,
    input: &str,
    multiline: bool,
    max: usize,
) -> Result<(String, bool), PropertyError> {
    let mut out = String::with_capacity(input.len().min(max * 4));
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' if multiline => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\t' | '\r' | '\n' if !multiline => out.push(' '),
            c if xml_char_ok(c) => out.push(c),
            _ => {}
        }
        if out.len() > max * 4 {
            break;
        }
    }
    let len = out.chars().count();
    if len > max {
        return Err(PropertyError::TooLong { field, max, len });
    }
    let changed = out != input;
    Ok((out, changed))
}

/// Same rules for the values the caller supplies when saving, truncated
/// instead of refused (a long user name must not stop a save).
fn clean_truncated(input: &str, max: usize) -> Option<String> {
    let (s, _) = clean_field("", input, false, usize::MAX / 8).ok()?;
    let s: String = s.chars().take(max).collect();
    (!s.is_empty()).then_some(s)
}

// ---------------------------------------------------------------------------
// Scanner and in-place patching
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct ScanError;

#[derive(Debug)]
struct Child {
    ns: String,
    local: String,
    start: usize,
    open_end: usize,
    close_start: usize,
    end: usize,
    empty: bool,
}

#[derive(Debug)]
struct Scan {
    root_open_end: usize,
    root_close_start: usize,
    root_empty: bool,
    root_ns: String,
    decls: Vec<(String, String)>,
    children: Vec<Child>,
}

struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
    empty: bool,
    end: usize,
}

const MAX_CHILDREN: usize = 100_000;
const MAX_DEPTH: usize = 1024;
const MAX_ATTRS: usize = 1024;

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n')
}

fn parse_tag(s: &str, at: usize) -> Result<Tag, ScanError> {
    let b = s.as_bytes();
    let mut p = at + 1;
    let ns = p;
    while p < b.len() && !is_ws(b[p]) && b[p] != b'/' && b[p] != b'>' {
        p += 1;
    }
    if p == ns {
        return Err(ScanError);
    }
    let name = s[ns..p].to_owned();
    let mut attrs = Vec::new();
    loop {
        while p < b.len() && is_ws(b[p]) {
            p += 1;
        }
        match b.get(p) {
            None => return Err(ScanError),
            Some(b'>') => {
                return Ok(Tag {
                    name,
                    attrs,
                    empty: false,
                    end: p + 1,
                });
            }
            Some(b'/') => {
                if b.get(p + 1) != Some(&b'>') {
                    return Err(ScanError);
                }
                return Ok(Tag {
                    name,
                    attrs,
                    empty: true,
                    end: p + 2,
                });
            }
            _ => {}
        }
        if attrs.len() >= MAX_ATTRS {
            return Err(ScanError);
        }
        let an = p;
        while p < b.len() && !is_ws(b[p]) && b[p] != b'=' && b[p] != b'>' && b[p] != b'/' {
            p += 1;
        }
        if p == an {
            return Err(ScanError);
        }
        let aname = s[an..p].to_owned();
        while p < b.len() && is_ws(b[p]) {
            p += 1;
        }
        if b.get(p) != Some(&b'=') {
            return Err(ScanError);
        }
        p += 1;
        while p < b.len() && is_ws(b[p]) {
            p += 1;
        }
        let q = *b.get(p).ok_or(ScanError)?;
        if q != b'"' && q != b'\'' {
            return Err(ScanError);
        }
        p += 1;
        let vs = p;
        while p < b.len() && b[p] != q {
            p += 1;
        }
        if p >= b.len() {
            return Err(ScanError);
        }
        attrs.push((aname, decode_entities(&s[vs..p])));
        p += 1;
    }
}

fn find_from(s: &str, pat: &str, from: usize) -> Result<usize, ScanError> {
    s.get(from..)
        .and_then(|t| t.find(pat))
        .map(|i| i + from)
        .ok_or(ScanError)
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(j) = rest.find(';').filter(|&j| j <= 12) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..j];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[j + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn decls_of(attrs: &[(String, String)]) -> Vec<(String, String)> {
    attrs
        .iter()
        .filter_map(|(n, v)| {
            if n == "xmlns" {
                Some((String::new(), v.clone()))
            } else {
                n.strip_prefix("xmlns:").map(|p| (p.to_owned(), v.clone()))
            }
        })
        .collect()
}

fn resolve(name: &str, own: &[(String, String)], root: &[(String, String)]) -> (String, String) {
    let (prefix, local) = name.split_once(':').unwrap_or(("", name));
    let own = decls_of(own);
    let found = own
        .iter()
        .chain(root.iter())
        .find(|(p, _)| p == prefix)
        .map(|(_, u)| u.clone());
    (found.unwrap_or_default(), local.to_owned())
}

fn scan(bytes: &[u8]) -> Result<Scan, ScanError> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let s = std::str::from_utf8(bytes).map_err(|_| ScanError)?;
    let b = s.as_bytes();
    let mut pos = 0usize;
    // Prolog.
    let root = loop {
        let i = find_from(s, "<", pos)?;
        if s[i..].starts_with("<?") {
            pos = find_from(s, "?>", i)? + 2;
        } else if s[i..].starts_with("<!--") {
            pos = find_from(s, "-->", i)? + 3;
        } else if s[i..].starts_with("<!") || s[i..].starts_with("</") {
            return Err(ScanError);
        } else {
            break parse_tag(s, i)?;
        }
    };
    let decls = decls_of(&root.attrs);
    let (root_ns, _) = resolve(&root.name, &root.attrs, &[]);
    let mut scan = Scan {
        root_open_end: root.end,
        root_close_start: root.end,
        root_empty: root.empty,
        root_ns,
        decls,
        children: Vec::new(),
    };
    if root.empty {
        scan.root_close_start = root.end;
        return Ok(scan);
    }
    pos = root.end;
    loop {
        let i = find_from(s, "<", pos)?;
        let rest = &s[i..];
        if rest.starts_with("<!--") {
            pos = find_from(s, "-->", i)? + 3;
        } else if rest.starts_with("<![CDATA[") {
            pos = find_from(s, "]]>", i)? + 3;
        } else if rest.starts_with("<?") {
            pos = find_from(s, "?>", i)? + 2;
        } else if rest.starts_with("</") {
            scan.root_close_start = i;
            return Ok(scan);
        } else if rest.starts_with("<!") {
            return Err(ScanError);
        } else {
            if scan.children.len() >= MAX_CHILDREN {
                return Err(ScanError);
            }
            let tag = parse_tag(s, i)?;
            let (ns, local) = resolve(&tag.name, &tag.attrs, &scan.decls);
            let (close_start, end) = if tag.empty {
                (tag.end, tag.end)
            } else {
                skip_content(s, tag.end, &tag.name)?
            };
            scan.children.push(Child {
                ns,
                local,
                start: i,
                open_end: tag.end,
                close_start,
                end,
                empty: tag.empty,
            });
            pos = end;
        }
        if pos > b.len() {
            return Err(ScanError);
        }
    }
}

/// Finds the end of an element whose start tag ended at `from`.
fn skip_content(s: &str, from: usize, qname: &str) -> Result<(usize, usize), ScanError> {
    let mut depth = 1usize;
    let mut pos = from;
    loop {
        let i = find_from(s, "<", pos)?;
        let rest = &s[i..];
        if rest.starts_with("<!--") {
            pos = find_from(s, "-->", i)? + 3;
        } else if rest.starts_with("<![CDATA[") {
            pos = find_from(s, "]]>", i)? + 3;
        } else if rest.starts_with("<?") {
            pos = find_from(s, "?>", i)? + 2;
        } else if rest.starts_with("<!") {
            return Err(ScanError);
        } else if rest.starts_with("</") {
            let j = find_from(s, ">", i)?;
            depth -= 1;
            if depth == 0 {
                if s[i + 2..j].trim_end() != qname {
                    return Err(ScanError);
                }
                return Ok((i, j + 1));
            }
            pos = j + 1;
        } else {
            let tag = parse_tag(s, i)?;
            if !tag.empty {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(ScanError);
                }
            }
            pos = tag.end;
        }
    }
}

/// Text of an element's content: text and CDATA, tags and comments removed.
fn inner_text(s: &str) -> String {
    let mut out = String::new();
    let mut pos = 0usize;
    while pos < s.len() {
        let Some(i) = s[pos..].find('<').map(|i| i + pos) else {
            out.push_str(&decode_entities(&s[pos..]));
            break;
        };
        out.push_str(&decode_entities(&s[pos..i]));
        let rest = &s[i..];
        let next = if rest.starts_with("<![CDATA[") {
            match s[i..].find("]]>") {
                Some(j) => {
                    out.push_str(&s[i + 9..i + j]);
                    i + j + 3
                }
                None => break,
            }
        } else if rest.starts_with("<!--") {
            s[i..].find("-->").map_or(s.len(), |j| i + j + 3)
        } else if rest.starts_with("<?") {
            s[i..].find("?>").map_or(s.len(), |j| i + j + 2)
        } else {
            s[i..].find('>').map_or(s.len(), |j| i + j + 1)
        };
        pos = next;
    }
    out
}

fn child_text(source: &str, c: &Child) -> String {
    if c.empty {
        String::new()
    } else {
        inner_text(&source[c.open_end..c.close_start])
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Kind {
    Core,
    App,
}

struct FieldDef {
    ns: &'static str,
    local: &'static str,
    /// Written with `xsi:type="dcterms:W3CDTF"`.
    w3cdtf: bool,
}
const fn f(ns: &'static str, local: &'static str) -> FieldDef {
    FieldDef {
        ns,
        local,
        w3cdtf: false,
    }
}
const fn fd(ns: &'static str, local: &'static str) -> FieldDef {
    FieldDef {
        ns,
        local,
        w3cdtf: true,
    }
}

/// Canonical order of `core.xml` (that of Word's own files).
const CORE_FIELDS: [FieldDef; 15] = [
    f(DC_NS, "title"),
    f(DC_NS, "subject"),
    f(DC_NS, "creator"),
    f(CORE_NS, "keywords"),
    f(DC_NS, "description"),
    f(CORE_NS, "lastModifiedBy"),
    f(CORE_NS, "revision"),
    f(CORE_NS, "lastPrinted"),
    fd(DCTERMS_NS, "created"),
    fd(DCTERMS_NS, "modified"),
    f(CORE_NS, "category"),
    f(CORE_NS, "contentStatus"),
    f(DC_NS, "identifier"),
    f(DC_NS, "language"),
    f(CORE_NS, "version"),
];
/// Canonical order of `app.xml`.
const APP_FIELDS: [FieldDef; 14] = [
    f(APP_NS, "Template"),
    f(APP_NS, "TotalTime"),
    f(APP_NS, "Manager"),
    f(APP_NS, "Company"),
    f(APP_NS, "Pages"),
    f(APP_NS, "Words"),
    f(APP_NS, "Characters"),
    f(APP_NS, "Application"),
    f(APP_NS, "DocSecurity"),
    f(APP_NS, "Lines"),
    f(APP_NS, "Paragraphs"),
    f(APP_NS, "CharactersWithSpaces"),
    f(APP_NS, "HyperlinkBase"),
    f(APP_NS, "AppVersion"),
];

fn fields(kind: Kind) -> &'static [FieldDef] {
    match kind {
        Kind::Core => &CORE_FIELDS,
        Kind::App => &APP_FIELDS,
    }
}

fn root_ns(kind: Kind) -> &'static str {
    match kind {
        Kind::Core => CORE_NS,
        Kind::App => APP_NS,
    }
}

fn template(kind: Kind) -> String {
    match kind {
        Kind::Core => format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties xmlns:cp=\"{CORE_NS}\" xmlns:dc=\"{DC_NS}\" xmlns:dcterms=\"{DCTERMS_NS}\" xmlns:dcmitype=\"{DCMITYPE_NS}\" xmlns:xsi=\"{XSI_NS}\"></cp:coreProperties>"
        ),
        Kind::App => format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Properties xmlns=\"{APP_NS}\" xmlns:vt=\"{VT_NS}\"></Properties>"
        ),
    }
}

/// Values of the fields, in the order of `fields(kind)`.
fn core_values(c: &CoreProperties, x: &CoreExtra) -> Vec<Option<String>> {
    vec![
        c.title.clone(),
        c.subject.clone(),
        c.creator.clone(),
        c.keywords.clone(),
        c.description.clone(),
        c.last_modified_by.clone(),
        c.revision.clone(),
        x.last_printed.clone(),
        c.created.clone(),
        c.modified.clone(),
        c.category.clone(),
        c.content_status.clone(),
        c.identifier.clone(),
        c.language.clone(),
        c.version.clone(),
    ]
}
fn app_values(a: &AppProperties, x: &AppExtra) -> Vec<Option<String>> {
    vec![
        a.template.clone(),
        x.total_time.clone(),
        a.manager.clone(),
        a.company.clone(),
        a.pages.clone(),
        a.words.clone(),
        a.characters.clone(),
        a.application.clone(),
        x.doc_security.clone(),
        a.lines.clone(),
        a.paragraphs.clone(),
        x.characters_with_spaces.clone(),
        x.hyperlink_base.clone(),
        a.app_version.clone(),
    ]
}

/// Last matching element's text for each field (the reader's duplicate rule).
fn scanned_values(kind: Kind, bytes: &[u8]) -> Option<Vec<Option<String>>> {
    let sc = scan(bytes).ok()?;
    if sc.root_ns != root_ns(kind) {
        return None;
    }
    let s = text_of(bytes)?;
    Some(
        fields(kind)
            .iter()
            .map(|d| {
                sc.children
                    .iter()
                    .rfind(|c| c.ns == d.ns && c.local == d.local)
                    .map(|c| child_text(&s, c))
            })
            .collect(),
    )
}

/// The document text with any BOM removed; offsets of `scan` refer to it.
fn text_of(bytes: &[u8]) -> Option<String> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    std::str::from_utf8(bytes).ok().map(str::to_owned)
}

struct Op {
    pos: usize,
    end: usize,
    text: String,
}

fn lead_ws(s: &str, start: usize) -> usize {
    let b = s.as_bytes();
    let mut k = start;
    while k > 0 && is_ws(b[k - 1]) {
        k -= 1;
    }
    if k > 0 && k < start && b[k - 1] == b'>' {
        k
    } else {
        start
    }
}

/// The prefixed name to use for an element of namespace `ns`, with any
/// declaration it must carry itself.
fn qname_for(
    ns: &str,
    local: &str,
    decls: &[(String, String)],
    extra_decls: &mut Vec<(String, String)>,
) -> String {
    if let Some((p, _)) = decls.iter().find(|(p, u)| u == ns && !p.is_empty()) {
        return format!("{p}:{local}");
    }
    if decls.iter().any(|(p, u)| p.is_empty() && u == ns) {
        return local.to_owned();
    }
    if let Some((p, _)) = extra_decls.iter().find(|(_, u)| u == ns) {
        return format!("{p}:{local}");
    }
    let canonical = match ns {
        DC_NS => "dc",
        DCTERMS_NS => "dcterms",
        CORE_NS => "cp",
        XSI_NS => "xsi",
        _ => "ns",
    };
    let mut prefix = canonical.to_owned();
    let mut n = 1u32;
    while decls
        .iter()
        .chain(extra_decls.iter())
        .any(|(p, _)| *p == prefix)
    {
        prefix = format!("{canonical}{n}");
        n += 1;
    }
    extra_decls.push((prefix.clone(), ns.to_owned()));
    format!("{prefix}:{local}")
}

fn new_element(def: &FieldDef, value: &str, decls: &[(String, String)]) -> String {
    let mut extra = Vec::new();
    let q = qname_for(def.ns, def.local, decls, &mut extra);
    let mut attr = String::new();
    if def.w3cdtf {
        // The attribute value names the dcterms prefix, which must be bound.
        let has_prefix = q.contains(':');
        if has_prefix {
            let xsi = qname_for(XSI_NS, "type", decls, &mut extra);
            let pfx = q.split(':').next().unwrap_or("dcterms");
            attr = format!(" {xsi}=\"{pfx}:W3CDTF\"");
        }
    }
    let mut decl = String::new();
    for (p, u) in &extra {
        decl.push_str(&format!(" xmlns:{p}=\"{u}\""));
    }
    format!("<{q}{decl}{attr}>{}</{q}>", esc_text(value))
}

/// Edits the original part so that exactly the fields in `changes` differ.
fn patch(
    kind: Kind,
    original: &[u8],
    changes: &[(usize, Option<String>)],
) -> Result<Vec<u8>, ScanError> {
    let mut text = text_of(original).ok_or(ScanError)?;
    let mut sc = scan(original)?;
    if sc.root_ns != root_ns(kind) {
        return Err(ScanError);
    }
    if sc.root_empty {
        // `<r a="1"/>` becomes `<r a="1"></r>` so that children can be added.
        let tag_end = sc.root_open_end;
        let close = {
            let open = text[..tag_end].rfind('<').ok_or(ScanError)?;
            let t = parse_tag(&text, open)?;
            t.name
        };
        let slash = text[..tag_end].rfind('/').ok_or(ScanError)?;
        text.replace_range(slash..tag_end, &format!("></{close}>"));
        sc = scan(text.as_bytes())?;
    }
    let defs = fields(kind);
    let mut ops: Vec<Op> = Vec::new();
    for (idx, new) in changes {
        let def = &defs[*idx];
        let matches: Vec<&Child> = sc
            .children
            .iter()
            .filter(|c| c.ns == def.ns && c.local == def.local)
            .collect();
        match new {
            None => {
                for c in matches {
                    ops.push(Op {
                        pos: lead_ws(&text, c.start),
                        end: c.end,
                        text: String::new(),
                    });
                }
            }
            Some(v) => {
                if let Some(c) = matches.last() {
                    if c.empty {
                        let tag = &text[c.start..c.open_end];
                        let body = tag.strip_suffix("/>").ok_or(ScanError)?.trim_end();
                        let name = body[1..].split(|ch: char| ch.is_ascii_whitespace()).next();
                        let name = name.ok_or(ScanError)?;
                        ops.push(Op {
                            pos: c.start,
                            end: c.end,
                            text: format!("{body}>{}</{name}>", esc_text(v)),
                        });
                    } else {
                        ops.push(Op {
                            pos: c.open_end,
                            end: c.close_start,
                            text: esc_text(v),
                        });
                    }
                } else {
                    let anchor = sc
                        .children
                        .iter()
                        .filter_map(|c| {
                            defs.iter()
                                .position(|d| d.ns == c.ns && d.local == c.local)
                                .filter(|&p| p < *idx)
                                .map(|p| (p, c))
                        })
                        .max_by_key(|(p, c)| (*p, c.start))
                        .map(|(_, c)| c);
                    let el = new_element(def, v, &sc.decls);
                    match anchor {
                        Some(a) => {
                            let lead = lead_ws(&text, a.start);
                            ops.push(Op {
                                pos: a.end,
                                end: a.end,
                                text: format!("{}{el}", &text[lead..a.start]),
                            });
                        }
                        None => {
                            let indent = sc
                                .children
                                .first()
                                .map(|c| {
                                    let lead = lead_ws(&text, c.start);
                                    text[lead..c.start].to_owned()
                                })
                                .unwrap_or_default();
                            ops.push(Op {
                                pos: sc.root_open_end,
                                end: sc.root_open_end,
                                text: format!("{indent}{el}"),
                            });
                        }
                    }
                }
            }
        }
    }
    // Insertions at one position keep field order (stable sort).
    ops.sort_by_key(|o| o.pos);
    let mut out = String::with_capacity(text.len() + 256);
    let mut cursor = 0usize;
    for op in &ops {
        if op.pos < cursor || op.end < op.pos || op.end > text.len() {
            return Err(ScanError);
        }
        out.push_str(&text[cursor..op.pos]);
        out.push_str(&op.text);
        cursor = op.end;
    }
    out.push_str(&text[cursor..]);
    let _ = sc.root_close_start;
    Ok(out.into_bytes())
}

/// Bytes of a part holding `new`, derived from `original` when there is one.
fn render(
    kind: Kind,
    original: Option<&[u8]>,
    new: &[Option<String>],
) -> Result<Vec<u8>, ScanError> {
    let (base, old): (Vec<u8>, Vec<Option<String>>) = match original {
        Some(bytes) => {
            let old = legacy_values(kind, bytes).ok_or(ScanError)?;
            (bytes.to_vec(), old)
        }
        None => (template(kind).into_bytes(), vec![None; new.len()]),
    };
    let changes: Vec<(usize, Option<String>)> = new
        .iter()
        .enumerate()
        .filter(|(i, n)| old.get(*i) != Some(*n))
        .map(|(i, n)| (i, n.clone()))
        .collect();
    if changes.is_empty() {
        return Ok(base);
    }
    patch(kind, &base, &changes)
}

/// The values the package reader would have produced from `bytes`, plus the
/// extras, in field order.
fn legacy_values(kind: Kind, bytes: &[u8]) -> Option<Vec<Option<String>>> {
    let limits = XmlLimits::default();
    match kind {
        Kind::Core => {
            let (c, _) = crate::parse_core(bytes, limits).ok()?;
            let x = scanned_values(kind, bytes)?;
            let mut v = core_values(&c, &CoreExtra::default());
            v[7] = x[7].clone();
            Some(v)
        }
        Kind::App => {
            let (a, _) = crate::parse_app(bytes, limits).ok()?;
            let x = scanned_values(kind, bytes)?;
            let mut v = app_values(&a, &AppExtra::default());
            for i in [1, 8, 11, 12] {
                v[i] = x[i].clone();
            }
            Some(v)
        }
    }
}

fn count_duplicates(kind: Kind, bytes: &[u8]) -> Vec<(&'static str, usize)> {
    let Ok(sc) = scan(bytes) else {
        return Vec::new();
    };
    fields(kind)
        .iter()
        .filter_map(|d| {
            let n = sc
                .children
                .iter()
                .filter(|c| c.ns == d.ns && c.local == d.local)
                .count();
            (n > 1).then_some((d.local, n))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Package integration
// ---------------------------------------------------------------------------

fn opt(s: &Option<String>) -> String {
    s.clone().unwrap_or_default()
}
fn non_empty(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}
fn number(s: &Option<String>) -> Option<u64> {
    s.as_deref().and_then(|t| t.trim().parse().ok())
}
fn date_of(s: &Option<String>) -> Option<PropertyDate> {
    s.as_deref().map(PropertyDate::from_text)
}

impl Package {
    pub(crate) fn load_extras(&mut self) {
        if let Some(p) = self.core_uri.as_ref().and_then(|u| self.parts.get(u)) {
            if let Some(v) = scanned_values(Kind::Core, &p.bytes) {
                self.core_extra = CoreExtra {
                    last_printed: v[7].clone(),
                };
            }
        }
        if let Some(p) = self.app_uri.as_ref().and_then(|u| self.parts.get(u)) {
            if let Some(v) = scanned_values(Kind::App, &p.bytes) {
                self.app_extra = AppExtra {
                    total_time: v[1].clone(),
                    doc_security: v[8].clone(),
                    characters_with_spaces: v[11].clone(),
                    hyperlink_base: v[12].clone(),
                };
            }
        }
    }

    pub(crate) fn render_core_part(&self) -> Result<Vec<u8>, OpcError> {
        let original = self
            .core_uri
            .as_ref()
            .and_then(|u| self.parts.get(u))
            .map(|p| p.bytes.as_slice());
        let new = core_values(&self.core, &self.core_extra);
        match render(Kind::Core, original, &new) {
            Ok(b) => Ok(b),
            Err(_) => crate::write_core(&self.core, &self.core_unknown),
        }
    }

    pub(crate) fn render_app_part(&self) -> Result<Vec<u8>, OpcError> {
        let original = self
            .app_uri
            .as_ref()
            .and_then(|u| self.parts.get(u))
            .map(|p| p.bytes.as_slice());
        let new = app_values(&self.app, &self.app_extra);
        match render(Kind::App, original, &new) {
            Ok(b) => Ok(b),
            Err(_) => crate::write_app(&self.app, &self.app_unknown),
        }
    }

    /// Extra `cp:` fields (`lastPrinted`) read from the file.
    pub fn core_extra(&self) -> &CoreExtra {
        &self.core_extra
    }
    /// Extra `app.xml` fields read from the file.
    pub fn app_extra(&self) -> &AppExtra {
        &self.app_extra
    }
    /// Mutable access to the extra `cp:` fields; marks `core.xml` for writing.
    /// Values are not validated: prefer `apply_property_changes`.
    pub fn core_extra_mut(&mut self) -> &mut CoreExtra {
        self.core_dirty = true;
        &mut self.core_extra
    }
    /// Mutable access to the extra `app.xml` fields; marks `app.xml` for writing.
    pub fn app_extra_mut(&mut self) -> &mut AppExtra {
        self.app_dirty = true;
        &mut self.app_extra
    }

    /// Reads the properties for the dialog. Never fails: oddities of the file
    /// come back as notices.
    pub fn properties_snapshot(&self) -> PropertiesSnapshot {
        let c = &self.core;
        let a = &self.app;
        let (cx, ax) = (&self.core_extra, &self.app_extra);
        let editable = EditableProperties {
            title: opt(&c.title),
            subject: opt(&c.subject),
            author: opt(&c.creator),
            manager: opt(&a.manager),
            company: opt(&a.company),
            category: opt(&c.category),
            keywords: opt(&c.keywords),
            comments: opt(&c.description),
            hyperlink_base: opt(&ax.hyperlink_base),
            content_status: opt(&c.content_status),
            language: opt(&c.language),
        };
        let read_only = ReadOnlyProperties {
            created: date_of(&c.created),
            modified: date_of(&c.modified),
            last_printed: date_of(&cx.last_printed),
            last_modified_by: c.last_modified_by.clone(),
            revision: c.revision.clone(),
            identifier: c.identifier.clone(),
            version: c.version.clone(),
            application: a.application.clone(),
            app_version: a.app_version.clone(),
            template: a.template.clone(),
            total_time: number(&ax.total_time),
            doc_security: number(&ax.doc_security),
            statistics: DocumentStatistics {
                pages: number(&a.pages),
                words: number(&a.words),
                characters: number(&a.characters),
                characters_with_spaces: number(&ax.characters_with_spaces),
                lines: number(&a.lines),
                paragraphs: number(&a.paragraphs),
            },
        };
        let mut notices = Vec::new();
        for (field, d) in [
            ("created", &read_only.created),
            ("modified", &read_only.modified),
            ("lastPrinted", &read_only.last_printed),
        ] {
            if let Some(PropertyDate::Invalid(t)) = d {
                notices.push(PropertyNotice::InvalidDate {
                    field,
                    text: t.clone(),
                });
            }
        }
        for (field, raw, ok) in [
            ("revision", &c.revision, number(&c.revision).is_some()),
            (
                "TotalTime",
                &ax.total_time,
                number(&ax.total_time).is_some(),
            ),
            (
                "DocSecurity",
                &ax.doc_security,
                number(&ax.doc_security).is_some(),
            ),
            ("Pages", &a.pages, number(&a.pages).is_some()),
            ("Words", &a.words, number(&a.words).is_some()),
            ("Characters", &a.characters, number(&a.characters).is_some()),
            (
                "CharactersWithSpaces",
                &ax.characters_with_spaces,
                number(&ax.characters_with_spaces).is_some(),
            ),
            ("Lines", &a.lines, number(&a.lines).is_some()),
            ("Paragraphs", &a.paragraphs, number(&a.paragraphs).is_some()),
        ] {
            if let (Some(t), false) = (raw, ok) {
                notices.push(PropertyNotice::NonNumeric {
                    field,
                    text: t.clone(),
                });
            }
        }
        for (kind, uri, part) in [
            (Kind::Core, &self.core_uri, "docProps/core.xml"),
            (Kind::App, &self.app_uri, "docProps/app.xml"),
        ] {
            if let Some(p) = uri.as_ref().and_then(|u| self.parts.get(u)) {
                for (element, count) in count_duplicates(kind, &p.bytes) {
                    notices.push(PropertyNotice::DuplicateElement {
                        part,
                        element,
                        count,
                    });
                }
            }
        }
        if self.core_orphan {
            notices.push(PropertyNotice::UnreferencedPart {
                part: "docProps/core.xml",
            });
        }
        if self.app_orphan {
            notices.push(PropertyNotice::UnreferencedPart {
                part: "docProps/app.xml",
            });
        }
        PropertiesSnapshot {
            editable,
            read_only,
            notices,
        }
    }

    /// Applies the dialog's changes. All values are checked first; on error
    /// nothing is applied. A value equal to the stored one changes nothing
    /// (and does not mark the part for writing).
    pub fn apply_property_changes(
        &mut self,
        ch: &PropertyChanges,
    ) -> Result<ApplyReport, PropertyError> {
        struct Item {
            field: &'static str,
            value: Option<String>,
            sanitized: bool,
        }
        let spec: [(&'static str, &Option<String>, bool, usize); 11] = [
            ("title", &ch.title, false, MAX_LINE_CHARS),
            ("subject", &ch.subject, false, MAX_LINE_CHARS),
            ("author", &ch.author, false, MAX_LINE_CHARS),
            ("manager", &ch.manager, false, MAX_LINE_CHARS),
            ("company", &ch.company, false, MAX_LINE_CHARS),
            ("category", &ch.category, false, MAX_LINE_CHARS),
            ("keywords", &ch.keywords, false, MAX_KEYWORDS_CHARS),
            ("comments", &ch.comments, true, MAX_COMMENTS_CHARS),
            (
                "hyperlink_base",
                &ch.hyperlink_base,
                false,
                MAX_HYPERLINK_BASE_CHARS,
            ),
            ("content_status", &ch.content_status, false, MAX_LINE_CHARS),
            ("language", &ch.language, false, MAX_LANGUAGE_CHARS),
        ];
        let mut items = Vec::new();
        for (field, value, multiline, max) in spec {
            if let Some(v) = value {
                let (clean, sanitized) = clean_field(field, v, multiline, max)?;
                items.push(Item {
                    field,
                    value: non_empty(clean),
                    sanitized,
                });
            }
        }
        let mut report = ApplyReport::default();
        for it in items {
            let (slot, is_app): (&mut Option<String>, bool) = match it.field {
                "title" => (&mut self.core.title, false),
                "subject" => (&mut self.core.subject, false),
                "author" => (&mut self.core.creator, false),
                "manager" => (&mut self.app.manager, true),
                "company" => (&mut self.app.company, true),
                "category" => (&mut self.core.category, false),
                "keywords" => (&mut self.core.keywords, false),
                "comments" => (&mut self.core.description, false),
                "hyperlink_base" => (&mut self.app_extra.hyperlink_base, true),
                "content_status" => (&mut self.core.content_status, false),
                _ => (&mut self.core.language, false),
            };
            if it.sanitized {
                report.sanitized.push(it.field);
            }
            if *slot != it.value {
                *slot = it.value;
                report.changed.push(it.field);
                if is_app {
                    self.app_dirty = true;
                } else {
                    self.core_dirty = true;
                }
            }
        }
        Ok(report)
    }

    /// To be called by the application when it saves, never when it opens.
    ///
    /// Sets `dcterms:modified` to `now` (in UTC, seconds), `cp:lastModifiedBy`
    /// to the user, increments `cp:revision`, sets `dcterms:created` and
    /// `dc:creator` when absent, sets `Application` and `AppVersion`, and
    /// writes the statistics that are `Some`.
    ///
    /// Revision: a decimal number is incremented (saturating); an absent,
    /// empty or non-numeric value restarts at 1 (`revision_restarted`).
    pub fn update_on_save(&mut self, ctx: &SaveContext) -> SaveReport {
        let now = ctx.now.to_utc().map_or_else(
            || ctx.now.to_w3cdtf(),
            |u| {
                W3cDateTime {
                    precision: DatePrecision::Second,
                    nanosecond: 0,
                    ..u
                }
                .to_w3cdtf()
            },
        );
        let mut report = SaveReport::default();
        let user = ctx
            .user
            .as_deref()
            .and_then(|u| clean_truncated(u, MAX_LINE_CHARS));
        let c = &mut self.core;
        c.modified = Some(now.clone());
        if let Some(u) = &user {
            c.last_modified_by = Some(u.clone());
            if c.creator.as_deref().is_none_or(str::is_empty) {
                c.creator = Some(u.clone());
                report.creator_set = true;
            }
        }
        if c.created.as_deref().is_none_or(|s| s.trim().is_empty()) {
            c.created = Some(now);
            report.created_set = true;
        }
        let next = match c
            .revision
            .as_deref()
            .and_then(|r| r.trim().parse::<u64>().ok())
        {
            Some(n) => n.saturating_add(1),
            None => {
                report.revision_restarted = true;
                1
            }
        };
        report.revision = next.to_string();
        c.revision = Some(report.revision.clone());
        self.core_dirty = true;

        let a = &mut self.app;
        if let Some(v) = ctx
            .application
            .as_deref()
            .and_then(|v| clean_truncated(v, MAX_LINE_CHARS))
        {
            a.application = Some(v);
        }
        if let Some(v) = ctx
            .app_version
            .as_deref()
            .and_then(|v| clean_truncated(v, MAX_LINE_CHARS))
        {
            a.app_version = Some(v);
        }
        if let Some(st) = &ctx.statistics {
            let put = |slot: &mut Option<String>, v: Option<u64>| {
                if let Some(v) = v {
                    *slot = Some(v.to_string());
                }
            };
            put(&mut a.pages, st.pages);
            put(&mut a.words, st.words);
            put(&mut a.characters, st.characters);
            put(
                &mut self.app_extra.characters_with_spaces,
                st.characters_with_spaces,
            );
            put(&mut a.lines, st.lines);
            put(&mut a.paragraphs, st.paragraphs);
        }
        self.app_dirty = true;
        report
    }

    /// The metadata the PDF writer takes, from the current properties.
    pub fn pdf_metadata(&self) -> PdfMetadata {
        let s = self.properties_snapshot();
        PdfMetadata::from_snapshot(&s)
    }

    /// Property parts that can be read. Used by the integrator to know
    /// whether a save will create them.
    pub fn has_property_parts(&self) -> (bool, bool) {
        (
            self.core_uri
                .as_ref()
                .is_some_and(|u| self.parts.contains_key(u)),
            self.app_uri
                .as_ref()
                .is_some_and(|u| self.parts.contains_key(u)),
        )
    }
}

impl PdfMetadata {
    pub fn from_snapshot(s: &PropertiesSnapshot) -> Self {
        let date = |d: &Option<PropertyDate>| match d {
            Some(PropertyDate::Valid(v)) => pdf_date(v),
            _ => None,
        };
        let e = &s.editable;
        let some = |t: &String| (!t.is_empty()).then(|| t.clone());
        Self {
            title: some(&e.title),
            author: some(&e.author),
            subject: some(&e.subject),
            keywords: some(&e.keywords),
            creation_date: date(&s.read_only.created),
            modification_date: date(&s.read_only.modified),
        }
    }
}

fn pdf_date(d: &W3cDateTime) -> Option<PdfDateFields> {
    // PDF writers accept offsets up to 14 hours: normalise larger ones.
    let d = match d.offset_minutes {
        Some(o) if o.abs() > 14 * 60 => d.to_utc()?,
        _ => *d,
    };
    Some(PdfDateFields {
        year: d.year,
        month: d.month,
        day: d.day,
        hour: d.hour,
        minute: d.minute,
        second: d.second,
        utc_offset_minutes: d.offset_minutes.unwrap_or(0),
    })
}
