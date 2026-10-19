// SPDX-License-Identifier: AGPL-3.0-or-later
//! Messages of the formatting family (F0): `FormatState`, `ApplyCharFormat`,
//! `ApplyParaFormat`, `ListStyles`, `ApplyStyle`, `PreviewStyle` and the
//! replies `FormatState` and `StyleList`. Contract, byte layouts and rules:
//! `docs/us-240-format-protocol.md`. Grouped here so that the envelope module
//! only holds the hooks and so that US-240, US-243 and US-250 never touch the
//! codec: they implement the handlers of `rich_editor/format_char.rs`,
//! `format_para.rs` and `styles.rs`.

use super::{Command, ErrorCode, Reader, Response, put_text16, put_u32, put_u64};

type Fail = ErrorCode;

/// Revision guard value meaning "do not check".
pub const NO_REVISION_GUARD: u64 = u64::MAX;
/// Attribute edits one `ApplyCharFormat` or `ApplyParaFormat` carries.
pub const MAX_FORMAT_EDITS: usize = 16;
/// Longest font name or style identifier, in bytes.
pub const MAX_NAME_BYTES: usize = 255;
/// Styles one `StyleList` carries.
pub const MAX_STYLE_LIST: usize = 4096;
/// Smallest and largest character size, in half-points (1 pt to 1638 pt).
pub const MIN_HALF_POINTS: u16 = 2;
pub const MAX_HALF_POINTS: u16 = 3276;
/// Largest indent or spacing, in twips (22 in, the limit of Word).
pub const MAX_TWIPS: i32 = 31_680;
/// Number of border sides: top, left, bottom, right, between, bar.
pub const BORDER_SIDES: usize = 6;
/// Largest border width (1/8 pt) and space (pt).
pub const MAX_BORDER_EIGHTHS: u16 = 96;
pub const MAX_BORDER_SPACE: u8 = 31;

/// Word's sizes for Ctrl+Maj+> and Ctrl+Maj+<, in half-points: the list of the
/// size box. Above the last one the step is 10 pt (20 half-points), below the
/// first one 1 pt, always inside `MIN_HALF_POINTS..=MAX_HALF_POINTS`.
pub const SIZE_STEPS: [u16; 17] = [
    16, 18, 20, 21, 22, 24, 28, 32, 36, 40, 44, 48, 52, 56, 72, 96, 144,
];

/// Size reached by one step of Ctrl+Maj+> (`up`) or Ctrl+Maj+< from `half`
/// half-points: the next (previous) listed size, 10 pt steps above 72 pt, 1 pt
/// steps below 8 pt. A size is never changed by a step that cannot move it.
pub fn size_step(half: u16, up: bool) -> u16 {
    let top = SIZE_STEPS[SIZE_STEPS.len() - 1];
    let next = if up {
        if half >= top {
            // 80, 90, ... then the largest size.
            (half / 20 + 1).saturating_mul(20)
        } else if half < SIZE_STEPS[0] {
            half + 2
        } else {
            SIZE_STEPS
                .iter()
                .copied()
                .find(|s| *s > half)
                .unwrap_or(top)
        }
    } else if half > top {
        let below = (half - 1) / 20 * 20;
        below.max(top)
    } else if half > SIZE_STEPS[0] {
        SIZE_STEPS
            .iter()
            .rev()
            .copied()
            .find(|s| *s < half)
            .unwrap_or(SIZE_STEPS[0])
    } else {
        half.saturating_sub(2)
    };
    next.clamp(MIN_HALF_POINTS, MAX_HALF_POINTS)
}

/// An RGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// What an edit does to an attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EditOp {
    /// Defines the attribute directly (a value follows).
    Set = 1,
    /// Removes the direct definition: the attribute comes back from the style.
    Clear = 2,
    /// Booleans: on when any character lacks it, else off. Enumerations
    /// (underline, baseline shift): to the given value when any character
    /// differs, else back to the neutral one. Space before or after: add 12 pt
    /// when every paragraph has none, else remove.
    Toggle = 3,
    /// Size: next Word step (Ctrl+Maj+>). Left indent: next 720 twips.
    StepUp = 4,
    StepDown = 5,
    /// Size: one point more or less (Ctrl+[ and Ctrl+]).
    StepUpPoint = 6,
    StepDownPoint = 7,
}

impl EditOp {
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            1 => Self::Set,
            2 => Self::Clear,
            3 => Self::Toggle,
            4 => Self::StepUp,
            5 => Self::StepDown,
            6 => Self::StepUpPoint,
            7 => Self::StepDownPoint,
            _ => return None,
        })
    }
}

/// Character attributes an edit can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CharAttr {
    Font = 1,
    Size = 2,
    Bold = 3,
    Italic = 4,
    Underline = 5,
    Strike = 6,
    VerticalAlign = 7,
    Color = 8,
    Highlight = 9,
}

impl CharAttr {
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            1 => Self::Font,
            2 => Self::Size,
            3 => Self::Bold,
            4 => Self::Italic,
            5 => Self::Underline,
            6 => Self::Strike,
            7 => Self::VerticalAlign,
            8 => Self::Color,
            9 => Self::Highlight,
            _ => return None,
        })
    }
}

/// Paragraph attributes an edit can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ParaAttr {
    Alignment = 1,
    IndentLeft = 2,
    IndentRight = 3,
    FirstLine = 4,
    SpaceBefore = 5,
    SpaceAfter = 6,
    LineSpacing = 7,
    Shading = 8,
    Borders = 9,
}

impl ParaAttr {
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            1 => Self::Alignment,
            2 => Self::IndentLeft,
            3 => Self::IndentRight,
            4 => Self::FirstLine,
            5 => Self::SpaceBefore,
            6 => Self::SpaceAfter,
            7 => Self::LineSpacing,
            8 => Self::Shading,
            9 => Self::Borders,
            _ => return None,
        })
    }
}

/// Underline names, in wire order: `none single double thick dotted dashed wave`.
pub const UNDERLINES: u8 = 7;
/// Baseline shifts: `baseline superscript subscript`.
pub const VERTICAL_ALIGNS: u8 = 3;
/// Alignments: `left center right justify`.
pub const ALIGNMENTS: u8 = 4;
/// Line spacing rules: `auto exact atLeast`.
pub const LINE_RULES: u8 = 3;
/// Border styles: `single double dotted dashed`.
pub const BORDER_STYLES: u8 = 4;

/// The value of a character edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CharValue {
    /// Font family name.
    Text(String),
    /// Size in half-points.
    Half(u16),
    Flag(bool),
    /// Underline or baseline shift number.
    Choice(u8),
    Color(Rgb),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharEdit {
    pub attr: CharAttr,
    pub op: EditOp,
    pub value: Option<CharValue>,
}

/// Where a character format goes when the selection is empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FormatScope {
    /// Word's rule: the word under the caret, else the attributes of the next
    /// typed text.
    WordOrTyping = 0,
    /// The selection exactly; an empty one only sets the typing attributes.
    SelectionOnly = 1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharFormat {
    /// Revision the caller believes it applies to; `NO_REVISION_GUARD` skips
    /// the check. A mismatch is `RICH_STALE_REVISION` and nothing changes.
    pub base_revision: u64,
    /// Identifier chosen by the caller (non-zero, one per command): the engine
    /// refuses a second command with the identifier of the last one it
    /// accepted (`RICH_DUPLICATE_COMMAND`) and reports it in `FormatState`, so
    /// the outcome of a command whose reply was lost is certain. 0: none.
    pub command_id: u32,
    pub scope: FormatScope,
    pub edits: Vec<CharEdit>,
}

/// One side of a border edit or state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BorderLine {
    pub style: u8,
    /// 1/8 pt, `2..=96`.
    pub eighths: u16,
    /// Distance from the text, points, `0..=31`.
    pub space: u8,
    /// `None` is the automatic colour.
    pub color: Option<Rgb>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderEdit {
    Keep,
    None,
    Line(BorderLine),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParaValue {
    /// Alignment number.
    Choice(u8),
    /// Signed indent, twips.
    Twips(i32),
    /// Spacing, twips.
    Length(u32),
    /// Line spacing: rule number and value (240ths of a line for `auto`, else twips).
    Line {
        rule: u8,
        value: u32,
    },
    Color(Rgb),
    Borders([BorderEdit; BORDER_SIDES]),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParaEdit {
    pub attr: ParaAttr,
    pub op: EditOp,
    pub value: Option<ParaValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParaFormat {
    pub base_revision: u64,
    pub command_id: u32,
    pub edits: Vec<ParaEdit>,
}

/// Which styles `ListStyles` returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StyleFilter {
    All = 0,
    Paragraph = 1,
    Character = 2,
}

/// Kind of a style named by `ApplyStyle` or `PreviewStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StyleTarget {
    Paragraph = 0,
    Character = 1,
}

/// A style of the catalog, with what the gallery needs to draw its tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleEntry {
    pub id: String,
    /// Display name; empty when it is the identifier (the model keeps no name yet).
    pub name: String,
    pub based_on: String,
    pub target: StyleTarget,
    /// Bit 0 predefined, 1 used in the document, 2 current style of the
    /// selection, 3 default style of its kind, 4 shown in the quick gallery.
    pub flags: u8,
    /// 1 to 9 for a heading style, else 0.
    pub heading_level: u8,
    pub font: String,
    pub half_points: u16,
    /// Bit 0 bold, 1 italic, 2 strike.
    pub char_flags: u8,
    pub underline: u8,
    pub color: Option<Rgb>,
    pub alignment: u8,
}

/// Value of one attribute over the selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tri<T> {
    /// Several values.
    Mixed,
    /// No value: automatic colour, no highlight, no style, no border.
    Unset,
    Value(T),
}

/// The font of the selection: the family the document asks for and the one
/// the engine really draws with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontState {
    pub requested: String,
    pub used: String,
    /// `used` is another family than `requested` (a metric substitute).
    pub substituted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharState {
    pub font: Tri<FontState>,
    pub half_points: Tri<u16>,
    pub bold: Tri<bool>,
    pub italic: Tri<bool>,
    pub underline: Tri<u8>,
    pub strike: Tri<bool>,
    pub vertical_align: Tri<u8>,
    pub color: Tri<Rgb>,
    pub highlight: Tri<Rgb>,
    /// Direct character style of the selection (`Unset`: none).
    pub style: Tri<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParaState {
    pub alignment: Tri<u8>,
    pub indent_left: Tri<i32>,
    pub indent_right: Tri<i32>,
    pub first_line: Tri<i32>,
    pub space_before: Tri<u32>,
    pub space_after: Tri<u32>,
    pub line_spacing: Tri<(u8, u32)>,
    pub shading: Tri<Rgb>,
    pub borders: [Tri<BorderLine>; BORDER_SIDES],
    /// Paragraph style of the selection (`Unset`: none, the default style).
    pub style: Tri<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatStateData {
    pub revision: u64,
    /// The selection the state describes (the client checks it is still the current one).
    pub anchor: u64,
    pub focus: u64,
    /// `command_id` of the last formatting command the engine accepted
    /// (applied or found to change nothing); 0 when none.
    pub last_command_id: u32,
    /// The selection is a caret.
    pub collapsed: bool,
    pub read_only: bool,
    /// More runs than the engine scans (`MAX_STATE_RUNS`): the first ones
    /// answered, the state may differ beyond them.
    pub partial: bool,
    /// The character state shows pending typing attributes.
    pub typing: bool,
    /// The document is plain text (TXT profile): it holds no formatting and the
    /// changing commands answer `RICH_FORMAT_PLAIN_TEXT`.
    pub plain: bool,
    pub chars: CharState,
    pub paragraph: ParaState,
}

/// Runs `FormatState` examines at most; beyond it `partial` is set.
pub const MAX_STATE_RUNS: usize = 20_000;
/// Families one `FontList` carries.
pub const MAX_FONT_LIST: usize = 512;
/// Marks one `FormatMarks` reply carries.
pub const MAX_FORMAT_MARKS: usize = 16_384;
/// Kinds of formatting marks: paragraph end, space, tab, no-break space, line
/// break, page break, column break.
pub const MARK_KINDS: u8 = 7;

/// A font family of `FontList`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontEntry {
    pub name: String,
    /// Bit 0 has a bold face, 1 has an italic face, 2 used by the document,
    /// 3 drawn with another family (`substitute`).
    pub flags: u8,
    /// Family that draws it when bit 3 is set, else empty.
    pub substitute: String,
}

/// The fonts the engine can draw and those the document names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontListData {
    pub revision: u64,
    /// Heading and body fonts of the document theme (empty when it has none).
    pub theme_major: String,
    pub theme_minor: String,
    pub families: Vec<FontEntry>,
}

/// A formatting mark to draw over a page: `x`, `y` is the origin of the marked
/// character on the baseline and `width` its advance, in page points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FormatMark {
    pub page: u32,
    pub kind: u8,
    pub x: f64,
    pub y: f64,
    pub width: f64,
}

// ---------------------------------------------------------------- helpers

fn name(r: &mut Reader<'_>, allow_empty: bool) -> Result<String, Fail> {
    let text = r.text16()?;
    if text.len() > MAX_NAME_BYTES || (!allow_empty && text.is_empty()) {
        return Err(Fail::InvalidBody);
    }
    if text.chars().any(char::is_control) {
        return Err(Fail::InvalidBody);
    }
    Ok(text)
}

fn put_name(b: &mut Vec<u8>, text: &str) -> Result<(), Fail> {
    if text.len() > MAX_NAME_BYTES {
        return Err(Fail::InvalidBody);
    }
    put_text16(b, text)
}

fn rgb(r: &mut Reader<'_>) -> Result<Rgb, Fail> {
    Ok(Rgb {
        r: r.u8()?,
        g: r.u8()?,
        b: r.u8()?,
    })
}

fn put_rgb(b: &mut Vec<u8>, c: Rgb) {
    b.extend_from_slice(&[c.r, c.g, c.b]);
}

fn choice(r: &mut Reader<'_>, count: u8) -> Result<u8, Fail> {
    let v = r.u8()?;
    if v < count {
        Ok(v)
    } else {
        Err(Fail::InvalidBody)
    }
}

fn i32_le(r: &mut Reader<'_>) -> Result<i32, Fail> {
    Ok(r.u32()? as i32)
}

fn twips_signed(r: &mut Reader<'_>) -> Result<i32, Fail> {
    let v = i32_le(r)?;
    if (-MAX_TWIPS..=MAX_TWIPS).contains(&v) {
        Ok(v)
    } else {
        Err(Fail::InvalidBody)
    }
}

fn twips_length(r: &mut Reader<'_>) -> Result<u32, Fail> {
    let v = r.u32()?;
    if v <= MAX_TWIPS as u32 {
        Ok(v)
    } else {
        Err(Fail::InvalidBody)
    }
}

fn put_i32(b: &mut Vec<u8>, v: i32) {
    b.extend_from_slice(&v.to_le_bytes());
}

fn guard(r: &mut Reader<'_>) -> Result<u64, Fail> {
    r.u64()
}

// ---------------------------------------------------------------- character edits

/// Whether an operation on a character attribute carries a value.
fn char_value_follows(attr: CharAttr, op: EditOp) -> Result<bool, Fail> {
    use CharAttr as A;
    use EditOp as O;
    match (op, attr) {
        (O::Set, _) => Ok(true),
        (O::Clear, _) => Ok(false),
        (O::Toggle, A::Bold | A::Italic | A::Strike) => Ok(false),
        (O::Toggle, A::Underline | A::VerticalAlign) => Ok(true),
        (O::StepUp | O::StepDown | O::StepUpPoint | O::StepDownPoint, A::Size) => Ok(false),
        _ => Err(Fail::InvalidBody),
    }
}

fn read_char_value(r: &mut Reader<'_>, attr: CharAttr, op: EditOp) -> Result<CharValue, Fail> {
    Ok(match attr {
        CharAttr::Font => CharValue::Text(name(r, false)?),
        CharAttr::Size => {
            let v = r.u16()?;
            if !(MIN_HALF_POINTS..=MAX_HALF_POINTS).contains(&v) {
                return Err(Fail::InvalidBody);
            }
            CharValue::Half(v)
        }
        CharAttr::Bold | CharAttr::Italic | CharAttr::Strike => CharValue::Flag(r.flag()?),
        CharAttr::Underline => {
            let v = choice(r, UNDERLINES)?;
            // Toggling to "none" is a Clear or a Set; the target is a real style.
            if op == EditOp::Toggle && v == 0 {
                return Err(Fail::InvalidBody);
            }
            CharValue::Choice(v)
        }
        CharAttr::VerticalAlign => {
            let v = choice(r, VERTICAL_ALIGNS)?;
            if op == EditOp::Toggle && v == 0 {
                return Err(Fail::InvalidBody);
            }
            CharValue::Choice(v)
        }
        CharAttr::Color | CharAttr::Highlight => CharValue::Color(rgb(r)?),
    })
}

fn put_char_value(b: &mut Vec<u8>, value: &CharValue) -> Result<(), Fail> {
    match value {
        CharValue::Text(t) => put_name(b, t)?,
        CharValue::Half(v) => b.extend_from_slice(&v.to_le_bytes()),
        CharValue::Flag(v) => b.push(u8::from(*v)),
        CharValue::Choice(v) => b.push(*v),
        CharValue::Color(c) => put_rgb(b, *c),
    }
    Ok(())
}

fn parse_char_format(r: &mut Reader<'_>) -> Result<CharFormat, Fail> {
    let base_revision = guard(r)?;
    let command_id = r.u32()?;
    let scope = match r.u8()? {
        0 => FormatScope::WordOrTyping,
        1 => FormatScope::SelectionOnly,
        _ => return Err(Fail::InvalidBody),
    };
    r.zero(1)?;
    let count = usize::from(r.u16()?);
    if count == 0 || count > MAX_FORMAT_EDITS {
        return Err(Fail::InvalidBody);
    }
    let mut edits: Vec<CharEdit> = Vec::with_capacity(count);
    for _ in 0..count {
        let attr = CharAttr::from_u8(r.u8()?).ok_or(Fail::InvalidBody)?;
        let op = EditOp::from_u8(r.u8()?).ok_or(Fail::InvalidBody)?;
        if edits.iter().any(|e| e.attr == attr) {
            return Err(Fail::InvalidBody);
        }
        let value = if char_value_follows(attr, op)? {
            Some(read_char_value(r, attr, op)?)
        } else {
            None
        };
        edits.push(CharEdit { attr, op, value });
    }
    Ok(CharFormat {
        base_revision,
        command_id,
        scope,
        edits,
    })
}

fn char_value_matches(attr: CharAttr, value: &CharValue) -> bool {
    matches!(
        (attr, value),
        (CharAttr::Font, CharValue::Text(_))
            | (CharAttr::Size, CharValue::Half(_))
            | (
                CharAttr::Bold | CharAttr::Italic | CharAttr::Strike,
                CharValue::Flag(_)
            )
            | (
                CharAttr::Underline | CharAttr::VerticalAlign,
                CharValue::Choice(_)
            )
            | (CharAttr::Color | CharAttr::Highlight, CharValue::Color(_))
    )
}

fn put_char_format(b: &mut Vec<u8>, f: &CharFormat) -> Result<(), Fail> {
    put_u64(b, f.base_revision);
    put_u32(b, f.command_id);
    b.extend_from_slice(&[f.scope as u8, 0]);
    b.extend_from_slice(
        &u16::try_from(f.edits.len())
            .map_err(|_| Fail::InvalidBody)?
            .to_le_bytes(),
    );
    for e in &f.edits {
        b.extend_from_slice(&[e.attr as u8, e.op as u8]);
        match (char_value_follows(e.attr, e.op)?, &e.value) {
            (true, Some(v)) if char_value_matches(e.attr, v) => put_char_value(b, v)?,
            (false, None) => {}
            _ => return Err(Fail::InvalidBody),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- paragraph edits

fn para_value_follows(attr: ParaAttr, op: EditOp) -> Result<bool, Fail> {
    use EditOp as O;
    use ParaAttr as A;
    match (op, attr) {
        (O::Set, _) => Ok(true),
        (O::Clear, _) => Ok(false),
        (O::Toggle, A::SpaceBefore | A::SpaceAfter) => Ok(false),
        (O::StepUp | O::StepDown, A::IndentLeft) => Ok(false),
        _ => Err(Fail::InvalidBody),
    }
}

fn read_line(r: &mut Reader<'_>) -> Result<BorderLine, Fail> {
    let style = choice(r, BORDER_STYLES)?;
    let eighths = r.u16()?;
    if !(2..=MAX_BORDER_EIGHTHS).contains(&eighths) {
        return Err(Fail::InvalidBody);
    }
    let space = r.u8()?;
    if space > MAX_BORDER_SPACE {
        return Err(Fail::InvalidBody);
    }
    let color = match r.u8()? {
        0 => {
            r.zero(3)?;
            None
        }
        1 => Some(rgb(r)?),
        _ => return Err(Fail::InvalidBody),
    };
    Ok(BorderLine {
        style,
        eighths,
        space,
        color,
    })
}

/// The 8 bytes of a border: style, width, space, colour mode, colour.
fn put_line(b: &mut Vec<u8>, l: &BorderLine) {
    b.push(l.style);
    b.extend_from_slice(&l.eighths.to_le_bytes());
    b.push(l.space);
    match l.color {
        None => b.extend_from_slice(&[0, 0, 0, 0]),
        Some(c) => {
            b.push(1);
            put_rgb(b, c);
        }
    }
}

fn read_border_edit(r: &mut Reader<'_>) -> Result<BorderEdit, Fail> {
    match r.u8()? {
        0 => {
            r.zero(8)?;
            Ok(BorderEdit::Keep)
        }
        1 => {
            r.zero(8)?;
            Ok(BorderEdit::None)
        }
        2 => Ok(BorderEdit::Line(read_line(r)?)),
        _ => Err(Fail::InvalidBody),
    }
}

fn put_border_edit(b: &mut Vec<u8>, e: &BorderEdit) {
    match e {
        BorderEdit::Keep => b.extend_from_slice(&[0; 9]),
        BorderEdit::None => {
            b.push(1);
            b.extend_from_slice(&[0; 8]);
        }
        BorderEdit::Line(l) => {
            b.push(2);
            put_line(b, l);
        }
    }
}

fn read_para_value(r: &mut Reader<'_>, attr: ParaAttr) -> Result<ParaValue, Fail> {
    Ok(match attr {
        ParaAttr::Alignment => ParaValue::Choice(choice(r, ALIGNMENTS)?),
        ParaAttr::IndentLeft | ParaAttr::IndentRight | ParaAttr::FirstLine => {
            ParaValue::Twips(twips_signed(r)?)
        }
        ParaAttr::SpaceBefore | ParaAttr::SpaceAfter => ParaValue::Length(twips_length(r)?),
        ParaAttr::LineSpacing => {
            let rule = choice(r, LINE_RULES)?;
            let value = r.u32()?;
            // Auto: 15 (0.06 lines) to 2400 (10 lines); else 1 twip to 22 in.
            let ok = if rule == 0 {
                (15..=2400).contains(&value)
            } else {
                (1..=MAX_TWIPS as u32).contains(&value)
            };
            if !ok {
                return Err(Fail::InvalidBody);
            }
            ParaValue::Line { rule, value }
        }
        ParaAttr::Shading => ParaValue::Color(rgb(r)?),
        ParaAttr::Borders => {
            let mut sides = [BorderEdit::Keep; BORDER_SIDES];
            for side in &mut sides {
                *side = read_border_edit(r)?;
            }
            ParaValue::Borders(sides)
        }
    })
}

fn put_para_value(b: &mut Vec<u8>, value: &ParaValue) {
    match value {
        ParaValue::Choice(v) => b.push(*v),
        ParaValue::Twips(v) => put_i32(b, *v),
        ParaValue::Length(v) => put_u32(b, *v),
        ParaValue::Line { rule, value } => {
            b.push(*rule);
            put_u32(b, *value);
        }
        ParaValue::Color(c) => put_rgb(b, *c),
        ParaValue::Borders(sides) => sides.iter().for_each(|s| put_border_edit(b, s)),
    }
}

fn para_value_matches(attr: ParaAttr, value: &ParaValue) -> bool {
    matches!(
        (attr, value),
        (ParaAttr::Alignment, ParaValue::Choice(_))
            | (
                ParaAttr::IndentLeft | ParaAttr::IndentRight | ParaAttr::FirstLine,
                ParaValue::Twips(_)
            )
            | (
                ParaAttr::SpaceBefore | ParaAttr::SpaceAfter,
                ParaValue::Length(_)
            )
            | (ParaAttr::LineSpacing, ParaValue::Line { .. })
            | (ParaAttr::Shading, ParaValue::Color(_))
            | (ParaAttr::Borders, ParaValue::Borders(_))
    )
}

fn parse_para_format(r: &mut Reader<'_>) -> Result<ParaFormat, Fail> {
    let base_revision = guard(r)?;
    let command_id = r.u32()?;
    let count = usize::from(r.u16()?);
    r.zero(2)?;
    if count == 0 || count > MAX_FORMAT_EDITS {
        return Err(Fail::InvalidBody);
    }
    let mut edits: Vec<ParaEdit> = Vec::with_capacity(count);
    for _ in 0..count {
        let attr = ParaAttr::from_u8(r.u8()?).ok_or(Fail::InvalidBody)?;
        let op = EditOp::from_u8(r.u8()?).ok_or(Fail::InvalidBody)?;
        if edits.iter().any(|e| e.attr == attr) {
            return Err(Fail::InvalidBody);
        }
        let value = if para_value_follows(attr, op)? {
            Some(read_para_value(r, attr)?)
        } else {
            None
        };
        edits.push(ParaEdit { attr, op, value });
    }
    Ok(ParaFormat {
        base_revision,
        command_id,
        edits,
    })
}

fn put_para_format(b: &mut Vec<u8>, f: &ParaFormat) -> Result<(), Fail> {
    put_u64(b, f.base_revision);
    put_u32(b, f.command_id);
    b.extend_from_slice(
        &u16::try_from(f.edits.len())
            .map_err(|_| Fail::InvalidBody)?
            .to_le_bytes(),
    );
    b.extend_from_slice(&[0, 0]);
    for e in &f.edits {
        b.extend_from_slice(&[e.attr as u8, e.op as u8]);
        match (para_value_follows(e.attr, e.op)?, &e.value) {
            (true, Some(v)) if para_value_matches(e.attr, v) => put_para_value(b, v),
            (false, None) => {}
            _ => return Err(Fail::InvalidBody),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- commands

pub(super) fn parse_command(opcode: u16, r: &mut Reader<'_>) -> Result<Command, Fail> {
    Ok(match opcode {
        super::opcode::FORMAT_STATE => Command::FormatState,
        super::opcode::APPLY_CHAR_FORMAT => Command::ApplyCharFormat(parse_char_format(r)?),
        super::opcode::APPLY_PARA_FORMAT => Command::ApplyParaFormat(parse_para_format(r)?),
        super::opcode::LIST_STYLES => {
            let filter = match r.u8()? {
                0 => StyleFilter::All,
                1 => StyleFilter::Paragraph,
                2 => StyleFilter::Character,
                _ => return Err(Fail::InvalidBody),
            };
            let used_only = r.flag()?;
            r.zero(2)?;
            Command::ListStyles { filter, used_only }
        }
        super::opcode::APPLY_STYLE => {
            let base_revision = guard(r)?;
            let command_id = r.u32()?;
            let target = read_target(r)?;
            r.zero(3)?;
            // A paragraph style is always named; an empty character style
            // removes the character style.
            let id = name(r, target == StyleTarget::Character)?;
            Command::ApplyStyle {
                base_revision,
                command_id,
                target,
                id,
            }
        }
        super::opcode::PREVIEW_STYLE => {
            let target = read_target(r)?;
            let show = match r.u8()? {
                0 => true,
                1 => false,
                _ => return Err(Fail::InvalidBody),
            };
            r.zero(2)?;
            let id = name(r, !show)?;
            if !show && !id.is_empty() {
                return Err(Fail::InvalidBody);
            }
            Command::PreviewStyle { target, show, id }
        }
        super::opcode::LIST_FONTS => Command::ListFonts,
        super::opcode::FORMAT_MARKS => {
            let (first, count) = (r.u32()?, r.u32()?);
            if count == 0 {
                return Err(Fail::InvalidBody);
            }
            if count as usize > super::MAX_PAGES_PER_RESPONSE {
                return Err(Fail::TooManyPages);
            }
            if first >= super::MAX_PAGES {
                return Err(Fail::PageRange);
            }
            Command::FormatMarks { first, count }
        }
        _ => return Err(Fail::UnknownOpcode),
    })
}

fn read_target(r: &mut Reader<'_>) -> Result<StyleTarget, Fail> {
    match r.u8()? {
        0 => Ok(StyleTarget::Paragraph),
        1 => Ok(StyleTarget::Character),
        _ => Err(Fail::InvalidBody),
    }
}

pub(super) fn encode_command(command: &Command, b: &mut Vec<u8>) -> Result<u16, Fail> {
    use super::opcode as op;
    Ok(match command {
        Command::FormatState => op::FORMAT_STATE,
        Command::ApplyCharFormat(f) => {
            put_char_format(b, f)?;
            op::APPLY_CHAR_FORMAT
        }
        Command::ApplyParaFormat(f) => {
            put_para_format(b, f)?;
            op::APPLY_PARA_FORMAT
        }
        Command::ListStyles { filter, used_only } => {
            b.extend_from_slice(&[*filter as u8, u8::from(*used_only), 0, 0]);
            op::LIST_STYLES
        }
        Command::ApplyStyle {
            base_revision,
            command_id,
            target,
            id,
        } => {
            put_u64(b, *base_revision);
            put_u32(b, *command_id);
            b.extend_from_slice(&[*target as u8, 0, 0, 0]);
            put_name(b, id)?;
            op::APPLY_STYLE
        }
        Command::PreviewStyle { target, show, id } => {
            b.extend_from_slice(&[*target as u8, u8::from(!*show), 0, 0]);
            put_name(b, id)?;
            op::PREVIEW_STYLE
        }
        Command::ListFonts => op::LIST_FONTS,
        Command::FormatMarks { first, count } => {
            put_u32(b, *first);
            put_u32(b, *count);
            op::FORMAT_MARKS
        }
        _ => return Err(Fail::Internal),
    })
}

// ---------------------------------------------------------------- replies

fn put_tri<T>(
    b: &mut Vec<u8>,
    value: &Tri<T>,
    put: impl FnOnce(&mut Vec<u8>, &T) -> Result<(), Fail>,
) -> Result<(), Fail> {
    match value {
        Tri::Mixed => b.push(0),
        Tri::Value(v) => {
            b.push(1);
            put(b, v)?;
        }
        Tri::Unset => b.push(2),
    }
    Ok(())
}

/// `Reader::flag` as a free function (a method path is not higher-ranked over the reader's lifetime).
fn read_flag(r: &mut Reader<'_>) -> Result<bool, Fail> {
    r.flag()
}

fn read_tri<T>(
    r: &mut Reader<'_>,
    read: impl FnOnce(&mut Reader<'_>) -> Result<T, Fail>,
) -> Result<Tri<T>, Fail> {
    match r.u8()? {
        0 => Ok(Tri::Mixed),
        1 => Ok(Tri::Value(read(r)?)),
        2 => Ok(Tri::Unset),
        _ => Err(Fail::InvalidBody),
    }
}

fn put_char_state(b: &mut Vec<u8>, s: &CharState) -> Result<(), Fail> {
    put_tri(b, &s.font, |b, f| {
        put_name(b, &f.requested)?;
        put_name(b, &f.used)?;
        b.push(u8::from(f.substituted));
        Ok(())
    })?;
    put_tri(b, &s.half_points, |b, v| {
        b.extend_from_slice(&v.to_le_bytes());
        Ok(())
    })?;
    for flag in [&s.bold, &s.italic] {
        put_tri(b, flag, |b, v| {
            b.push(u8::from(*v));
            Ok(())
        })?;
    }
    put_tri(b, &s.underline, |b, v| {
        b.push(*v);
        Ok(())
    })?;
    put_tri(b, &s.strike, |b, v| {
        b.push(u8::from(*v));
        Ok(())
    })?;
    put_tri(b, &s.vertical_align, |b, v| {
        b.push(*v);
        Ok(())
    })?;
    for color in [&s.color, &s.highlight] {
        put_tri(b, color, |b, c| {
            put_rgb(b, *c);
            Ok(())
        })?;
    }
    put_tri(b, &s.style, |b, id| put_name(b, id))
}

fn read_char_state(r: &mut Reader<'_>) -> Result<CharState, Fail> {
    let font = read_tri(r, |r| {
        let requested = name(r, false)?;
        let used = name(r, false)?;
        let substituted = r.flag()?;
        Ok(FontState {
            requested,
            used,
            substituted,
        })
    })?;
    let half_points = read_tri(r, |r| {
        let v = r.u16()?;
        if (MIN_HALF_POINTS..=MAX_HALF_POINTS).contains(&v) {
            Ok(v)
        } else {
            Err(Fail::InvalidBody)
        }
    })?;
    let bold = read_tri(r, read_flag)?;
    let italic = read_tri(r, read_flag)?;
    let underline = read_tri(r, |r| choice(r, UNDERLINES))?;
    let strike = read_tri(r, read_flag)?;
    let vertical_align = read_tri(r, |r| choice(r, VERTICAL_ALIGNS))?;
    let color = read_tri(r, rgb)?;
    let highlight = read_tri(r, rgb)?;
    let style = read_tri(r, |r| name(r, false))?;
    Ok(CharState {
        font,
        half_points,
        bold,
        italic,
        underline,
        strike,
        vertical_align,
        color,
        highlight,
        style,
    })
}

fn put_para_state(b: &mut Vec<u8>, s: &ParaState) -> Result<(), Fail> {
    put_tri(b, &s.alignment, |b, v| {
        b.push(*v);
        Ok(())
    })?;
    for indent in [&s.indent_left, &s.indent_right, &s.first_line] {
        put_tri(b, indent, |b, v| {
            put_i32(b, *v);
            Ok(())
        })?;
    }
    for space in [&s.space_before, &s.space_after] {
        put_tri(b, space, |b, v| {
            put_u32(b, *v);
            Ok(())
        })?;
    }
    put_tri(b, &s.line_spacing, |b, (rule, value)| {
        b.push(*rule);
        put_u32(b, *value);
        Ok(())
    })?;
    put_tri(b, &s.shading, |b, c| {
        put_rgb(b, *c);
        Ok(())
    })?;
    for side in &s.borders {
        put_tri(b, side, |b, l| {
            put_line(b, l);
            Ok(())
        })?;
    }
    put_tri(b, &s.style, |b, id| put_name(b, id))
}

fn read_para_state(r: &mut Reader<'_>) -> Result<ParaState, Fail> {
    let alignment = read_tri(r, |r| choice(r, ALIGNMENTS))?;
    let indent_left = read_tri(r, twips_signed)?;
    let indent_right = read_tri(r, twips_signed)?;
    let first_line = read_tri(r, twips_signed)?;
    let space_before = read_tri(r, twips_length)?;
    let space_after = read_tri(r, twips_length)?;
    let line_spacing = read_tri(r, |r| {
        let rule = choice(r, LINE_RULES)?;
        Ok((rule, r.u32()?))
    })?;
    let shading = read_tri(r, rgb)?;
    let mut borders = [const { Tri::Unset }; BORDER_SIDES];
    for side in &mut borders {
        *side = read_tri(r, read_line)?;
    }
    let style = read_tri(r, |r| name(r, false))?;
    Ok(ParaState {
        alignment,
        indent_left,
        indent_right,
        first_line,
        space_before,
        space_after,
        line_spacing,
        shading,
        borders,
        style,
    })
}

fn put_style_entry(b: &mut Vec<u8>, s: &StyleEntry) -> Result<(), Fail> {
    put_name(b, &s.id)?;
    put_name(b, &s.name)?;
    put_name(b, &s.based_on)?;
    b.extend_from_slice(&[s.target as u8, s.flags, s.heading_level, 0]);
    put_name(b, &s.font)?;
    b.extend_from_slice(&s.half_points.to_le_bytes());
    b.extend_from_slice(&[s.char_flags, s.underline]);
    match s.color {
        None => b.extend_from_slice(&[0, 0, 0, 0]),
        Some(c) => {
            b.push(1);
            put_rgb(b, c);
        }
    }
    b.push(s.alignment);
    Ok(())
}

fn read_style_entry(r: &mut Reader<'_>) -> Result<StyleEntry, Fail> {
    let id = name(r, false)?;
    let display = name(r, true)?;
    let based_on = name(r, true)?;
    let target = read_target(r)?;
    let flags = r.u8()?;
    if flags & !0x1f != 0 {
        return Err(Fail::InvalidBody);
    }
    let heading_level = r.u8()?;
    if heading_level > 9 {
        return Err(Fail::InvalidBody);
    }
    r.zero(1)?;
    let font = name(r, true)?;
    let half_points = r.u16()?;
    if half_points != 0 && !(MIN_HALF_POINTS..=MAX_HALF_POINTS).contains(&half_points) {
        return Err(Fail::InvalidBody);
    }
    let char_flags = r.u8()?;
    if char_flags & !0x07 != 0 {
        return Err(Fail::InvalidBody);
    }
    let underline = choice(r, UNDERLINES)?;
    let color = match r.u8()? {
        0 => {
            r.zero(3)?;
            None
        }
        1 => Some(rgb(r)?),
        _ => return Err(Fail::InvalidBody),
    };
    let alignment = choice(r, ALIGNMENTS)?;
    Ok(StyleEntry {
        id,
        name: display,
        based_on,
        target,
        flags,
        heading_level,
        font,
        half_points,
        char_flags,
        underline,
        color,
        alignment,
    })
}

pub(super) fn encode_reply(response: &Response, b: &mut Vec<u8>) -> Result<u16, Fail> {
    use super::opcode as op;
    Ok(match response {
        Response::FormatState(s) => {
            put_u64(b, s.revision);
            put_u64(b, s.anchor);
            put_u64(b, s.focus);
            put_u32(b, s.last_command_id);
            b.push(
                u8::from(s.collapsed)
                    | u8::from(s.read_only) << 1
                    | u8::from(s.partial) << 2
                    | u8::from(s.typing) << 3
                    | u8::from(s.plain) << 4,
            );
            b.extend_from_slice(&[0, 0, 0]);
            put_char_state(b, &s.chars)?;
            put_para_state(b, &s.paragraph)?;
            op::R_FORMAT_STATE
        }
        Response::StyleList { revision, styles } => {
            if styles.len() > MAX_STYLE_LIST {
                return Err(Fail::InvalidBody);
            }
            put_u64(b, *revision);
            put_u32(b, styles.len() as u32);
            for s in styles {
                put_style_entry(b, s)?;
            }
            op::R_STYLE_LIST
        }
        Response::FontList(list) => {
            if list.families.len() > MAX_FONT_LIST {
                return Err(Fail::InvalidBody);
            }
            put_u64(b, list.revision);
            put_name(b, &list.theme_major)?;
            put_name(b, &list.theme_minor)?;
            put_u32(b, list.families.len() as u32);
            for f in &list.families {
                put_name(b, &f.name)?;
                b.push(f.flags);
                put_name(b, &f.substitute)?;
            }
            op::R_FONT_LIST
        }
        Response::FormatMarks { revision, marks } => {
            if marks.len() > MAX_FORMAT_MARKS {
                return Err(Fail::InvalidBody);
            }
            put_u64(b, *revision);
            put_u32(b, marks.len() as u32);
            for m in marks {
                put_u32(b, m.page);
                b.extend_from_slice(&[m.kind, 0, 0, 0]);
                for v in [m.x, m.y, m.width] {
                    b.extend_from_slice(&v.to_bits().to_le_bytes());
                }
            }
            op::R_FORMAT_MARKS
        }
        _ => return Err(Fail::Internal),
    })
}

pub(super) fn parse_reply(opcode: u16, r: &mut Reader<'_>) -> Result<Response, Fail> {
    Ok(match opcode {
        super::opcode::R_FORMAT_STATE => {
            let revision = r.u64()?;
            let (anchor, focus) = (r.u64()?, r.u64()?);
            let last_command_id = r.u32()?;
            let flags = r.u8()?;
            if flags & !0x1f != 0 {
                return Err(Fail::InvalidBody);
            }
            r.zero(3)?;
            let chars = read_char_state(r)?;
            let paragraph = read_para_state(r)?;
            Response::FormatState(Box::new(FormatStateData {
                revision,
                anchor,
                focus,
                last_command_id,
                collapsed: flags & 1 != 0,
                read_only: flags & 2 != 0,
                partial: flags & 4 != 0,
                typing: flags & 8 != 0,
                plain: flags & 16 != 0,
                chars,
                paragraph,
            }))
        }
        super::opcode::R_STYLE_LIST => {
            let revision = r.u64()?;
            let count = r.u32()? as usize;
            if count > MAX_STYLE_LIST {
                return Err(Fail::InvalidBody);
            }
            let mut styles = Vec::with_capacity(count.min(256));
            for _ in 0..count {
                styles.push(read_style_entry(r)?);
            }
            Response::StyleList { revision, styles }
        }
        super::opcode::R_FONT_LIST => {
            let revision = r.u64()?;
            let theme_major = name(r, true)?;
            let theme_minor = name(r, true)?;
            let count = r.u32()? as usize;
            if count > MAX_FONT_LIST {
                return Err(Fail::InvalidBody);
            }
            let mut families = Vec::with_capacity(count.min(64));
            for _ in 0..count {
                let family = name(r, false)?;
                let flags = r.u8()?;
                if flags & !0x0f != 0 {
                    return Err(Fail::InvalidBody);
                }
                let substitute = name(r, true)?;
                if (flags & 8 != 0) == substitute.is_empty() {
                    return Err(Fail::InvalidBody);
                }
                families.push(FontEntry {
                    name: family,
                    flags,
                    substitute,
                });
            }
            Response::FontList(FontListData {
                revision,
                theme_major,
                theme_minor,
                families,
            })
        }
        super::opcode::R_FORMAT_MARKS => {
            let revision = r.u64()?;
            let count = r.u32()? as usize;
            if count > MAX_FORMAT_MARKS {
                return Err(Fail::InvalidBody);
            }
            let mut marks = Vec::with_capacity(count.min(1024));
            for _ in 0..count {
                let page = r.u32()?;
                if page >= super::MAX_PAGES {
                    return Err(Fail::PageRange);
                }
                let kind = r.u8()?;
                if kind >= MARK_KINDS {
                    return Err(Fail::InvalidBody);
                }
                r.zero(3)?;
                let (x, y, width) = (r.f64()?, r.f64()?, r.f64()?);
                if width < 0.0 {
                    return Err(Fail::InvalidBody);
                }
                marks.push(FormatMark {
                    page,
                    kind,
                    x,
                    y,
                    width,
                });
            }
            Response::FormatMarks { revision, marks }
        }
        _ => return Err(Fail::UnknownOpcode),
    })
}

#[cfg(test)]
mod size_tests {
    use super::*;

    #[test]
    fn word_steps_up_and_down() {
        // 11 pt (22) -> 12 pt -> 14 pt; 8 pt is the first listed size.
        assert_eq!(size_step(22, true), 24);
        assert_eq!(size_step(24, true), 28);
        assert_eq!(size_step(22, false), 21);
        assert_eq!(size_step(21, false), 20);
        assert_eq!(size_step(16, false), 14);
        assert_eq!(size_step(4, false), 2);
        assert_eq!(size_step(4, true), 6);
        assert_eq!(size_step(14, true), 16);
        assert_eq!(size_step(2, false), 2);
        // A size between two listed ones goes to the next listed one.
        assert_eq!(size_step(25, true), 28);
        assert_eq!(size_step(25, false), 24);
        // Above 72 pt: 10 pt steps up to the largest.
        assert_eq!(size_step(144, true), 160);
        assert_eq!(size_step(160, true), 180);
        assert_eq!(size_step(170, false), 160);
        assert_eq!(size_step(160, false), 144);
        assert_eq!(size_step(3270, true), MAX_HALF_POINTS);
        assert_eq!(size_step(MAX_HALF_POINTS, true), MAX_HALF_POINTS);
    }

    #[test]
    fn a_step_always_moves_or_hits_a_bound() {
        for half in MIN_HALF_POINTS..=MAX_HALF_POINTS {
            let up = size_step(half, true);
            let down = size_step(half, false);
            assert!(up > half || half == MAX_HALF_POINTS, "{half} up {up}");
            assert!(down < half || half == MIN_HALF_POINTS, "{half} down {down}");
        }
    }
}
