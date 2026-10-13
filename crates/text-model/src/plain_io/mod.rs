// SPDX-License-Identifier: AGPL-3.0-or-later
//! Opening and saving plain-text files (US-316).
//!
//! Works on bytes in memory. Opening detects the encoding (or takes the
//! caller's), decodes with `atelier-unicode`, normalises every line break to
//! `\n` and reports what it found; the result feeds a [`PlainTextDocument`] or
//! a rich document with one paragraph per line. Saving converts back, with the
//! caller's encoding, BOM and line-ending choices, and never loses a character
//! silently. [`rich`] holds the rich-document side, including the report of
//! what a save as text discards.
//!
//! Line model: the text of an opened file is its content with every `\r\n`,
//! `\r` and `\n` turned into `\n`. A file ending with a break therefore ends
//! with `\n` (one more, empty, last line), a file without one does not, so the
//! two cases stay distinct and a round trip is exact.

pub mod rich;

use crate::{PlainTextDocument, PlainTextError, PlainTextLimits};
use atelier_unicode::{Encoding, decode, detect_encoding, encode};
use std::borrow::Cow;

pub use rich::{FormattingLoss, LossCategory, formatting_loss, rich_to_text};

/// Default size limit, in bytes of input (open) or of text (save).
pub const DEFAULT_MAX_BYTES: usize = 16 * 1024 * 1024;
/// At most this many unrepresentable characters are listed in an error.
pub const MAX_LISTED_CHARACTERS: usize = 1000;

/// Encodings offered for a manual choice when opening, all supported by
/// `atelier-unicode`.
pub const MANUAL_ENCODINGS: [Encoding; 7] = [
    Encoding::Utf8,
    Encoding::Utf16Le,
    Encoding::Utf16Be,
    Encoding::Windows1252,
    Encoding::Iso8859_1,
    Encoding::MacRoman,
    Encoding::Cp437,
];

/// Encodings offered when saving (besides "as opened").
pub const SAVE_ENCODINGS: [SaveEncoding; 5] = [
    SaveEncoding::Utf8,
    SaveEncoding::Utf8Bom,
    SaveEncoding::Utf16Le,
    SaveEncoding::Utf16Be,
    SaveEncoding::Windows1252,
];

/// A stable display name for an encoding (not localised).
pub const fn encoding_label(encoding: Encoding) -> &'static str {
    match encoding {
        Encoding::Utf8 => "UTF-8",
        Encoding::Utf16Le => "UTF-16 LE",
        Encoding::Utf16Be => "UTF-16 BE",
        Encoding::Windows1252 => "Windows-1252",
        Encoding::Iso8859_1 => "ISO-8859-1",
        Encoding::Cp437 => "CP437",
        Encoding::MacRoman => "MacRoman",
    }
}

/// Failure while opening or saving text.
#[derive(Debug, Eq, PartialEq)]
pub enum TextIoError {
    /// Input larger than the limit; nothing was allocated.
    TooLarge {
        limit: usize,
        actual: usize,
    },
    /// More lines than the rich document may receive.
    TooManyLines {
        limit: usize,
        actual: usize,
    },
    AllocationFailed,
    /// Characters the target encoding cannot represent (save in `Error` mode).
    /// `characters` lists at most [`MAX_LISTED_CHARACTERS`] of `total`.
    Unrepresentable {
        encoding: Encoding,
        characters: Vec<Unrepresentable>,
        total: usize,
    },
    Plain(PlainTextError),
    Rich(crate::rich::RichError),
}

impl core::fmt::Display for TextIoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TooLarge { limit, actual } => {
                write!(f, "text has {actual} bytes; limit is {limit}")
            }
            Self::TooManyLines { limit, actual } => {
                write!(f, "text has {actual} lines; limit is {limit}")
            }
            Self::AllocationFailed => f.write_str("could not reserve storage"),
            Self::Unrepresentable {
                encoding, total, ..
            } => write!(
                f,
                "{total} character(s) cannot be represented in {}",
                encoding_label(*encoding)
            ),
            Self::Plain(e) => write!(f, "{e}"),
            Self::Rich(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for TextIoError {}

/// A character the target encoding cannot represent, with its place in the
/// text: `line` and `column` are 1-based, the column counts Unicode scalars.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Unrepresentable {
    pub character: char,
    pub line: usize,
    pub column: usize,
}

/// How far the detected encoding can be trusted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Confidence {
    /// A byte-order mark names the encoding.
    Certain,
    /// Valid UTF-8 with non-ASCII characters, or the UTF-16 null-byte pattern.
    Likely,
    /// Pure ASCII: every candidate encoding gives the same text.
    Indifferent,
    /// Fallback to Windows-1252: nothing else fitted, nothing confirms it.
    Uncertain,
    /// The caller imposed the encoding.
    Chosen,
}

/// Result of [`detect`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Detection {
    pub encoding: Encoding,
    pub confidence: Confidence,
    /// A UTF-32 BOM was seen. UTF-32 is not supported: `encoding` is only the
    /// closest reading and `confidence` is `Uncertain`.
    pub utf32_suspected: bool,
}

/// A resolved line-break style.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Eol {
    Lf,
    CrLf,
    Cr,
}

impl Eol {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::Cr => "\r",
        }
    }
}

/// What kinds of line break a file used.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LineEndingStyle {
    /// No line break at all.
    #[default]
    None,
    Lf,
    CrLf,
    Cr,
    /// At least two kinds.
    Mixed,
}

/// Counts of each kind of line break found when opening.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LineEndings {
    pub lf: usize,
    pub crlf: usize,
    pub cr: usize,
}

impl LineEndings {
    pub fn style(&self) -> LineEndingStyle {
        match (self.lf > 0, self.crlf > 0, self.cr > 0) {
            (false, false, false) => LineEndingStyle::None,
            (true, false, false) => LineEndingStyle::Lf,
            (false, true, false) => LineEndingStyle::CrLf,
            (false, false, true) => LineEndingStyle::Cr,
            _ => LineEndingStyle::Mixed,
        }
    }
    /// The most frequent kind (ties: CRLF, then LF, then CR); LF without breaks.
    pub fn dominant(&self) -> Eol {
        if self.crlf >= self.lf && self.crlf >= self.cr && self.crlf > 0 {
            Eol::CrLf
        } else if self.lf >= self.cr {
            Eol::Lf
        } else {
            Eol::Cr
        }
    }
}

/// The on-disk shape of an opened file, used by "as opened" saves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileFormat {
    pub encoding: Encoding,
    pub bom: bool,
    pub eol: Eol,
}

impl Default for FileFormat {
    fn default() -> Self {
        Self {
            encoding: Encoding::Utf8,
            bom: false,
            eol: Eol::Lf,
        }
    }
}

/// Options of [`open_text`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpenOptions {
    /// `None` detects the encoding.
    pub encoding: Option<Encoding>,
    /// Limit on the input size, checked before any allocation. Decoding can
    /// grow the text up to three times (Windows-1252 `€` is three UTF-8 bytes).
    pub max_bytes: usize,
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self {
            encoding: None,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

/// A decoded file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenedText {
    /// The text, every line break being `\n`.
    pub text: String,
    pub encoding: Encoding,
    /// A BOM matching `encoding` started the file (and is not in `text`).
    pub bom: bool,
    pub line_endings: LineEndings,
    /// Malformed sequences replaced by U+FFFD (not counting genuine U+FFFD).
    pub invalid_sequences: usize,
    /// Control characters kept verbatim: C0 except TAB, LF and CR (so NUL and
    /// form feed are counted), and DEL. A high count suggests a binary file.
    pub control_chars: usize,
    pub confidence: Confidence,
    /// See [`Detection::utf32_suspected`].
    pub utf32_suspected: bool,
}

impl OpenedText {
    /// The format to give back to [`SaveOptions::as_opened`].
    pub fn format(&self) -> FileFormat {
        FileFormat {
            encoding: self.encoding,
            bom: self.bom,
            eol: self.line_endings.dominant(),
        }
    }

    /// Whether the interface should offer the manual encoding choice.
    pub fn suggest_manual_choice(&self) -> bool {
        self.confidence == Confidence::Uncertain || self.invalid_sequences > 0
    }

    /// The lines, without breaks. A final break yields a last empty line.
    pub fn lines(&self) -> core::str::Split<'_, char> {
        self.text.split('\n')
    }

    /// Whether saving "as opened" gives the original bytes back: no break
    /// style mixed and nothing was replaced.
    pub fn round_trip_is_exact(&self) -> bool {
        self.invalid_sequences == 0 && self.line_endings.style() != LineEndingStyle::Mixed
    }

    /// Builds the plain-text document. A UTF-8 BOM is recorded in it the way
    /// `PlainTextDocument::from_utf8` does; the text is otherwise unchanged.
    pub fn to_plain_document(
        &self,
        limits: PlainTextLimits,
    ) -> Result<PlainTextDocument, TextIoError> {
        const BOM: &[u8] = b"\xef\xbb\xbf";
        let keep_bom =
            (self.bom && self.encoding == Encoding::Utf8) || self.text.starts_with('\u{FEFF}');
        if !keep_bom {
            return PlainTextDocument::from_utf8(self.text.as_bytes(), limits)
                .map_err(TextIoError::Plain);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(self.text.len() + BOM.len())
            .map_err(|_| TextIoError::AllocationFailed)?;
        bytes.extend_from_slice(BOM);
        bytes.extend_from_slice(self.text.as_bytes());
        PlainTextDocument::from_utf8(&bytes, limits).map_err(TextIoError::Plain)
    }

    /// Builds a rich document, one default-style paragraph per line.
    pub fn to_rich_document(
        &self,
        max_paragraphs: usize,
    ) -> Result<crate::rich::RichDocument, TextIoError> {
        rich::text_to_rich(&self.text, max_paragraphs)
    }
}

const fn bom_of(encoding: Encoding) -> &'static [u8] {
    match encoding {
        Encoding::Utf8 => b"\xef\xbb\xbf",
        Encoding::Utf16Le => b"\xff\xfe",
        Encoding::Utf16Be => b"\xfe\xff",
        _ => b"",
    }
}

/// UTF-16 without BOM, from the null bytes of mostly Latin text: in
/// little-endian the high byte of each unit (odd offsets) is mostly 0 and the
/// low byte (even offsets) is almost never 0; mirrored for big-endian. At
/// least 2 units, a quarter of the units with a null high byte, and 16 times
/// fewer nulls on the other side. Text without a Latin majority (CJK...) has
/// no such pattern and is not recognised. Looks at the first 64 KiB.
fn utf16_pattern(bytes: &[u8]) -> Option<Encoding> {
    let sample = &bytes[..bytes.len().min(65_536)];
    let pairs = sample.len() / 2;
    if pairs < 2 {
        return None;
    }
    let (mut even, mut odd) = (0usize, 0usize);
    for pair in sample.chunks_exact(2) {
        even += usize::from(pair[0] == 0);
        odd += usize::from(pair[1] == 0);
    }
    if odd * 4 >= pairs && even * 16 < odd {
        Some(Encoding::Utf16Le)
    } else if even * 4 >= pairs && odd * 16 < even {
        Some(Encoding::Utf16Be)
    } else {
        None
    }
}

/// Detects the encoding: BOM (certain), then the UTF-16 null-byte pattern,
/// then `atelier_unicode::detect_encoding` (valid UTF-8, else Windows-1252).
pub fn detect(bytes: &[u8]) -> Detection {
    // UTF-32 is not supported. Its BOMs start like (LE) or contain (BE) the
    // UTF-16 ones, so they are tested first and never read in silence: the
    // result is `Uncertain` with `utf32_suspected`, and the manual choice is
    // proposed. `FF FE 00 00` is also a legitimate UTF-16 LE BOM followed by
    // a NUL character; the two cannot be told apart, the rule is to ask.
    if bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) {
        return Detection {
            encoding: Encoding::Utf16Le,
            confidence: Confidence::Uncertain,
            utf32_suspected: true,
        };
    }
    if bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        return Detection {
            encoding: Encoding::Utf16Be,
            confidence: Confidence::Uncertain,
            utf32_suspected: true,
        };
    }
    let found = |encoding, confidence| Detection {
        encoding,
        confidence,
        utf32_suspected: false,
    };
    let base = detect_encoding(bytes);
    let has_bom = bytes.starts_with(bom_of(base));
    if has_bom && !bom_of(base).is_empty() {
        return found(base, Confidence::Certain);
    }
    if let Some(encoding) = utf16_pattern(bytes) {
        return found(encoding, Confidence::Likely);
    }
    match base {
        Encoding::Utf8 if bytes.is_ascii() => match ascii_control_suspicion(bytes) {
            None => found(base, Confidence::Indifferent),
            Some(candidate) => found(candidate.unwrap_or(base), Confidence::Uncertain),
        },
        Encoding::Utf8 => found(base, Confidence::Likely),
        _ => found(base, Confidence::Uncertain),
    }
}

/// For bytes that are all ASCII: `Some(_)` when they hold an abnormal share of
/// C0 controls (other than TAB, LF, CR), which plain text does not: at least
/// two and more than 1/8 of the bytes. The inner option is a UTF-16 candidate
/// when the controls sit on one byte parity, as the high bytes (0x00-0x1F)
/// of Cyrillic, Greek or Hebrew text do (U+0410 is `10 04` in LE): at least
/// 3/4 of the bytes of one parity are controls (TAB, LF, CR included). A form
/// feed in normal text, or tabs in source code, never trigger this.
fn ascii_control_suspicion(bytes: &[u8]) -> Option<Option<Encoding>> {
    let controls = bytes
        .iter()
        .filter(|&&b| b < 0x20 && !matches!(b, b'\t' | b'\n' | b'\r'))
        .count();
    if controls < 2 || controls * 8 <= bytes.len() {
        return None;
    }
    let (mut even, mut odd) = (0usize, 0usize);
    for (i, &b) in bytes.iter().enumerate() {
        if b < 0x20 {
            if i % 2 == 0 {
                even += 1;
            } else {
                odd += 1;
            }
        }
    }
    let (n_even, n_odd) = (bytes.len().div_ceil(2), bytes.len() / 2);
    Some(if odd * 4 >= n_odd * 3 && even * 2 < n_even {
        Some(Encoding::Utf16Le)
    } else if even * 4 >= n_even * 3 && odd * 2 < n_odd {
        Some(Encoding::Utf16Be)
    } else {
        None
    })
}

/// For a single-byte encoding: which bytes decode to nothing (U+FFFD).
fn undefined_bytes(encoding: Encoding) -> [bool; 256] {
    let mut table = [false; 256];
    for (b, slot) in table.iter_mut().enumerate() {
        *slot = decode(&[b as u8], encoding) == "\u{FFFD}";
    }
    table
}

fn count_invalid(bytes: &[u8], encoding: Encoding) -> usize {
    let own_bom = bom_of(encoding);
    let body = if !own_bom.is_empty() && bytes.starts_with(own_bom) {
        &bytes[own_bom.len()..]
    } else {
        bytes
    };
    match encoding {
        Encoding::Utf8 => body
            .utf8_chunks()
            .filter(|chunk| !chunk.invalid().is_empty())
            .count(),
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let little = encoding == Encoding::Utf16Le;
            let units = body.chunks_exact(2).map(|p| {
                if little {
                    u16::from_le_bytes([p[0], p[1]])
                } else {
                    u16::from_be_bytes([p[0], p[1]])
                }
            });
            char::decode_utf16(units).filter(Result::is_err).count() + body.len() % 2
        }
        _ => {
            let undefined = undefined_bytes(encoding);
            body.iter().filter(|&&b| undefined[b as usize]).count()
        }
    }
}

fn scan(text: &str) -> (LineEndings, usize) {
    let bytes = text.as_bytes();
    let (mut ends, mut controls) = (LineEndings::default(), 0);
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => {
                ends.crlf += 1;
                i += 1;
            }
            b'\r' => ends.cr += 1,
            b'\n' => ends.lf += 1,
            b'\t' => {}
            0..=0x1F | 0x7F => controls += 1,
            _ => {}
        }
        i += 1;
    }
    (ends, controls)
}

/// Appends `text` with every break (`\r\n`, `\r`, `\n`) replaced by `eol`.
pub(crate) fn push_with_breaks(out: &mut String, text: &str, eol: &str) {
    let bytes = text.as_bytes();
    let (mut start, mut i) = (0, 0);
    while i < bytes.len() {
        match bytes[i] {
            b'\r' | b'\n' => {
                out.push_str(&text[start..i]);
                out.push_str(eol);
                if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push_str(&text[start..]);
}

fn reserve(out: &mut String, additional: usize) -> Result<(), TextIoError> {
    out.try_reserve(additional)
        .map_err(|_| TextIoError::AllocationFailed)
}

/// Opens a text file from its bytes.
///
/// Never panics. Malformed sequences become U+FFFD and are counted in
/// `invalid_sequences`; a later save cannot give the original bytes back.
pub fn open_text(bytes: &[u8], options: &OpenOptions) -> Result<OpenedText, TextIoError> {
    if bytes.len() > options.max_bytes {
        return Err(TextIoError::TooLarge {
            limit: options.max_bytes,
            actual: bytes.len(),
        });
    }
    let (encoding, confidence, utf32_suspected) = match options.encoding {
        Some(encoding) => (encoding, Confidence::Chosen, false),
        None => {
            let found = detect(bytes);
            (found.encoding, found.confidence, found.utf32_suspected)
        }
    };
    let own_bom = bom_of(encoding);
    let bom = !own_bom.is_empty() && bytes.starts_with(own_bom);
    let invalid_sequences = count_invalid(bytes, encoding);
    let decoded = decode(bytes, encoding);
    let (line_endings, control_chars) = scan(&decoded);
    let text = if line_endings.crlf + line_endings.cr == 0 {
        decoded
    } else {
        let mut text = String::new();
        reserve(&mut text, decoded.len())?;
        push_with_breaks(&mut text, &decoded, "\n");
        text
    };
    Ok(OpenedText {
        text,
        encoding,
        bom,
        line_endings,
        invalid_sequences,
        control_chars,
        confidence,
        utf32_suspected,
    })
}

/// Target encoding of a save.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveEncoding {
    /// Encoding and BOM of the opened file ([`SaveOptions::origin`]).
    AsOpened,
    /// UTF-8 without BOM (the default).
    Utf8,
    Utf8Bom,
    /// UTF-16 with BOM.
    Utf16Le,
    Utf16Be,
    Windows1252,
}

/// Line breaks of a save.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LineEndingChoice {
    Lf,
    CrLf,
    /// The opened file's dominant style ([`FileFormat::eol`]).
    AsOpened,
}

/// What to do with a character the encoding cannot represent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unmappable {
    /// Fail with [`TextIoError::Unrepresentable`] listing them.
    Error,
    /// Write `?` and count it in [`SavedText::replaced`].
    Replace,
}

/// Options of the save functions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SaveOptions {
    pub encoding: SaveEncoding,
    pub line_ending: LineEndingChoice,
    pub unmappable: Unmappable,
    /// Used by the "as opened" choices.
    pub origin: FileFormat,
    /// Limit on the text size, checked before any allocation.
    pub max_bytes: usize,
}

impl Default for SaveOptions {
    /// UTF-8 without BOM, LF, unrepresentable characters are an error.
    fn default() -> Self {
        Self {
            encoding: SaveEncoding::Utf8,
            line_ending: LineEndingChoice::Lf,
            unmappable: Unmappable::Error,
            origin: FileFormat::default(),
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

impl SaveOptions {
    /// Everything as when the file was opened.
    pub fn as_opened(origin: FileFormat) -> Self {
        Self {
            encoding: SaveEncoding::AsOpened,
            line_ending: LineEndingChoice::AsOpened,
            origin,
            ..Self::default()
        }
    }
}

/// The bytes to write.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedText {
    pub bytes: Vec<u8>,
    pub encoding: Encoding,
    pub bom: bool,
    pub eol: Eol,
    /// Characters replaced by `?` ([`Unmappable::Replace`]).
    pub replaced: usize,
}

/// Which characters a single-byte encoding can write.
fn mappable_chars(encoding: Encoding) -> Vec<char> {
    let undefined = undefined_bytes(encoding);
    let mut chars: Vec<char> = (0..=255u8)
        .filter(|&b| !undefined[b as usize])
        .filter_map(|b| decode(&[b], encoding).chars().next())
        .collect();
    chars.sort_unstable();
    chars
}

/// Saves text whose line breaks are `\n`, `\r\n` or `\r` (all are converted).
pub fn save_text(text: &str, options: &SaveOptions) -> Result<SavedText, TextIoError> {
    if text.len() > options.max_bytes {
        return Err(TextIoError::TooLarge {
            limit: options.max_bytes,
            actual: text.len(),
        });
    }
    let (encoding, bom) = match options.encoding {
        SaveEncoding::AsOpened => {
            let o = options.origin;
            (o.encoding, o.bom && !bom_of(o.encoding).is_empty())
        }
        SaveEncoding::Utf8 => (Encoding::Utf8, false),
        SaveEncoding::Utf8Bom => (Encoding::Utf8, true),
        SaveEncoding::Utf16Le => (Encoding::Utf16Le, true),
        SaveEncoding::Utf16Be => (Encoding::Utf16Be, true),
        SaveEncoding::Windows1252 => (Encoding::Windows1252, false),
    };
    let eol = match options.line_ending {
        LineEndingChoice::Lf => Eol::Lf,
        LineEndingChoice::CrLf => Eol::CrLf,
        LineEndingChoice::AsOpened => options.origin.eol,
    };
    // Step 1: breaks as `\n`, so that lines and columns are the document's.
    let lf_text: Cow<'_, str> = if text.contains('\r') {
        let mut out = String::new();
        reserve(&mut out, text.len())?;
        push_with_breaks(&mut out, text, "\n");
        Cow::Owned(out)
    } else {
        Cow::Borrowed(text)
    };
    // Step 2: single-byte targets may not represent everything.
    let mut replaced = 0;
    let mut checked: Cow<'_, str> = lf_text;
    let single_byte = !matches!(
        encoding,
        Encoding::Utf8 | Encoding::Utf16Le | Encoding::Utf16Be
    );
    if single_byte && encode(&checked, encoding).is_err() {
        let allowed = mappable_chars(encoding);
        let (mut line, mut column) = (1, 1);
        let mut listed = Vec::new();
        let mut total = 0;
        let mut fixed = String::new();
        if options.unmappable == Unmappable::Replace {
            reserve(&mut fixed, checked.len())?;
        }
        for character in checked.chars() {
            let representable = allowed.binary_search(&character).is_ok();
            if !representable {
                total += 1;
                if listed.len() < MAX_LISTED_CHARACTERS {
                    listed.push(Unrepresentable {
                        character,
                        line,
                        column,
                    });
                }
            }
            if options.unmappable == Unmappable::Replace {
                fixed.push(if representable { character } else { '?' });
            }
            if character == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        if options.unmappable == Unmappable::Error {
            return Err(TextIoError::Unrepresentable {
                encoding,
                characters: listed,
                total,
            });
        }
        replaced = total;
        checked = Cow::Owned(fixed);
    }
    // Step 3: the requested break, then the bytes.
    let final_text: Cow<'_, str> = if eol == Eol::Lf {
        checked
    } else {
        let mut out = String::new();
        reserve(&mut out, checked.len() + checked.len() / 8)?;
        push_with_breaks(&mut out, &checked, eol.as_str());
        Cow::Owned(out)
    };
    let body = encode(&final_text, encoding).map_err(|e| TextIoError::Unrepresentable {
        encoding,
        characters: vec![Unrepresentable {
            character: e.character,
            line: 0,
            column: 0,
        }],
        total: 1,
    })?;
    let mut bytes = Vec::new();
    let prefix = if bom { bom_of(encoding) } else { b"" };
    bytes
        .try_reserve_exact(prefix.len() + body.len())
        .map_err(|_| TextIoError::AllocationFailed)?;
    bytes.extend_from_slice(prefix);
    bytes.extend_from_slice(&body);
    Ok(SavedText {
        bytes,
        encoding,
        bom,
        eol,
        replaced,
    })
}

/// Saves the text of a [`PlainTextDocument`].
pub fn save_plain_document(
    document: &PlainTextDocument,
    options: &SaveOptions,
) -> Result<SavedText, TextIoError> {
    save_text(document.text(), options)
}

/// Saves a rich document as text (see [`rich_to_text`] for the rules and
/// [`formatting_loss`] for the warning to show beforehand).
pub fn save_rich_document(
    document: &crate::rich::RichDocument,
    options: &SaveOptions,
) -> Result<SavedText, TextIoError> {
    save_text(&rich_to_text(document), options)
}
