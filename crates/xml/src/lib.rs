// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded, event-based XML 1.0 reader and deterministic writer.
//!
//! The strict reader supports UTF-8 and BOM-marked UTF-16, predefined and numeric
//! references, namespaces, comments, processing instructions and CDATA (as text).
//! DTDs are skipped without processing declarations; entities declared in them are
//! never expanded. [`Reader::new_tolerant`] is opt-in and only repairs an omitted
//! final end tag, reporting that repair in [`Reader::diagnostics`].

#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::fmt;
use std::io::{self, Read, Write};

const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Position {
    pub byte: u64,
    pub line: u64,
    pub column: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    Io,
    InvalidEncoding,
    InvalidXmlCharacter,
    Syntax,
    UnknownEntity,
    Namespace,
    DuplicateAttribute,
    Limit,
    WriterState,
}

#[derive(Debug)]
pub struct XmlError {
    pub kind: ErrorKind,
    pub position: Position,
    pub message: String,
    source: Option<io::Error>,
}

impl XmlError {
    fn new(kind: ErrorKind, position: Position, message: impl Into<String>) -> Self {
        Self {
            kind,
            position,
            message: message.into(),
            source: None,
        }
    }
    fn io(position: Position, error: io::Error) -> Self {
        Self {
            kind: ErrorKind::Io,
            position,
            message: error.to_string(),
            source: Some(error),
        }
    }
}
impl fmt::Display for XmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "XML {:?} at {}:{} (byte {}): {}",
            self.kind, self.position.line, self.position.column, self.position.byte, self.message
        )
    }
}
impl std::error::Error for XmlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_ref().map(|e| e as _)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Limits {
    pub max_depth: usize,
    pub max_attributes_per_element: usize,
    pub max_name_bytes: usize,
    pub max_attribute_bytes: usize,
    pub max_text_bytes: usize,
    pub max_events: usize,
    pub max_namespaces_per_element: usize,
    pub max_memory_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_depth: 256,
            max_attributes_per_element: 256,
            max_name_bytes: 1024,
            max_attribute_bytes: 1024 * 1024,
            max_text_bytes: 4 * 1024 * 1024,
            max_events: 10_000_000,
            max_namespaces_per_element: 256,
            max_memory_bytes: 16 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Name {
    pub prefix: Option<String>,
    pub local: String,
    pub namespace: Option<String>,
}
impl Name {
    pub fn qualified(&self) -> String {
        match &self.prefix {
            Some(p) => format!("{p}:{}", self.local),
            None => self.local.clone(),
        }
    }
    pub fn expanded(&self) -> (&str, &str) {
        (self.namespace.as_deref().unwrap_or(""), &self.local)
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Attribute {
    pub name: Name,
    pub value: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    StartElement {
        name: Name,
        attributes: Vec<Attribute>,
        empty: bool,
    },
    EndElement {
        name: Name,
    },
    Text(String),
    Comment(String),
    ProcessingInstruction {
        target: String,
        data: String,
    },
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub position: Position,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
}

/// Character-at-a-time decoder. It retains at most a short encoding prefix and
/// reports exact byte offsets; it never uses lossy conversion.
struct Decoder<R> {
    input: R,
    bytes: Vec<u8>,
    cursor: usize,
    eof: bool,
    encoding: Option<Encoding>,
    byte_offset: u64,
    line: u64,
    column: u64,
    after_cr: bool,
}
impl<R: Read> Decoder<R> {
    fn new(input: R) -> Self {
        Self {
            input,
            bytes: Vec::new(),
            cursor: 0,
            eof: false,
            encoding: None,
            byte_offset: 0,
            line: 1,
            column: 1,
            after_cr: false,
        }
    }
    fn position(&self) -> Position {
        Position {
            byte: self.byte_offset,
            line: self.line,
            column: self.column,
        }
    }
    fn fill(&mut self, need: usize) -> Result<(), XmlError> {
        while self.bytes.len().saturating_sub(self.cursor) < need && !self.eof {
            if self.cursor > 0 {
                self.bytes.drain(..self.cursor);
                self.cursor = 0;
            }
            let mut temp = [0u8; 4];
            match self.input.read(&mut temp) {
                Ok(0) => self.eof = true,
                Ok(n) => self.bytes.extend_from_slice(&temp[..n]),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(XmlError::io(self.position(), e)),
            }
        }
        Ok(())
    }
    fn initialize(&mut self) -> Result<(), XmlError> {
        if self.encoding.is_some() {
            return Ok(());
        }
        self.fill(2)?;
        let first = self.bytes.get(self.cursor).copied();
        let (enc, bom) = if first == Some(0xEF) {
            self.fill(3)?;
            if self.bytes[self.cursor..].starts_with(&[0xEF, 0xBB, 0xBF]) {
                (Encoding::Utf8, 3)
            } else {
                (Encoding::Utf8, 0)
            }
        } else if self.bytes[self.cursor..].starts_with(&[0xFF, 0xFE]) {
            (Encoding::Utf16Le, 2)
        } else if self.bytes[self.cursor..].starts_with(&[0xFE, 0xFF]) {
            (Encoding::Utf16Be, 2)
        } else {
            (Encoding::Utf8, 0)
        };
        self.cursor += bom;
        self.byte_offset += bom as u64;
        self.encoding = Some(enc);
        Ok(())
    }
    fn next_char(&mut self) -> Result<Option<char>, XmlError> {
        self.initialize()?;
        let pos = self.position();
        let ch = match self.encoding.unwrap_or(Encoding::Utf8) {
            Encoding::Utf8 => {
                self.fill(1)?;
                if self.cursor == self.bytes.len() {
                    return Ok(None);
                }
                let first = self.bytes[self.cursor];
                let n = if first < 0x80 {
                    1
                } else if (0xC2..=0xDF).contains(&first) {
                    2
                } else if (0xE0..=0xEF).contains(&first) {
                    3
                } else if (0xF0..=0xF4).contains(&first) {
                    4
                } else {
                    return Err(XmlError::new(
                        ErrorKind::InvalidEncoding,
                        pos,
                        "invalid UTF-8 leading byte",
                    ));
                };
                self.fill(n)?;
                if self.bytes.len() - self.cursor < n {
                    return Err(XmlError::new(
                        ErrorKind::InvalidEncoding,
                        pos,
                        "truncated UTF-8 sequence",
                    ));
                }
                let s = std::str::from_utf8(&self.bytes[self.cursor..self.cursor + n]).map_err(
                    |_| XmlError::new(ErrorKind::InvalidEncoding, pos, "invalid UTF-8 sequence"),
                )?;
                let c = s.chars().next().ok_or_else(|| {
                    XmlError::new(ErrorKind::InvalidEncoding, pos, "empty UTF-8 sequence")
                })?;
                self.cursor += n;
                self.byte_offset += n as u64;
                c
            }
            enc => {
                self.fill(2)?;
                if self.cursor == self.bytes.len() {
                    return Ok(None);
                }
                if self.bytes.len() - self.cursor < 2 {
                    return Err(XmlError::new(
                        ErrorKind::InvalidEncoding,
                        pos,
                        "truncated UTF-16 code unit",
                    ));
                }
                let unit = if enc == Encoding::Utf16Le {
                    u16::from_le_bytes([self.bytes[self.cursor], self.bytes[self.cursor + 1]])
                } else {
                    u16::from_be_bytes([self.bytes[self.cursor], self.bytes[self.cursor + 1]])
                };
                self.cursor += 2;
                self.byte_offset += 2;
                let scalar = if (0xD800..=0xDBFF).contains(&unit) {
                    self.fill(2)?;
                    if self.bytes.len() - self.cursor < 2 {
                        return Err(XmlError::new(
                            ErrorKind::InvalidEncoding,
                            pos,
                            "truncated UTF-16 surrogate pair",
                        ));
                    }
                    let low = if enc == Encoding::Utf16Le {
                        u16::from_le_bytes([self.bytes[self.cursor], self.bytes[self.cursor + 1]])
                    } else {
                        u16::from_be_bytes([self.bytes[self.cursor], self.bytes[self.cursor + 1]])
                    };
                    if !(0xDC00..=0xDFFF).contains(&low) {
                        return Err(XmlError::new(
                            ErrorKind::InvalidEncoding,
                            pos,
                            "invalid UTF-16 surrogate pair",
                        ));
                    }
                    self.cursor += 2;
                    self.byte_offset += 2;
                    0x10000 + (((unit as u32 - 0xD800) << 10) | (low as u32 - 0xDC00))
                } else if (0xDC00..=0xDFFF).contains(&unit) {
                    return Err(XmlError::new(
                        ErrorKind::InvalidEncoding,
                        pos,
                        "unpaired UTF-16 low surrogate",
                    ));
                } else {
                    unit as u32
                };
                char::from_u32(scalar).ok_or_else(|| {
                    XmlError::new(ErrorKind::InvalidEncoding, pos, "invalid Unicode scalar")
                })?
            }
        };
        if !is_xml_char(ch) {
            return Err(XmlError::new(
                ErrorKind::InvalidXmlCharacter,
                pos,
                "character is forbidden by XML 1.0",
            ));
        }
        if self.after_cr && ch == '\n' {
            self.after_cr = false;
            return self.next_char();
        }
        if ch == '\r' {
            self.line += 1;
            self.column = 1;
            self.after_cr = true;
            return Ok(Some('\n'));
        } else if ch == '\n' {
            self.after_cr = false;
            self.line += 1;
            self.column = 1;
        } else {
            self.after_cr = false;
            self.column += 1;
        }
        Ok(Some(ch))
    }
}

/// Incremental parser over any `Read` source. A call to `next_event` only reads
/// through the next event, and token sizes are checked before extending buffers.
pub struct Reader<R> {
    decoder: Decoder<R>,
    limits: Limits,
    stack: Vec<Name>,
    namespace_bindings: Vec<(String, String)>,
    namespace_checkpoints: Vec<usize>,
    char_pending: Option<char>,
    events: usize,
    tolerant: bool,
    diagnostics: Vec<Diagnostic>,
    started: bool,
    ended: bool,
    root_seen: bool,
    doctype_seen: bool,
    failed: bool,
}
impl<R: Read> Reader<R> {
    pub fn new(input: R) -> Self {
        Self::with_limits(input, Limits::default())
    }
    pub fn with_limits(input: R, limits: Limits) -> Self {
        Self {
            decoder: Decoder::new(input),
            limits,
            stack: Vec::new(),
            namespace_bindings: Vec::new(),
            namespace_checkpoints: Vec::new(),
            char_pending: None,
            events: 0,
            tolerant: false,
            diagnostics: Vec::new(),
            started: false,
            ended: false,
            root_seen: false,
            doctype_seen: false,
            failed: false,
        }
    }
    /// Enables only one repair: synthesizing end events at EOF for open elements.
    pub fn new_tolerant(input: R) -> Self {
        let mut reader = Self::new(input);
        reader.tolerant = true;
        reader
    }
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
    fn error(&self, kind: ErrorKind, message: impl Into<String>) -> XmlError {
        XmlError::new(kind, self.decoder.position(), message)
    }
    fn memory_used(&self) -> usize {
        self.stack
            .capacity()
            .saturating_mul(std::mem::size_of::<Name>())
            .saturating_add(
                self.namespace_checkpoints
                    .capacity()
                    .saturating_mul(std::mem::size_of::<usize>()),
            )
            .saturating_add(
                self.namespace_bindings
                    .capacity()
                    .saturating_mul(std::mem::size_of::<(String, String)>()),
            )
            .saturating_add(self.stack.iter().fold(0usize, |used, name| {
                used.saturating_add(name.local.capacity())
                    .saturating_add(name.prefix.as_ref().map_or(0, String::capacity))
                    .saturating_add(name.namespace.as_ref().map_or(0, String::capacity))
            }))
            .saturating_add(
                self.namespace_bindings
                    .iter()
                    .fold(0usize, |used, (prefix, uri)| {
                        used.saturating_add(prefix.capacity())
                            .saturating_add(uri.capacity())
                    }),
            )
            .saturating_add(
                self.diagnostics
                    .capacity()
                    .saturating_mul(std::mem::size_of::<Diagnostic>()),
            )
            .saturating_add(self.diagnostics.iter().fold(0usize, |used, diagnostic| {
                used.saturating_add(diagnostic.message.capacity())
            }))
    }
    fn text_budget(&self) -> usize {
        self.limits.max_text_bytes.min(
            self.limits
                .max_memory_bytes
                .saturating_sub(self.memory_used()),
        )
    }
    fn ch(&mut self) -> Result<Option<char>, XmlError> {
        if self.char_pending.is_some() {
            Ok(self.char_pending.take())
        } else {
            self.decoder.next_char()
        }
    }
    fn expect(&mut self, expected: char) -> Result<(), XmlError> {
        match self.ch()? {
            Some(c) if c == expected => Ok(()),
            _ => Err(self.error(ErrorKind::Syntax, format!("expected {expected:?}"))),
        }
    }
    // A single-character pushback is needed by grammar scanners and does not grow with input.
    fn pending_char(&mut self, c: char) {
        self.char_pending = Some(c);
    }
    fn read_char(&mut self) -> Result<Option<char>, XmlError> {
        self.ch()
    }
    fn read_name(&mut self) -> Result<String, XmlError> {
        self.read_name_with_extra(0)
    }
    fn read_name_with_extra(&mut self, retained: usize) -> Result<String, XmlError> {
        let Some(first) = self.read_char()? else {
            return Err(self.error(ErrorKind::Syntax, "expected name"));
        };
        if !is_name_start(first) {
            return Err(self.error(ErrorKind::Syntax, "invalid XML name start"));
        }
        let mut name = String::new();
        let name_budget = self.limits.max_name_bytes.min(
            self.limits
                .max_memory_bytes
                .saturating_sub(self.memory_used().saturating_add(retained)),
        );
        if first.len_utf8() > name_budget {
            return Err(self.error(ErrorKind::Limit, "XML name exceeds configured limit"));
        }
        bounded_push(&mut name, first, name_budget, self.decoder.position())?;
        while let Some(c) = self.ch()? {
            if !is_name_char(c) {
                self.pending_char(c);
                break;
            }
            if name.len() + c.len_utf8() > name_budget {
                return Err(self.error(ErrorKind::Limit, "XML name exceeds configured limit"));
            }
            bounded_push(&mut name, c, name_budget, self.decoder.position())?;
        }
        Ok(name)
    }
    fn whitespace(&mut self) -> Result<(), XmlError> {
        while let Some(c) = self.ch()? {
            if !is_xml_space(c) {
                self.pending_char(c);
                break;
            }
        }
        Ok(())
    }
    fn ref_value(&mut self, out: &mut String, max: usize) -> Result<(), XmlError> {
        let mut token = [0u8; 32];
        let mut token_len = 0usize;
        loop {
            let Some(c) = self.ch()? else {
                return Err(self.error(ErrorKind::Syntax, "unterminated entity reference"));
            };
            if c == ';' {
                break;
            }
            if token_len + c.len_utf8() > token.len() {
                return Err(self.error(ErrorKind::Limit, "entity reference too long"));
            }
            if !c.is_ascii() {
                return Err(self.error(ErrorKind::UnknownEntity, "unknown entity reference"));
            }
            token[token_len] = c as u8;
            token_len += 1;
        }
        let token = std::str::from_utf8(&token[..token_len])
            .map_err(|_| self.error(ErrorKind::Syntax, "invalid entity reference"))?;
        let value = if let Some(hex) = token.strip_prefix("#x") {
            (!hex.is_empty() && hex.bytes().all(|c| c.is_ascii_hexdigit()))
                .then(|| u32::from_str_radix(hex, 16).ok().and_then(char::from_u32))
                .flatten()
        } else if let Some(dec) = token.strip_prefix('#') {
            (!dec.is_empty() && dec.bytes().all(|c| c.is_ascii_digit()))
                .then(|| dec.parse::<u32>().ok().and_then(char::from_u32))
                .flatten()
        } else {
            match token {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "apos" => Some('\''),
                "quot" => Some('"'),
                _ => {
                    return Err(self.error(ErrorKind::UnknownEntity, "unknown entity reference"));
                }
            }
        };
        let c =
            value.ok_or_else(|| self.error(ErrorKind::Syntax, "invalid character reference"))?;
        if !is_xml_char(c) {
            return Err(self.error(
                ErrorKind::InvalidXmlCharacter,
                "character reference is forbidden by XML 1.0",
            ));
        }
        if out.len() + c.len_utf8() > max {
            return Err(self.error(ErrorKind::Limit, "decoded value exceeds configured limit"));
        }
        bounded_push(out, c, max, self.decoder.position())?;
        Ok(())
    }
    fn read_quoted(&mut self, budget: usize) -> Result<String, XmlError> {
        let quote = self
            .read_char()?
            .ok_or_else(|| self.error(ErrorKind::Syntax, "expected quoted value"))?;
        if quote != '\'' && quote != '"' {
            return Err(self.error(ErrorKind::Syntax, "expected quote"));
        }
        let mut out = String::new();
        loop {
            let Some(c) = self.ch()? else {
                return Err(self.error(ErrorKind::Syntax, "unterminated attribute"));
            };
            if c == quote {
                break;
            }
            if c == '<' {
                return Err(self.error(ErrorKind::Syntax, "'<'' is forbidden in attribute values"));
            }
            if c == '&' {
                self.ref_value(&mut out, budget)?;
            } else {
                let c = if c == '\r' || c == '\n' || c == '\t' {
                    ' '
                } else {
                    c
                };
                if out.len() + c.len_utf8() > budget {
                    return Err(self.error(ErrorKind::Limit, "attribute exceeds configured limit"));
                }
                bounded_push(&mut out, c, budget, self.decoder.position())?;
            }
        }
        Ok(out)
    }
    fn raw_until(&mut self, terminator: &str, limit: usize) -> Result<String, XmlError> {
        let mut out = String::new();
        let mut pending = ['\0'; 3];
        let mut pending_len = 0usize;
        let term_len = terminator.chars().count();
        loop {
            let Some(c) = self.ch()? else {
                return Err(self.error(ErrorKind::Syntax, "unterminated markup"));
            };
            if pending_len == pending.len() {
                return Err(self.error(ErrorKind::Syntax, "invalid parser delimiter state"));
            }
            pending[pending_len] = c;
            pending_len += 1;
            let is_prefix = |chars: &[char]| {
                terminator
                    .chars()
                    .zip(chars.iter().copied())
                    .all(|(a, b)| a == b)
                    && chars.len() <= term_len
            };
            while !is_prefix(&pending[..pending_len]) {
                let first = pending[0];
                pending.copy_within(1..pending_len, 0);
                pending_len -= 1;
                bounded_push(&mut out, first, limit, self.decoder.position())?;
            }
            if pending_len == term_len {
                return Ok(out);
            }
        }
    }
    fn expanded(
        &self,
        raw: &str,
        namespaces: &[(String, String)],
        attribute: bool,
        event_memory: usize,
    ) -> Result<Name, XmlError> {
        let mut split = raw.split(':');
        let first = split.next().unwrap_or("");
        let second = split.next();
        if split.next().is_some() {
            return Err(self.error(ErrorKind::Namespace, "QName contains more than one colon"));
        }
        let (prefix, local) = if let Some(local) = second {
            (Some(first), local)
        } else {
            (None, first)
        };
        if !is_ncname(local) || prefix.is_some_and(|p| !is_ncname(p)) {
            return Err(self.error(ErrorKind::Namespace, "invalid qualified name"));
        }
        if !attribute && prefix == Some("xmlns") {
            return Err(self.error(
                ErrorKind::Namespace,
                "xmlns is reserved for namespace declarations",
            ));
        }
        let namespace_ref = if prefix == Some("xmlns") {
            Some(XMLNS_NS)
        } else if prefix == Some("xml") {
            Some(XML_NS)
        } else if let Some(p) = prefix {
            Some(
                namespaces
                    .iter()
                    .rev()
                    .find(|(k, _)| k == p)
                    .map(|(_, v)| v.as_str())
                    .ok_or_else(|| self.error(ErrorKind::Namespace, "unbound namespace prefix"))?,
            )
        } else if attribute {
            None
        } else {
            namespaces
                .iter()
                .rev()
                .find(|(k, _)| k.is_empty())
                .map(|(_, v)| v.as_str())
                .filter(|v| !v.is_empty())
        };
        let allocation = local
            .len()
            .saturating_add(prefix.map_or(0, str::len))
            .saturating_add(namespace_ref.map_or(0, str::len));
        if self
            .memory_used()
            .saturating_add(event_memory)
            .saturating_add(allocation)
            > self.limits.max_memory_bytes
        {
            return Err(self.error(ErrorKind::Limit, "expanded names exceed memory budget"));
        }
        Ok(Name {
            prefix: prefix.map(str::to_owned),
            local: local.to_owned(),
            namespace: namespace_ref.map(str::to_owned),
        })
    }
    fn parse_start(&mut self) -> Result<Event, XmlError> {
        let raw = self.read_name()?;
        let mut raw_attrs: Vec<(String, String)> = Vec::new();
        let mut empty = false;
        loop {
            let separator = self.ch()?;
            let separated = separator.is_some_and(is_xml_space);
            if let Some(c) = separator {
                self.pending_char(c);
            }
            self.whitespace()?;
            match self.read_char()? {
                Some('>') => break,
                Some('/') => {
                    self.expect('>')?;
                    empty = true;
                    break;
                }
                Some(c) => {
                    if !separated {
                        return Err(self.error(
                            ErrorKind::Syntax,
                            "attributes require whitespace separation",
                        ));
                    }
                    self.pending_char(c);
                    let retained_names = raw
                        .len()
                        .saturating_add(raw_attrs.iter().fold(0usize, |used, (n, v)| {
                            used.saturating_add(n.len()).saturating_add(v.len())
                        }))
                        .saturating_add(raw_attrs.len().saturating_mul(std::mem::size_of::<(
                            String,
                            String,
                        )>(
                        )))
                        .saturating_add(std::mem::size_of::<(String, String)>());
                    let name = self.read_name_with_extra(retained_names)?;
                    if raw_attrs.iter().any(|(existing, _)| existing == &name) {
                        return Err(self
                            .error(ErrorKind::DuplicateAttribute, "duplicate lexical attribute"));
                    }
                    if raw_attrs.len() >= self.limits.max_attributes_per_element {
                        return Err(self.error(ErrorKind::Limit, "too many attributes"));
                    }
                    self.whitespace()?;
                    self.expect('=')?;
                    self.whitespace()?;
                    let retained = self
                        .memory_used()
                        .saturating_add(raw.len())
                        .saturating_add(raw_attrs.iter().fold(0usize, |used, (n, v)| {
                            used.saturating_add(n.len()).saturating_add(v.len())
                        }))
                        .saturating_add(raw_attrs.len().saturating_mul(std::mem::size_of::<(
                            String,
                            String,
                        )>(
                        )))
                        .saturating_add(name.len())
                        .saturating_add(std::mem::size_of::<(String, String)>());
                    let remaining = self
                        .limits
                        .max_memory_bytes
                        .saturating_sub(retained)
                        .min(self.limits.max_attribute_bytes);
                    if remaining == 0 {
                        return Err(self.error(ErrorKind::Limit, "XML event exceeds memory budget"));
                    }
                    raw_attrs.try_reserve_exact(1).map_err(|_| {
                        self.error(ErrorKind::Limit, "cannot reserve bounded attribute slot")
                    })?;
                    let value = self.read_quoted(remaining)?;
                    raw_attrs.push((name, value));
                }
                None => return Err(self.error(ErrorKind::Syntax, "unterminated start tag")),
            }
        }
        if self.stack.len() >= self.limits.max_depth {
            return Err(self.error(ErrorKind::Limit, "maximum element depth exceeded"));
        }
        if self.stack.is_empty() {
            if self.root_seen {
                return Err(self.error(ErrorKind::Syntax, "multiple document elements"));
            }
            self.root_seen = true;
        }
        let checkpoint = self.namespace_bindings.len();
        let mut declarations = 0usize;
        for (key, value) in &raw_attrs {
            let prefix = if key == "xmlns" {
                Some("")
            } else {
                key.strip_prefix("xmlns:")
            };
            if let Some(p) = prefix {
                declarations += 1;
                if declarations > self.limits.max_namespaces_per_element {
                    return Err(self.error(ErrorKind::Limit, "too many namespace declarations"));
                }
                if p == "xmlns"
                    || key != "xmlns" && (p.is_empty() || !is_ncname(p))
                    || (p == "xml" && value != XML_NS)
                    || (p != "xml" && value == XML_NS)
                    || value == XMLNS_NS
                    || (!p.is_empty() && value.is_empty())
                {
                    return Err(self.error(
                        ErrorKind::Namespace,
                        "invalid reserved or empty namespace binding",
                    ));
                }
                if p != "xml" {
                    let raw_bytes = raw
                        .len()
                        .saturating_add(raw_attrs.iter().fold(0usize, |used, (n, v)| {
                            used.saturating_add(n.len()).saturating_add(v.len())
                        }))
                        .saturating_add(raw_attrs.len().saturating_mul(std::mem::size_of::<(
                            String,
                            String,
                        )>(
                        )));
                    let binding_bytes = p
                        .len()
                        .saturating_add(value.len())
                        .saturating_add(std::mem::size_of::<(String, String)>());
                    if self
                        .memory_used()
                        .saturating_add(raw_bytes)
                        .saturating_add(binding_bytes)
                        > self.limits.max_memory_bytes
                    {
                        return Err(
                            self.error(ErrorKind::Limit, "namespace bindings exceed memory budget")
                        );
                    }
                    self.namespace_bindings.try_reserve_exact(1).map_err(|_| {
                        self.error(ErrorKind::Limit, "cannot reserve bounded namespace slot")
                    })?;
                    self.namespace_bindings.push((p.to_owned(), value.clone()));
                }
            }
        }
        let mut event_memory = raw
            .len()
            .saturating_add(raw_attrs.iter().fold(0usize, |used, (n, v)| {
                used.saturating_add(n.len()).saturating_add(v.len())
            }))
            .saturating_add(
                raw_attrs
                    .len()
                    .saturating_mul(std::mem::size_of::<(String, String)>()),
            );
        let name = self.expanded(&raw, &self.namespace_bindings, false, event_memory)?;
        event_memory = event_memory
            .saturating_add(name.local.len())
            .saturating_add(name.prefix.as_ref().map_or(0, String::len))
            .saturating_add(name.namespace.as_ref().map_or(0, String::len));
        let attrs_remaining = self
            .limits
            .max_memory_bytes
            .saturating_sub(self.memory_used().saturating_add(event_memory));
        let metadata_bytes = raw_attrs.len().saturating_mul(
            std::mem::size_of::<Attribute>() + std::mem::size_of::<(Option<String>, String)>(),
        );
        if metadata_bytes > attrs_remaining {
            return Err(self.error(ErrorKind::Limit, "attribute event exceeds memory budget"));
        }
        let mut attrs: Vec<Attribute> = Vec::new();
        attrs
            .try_reserve_exact(raw_attrs.len())
            .map_err(|_| self.error(ErrorKind::Limit, "cannot reserve bounded attribute event"))?;
        for (raw, value) in raw_attrs {
            if raw == "xmlns" || raw.starts_with("xmlns:") {
                continue;
            }
            let n = self.expanded(
                &raw,
                &self.namespace_bindings,
                true,
                event_memory.saturating_add(metadata_bytes),
            )?;
            event_memory = event_memory
                .saturating_add(n.local.len())
                .saturating_add(n.prefix.as_ref().map_or(0, String::len))
                .saturating_add(n.namespace.as_ref().map_or(0, String::len));
            if self
                .memory_used()
                .saturating_add(event_memory)
                .saturating_add(metadata_bytes)
                > self.limits.max_memory_bytes
            {
                return Err(self.error(ErrorKind::Limit, "attribute names exceed memory budget"));
            }
            if attrs.iter().any(|existing| {
                existing.name.namespace == n.namespace && existing.name.local == n.local
            }) {
                return Err(self.error(
                    ErrorKind::DuplicateAttribute,
                    "duplicate attribute expanded name",
                ));
            }
            attrs.push(Attribute { name: n, value });
        }
        if !empty {
            let name_bytes = name
                .local
                .len()
                .saturating_add(name.prefix.as_ref().map_or(0, String::len))
                .saturating_add(name.namespace.as_ref().map_or(0, String::len));
            let stack_extra = name_bytes
                .saturating_add(std::mem::size_of::<Name>())
                .saturating_add(std::mem::size_of::<usize>());
            if self
                .memory_used()
                .saturating_add(event_memory)
                .saturating_add(metadata_bytes)
                .saturating_add(stack_extra)
                > self.limits.max_memory_bytes
            {
                return Err(self.error(ErrorKind::Limit, "element scope exceeds memory budget"));
            }
            self.stack.try_reserve_exact(1).map_err(|_| {
                self.error(ErrorKind::Limit, "cannot reserve bounded element scope")
            })?;
            self.namespace_checkpoints
                .try_reserve_exact(1)
                .map_err(|_| {
                    self.error(
                        ErrorKind::Limit,
                        "cannot reserve bounded namespace checkpoint",
                    )
                })?;
            self.stack.push(name.clone());
            self.namespace_checkpoints.push(checkpoint);
        } else {
            self.namespace_bindings.truncate(checkpoint);
        }
        Ok(Event::StartElement {
            name,
            attributes: attrs,
            empty,
        })
    }
    fn next_inner(&mut self) -> Result<Option<Event>, XmlError> {
        if self.ended {
            return Ok(None);
        }
        let declaration_allowed = !self.started;
        self.started = true;
        let Some(c) = self.ch()? else {
            if !self.stack.is_empty() {
                if self.tolerant {
                    let diagnostic_message = "inserted missing closing tag";
                    let diagnostic_bytes =
                        std::mem::size_of::<Diagnostic>().saturating_add(diagnostic_message.len());
                    if self.memory_used().saturating_add(diagnostic_bytes)
                        > self.limits.max_memory_bytes
                    {
                        return Err(
                            self.error(ErrorKind::Limit, "diagnostic exceeds memory budget")
                        );
                    }
                    self.diagnostics.try_reserve_exact(1).map_err(|_| {
                        self.error(ErrorKind::Limit, "cannot reserve bounded diagnostic")
                    })?;
                    let n = self
                        .stack
                        .pop()
                        .ok_or_else(|| self.error(ErrorKind::Syntax, "empty element stack"))?;
                    if let Some(checkpoint) = self.namespace_checkpoints.pop() {
                        self.namespace_bindings.truncate(checkpoint);
                    }
                    self.diagnostics.push(Diagnostic {
                        position: self.decoder.position(),
                        message: diagnostic_message.to_owned(),
                    });
                    return Ok(Some(Event::EndElement { name: n }));
                }
                return Err(self.error(ErrorKind::Syntax, "unexpected end of input inside element"));
            }
            if !self.root_seen {
                return Err(self.error(ErrorKind::Syntax, "document has no root element"));
            }
            self.ended = true;
            return Ok(None);
        };
        if c != '<' {
            let mut out = String::new();
            if c == '&' {
                self.ref_value(&mut out, self.text_budget())?;
            } else {
                let text_budget = self.text_budget();
                if c.len_utf8() > text_budget {
                    return Err(self.error(ErrorKind::Limit, "text exceeds configured limit"));
                }
                bounded_push(
                    &mut out,
                    if c == '\r' { '\n' } else { c },
                    text_budget,
                    self.decoder.position(),
                )?;
            }
            while let Some(ch) = self.ch()? {
                if ch == '<' {
                    self.pending_char(ch);
                    break;
                }
                if ch == '&' {
                    let total_budget = self.text_budget();
                    self.ref_value(&mut out, total_budget)?;
                } else {
                    let remaining = self.text_budget().saturating_sub(out.len());
                    if ch.len_utf8() > remaining {
                        return Err(self.error(ErrorKind::Limit, "text exceeds configured limit"));
                    }
                    bounded_push(
                        &mut out,
                        if ch == '\r' { '\n' } else { ch },
                        self.text_budget(),
                        self.decoder.position(),
                    )?;
                }
            }
            if out.contains("]]>") {
                return Err(self.error(ErrorKind::Syntax, "']]>' is forbidden in character data"));
            }
            if self.stack.is_empty() {
                if out.chars().all(is_xml_space) {
                    return self.next_inner();
                }
                return Err(self.error(ErrorKind::Syntax, "text outside document element"));
            }
            return Ok(Some(Event::Text(out)));
        }
        match self.ch()? {
            Some('/') => {
                let raw = self.read_name()?;
                self.whitespace()?;
                self.expect('>')?;
                let top = self.stack.pop().ok_or_else(|| {
                    self.error(ErrorKind::Syntax, "closing tag without an open element")
                })?;
                let checkpoint = self.namespace_checkpoints.pop().ok_or_else(|| {
                    self.error(ErrorKind::Syntax, "missing namespace scope checkpoint")
                })?;
                self.namespace_bindings.truncate(checkpoint);
                let matches_raw = match top.prefix.as_deref() {
                    Some(prefix) => raw.split_once(':') == Some((prefix, top.local.as_str())),
                    None => raw == top.local,
                };
                if !matches_raw {
                    return Err(self.error(ErrorKind::Syntax, "mismatched closing tag"));
                }
                Ok(Some(Event::EndElement { name: top }))
            }
            Some('?') => {
                let target = self.read_name()?;
                if target.eq_ignore_ascii_case("xml") && (target != "xml" || !declaration_allowed) {
                    return Err(self.error(
                        ErrorKind::Syntax,
                        "XML declaration is only allowed at document start",
                    ));
                }
                let separator = self.ch()?;
                let separated = separator.is_some_and(is_xml_space);
                if let Some(c) = separator {
                    self.pending_char(c);
                }
                self.whitespace()?;
                let data_budget = self.text_budget().saturating_sub(target.capacity());
                let data = self.raw_until("?>", data_budget)?;
                if !separated && !data.is_empty() {
                    return Err(self.error(ErrorKind::Syntax, "PI data requires whitespace"));
                }
                if target.eq_ignore_ascii_case("xml") {
                    validate_declaration(&data, self.decoder.encoding.unwrap_or(Encoding::Utf8))
                        .map_err(|message| self.error(ErrorKind::Syntax, message))?;
                    self.next_inner()
                } else {
                    Ok(Some(Event::ProcessingInstruction { target, data }))
                }
            }
            Some('!') => match self.ch()? {
                Some('-') => {
                    self.expect('-')?;
                    let data = self.raw_until("-->", self.text_budget())?;
                    if data.contains("--") || data.ends_with('-') {
                        return Err(self.error(ErrorKind::Syntax, "'--' inside comment"));
                    }
                    Ok(Some(Event::Comment(data)))
                }
                Some('[') => {
                    if self.stack.is_empty() {
                        return Err(self.error(ErrorKind::Syntax, "CDATA outside document element"));
                    }
                    for ch in "CDATA[".chars() {
                        self.expect(ch)?;
                    }
                    let data = self.raw_until("]]>", self.text_budget())?;
                    let data = data.replace('\r', "\n");
                    Ok(Some(Event::Text(data)))
                }
                Some('D') => {
                    if self.root_seen || self.doctype_seen {
                        return Err(self.error(ErrorKind::Syntax, "DOCTYPE after document element"));
                    }
                    for ch in "OCTYPE".chars() {
                        self.expect(ch)?;
                    }
                    self.doctype_seen = true;
                    self.skip_doctype()?;
                    self.next_inner()
                }
                _ => Err(self.error(ErrorKind::Syntax, "unsupported declaration")),
            },
            Some(c) => {
                self.pending_char(c);
                self.parse_start().map(Some)
            }
            None => Err(self.error(ErrorKind::Syntax, "truncated markup")),
        }
    }
    fn skip_doctype(&mut self) -> Result<(), XmlError> {
        if !self.ch()?.is_some_and(is_xml_space) {
            return Err(self.error(
                ErrorKind::Syntax,
                "DOCTYPE requires whitespace before its name",
            ));
        }
        self.whitespace()?;
        let _doctype_name = self.read_name()?;
        let mut quote = None;
        let mut subset = 0usize;
        let mut comment = false;
        let mut processing_instruction = false;
        let mut tail = ['\0'; 4];
        let mut tail_len = 0usize;
        loop {
            let Some(c) = self.ch()? else {
                return Err(self.error(ErrorKind::Syntax, "unterminated DOCTYPE"));
            };
            if comment {
                push_tail(&mut tail, &mut tail_len, c);
                if tail_ends_with(&tail, tail_len, "-->") {
                    comment = false;
                    tail_len = 0;
                }
                continue;
            }
            if processing_instruction {
                push_tail(&mut tail, &mut tail_len, c);
                if tail_ends_with(&tail, tail_len, "?>") {
                    processing_instruction = false;
                    tail_len = 0;
                }
                continue;
            }
            if let Some(q) = quote {
                if c == q {
                    quote = None;
                }
            } else {
                push_tail(&mut tail, &mut tail_len, c);
                if tail_ends_with(&tail, tail_len, "<!--") {
                    comment = true;
                    tail_len = 0;
                    continue;
                }
                if tail_ends_with(&tail, tail_len, "<?") {
                    processing_instruction = true;
                    tail_len = 0;
                    continue;
                }
                match c {
                    '\'' | '"' => quote = Some(c),
                    '[' => subset += 1,
                    ']' if subset > 0 => subset -= 1,
                    ']' => {
                        return Err(
                            self.error(ErrorKind::Syntax, "unmatched closing bracket in DOCTYPE")
                        );
                    }
                    '>' if subset == 0 => return Ok(()),
                    _ => {}
                }
            }
        }
    }
    fn after_event_budget(&mut self) -> Result<Option<Event>, XmlError> {
        if !self.root_seen || !self.stack.is_empty() {
            return Err(self.error(ErrorKind::Limit, "maximum event count exceeded"));
        }
        loop {
            match self.ch()? {
                Some(c) if is_xml_space(c) => {}
                None => {
                    self.ended = true;
                    return Ok(None);
                }
                Some(c) => {
                    self.pending_char(c);
                    return Err(self.error(ErrorKind::Limit, "maximum event count exceeded"));
                }
            }
        }
    }
    pub fn next_event(&mut self) -> Result<Option<Event>, XmlError> {
        if self.failed || self.ended {
            return Ok(None);
        }
        if self.events >= self.limits.max_events {
            return match self.after_event_budget() {
                Ok(event) => Ok(event),
                Err(error) => {
                    self.failed = true;
                    Err(error)
                }
            };
        }
        let event = match self.next_inner() {
            Ok(event) => event,
            Err(error) => {
                self.failed = true;
                return Err(error);
            }
        };
        if event.is_some() {
            self.events += 1;
        }
        Ok(event)
    }
}

/// Iterator adapter that stops after the first parse error.
impl<R: Read> Iterator for Reader<R> {
    type Item = Result<Event, XmlError>;
    fn next(&mut self) -> Option<Self::Item> {
        match self.next_event() {
            Ok(Some(e)) => Some(Ok(e)),
            Ok(None) => None,
            Err(e) => {
                self.ended = true;
                Some(Err(e))
            }
        }
    }
}

/// Deterministic XML writer. It emits a UTF-8 XML declaration and caller events
/// exactly once; invalid or unbalanced event streams fail before closing tags.
pub struct Writer<W: Write> {
    output: W,
    stack: Vec<String>,
    uris: Vec<String>,
    finished: bool,
    root_seen: bool,
}
impl<W: Write> Writer<W> {
    pub fn new(mut output: W) -> Result<Self, XmlError> {
        output
            .write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")
            .map_err(|e| XmlError::io(Position::default(), e))?;
        Ok(Self {
            output,
            stack: Vec::new(),
            uris: Vec::new(),
            finished: false,
            root_seen: false,
        })
    }
    fn qname(&mut self, name: &Name) -> Result<(String, Option<(String, String)>), XmlError> {
        validate_name(name)?;
        match name.namespace.as_deref().filter(|uri| !uri.is_empty()) {
            None => Ok((name.local.clone(), None)),
            Some(XML_NS) => Ok((format!("xml:{}", name.local), None)),
            Some(XMLNS_NS) => Err(XmlError::new(
                ErrorKind::Namespace,
                Position::default(),
                "the xmlns namespace is reserved for declarations",
            )),
            Some(uri) => {
                let index = self
                    .uris
                    .iter()
                    .position(|known| known == uri)
                    .unwrap_or_else(|| {
                        self.uris.push(uri.to_owned());
                        self.uris.len() - 1
                    });
                let prefix = format!("ns{index}");
                Ok((
                    format!("{prefix}:{}", name.local),
                    Some((prefix, uri.to_owned())),
                ))
            }
        }
    }
    pub fn write_event(&mut self, event: &Event) -> Result<(), XmlError> {
        let result = self.write_inner(event);
        if result.is_err() {
            self.finished = true;
        }
        result
    }
    fn write_inner(&mut self, event: &Event) -> Result<(), XmlError> {
        if self.finished {
            return Err(XmlError::new(
                ErrorKind::WriterState,
                Position::default(),
                "writer already finished",
            ));
        }
        match event {
            Event::StartElement {
                name,
                attributes,
                empty,
            } => {
                if self.stack.is_empty() {
                    if self.root_seen {
                        return Err(XmlError::new(
                            ErrorKind::WriterState,
                            Position::default(),
                            "multiple document elements",
                        ));
                    }
                    self.root_seen = true;
                }
                let (q, element_binding) = self.qname(name)?;
                write!(self.output, "<{}", q).map_err(|e| XmlError::io(Position::default(), e))?;
                let mut seen = HashSet::new();
                let mut bindings = Vec::new();
                if let Some(binding) = element_binding {
                    bindings.push(binding);
                }
                let mut rendered_attrs = Vec::with_capacity(attributes.len());
                for a in attributes {
                    validate_name(&a.name)?;
                    if a.name.local == "xmlns"
                        && a.name.namespace.as_deref().is_none_or(str::is_empty)
                    {
                        return Err(XmlError::new(
                            ErrorKind::Namespace,
                            Position::default(),
                            "namespace declarations are generated by the writer",
                        ));
                    }
                    if !seen.insert((
                        a.name.namespace.as_deref().unwrap_or(""),
                        a.name.local.as_str(),
                    )) {
                        return Err(XmlError::new(
                            ErrorKind::DuplicateAttribute,
                            Position::default(),
                            "duplicate expanded attribute",
                        ));
                    }
                    let (attr_name, binding) = self.qname(&a.name)?;
                    if let Some(binding) = binding {
                        bindings.push(binding);
                    }
                    rendered_attrs.push((attr_name, &a.value));
                }
                bindings.sort_by(|a, b| a.0.cmp(&b.0));
                bindings.dedup_by(|a, b| a.0 == b.0);
                for (prefix, uri) in bindings {
                    write!(self.output, " xmlns:{}=\"", prefix)
                        .map_err(|e| XmlError::io(Position::default(), e))?;
                    escape_attr(&mut self.output, &uri)?;
                    self.output
                        .write_all(b"\"")
                        .map_err(|e| XmlError::io(Position::default(), e))?;
                }
                for (attr_name, value) in rendered_attrs {
                    write!(self.output, " {}=\"", attr_name)
                        .map_err(|e| XmlError::io(Position::default(), e))?;
                    escape_attr(&mut self.output, value)?;
                    self.output
                        .write_all(b"\"")
                        .map_err(|e| XmlError::io(Position::default(), e))?;
                }
                if *empty {
                    self.output
                        .write_all(b"/>")
                        .map_err(|e| XmlError::io(Position::default(), e))?;
                } else {
                    self.output
                        .write_all(b">")
                        .map_err(|e| XmlError::io(Position::default(), e))?;
                    self.stack.push(q);
                }
            }
            Event::EndElement { name } => {
                let (q, _) = self.qname(name)?;
                let top = self.stack.last().ok_or_else(|| {
                    XmlError::new(
                        ErrorKind::WriterState,
                        Position::default(),
                        "end event without start",
                    )
                })?;
                if &q != top {
                    return Err(XmlError::new(
                        ErrorKind::WriterState,
                        Position::default(),
                        "end event does not match open element",
                    ));
                }
                self.stack.pop();
                write!(self.output, "</{}>", q)
                    .map_err(|e| XmlError::io(Position::default(), e))?;
            }
            Event::Text(s) => {
                if self.stack.is_empty() && !s.chars().all(is_xml_space) {
                    return Err(XmlError::new(
                        ErrorKind::WriterState,
                        Position::default(),
                        "text outside document element",
                    ));
                }
                escape_text(&mut self.output, s)?;
            }
            Event::Comment(s) => {
                validate_chars(s)?;
                if s.contains("--") || s.ends_with('-') {
                    return Err(XmlError::new(
                        ErrorKind::Syntax,
                        Position::default(),
                        "invalid comment",
                    ));
                }
                self.output
                    .write_all(b"<!--")
                    .map_err(|e| XmlError::io(Position::default(), e))?;
                write_eol_normalized(&mut self.output, s)?;
                self.output
                    .write_all(b"-->")
                    .map_err(|e| XmlError::io(Position::default(), e))?;
            }
            Event::ProcessingInstruction { target, data } => {
                validate_chars(data)?;
                if !is_ncname(target) || target.eq_ignore_ascii_case("xml") || data.contains("?>") {
                    return Err(XmlError::new(
                        ErrorKind::Syntax,
                        Position::default(),
                        "invalid processing instruction",
                    ));
                }
                write!(self.output, "<?{} ", target)
                    .map_err(|e| XmlError::io(Position::default(), e))?;
                write_eol_normalized(&mut self.output, data)?;
                self.output
                    .write_all(b"?>")
                    .map_err(|e| XmlError::io(Position::default(), e))?;
            }
        }
        Ok(())
    }
    pub fn finish(mut self) -> Result<W, XmlError> {
        if self.finished || !self.root_seen || !self.stack.is_empty() {
            return Err(XmlError::new(
                ErrorKind::WriterState,
                Position::default(),
                "unclosed element",
            ));
        }
        self.output
            .flush()
            .map_err(|e| XmlError::io(Position::default(), e))?;
        self.finished = true;
        Ok(self.output)
    }
}
fn validate_name(name: &Name) -> Result<(), XmlError> {
    if !is_ncname(&name.local) || name.prefix.as_deref().is_some_and(|p| !is_ncname(p)) {
        return Err(XmlError::new(
            ErrorKind::Syntax,
            Position::default(),
            "invalid XML name",
        ));
    }
    if name.prefix.is_some() && name.namespace.as_deref().is_none_or(str::is_empty)
        || name.prefix.as_deref() == Some("xmlns")
        || name.prefix.as_deref() == Some("xml") && name.namespace.as_deref() != Some(XML_NS)
        || name.namespace.as_deref() == Some(XML_NS)
            && name.prefix.as_deref().is_some_and(|p| p != "xml")
    {
        return Err(XmlError::new(
            ErrorKind::Namespace,
            Position::default(),
            "prefixed name has no namespace URI",
        ));
    }
    if let Some(uri) = &name.namespace {
        validate_chars(uri)?;
    }
    Ok(())
}
fn validate_chars(s: &str) -> Result<(), XmlError> {
    if s.chars().any(|c| !is_xml_char(c)) {
        Err(XmlError::new(
            ErrorKind::InvalidXmlCharacter,
            Position::default(),
            "illegal XML character",
        ))
    } else {
        Ok(())
    }
}
fn bounded_push(
    out: &mut String,
    c: char,
    limit: usize,
    position: Position,
) -> Result<(), XmlError> {
    let next = out
        .len()
        .checked_add(c.len_utf8())
        .ok_or_else(|| XmlError::new(ErrorKind::Limit, position, "XML buffer size overflow"))?;
    if next > limit {
        return Err(XmlError::new(
            ErrorKind::Limit,
            position,
            "XML buffer exceeds configured limit",
        ));
    }
    if out.capacity() < next {
        out.try_reserve_exact(next - out.len()).map_err(|_| {
            XmlError::new(
                ErrorKind::Limit,
                position,
                "cannot reserve bounded XML buffer",
            )
        })?;
    }
    out.push(c);
    Ok(())
}
fn push_tail(tail: &mut [char; 4], len: &mut usize, c: char) {
    if *len == tail.len() {
        tail.copy_within(1.., 0);
        *len -= 1;
    }
    tail[*len] = c;
    *len += 1;
}
fn tail_ends_with(tail: &[char; 4], len: usize, suffix: &str) -> bool {
    let suffix_len = suffix.chars().count();
    len >= suffix_len
        && suffix
            .chars()
            .zip(tail[len - suffix_len..len].iter().copied())
            .all(|(a, b)| a == b)
}
fn escape_text<W: Write>(out: &mut W, s: &str) -> Result<(), XmlError> {
    for c in s.chars() {
        if !is_xml_char(c) {
            return Err(XmlError::new(
                ErrorKind::InvalidXmlCharacter,
                Position::default(),
                "illegal XML character",
            ));
        }
        match c {
            '&' => out.write_all(b"&amp;"),
            '<' => out.write_all(b"&lt;"),
            '>' => out.write_all(b"&gt;"),
            '\r' => out.write_all(b"&#13;"),
            _ => {
                let mut b = [0; 4];
                out.write_all(c.encode_utf8(&mut b).as_bytes())
            }
        }
        .map_err(|e| XmlError::io(Position::default(), e))?;
    }
    Ok(())
}
fn escape_attr<W: Write>(out: &mut W, s: &str) -> Result<(), XmlError> {
    for c in s.chars() {
        if !is_xml_char(c) {
            return Err(XmlError::new(
                ErrorKind::InvalidXmlCharacter,
                Position::default(),
                "illegal XML character",
            ));
        }
        match c {
            '&' => out.write_all(b"&amp;"),
            '<' => out.write_all(b"&lt;"),
            '"' => out.write_all(b"&quot;"),
            '\t' => out.write_all(b"&#9;"),
            '\n' => out.write_all(b"&#10;"),
            '\r' => out.write_all(b"&#13;"),
            _ => {
                let mut b = [0; 4];
                out.write_all(c.encode_utf8(&mut b).as_bytes())
            }
        }
        .map_err(|e| XmlError::io(Position::default(), e))?;
    }
    Ok(())
}
fn write_eol_normalized<W: Write>(out: &mut W, s: &str) -> Result<(), XmlError> {
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        let c = if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            '\n'
        } else {
            c
        };
        let mut bytes = [0; 4];
        out.write_all(c.encode_utf8(&mut bytes).as_bytes())
            .map_err(|e| XmlError::io(Position::default(), e))?;
    }
    Ok(())
}
fn is_xml_char(c: char) -> bool {
    matches!(c as u32,0x9|0xA|0xD|0x20..=0xD7FF|0xE000..=0xFFFD|0x10000..=0x10FFFF)
}
fn is_name_start(c: char) -> bool {
    c == ':'
        || c == '_'
        || c.is_ascii_alphabetic()
        || matches!(c as u32,0xC0..=0xD6|0xD8..=0xF6|0xF8..=0x2FF|0x370..=0x37D|0x37F..=0x1FFF|0x200C..=0x200D|0x2070..=0x218F|0x2C00..=0x2FEF|0x3001..=0xD7FF|0xF900..=0xFDCF|0xFDF0..=0xFFFD|0x10000..=0xEFFFF)
}
fn is_xml_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

fn validate_declaration(mut data: &str, encoding: Encoding) -> Result<(), &'static str> {
    let mut fields = Vec::new();
    while !data.is_empty() {
        data = data.trim_start_matches(is_xml_space);
        if data.is_empty() {
            break;
        }
        let end = data
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(data.len());
        let key = &data[..end];
        data = data[end..].trim_start_matches(is_xml_space);
        data = data
            .strip_prefix('=')
            .ok_or("malformed XML declaration")?
            .trim_start_matches(is_xml_space);
        let quote = data
            .chars()
            .next()
            .filter(|c| matches!(c, '\'' | '"'))
            .ok_or("malformed XML declaration")?;
        data = &data[1..];
        let end = data.find(quote).ok_or("malformed XML declaration")?;
        let value = &data[..end];
        fields.push((key, value));
        if fields.len() > 3 {
            return Err("too many XML declaration fields");
        }
        data = &data[end + 1..];
        if data.chars().next().is_some_and(|c| !is_xml_space(c)) {
            return Err("XML declaration fields require whitespace");
        }
    }
    if fields.first() != Some(&("version", "1.0")) {
        return Err("XML 1.0 version declaration required");
    }
    let mut rest = fields[1..].iter();
    let mut next = rest.next();
    if let Some(("encoding", value)) = next {
        let valid = match encoding {
            Encoding::Utf8 => value.eq_ignore_ascii_case("UTF-8"),
            Encoding::Utf16Le => {
                value.eq_ignore_ascii_case("UTF-16") || value.eq_ignore_ascii_case("UTF-16LE")
            }
            Encoding::Utf16Be => {
                value.eq_ignore_ascii_case("UTF-16") || value.eq_ignore_ascii_case("UTF-16BE")
            }
        };
        if !valid {
            return Err("encoding declaration disagrees with decoded bytes");
        }
        next = rest.next();
    }
    if let Some(field) = next {
        if !matches!(field, ("standalone", "yes" | "no")) {
            return Err("invalid XML declaration field");
        }
        if rest.next().is_some() {
            return Err("invalid XML declaration order");
        }
    }
    Ok(())
}
fn is_name_char(c: char) -> bool {
    is_name_start(c)
        || c.is_ascii_digit()
        || c == '-'
        || c == '.'
        || c == '\u{B7}'
        || matches!(c as u32,0x300..=0x36F|0x203F..=0x2040)
}
fn is_ncname(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(),Some(c) if c!=':'&&is_name_start(c))
        && chars.all(|c| c != ':' && is_name_char(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn events(bytes: &[u8]) -> Result<Vec<Event>, XmlError> {
        Reader::new(Cursor::new(bytes)).collect()
    }
    #[test]
    fn utf_boundaries_and_fragment_reads() {
        struct One {
            b: Cursor<Vec<u8>>,
        }
        impl Read for One {
            fn read(&mut self, o: &mut [u8]) -> io::Result<usize> {
                let n = o.len().min(1);
                self.b.read(&mut o[..n])
            }
        }
        let input = "<r>é𝄞&amp; 🙂</r>";
        let got: Vec<_> = Reader::new(One {
            b: Cursor::new(input.as_bytes().to_vec()),
        })
        .collect::<Result<_, _>>()
        .unwrap();
        assert!(got.iter().any(|e| *e == Event::Text("é𝄞& 🙂".into())));
    }
    #[test]
    fn utf16_split_surrogate() {
        let mut b = vec![0xFF, 0xFE];
        for u in "<r>🙂</r>".encode_utf16() {
            b.extend(u.to_le_bytes());
        }
        assert!(events(&b).unwrap().contains(&Event::Text("🙂".into())));
    }
    #[test]
    fn namespaces_and_expanded_duplicate_detection() {
        let e = events(br#"<a xmlns="u" xmlns:p="v"><p:x plain="1" p:z="2"/></a>"#).unwrap();
        let Event::StartElement { name, .. } = &e[0] else {
            panic!()
        };
        assert_eq!(name.namespace.as_deref(), Some("u"));
        assert!(events(br#"<a xmlns:x="u" xmlns:y="u" x:b="1" y:b="2"/>"#).is_err());
    }
    #[test]
    fn constructs_and_dtd_do_not_expand() {
        let e =
            events(b"<!DOCTYPE r [<!ENTITY x 'bad'>]><r><!-- c --><![CDATA[a&b]]><?go now?></r>")
                .unwrap();
        assert!(e.iter().any(|e| matches!(e,Event::Text(s) if s=="a&b")));
        assert!(events(b"<!DOCTYPE r [<!ENTITY x 'bad'>]><r>&x;</r>").is_err());
    }
    #[test]
    fn strict_encoding_and_tag_errors_are_positioned() {
        for b in [&b"<r>\xFF</r>"[..], b"<r><x></r>"] {
            let e = events(b).unwrap_err();
            assert!(e.position.line >= 1);
        }
    }
    #[test]
    fn tolerant_mode_only_repairs_missing_final_close() {
        let mut r = Reader::new_tolerant(Cursor::new(b"<r><x/>"));
        let got: Vec<_> = r.by_ref().collect::<Result<_, _>>().unwrap();
        assert!(
            got.iter()
                .any(|e| matches!(e,Event::EndElement{name} if name.local=="r"))
        );
        assert_eq!(r.diagnostics().len(), 1);
    }
    #[test]
    fn writer_round_trip_and_balancing() {
        let mut w = Writer::new(Vec::new()).unwrap();
        let n = Name {
            prefix: None,
            local: "r".into(),
            namespace: None,
        };
        w.write_event(&Event::StartElement {
            name: n.clone(),
            attributes: vec![Attribute {
                name: Name {
                    prefix: None,
                    local: "a".into(),
                    namespace: None,
                },
                value: "<&\"🙂".into(),
            }],
            empty: false,
        })
        .unwrap();
        w.write_event(&Event::Text("<&🙂".into())).unwrap();
        w.write_event(&Event::EndElement { name: n }).unwrap();
        let out = w.finish().unwrap();
        assert!(events(&out).is_ok());
        let mut w = Writer::new(Vec::new()).unwrap();
        assert!(
            w.write_event(&Event::EndElement {
                name: Name {
                    prefix: None,
                    local: "x".into(),
                    namespace: None
                }
            })
            .is_err()
        );
        let mut w = Writer::new(Vec::new()).unwrap();
        let root = Name {
            prefix: Some("old".into()),
            local: "r".into(),
            namespace: Some("urn:root".into()),
        };
        let attr = Name {
            prefix: None,
            local: "a".into(),
            namespace: Some("urn:attr".into()),
        };
        w.write_event(&Event::StartElement {
            name: root.clone(),
            attributes: vec![Attribute {
                name: attr,
                value: "v".into(),
            }],
            empty: false,
        })
        .unwrap();
        w.write_event(&Event::EndElement { name: root }).unwrap();
        let parsed = events(&w.finish().unwrap()).unwrap();
        assert!(parsed.iter().any(|event| matches!(event, Event::StartElement { attributes, .. } if attributes[0].name.namespace.as_deref() == Some("urn:attr"))));
        let mut w = Writer::new(Vec::new()).unwrap();
        w.write_event(&Event::StartElement {
            name: Name {
                prefix: None,
                local: "open".into(),
                namespace: None,
            },
            attributes: vec![],
            empty: false,
        })
        .unwrap();
        assert_eq!(w.finish().unwrap_err().kind, ErrorKind::WriterState);
    }
    #[test]
    fn limits_stop_tokens() {
        let l = Limits {
            max_text_bytes: 3,
            ..Limits::default()
        };
        let e = Reader::with_limits(Cursor::new(b"<r>abcdef</r>"), l)
            .collect::<Result<Vec<_>, _>>()
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::Limit);
    }

    #[test]
    fn reference_fixtures_match_validity_contract() {
        let valid: &[&[u8]] = &[
            include_bytes!("../../../tests/format-reference/xml/utf8-boundaries.xml"),
            include_bytes!("../../../tests/format-reference/xml/utf16le-bom.xml"),
            include_bytes!("../../../tests/format-reference/xml/utf16be-bom.xml"),
            include_bytes!("../../../tests/format-reference/xml/namespaces.xml"),
            include_bytes!("../../../tests/format-reference/xml/misc-constructs.xml"),
        ];
        for input in valid {
            assert!(events(input).is_ok(), "valid reference fixture rejected");
        }
        let invalid: &[&[u8]] = &[
            include_bytes!("../../../tests/format-reference/xml/malformed-mismatched.xml"),
            include_bytes!("../../../tests/format-reference/xml/malformed-unknown-entity.xml"),
            include_bytes!("../../../tests/format-reference/xml/malformed-invalid-char.xml"),
            include_bytes!("../../../tests/format-reference/xml/malformed-truncated-utf8.xml"),
            include_bytes!("../../../tests/format-reference/xml/malformed-unbound-prefix.xml"),
            include_bytes!(
                "../../../tests/format-reference/xml/malformed-duplicate-expanded-attr.xml"
            ),
            include_bytes!("../../../tests/format-reference/xml/doctype-no-expansion.xml"),
        ];
        for input in invalid {
            assert!(events(input).is_err(), "invalid reference fixture accepted");
        }
    }

    #[test]
    fn line_ending_and_attribute_normalization() {
        let parsed = events(b"<r a=\"x\r\ny\t&#xD;\">a\r\nb\rc</r>").unwrap();
        let Event::StartElement { attributes, .. } = &parsed[0] else {
            panic!()
        };
        assert_eq!(attributes[0].value, "x y \r");
        assert!(parsed.contains(&Event::Text("a\nb\nc".into())));
    }

    #[test]
    fn first_event_does_not_consume_stream_end() {
        struct Counted {
            input: Cursor<Vec<u8>>,
            consumed: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        }
        impl Read for Counted {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                let n = out.len().min(1);
                let n = self.input.read(&mut out[..n])?;
                self.consumed
                    .fetch_add(n, std::sync::atomic::Ordering::Relaxed);
                Ok(n)
            }
        }
        let consumed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut reader = Reader::new(Counted {
            input: Cursor::new(b"<r/>after".to_vec()),
            consumed: consumed.clone(),
        });
        assert!(matches!(
            reader.next_event().unwrap(),
            Some(Event::StartElement { empty: true, .. })
        ));
        assert!(consumed.load(std::sync::atomic::Ordering::Relaxed) < 9);
    }

    #[test]
    fn strict_grammar_rejects_non_xml_whitespace_references_and_prolog() {
        for input in [
            &b"<r a='1'b='2'/>"[..],
            b"<r>]]></r>",
            b"<r>&#+65;</r>",
            b"<r>&#x+41;</r>",
            b"<r xmlns:p='a' xmlns:p='b'/>",
            b"<r xmlns:='urn:x'/>",
            b"<r xmlns:a:b='urn:x'/>",
            b"<r xmlns:p='http://www.w3.org/2000/xmlns/'/>",
            b"<r\xC2\xA0a='1'/>",
            b"<r/><?xml version='1.0'?>",
            b" <?xml version='1.0'?><r/>",
            b"<?xml?><r/>",
            b"<?XML version='1.0'?><r/>",
            b"<?xml version='1.1'?><r/>",
            b"<![CDATA[outside]]><r/>",
            b"<r><!--x---></r>",
        ] {
            assert!(
                events(input).is_err(),
                "accepted {}",
                String::from_utf8_lossy(input)
            );
        }
    }

    #[test]
    fn positions_count_lf_and_zero_limits_reject_first_scalar() {
        let error = events(b"<r>\n\n\x01</r>").unwrap_err();
        assert_eq!((error.position.line, error.position.column), (3, 1));
        let name_limits = Limits {
            max_name_bytes: 0,
            ..Limits::default()
        };
        assert_eq!(
            Reader::with_limits(&b"<r/>"[..], name_limits)
                .next_event()
                .unwrap_err()
                .kind,
            ErrorKind::Limit
        );
        let text_limits = Limits {
            max_text_bytes: 0,
            ..Limits::default()
        };
        let mut reader = Reader::with_limits(&b"<r>x</r>"[..], text_limits);
        reader.next_event().unwrap();
        assert_eq!(reader.next_event().unwrap_err().kind, ErrorKind::Limit);
    }

    #[test]
    fn text_limit_applies_to_total_event_across_raw_and_referenced_scalars() {
        for xml in [b"<r>abcd</r>".as_slice(), b"<r>ab&#99;d</r>"] {
            let limits = Limits {
                max_text_bytes: 4,
                ..Limits::default()
            };
            let mut reader = Reader::with_limits(xml, limits);
            assert!(matches!(
                reader.next_event(),
                Ok(Some(Event::StartElement { .. }))
            ));
            assert_eq!(
                reader.next_event().unwrap(),
                Some(Event::Text("abcd".into()))
            );
        }
    }

    #[test]
    fn writer_requires_one_root_and_preserves_numeric_carriage_return() {
        assert!(Writer::new(Vec::new()).unwrap().finish().is_err());
        let root = Name {
            prefix: None,
            local: "r".into(),
            namespace: None,
        };
        let empty = Event::StartElement {
            name: root.clone(),
            attributes: vec![],
            empty: true,
        };
        let mut writer = Writer::new(Vec::new()).unwrap();
        writer.write_event(&empty).unwrap();
        assert!(writer.write_event(&empty).is_err());
        assert!(writer.finish().is_err());
        let mut writer = Writer::new(Vec::new()).unwrap();
        assert!(writer.write_event(&Event::Text("outside".into())).is_err());
        assert!(writer.finish().is_err());
        for invalid in [
            Event::Comment("illegal\0".into()),
            Event::ProcessingInstruction {
                target: "p".into(),
                data: "illegal\0".into(),
            },
        ] {
            let mut writer = Writer::new(Vec::new()).unwrap();
            assert!(writer.write_event(&invalid).is_err());
        }
        let mut writer = Writer::new(Vec::new()).unwrap();
        writer
            .write_event(&Event::StartElement {
                name: root.clone(),
                attributes: vec![],
                empty: false,
            })
            .unwrap();
        writer.write_event(&Event::Text("a\rb".into())).unwrap();
        writer
            .write_event(&Event::EndElement { name: root })
            .unwrap();
        assert!(
            events(&writer.finish().unwrap())
                .unwrap()
                .contains(&Event::Text("a\rb".into()))
        );
    }

    #[test]
    fn all_byte_boundaries_of_utf16_and_bom_are_incremental() {
        struct Chunks<'a> {
            input: &'a [u8],
            size: usize,
        }
        impl Read for Chunks<'_> {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                let n = self.size.min(out.len()).min(self.input.len());
                out[..n].copy_from_slice(&self.input[..n]);
                self.input = &self.input[n..];
                Ok(n)
            }
        }
        for bytes in [
            include_bytes!("../../../tests/format-reference/xml/utf16le-bom.xml").as_slice(),
            include_bytes!("../../../tests/format-reference/xml/utf16be-bom.xml").as_slice(),
            b"\xEF\xBB\xBF<r>\xF0\x9F\x99\x82</r>".as_slice(),
        ] {
            let expected = events(bytes).unwrap();
            for size in 1..=bytes.len() {
                let actual = Reader::new(Chunks { input: bytes, size })
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                assert_eq!(actual, expected);
            }
        }
    }

    #[test]
    fn doctype_comments_and_quoted_brackets_are_ignored_safely() {
        let input = b"<!DOCTYPE r [<!-- ] > [ ignored --><!ENTITY x \"[ not a bracket ]\">]><r/>";
        assert!(events(input).is_ok());
        assert!(events(b"<!DOCTYPE r [ ] ]><r/>").is_err());
        assert!(events(b"<!DOCTYPE [<!ENTITY x 'y'>]><r/>").is_err());
    }

    #[test]
    fn comments_and_pi_normalize_crlf_and_cr() {
        let parsed = events(b"<r><!--a\r\nb\rc--><?p a\r\nb\rc?></r>").unwrap();
        assert!(parsed.contains(&Event::Comment("a\nb\nc".into())));
        assert!(parsed.contains(&Event::ProcessingInstruction {
            target: "p".into(),
            data: "a\nb\nc".into()
        }));
    }

    #[test]
    fn direct_next_event_is_terminal_after_parse_error() {
        let mut reader = Reader::new(Cursor::new(b"<r>&unknown;</r>"));
        assert!(matches!(
            reader.next_event().unwrap(),
            Some(Event::StartElement { .. })
        ));
        assert_eq!(
            reader.next_event().unwrap_err().kind,
            ErrorKind::UnknownEntity
        );
        assert!(reader.next_event().unwrap().is_none());
    }

    #[test]
    fn zero_event_budget_rejects_before_reading() {
        struct Counted<'a> {
            input: &'a [u8],
            consumed: &'a std::sync::atomic::AtomicUsize,
        }
        impl Read for Counted<'_> {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                let count = out.len().min(self.input.len());
                out[..count].copy_from_slice(&self.input[..count]);
                self.input = &self.input[count..];
                self.consumed
                    .fetch_add(count, std::sync::atomic::Ordering::Relaxed);
                Ok(count)
            }
        }
        let consumed = std::sync::atomic::AtomicUsize::new(0);
        let limits = Limits {
            max_events: 0,
            ..Limits::default()
        };
        let mut reader = Reader::with_limits(
            Counted {
                input: b"<r/>",
                consumed: &consumed,
            },
            limits,
        );
        assert_eq!(reader.next_event().unwrap_err().kind, ErrorKind::Limit);
        assert!(reader.next_event().unwrap().is_none());
        assert_eq!(consumed.load(std::sync::atomic::Ordering::Relaxed), 0);
    }

    #[test]
    fn exact_event_budget_allows_trailing_xml_whitespace_and_eof() {
        struct Counted<'a> {
            input: &'a [u8],
            reads: &'a std::sync::atomic::AtomicUsize,
        }
        impl Read for Counted<'_> {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                self.reads
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let count = out.len().min(self.input.len());
                out[..count].copy_from_slice(&self.input[..count]);
                self.input = &self.input[count..];
                Ok(count)
            }
        }
        let reads = std::sync::atomic::AtomicUsize::new(0);
        let limits = Limits {
            max_events: 2,
            ..Limits::default()
        };
        let mut reader = Reader::with_limits(
            Counted {
                input: b"<r></r> \r\n\t",
                reads: &reads,
            },
            limits,
        );
        assert!(matches!(
            reader.next_event().unwrap(),
            Some(Event::StartElement { .. })
        ));
        assert!(matches!(
            reader.next_event().unwrap(),
            Some(Event::EndElement { .. })
        ));
        assert_eq!(reader.next_event().unwrap(), None);
        let reads_at_eof = reads.load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!(reader.next_event().unwrap(), None);
        assert_eq!(
            reads.load(std::sync::atomic::Ordering::Relaxed),
            reads_at_eof
        );
    }

    #[test]
    fn extra_event_is_rejected_before_reading_its_large_payload() {
        struct Counted<'a> {
            input: &'a [u8],
            consumed: &'a std::sync::atomic::AtomicUsize,
        }
        impl Read for Counted<'_> {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                let count = out.len().min(self.input.len());
                out[..count].copy_from_slice(&self.input[..count]);
                self.input = &self.input[count..];
                self.consumed
                    .fetch_add(count, std::sync::atomic::Ordering::Relaxed);
                Ok(count)
            }
        }
        let payload = "x".repeat(1_000_000);
        let input = format!("<r/><large>{payload}</large>");
        let consumed = std::sync::atomic::AtomicUsize::new(0);
        let limits = Limits {
            max_events: 1,
            ..Limits::default()
        };
        let mut reader = Reader::with_limits(
            Counted {
                input: input.as_bytes(),
                consumed: &consumed,
            },
            limits,
        );
        assert!(matches!(
            reader.next_event().unwrap(),
            Some(Event::StartElement { .. })
        ));
        assert_eq!(reader.next_event().unwrap_err().kind, ErrorKind::Limit);
        assert!(reader.next_event().unwrap().is_none());
        assert!(consumed.load(std::sync::atomic::Ordering::Relaxed) < 32);
    }

    #[test]
    fn inherited_namespaces_are_bounded_without_scope_copies() {
        let limits = Limits {
            max_memory_bytes: 256,
            ..Limits::default()
        };
        let input =
            b"<r xmlns:p='urn:123456789012345678901234567890'><a><b><c><p:d/></c></b></a></r>";
        let error = Reader::with_limits(Cursor::new(input), limits)
            .collect::<Result<Vec<_>, _>>()
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Limit);
    }

    #[test]
    fn event_buffers_share_the_remaining_namespace_memory_budget() {
        let input = format!("<r xmlns:p='urn:x'><!--{}--></r>", "x".repeat(4096));
        let mut found = false;
        for max_memory_bytes in 1..8192 {
            let limits = Limits {
                max_memory_bytes,
                ..Limits::default()
            };
            let mut reader = Reader::with_limits(input.as_bytes(), limits);
            if reader
                .next_event()
                .is_ok_and(|event| matches!(event, Some(Event::StartElement { .. })))
            {
                match reader.next_event() {
                    Err(error) if error.kind == ErrorKind::Limit => {
                        found = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
        assert!(found, "comment buffer ignored memory held by active scopes");
    }

    #[test]
    fn processing_instruction_target_shares_event_memory_budget() {
        let input = format!("<r><?target {}?></r>", "x".repeat(4096));
        let mut found = false;
        for max_memory_bytes in 1..8192 {
            let limits = Limits {
                max_memory_bytes,
                ..Limits::default()
            };
            let mut reader = Reader::with_limits(input.as_bytes(), limits);
            if reader
                .next_event()
                .is_ok_and(|event| matches!(event, Some(Event::StartElement { .. })))
            {
                match reader.next_event() {
                    Err(error) if error.kind == ErrorKind::Limit => {
                        found = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
        assert!(found, "PI data allocation ignored the retained target");
    }

    #[test]
    fn tolerant_eof_diagnostic_obeys_memory_budget() {
        let mut found = false;
        for max_memory_bytes in 1..1024 {
            let limits = Limits {
                max_memory_bytes,
                ..Limits::default()
            };
            let mut reader = Reader::with_limits(&b"<r>"[..], limits);
            reader.tolerant = true;
            if reader
                .next_event()
                .is_ok_and(|event| matches!(event, Some(Event::StartElement { .. })))
                && matches!(
                    reader.next_event(),
                    Err(XmlError {
                        kind: ErrorKind::Limit,
                        ..
                    })
                )
            {
                found = true;
                break;
            }
        }
        assert!(
            found,
            "tolerant EOF diagnostic exceeded the configured budget"
        );
    }

    #[test]
    fn writer_normalizes_public_cr_in_comment_and_pi() {
        let mut writer = Writer::new(Vec::new()).unwrap();
        let root = Name {
            prefix: None,
            local: "r".into(),
            namespace: None,
        };
        writer
            .write_event(&Event::StartElement {
                name: root.clone(),
                attributes: vec![],
                empty: false,
            })
            .unwrap();
        writer
            .write_event(&Event::Comment("a\r\nb\rc".into()))
            .unwrap();
        writer
            .write_event(&Event::ProcessingInstruction {
                target: "p".into(),
                data: "a\rb".into(),
            })
            .unwrap();
        writer
            .write_event(&Event::EndElement { name: root })
            .unwrap();
        let parsed = events(&writer.finish().unwrap()).unwrap();
        assert!(parsed.contains(&Event::Comment("a\nb\nc".into())));
        assert!(parsed.contains(&Event::ProcessingInstruction {
            target: "p".into(),
            data: "a\nb".into()
        }));
    }

    #[test]
    fn unprefixed_element_named_xmlns_uses_default_namespace() {
        let parsed = events(b"<root xmlns='urn:x'><xmlns/></root>").unwrap();
        let Event::StartElement { name, .. } = &parsed[1] else {
            panic!()
        };
        assert_eq!(name.namespace.as_deref(), Some("urn:x"));
    }
}
