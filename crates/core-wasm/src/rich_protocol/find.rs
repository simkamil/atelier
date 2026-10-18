// SPDX-License-Identifier: AGPL-3.0-or-later
//! Messages of "search, replace, statistics and properties" (R7b-2): `Search`,
//! `Statistics`, `SearchRects`, `Replace`, `ReplaceAll`, `PropertiesRead`,
//! `PropertiesApply` and their replies. Grouped here so that the envelope
//! module only holds the hooks; layout in `docs/us-340-search.md` (section
//! "Rich editor protocol").

use super::{
    Command, ErrorCode, MAX_PAGES, MAX_PAGES_PER_RESPONSE, MAX_TEXT_BYTES, Reader, Response, State,
    check_range, encode_state, parse_state, put_f64, put_u32, put_u64, validate_edit_text,
};

/// Longest search or replace query, in bytes (as in the plain editor).
pub const MAX_QUERY_BYTES: usize = 1024;
/// Occurrences one `SearchResult` carries.
pub const MAX_SEARCH_ITEMS: usize = 1000;
/// Occurrences counted for `total`; above it `total_capped` is set.
pub const SEARCH_COUNT_CAP: usize = 10_000;
/// Longest excerpt of an occurrence, in bytes.
pub const MAX_EXCERPT_BYTES: usize = 4096;
/// Ranges one `SearchRects` takes.
pub const MAX_HIGHLIGHT_RANGES: usize = 1000;
/// Rectangles one `SearchRects` reply carries.
pub const MAX_HIGHLIGHT_RECTS: usize = 8192;
/// Longest value of a document property, in bytes.
pub const MAX_PROPERTY_BYTES: usize = 64 * 1024;
/// Editable properties, in wire order.
pub const PROPERTY_COUNT: usize = 11;
/// Most occurrences a `ReplaceAll` takes (the undo history holds them).
pub const MAX_REPLACE_MATCHES: usize = 5000;

/// The four comparison options of a search (bits 0 to 3 of the flags byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchFlags {
    pub match_case: bool,
    pub whole_word: bool,
    pub ignore_accents: bool,
    pub match_nbsp: bool,
}

impl SearchFlags {
    fn from_byte(byte: u8) -> Result<Self, ErrorCode> {
        if byte & !0x0f != 0 {
            return Err(ErrorCode::InvalidBody);
        }
        Ok(Self {
            match_case: byte & 1 != 0,
            whole_word: byte & 2 != 0,
            ignore_accents: byte & 4 != 0,
            match_nbsp: byte & 8 != 0,
        })
    }

    fn to_byte(self) -> u8 {
        u8::from(self.match_case)
            | u8::from(self.whole_word) << 1
            | u8::from(self.ignore_accents) << 2
            | u8::from(self.match_nbsp) << 3
    }
}

/// Text around an occurrence, inside its paragraph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchExcerpt {
    pub text: String,
    /// The occurrence inside `text`, in bytes, on scalar boundaries.
    pub start: u32,
    pub end: u32,
    pub truncated_before: bool,
    pub truncated_after: bool,
}

/// One occurrence: a flat range and its excerpt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchItem {
    pub start: u64,
    pub end: u64,
    pub excerpt: SearchExcerpt,
}

/// Counts of the document or of the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StatCounts {
    pub words: u32,
    pub characters: u32,
    pub characters_with_spaces: u32,
    pub paragraphs: u32,
    pub lines: u32,
    pub pages: u32,
}

/// One highlight rectangle of an occurrence (points, as in the display list).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HighlightRect {
    /// Index of the range in the request.
    pub item: u32,
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A date of the properties: `None` absent, else (valid W3CDTF, text).
pub type PropertyDate = Option<(bool, String)>;

/// What the properties dialog shows (same fields as the plain editor).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PropertiesSnapshot {
    /// title, subject, author, manager, company, category, keywords, comments,
    /// hyperlink base, content status, language.
    pub editable: [String; PROPERTY_COUNT],
    /// Longest value of each, in characters.
    pub limits: [u32; PROPERTY_COUNT],
    /// created, modified, last printed.
    pub dates: [PropertyDate; 3],
    /// last modified by, revision, application, application version, template.
    pub strings: [Option<String>; 5],
    /// Total editing time, in minutes.
    pub total_time: Option<u64>,
    /// pages, words, characters, characters with spaces, lines, paragraphs.
    pub statistics: [Option<u64>; 6],
    pub notices: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertiesStatus {
    Applied = 0,
    /// A value is longer than its limit; nothing was applied.
    TooLong = 1,
}

/// Answer of `PropertiesApply`.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertiesOutcome {
    pub status: PropertiesStatus,
    /// Properties whose stored value changed.
    pub changed: u16,
    /// For `TooLong`: index of the field, its limit and the length received
    /// (characters); zeros otherwise.
    pub field: u8,
    pub max: u32,
    pub len: u32,
    pub state: State,
}

fn read_query(r: &mut Reader<'_>, empty_allowed: bool) -> Result<String, ErrorCode> {
    let len = r.u32()? as usize;
    if len > MAX_QUERY_BYTES {
        return Err(ErrorCode::QueryTooLarge);
    }
    let raw = r.take(len)?;
    let text = core::str::from_utf8(raw).map_err(|_| ErrorCode::InvalidUtf8)?;
    if text.is_empty() && !empty_allowed {
        return Err(ErrorCode::EmptyQuery);
    }
    Ok(text.to_owned())
}

pub(super) fn parse_command(opcode: u16, r: &mut Reader<'_>) -> Result<Command, ErrorCode> {
    Ok(match opcode {
        super::opcode::SEARCH => {
            let revision = r.u64()?;
            let flags = SearchFlags::from_byte(r.u8()?)?;
            r.zero(3)?;
            let query = read_query(r, true)?;
            Command::Search {
                revision,
                flags,
                query,
            }
        }
        super::opcode::STATISTICS => Command::Statistics,
        super::opcode::SEARCH_RECTS => {
            let revision = r.u64()?;
            let (first_page, page_count) = (r.u32()?, r.u32()?);
            if page_count == 0 {
                return Err(ErrorCode::InvalidBody);
            }
            if page_count as usize > MAX_PAGES_PER_RESPONSE {
                return Err(ErrorCode::TooManyPages);
            }
            if first_page >= MAX_PAGES {
                return Err(ErrorCode::PageRange);
            }
            let count = r.u32()? as usize;
            if count == 0 || count > MAX_HIGHLIGHT_RANGES {
                return Err(ErrorCode::InvalidBody);
            }
            let mut ranges = Vec::with_capacity(count);
            for _ in 0..count {
                let (start, end) = (r.u64()?, r.u64()?);
                check_range(u64::MAX, start, end)?;
                ranges.push((start, end));
            }
            Command::SearchRects {
                revision,
                first_page,
                page_count,
                ranges,
            }
        }
        super::opcode::REPLACE | super::opcode::REPLACE_ALL => {
            let current = opcode == super::opcode::REPLACE;
            let revision = r.u64()?;
            let flags = SearchFlags::from_byte(r.u8()?)?;
            r.zero(3)?;
            let range = if current {
                let (start, end) = (r.u64()?, r.u64()?);
                check_range(u64::MAX, start, end)?;
                Some((start, end))
            } else {
                None
            };
            let query = read_query(r, false)?;
            if query.contains('\u{FFFC}') {
                return Err(ErrorCode::InvalidText);
            }
            let replacement = r.text(MAX_TEXT_BYTES, ErrorCode::FrameTooLarge)?;
            validate_edit_text(&replacement)?;
            match range {
                Some((start, end)) => Command::Replace {
                    revision,
                    flags,
                    start,
                    end,
                    query,
                    replacement,
                },
                None => Command::ReplaceAll {
                    revision,
                    flags,
                    query,
                    replacement,
                },
            }
        }
        super::opcode::PROPERTIES_READ => Command::PropertiesRead,
        super::opcode::PROPERTIES_APPLY => {
            let mut changes: [Option<String>; PROPERTY_COUNT] = Default::default();
            for slot in &mut changes {
                let present = r.flag()?;
                let len = r.u32()? as usize;
                if len > MAX_PROPERTY_BYTES {
                    return Err(ErrorCode::FrameTooLarge);
                }
                let raw = r.take(len)?;
                if present {
                    let text = core::str::from_utf8(raw).map_err(|_| ErrorCode::InvalidUtf8)?;
                    *slot = Some(text.to_owned());
                } else if len != 0 {
                    return Err(ErrorCode::InvalidBody);
                }
            }
            Command::PropertiesApply { changes }
        }
        _ => return Err(ErrorCode::UnknownOpcode),
    })
}

fn put_query(b: &mut Vec<u8>, query: &str) -> Result<(), ErrorCode> {
    put_u32(
        b,
        u32::try_from(query.len()).map_err(|_| ErrorCode::QueryTooLarge)?,
    );
    b.extend_from_slice(query.as_bytes());
    Ok(())
}

pub(super) fn encode_command(command: &Command, b: &mut Vec<u8>) -> Result<u16, ErrorCode> {
    use super::opcode as op;
    Ok(match command {
        Command::Search {
            revision,
            flags,
            query,
        } => {
            put_u64(b, *revision);
            b.extend_from_slice(&[flags.to_byte(), 0, 0, 0]);
            put_query(b, query)?;
            op::SEARCH
        }
        Command::Statistics => op::STATISTICS,
        Command::SearchRects {
            revision,
            first_page,
            page_count,
            ranges,
        } => {
            put_u64(b, *revision);
            put_u32(b, *first_page);
            put_u32(b, *page_count);
            put_u32(
                b,
                u32::try_from(ranges.len()).map_err(|_| ErrorCode::InvalidBody)?,
            );
            for (start, end) in ranges {
                put_u64(b, *start);
                put_u64(b, *end);
            }
            op::SEARCH_RECTS
        }
        Command::Replace {
            revision,
            flags,
            start,
            end,
            query,
            replacement,
        } => {
            put_u64(b, *revision);
            b.extend_from_slice(&[flags.to_byte(), 0, 0, 0]);
            put_u64(b, *start);
            put_u64(b, *end);
            put_query(b, query)?;
            super::put_text(b, replacement)?;
            op::REPLACE
        }
        Command::ReplaceAll {
            revision,
            flags,
            query,
            replacement,
        } => {
            put_u64(b, *revision);
            b.extend_from_slice(&[flags.to_byte(), 0, 0, 0]);
            put_query(b, query)?;
            super::put_text(b, replacement)?;
            op::REPLACE_ALL
        }
        Command::PropertiesRead => op::PROPERTIES_READ,
        Command::PropertiesApply { changes } => {
            for change in changes {
                b.push(u8::from(change.is_some()));
                let text = change.as_deref().unwrap_or("");
                put_u32(
                    b,
                    u32::try_from(text.len()).map_err(|_| ErrorCode::FrameTooLarge)?,
                );
                b.extend_from_slice(text.as_bytes());
            }
            op::PROPERTIES_APPLY
        }
        _ => return Err(ErrorCode::Internal),
    })
}

fn put_str(b: &mut Vec<u8>, text: &str) -> Result<(), ErrorCode> {
    if text.len() > MAX_PROPERTY_BYTES {
        return Err(ErrorCode::FrameTooLarge);
    }
    put_u32(b, text.len() as u32);
    b.extend_from_slice(text.as_bytes());
    Ok(())
}

fn read_str(r: &mut Reader<'_>) -> Result<String, ErrorCode> {
    r.text(MAX_PROPERTY_BYTES, ErrorCode::FrameTooLarge)
}

fn put_optional(b: &mut Vec<u8>, value: Option<&str>) -> Result<(), ErrorCode> {
    b.push(u8::from(value.is_some()));
    put_str(b, value.unwrap_or(""))
}

fn read_optional(r: &mut Reader<'_>) -> Result<Option<String>, ErrorCode> {
    let present = r.flag()?;
    let text = read_str(r)?;
    if !present && !text.is_empty() {
        return Err(ErrorCode::InvalidBody);
    }
    Ok(present.then_some(text))
}

fn put_number(b: &mut Vec<u8>, value: Option<u64>) {
    b.push(u8::from(value.is_some()));
    put_u64(b, value.unwrap_or(0));
}

fn read_number(r: &mut Reader<'_>) -> Result<Option<u64>, ErrorCode> {
    let present = r.flag()?;
    let value = r.u64()?;
    if !present && value != 0 {
        return Err(ErrorCode::InvalidBody);
    }
    Ok(present.then_some(value))
}

fn put_snapshot(b: &mut Vec<u8>, s: &PropertiesSnapshot) -> Result<(), ErrorCode> {
    for text in &s.editable {
        put_str(b, text)?;
    }
    for limit in &s.limits {
        put_u32(b, *limit);
    }
    for date in &s.dates {
        match date {
            None => {
                b.push(0);
                put_str(b, "")?;
            }
            Some((valid, text)) => {
                b.push(if *valid { 1 } else { 2 });
                put_str(b, text)?;
            }
        }
    }
    for text in &s.strings {
        put_optional(b, text.as_deref())?;
    }
    put_number(b, s.total_time);
    for value in &s.statistics {
        put_number(b, *value);
    }
    put_u32(b, s.notices);
    Ok(())
}

fn read_snapshot(r: &mut Reader<'_>) -> Result<PropertiesSnapshot, ErrorCode> {
    let mut s = PropertiesSnapshot::default();
    for slot in &mut s.editable {
        *slot = read_str(r)?;
    }
    for slot in &mut s.limits {
        *slot = r.u32()?;
    }
    for slot in &mut s.dates {
        let kind = r.u8()?;
        let text = read_str(r)?;
        *slot = match kind {
            0 if text.is_empty() => None,
            1 => Some((true, text)),
            2 => Some((false, text)),
            _ => return Err(ErrorCode::InvalidBody),
        };
    }
    for slot in &mut s.strings {
        *slot = read_optional(r)?;
    }
    s.total_time = read_number(r)?;
    for slot in &mut s.statistics {
        *slot = read_number(r)?;
    }
    s.notices = r.u32()?;
    Ok(s)
}

fn put_counts(b: &mut Vec<u8>, c: &StatCounts) {
    for value in [
        c.words,
        c.characters,
        c.characters_with_spaces,
        c.paragraphs,
        c.lines,
        c.pages,
    ] {
        put_u32(b, value);
    }
}

fn read_counts(r: &mut Reader<'_>) -> Result<StatCounts, ErrorCode> {
    let c = StatCounts {
        words: r.u32()?,
        characters: r.u32()?,
        characters_with_spaces: r.u32()?,
        paragraphs: r.u32()?,
        lines: r.u32()?,
        pages: r.u32()?,
    };
    if c.pages > MAX_PAGES {
        return Err(ErrorCode::TooManyPages);
    }
    Ok(c)
}

pub(super) fn encode_reply(response: &Response, b: &mut Vec<u8>) -> Result<u16, ErrorCode> {
    use super::opcode as op;
    Ok(match response {
        Response::SearchResult {
            revision,
            total,
            truncated,
            total_capped,
            items,
        } => {
            if items.len() > MAX_SEARCH_ITEMS {
                return Err(ErrorCode::InvalidBody);
            }
            put_u64(b, *revision);
            put_u32(b, *total);
            b.extend_from_slice(&[u8::from(*truncated) | u8::from(*total_capped) << 1, 0, 0, 0]);
            put_u32(b, items.len() as u32);
            for item in items {
                let e = &item.excerpt;
                if e.text.len() > MAX_EXCERPT_BYTES {
                    return Err(ErrorCode::InvalidBody);
                }
                put_u64(b, item.start);
                put_u64(b, item.end);
                put_u32(b, e.text.len() as u32);
                put_u32(b, e.start);
                put_u32(b, e.end);
                b.extend_from_slice(&[
                    u8::from(e.truncated_before) | u8::from(e.truncated_after) << 1,
                    0,
                    0,
                    0,
                ]);
                b.extend_from_slice(e.text.as_bytes());
            }
            op::R_SEARCH
        }
        Response::Statistics {
            revision,
            anchor,
            focus,
            layout_complete,
            document,
            selection,
        } => {
            put_u64(b, *revision);
            put_u64(b, *anchor);
            put_u64(b, *focus);
            b.extend_from_slice(&[u8::from(*layout_complete), 0, 0, 0]);
            put_counts(b, document);
            put_counts(b, selection);
            op::R_STATISTICS
        }
        Response::SearchRects { revision, rects } => {
            if rects.len() > MAX_HIGHLIGHT_RECTS {
                return Err(ErrorCode::InvalidBody);
            }
            put_u64(b, *revision);
            put_u32(b, rects.len() as u32);
            for rect in rects {
                put_u32(b, rect.item);
                put_u32(b, rect.page);
                for value in [rect.x, rect.y, rect.width, rect.height] {
                    put_f64(b, value);
                }
            }
            op::R_SEARCH_RECTS
        }
        Response::Replaced { count, state } => {
            put_u32(b, *count);
            encode_state(state, b)?;
            op::R_REPLACED
        }
        Response::Properties { revision, snapshot } => {
            put_u64(b, *revision);
            put_snapshot(b, snapshot)?;
            op::R_PROPERTIES
        }
        Response::PropertiesApplied(outcome) => {
            b.extend_from_slice(&[outcome.status as u8, 0]);
            b.extend_from_slice(&outcome.changed.to_le_bytes());
            b.extend_from_slice(&[outcome.field, 0, 0, 0]);
            put_u32(b, outcome.max);
            put_u32(b, outcome.len);
            encode_state(&outcome.state, b)?;
            op::R_PROPERTIES_APPLIED
        }
        _ => return Err(ErrorCode::Internal),
    })
}

pub(super) fn parse_reply(opcode: u16, r: &mut Reader<'_>) -> Result<Response, ErrorCode> {
    use super::opcode as op;
    Ok(match opcode {
        op::R_SEARCH => {
            let revision = r.u64()?;
            let total = r.u32()?;
            let flags = r.u8()?;
            r.zero(3)?;
            if flags & !3 != 0 {
                return Err(ErrorCode::InvalidBody);
            }
            let count = r.u32()? as usize;
            if count > MAX_SEARCH_ITEMS || count > total as usize {
                return Err(ErrorCode::InvalidBody);
            }
            let mut items: Vec<SearchItem> = Vec::with_capacity(count);
            for _ in 0..count {
                let (start, end) = (r.u64()?, r.u64()?);
                let len = r.u32()? as usize;
                let (e_start, e_end) = (r.u32()?, r.u32()?);
                let eflags = r.u8()?;
                r.zero(3)?;
                if len > MAX_EXCERPT_BYTES || eflags & !3 != 0 {
                    return Err(ErrorCode::InvalidBody);
                }
                let raw = r.take(len)?;
                let text = core::str::from_utf8(raw)
                    .map_err(|_| ErrorCode::InvalidUtf8)?
                    .to_owned();
                let inside = e_start < e_end
                    && e_end as usize <= len
                    && text.is_char_boundary(e_start as usize)
                    && text.is_char_boundary(e_end as usize);
                let ordered = items.last().is_none_or(|p| start >= p.end);
                if start >= end || !inside || !ordered {
                    return Err(ErrorCode::InvalidBody);
                }
                items.push(SearchItem {
                    start,
                    end,
                    excerpt: SearchExcerpt {
                        text,
                        start: e_start,
                        end: e_end,
                        truncated_before: eflags & 1 != 0,
                        truncated_after: eflags & 2 != 0,
                    },
                });
            }
            Response::SearchResult {
                revision,
                total,
                truncated: flags & 1 != 0,
                total_capped: flags & 2 != 0,
                items,
            }
        }
        op::R_STATISTICS => {
            let (revision, anchor, focus) = (r.u64()?, r.u64()?, r.u64()?);
            let complete = r.flag()?;
            r.zero(3)?;
            Response::Statistics {
                revision,
                anchor,
                focus,
                layout_complete: complete,
                document: read_counts(r)?,
                selection: read_counts(r)?,
            }
        }
        op::R_SEARCH_RECTS => {
            let revision = r.u64()?;
            let count = r.u32()? as usize;
            if count > MAX_HIGHLIGHT_RECTS {
                return Err(ErrorCode::InvalidBody);
            }
            let mut rects: Vec<HighlightRect> = Vec::with_capacity(count);
            for _ in 0..count {
                let rect = HighlightRect {
                    item: r.u32()?,
                    page: r.u32()?,
                    x: r.f64()?,
                    y: r.f64()?,
                    width: r.f64()?,
                    height: r.f64()?,
                };
                let ordered = rects.last().is_none_or(|p| rect.item >= p.item);
                if rect.page >= MAX_PAGES || rect.width < 0.0 || rect.height < 0.0 || !ordered {
                    return Err(ErrorCode::InvalidBody);
                }
                rects.push(rect);
            }
            Response::SearchRects { revision, rects }
        }
        op::R_REPLACED => Response::Replaced {
            count: r.u32()?,
            state: parse_state(r)?,
        },
        op::R_PROPERTIES => Response::Properties {
            revision: r.u64()?,
            snapshot: Box::new(read_snapshot(r)?),
        },
        op::R_PROPERTIES_APPLIED => {
            let status = match r.u8()? {
                0 => PropertiesStatus::Applied,
                1 => PropertiesStatus::TooLong,
                _ => return Err(ErrorCode::InvalidBody),
            };
            r.zero(1)?;
            let changed = r.u16()?;
            let field = r.u8()?;
            r.zero(3)?;
            let (max, len) = (r.u32()?, r.u32()?);
            let consistent = match status {
                PropertiesStatus::Applied => field == 0 && max == 0 && len == 0,
                PropertiesStatus::TooLong => {
                    usize::from(field) < PROPERTY_COUNT && len > max && changed == 0
                }
            };
            if !consistent || usize::from(changed) > PROPERTY_COUNT {
                return Err(ErrorCode::InvalidBody);
            }
            Response::PropertiesApplied(PropertiesOutcome {
                status,
                changed,
                field,
                max,
                len,
                state: parse_state(r)?,
            })
        }
        _ => return Err(ErrorCode::UnknownOpcode),
    })
}
