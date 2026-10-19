// SPDX-License-Identifier: AGPL-3.0-or-later
//! Typed character and paragraph properties. Every property is optional:
//! `None` means "not defined here", i.e. inherited (the style cascade is US-201).

use super::error::RichError;

/// An RGB colour.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Underline style. `None` is an explicit "no underline", unlike an unset property.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Underline {
    None,
    Single,
    Double,
    Thick,
    Dotted,
    Dashed,
    Wave,
}

/// Baseline shift.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerticalAlign {
    Baseline,
    Superscript,
    Subscript,
}

/// An attribute this model does not interpret, preserved verbatim (SPEC 5.2).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpaqueAttr {
    pub namespace: String,
    pub name: String,
    pub value: Vec<u8>,
}

impl OpaqueAttr {
    fn key(&self) -> (&str, &str) {
        (&self.namespace, &self.name)
    }
}

/// Inserts or replaces an opaque attribute, keeping the list sorted by key.
pub(crate) fn opaque_upsert(list: &mut Vec<OpaqueAttr>, attr: &OpaqueAttr) {
    match list.binary_search_by(|a| a.key().cmp(&attr.key())) {
        Ok(i) => list[i] = attr.clone(),
        Err(i) => list.insert(i, attr.clone()),
    }
}

/// Removes the opaque attributes whose key appears in `mask`.
pub(crate) fn opaque_clear(list: &mut Vec<OpaqueAttr>, mask: &[OpaqueAttr]) {
    list.retain(|a| !mask.iter().any(|m| m.key() == a.key()));
}

/// Checks that opaque attributes are strictly sorted by key.
pub(crate) fn opaque_valid(list: &[OpaqueAttr]) -> Result<(), RichError> {
    if list.windows(2).all(|w| w[0].key() < w[1].key()) {
        Ok(())
    } else {
        Err(RichError::InvalidAttribute(
            "opaque attributes must be sorted by unique key",
        ))
    }
}

/// Character properties of a run of text.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CharAttrs {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<Underline>,
    pub strike: Option<bool>,
    pub vertical_align: Option<VerticalAlign>,
    pub font: Option<String>,
    /// Size in half-points (Word `w:sz`).
    pub size_half_points: Option<u32>,
    pub color: Option<Color>,
    pub highlight: Option<Color>,
    /// Character style id (Word `w:rStyle`); a run attribute that counts 0
    /// bytes in flat text. Resolved by the style engine (`resolve_detached`),
    /// fed by the DOCX import and written back by the DOCX writer (US-204).
    pub char_style: Option<String>,
    /// Uninterpreted attributes, sorted by `(namespace, name)`.
    pub opaque: Vec<OpaqueAttr>,
}

impl CharAttrs {
    /// True when no property is defined.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Defines on `self` every property defined in `set`.
    pub fn overlay(&mut self, set: &CharAttrs) {
        macro_rules! take {
            ($($f:ident),*) => {$( if set.$f.is_some() { self.$f = set.$f.clone(); } )*};
        }
        take!(
            bold,
            italic,
            underline,
            strike,
            vertical_align,
            font,
            size_half_points,
            color,
            highlight,
            char_style
        );
        for a in &set.opaque {
            opaque_upsert(&mut self.opaque, a);
        }
    }

    /// Un-defines every property defined in the `mask` (values are ignored).
    pub fn clear(&mut self, mask: &CharAttrs) {
        macro_rules! drop_if {
            ($($f:ident),*) => {$( if mask.$f.is_some() { self.$f = None; } )*};
        }
        drop_if!(
            bold,
            italic,
            underline,
            strike,
            vertical_align,
            font,
            size_half_points,
            color,
            highlight,
            char_style
        );
        opaque_clear(&mut self.opaque, &mask.opaque);
    }

    pub(crate) fn validate(&self) -> Result<(), RichError> {
        if self.font.as_deref() == Some("") {
            return Err(RichError::InvalidAttribute("empty font name"));
        }
        if self.size_half_points == Some(0) {
            return Err(RichError::InvalidAttribute("zero font size"));
        }
        if self.char_style.as_deref() == Some("") {
            return Err(RichError::InvalidAttribute(
                "empty character style reference",
            ));
        }
        opaque_valid(&self.opaque)
    }
}

/// Horizontal alignment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Alignment {
    Left,
    Center,
    Right,
    Justify,
}

/// How a line spacing value is interpreted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LineRule {
    /// `value` is in 240ths of a line.
    Auto,
    /// `value` is exact, in twips.
    Exact,
    /// `value` is a minimum, in twips.
    AtLeast,
}

/// Line spacing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LineSpacing {
    pub rule: LineRule,
    pub value: u32,
}

/// Reference to a list and a nesting level (0 to 8).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ListRef {
    pub list_id: u32,
    pub level: u8,
}

/// Highest list nesting level (Word has nine levels, 0..=8).
pub const MAX_LIST_LEVEL: u8 = 8;

/// Line style of a paragraph border. `None` is an explicit "no border"
/// (`w:val="nil"`), which hides a border the style defines.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParaBorderStyle {
    None,
    Single,
    Double,
    Dotted,
    Dashed,
}

/// One side of a paragraph border.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParaBorder {
    pub style: ParaBorderStyle,
    /// Width in eighths of a point (Word: 2 to 96).
    pub eighths: u16,
    /// Distance from the text, in points (0 to 31).
    pub space: u8,
    /// `None` is the automatic colour (black on a light background).
    pub color: Option<Color>,
}

impl ParaBorder {
    /// True when the side draws something.
    pub fn is_line(&self) -> bool {
        self.style != ParaBorderStyle::None && self.eighths > 0
    }
}

/// The borders of a paragraph; `None` on a side is "not defined here".
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ParaBorders {
    pub top: Option<ParaBorder>,
    pub left: Option<ParaBorder>,
    pub bottom: Option<ParaBorder>,
    pub right: Option<ParaBorder>,
    /// Between two consecutive paragraphs with the same borders.
    pub between: Option<ParaBorder>,
    /// Vertical bar beside the paragraph (`w:bar`).
    pub bar: Option<ParaBorder>,
}

impl ParaBorders {
    /// The six sides in the order of the protocol: top, left, bottom, right,
    /// between, bar.
    pub fn sides(&self) -> [Option<ParaBorder>; 6] {
        [
            self.top,
            self.left,
            self.bottom,
            self.right,
            self.between,
            self.bar,
        ]
    }

    pub fn from_sides(s: [Option<ParaBorder>; 6]) -> Self {
        Self {
            top: s[0],
            left: s[1],
            bottom: s[2],
            right: s[3],
            between: s[4],
            bar: s[5],
        }
    }

    /// True when no side is defined.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// True when a side draws something.
    pub fn any_line(&self) -> bool {
        self.sides().iter().flatten().any(ParaBorder::is_line)
    }
}

/// Paragraph background (`w:shd`). `fill` is the background colour (`None`:
/// automatic, nothing painted); `pattern` is the `w:val` of a pattern other
/// than `clear` (`solid`, `pct25`...), drawn in `pattern_color`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ParaShading {
    pub fill: Option<Color>,
    pub pattern: Option<String>,
    pub pattern_color: Option<Color>,
}

impl ParaShading {
    /// The colour a viewer paints behind the paragraph: the fill, or for a
    /// pattern the mix of the pattern colour (black when automatic) over the
    /// fill (white when automatic) in the proportion the pattern names
    /// (`solid`, `pct25`...; other patterns use 50 %). `None`: nothing painted.
    pub fn paint_color(&self) -> Option<Color> {
        let Some(pattern) = self.pattern.as_deref() else {
            return self.fill;
        };
        let percent: u32 = match pattern {
            "solid" => 100,
            "nil" | "clear" => return self.fill,
            other => other
                .strip_prefix("pct")
                .and_then(|n| n.parse().ok())
                .unwrap_or(50)
                .min(100),
        };
        let ink = self.pattern_color.unwrap_or(Color { r: 0, g: 0, b: 0 });
        let paper = self.fill.unwrap_or(Color {
            r: 255,
            g: 255,
            b: 255,
        });
        let mix = |i: u8, p: u8| {
            ((u32::from(i) * percent + u32::from(p) * (100 - percent) + 50) / 100) as u8
        };
        Some(Color {
            r: mix(ink.r, paper.r),
            g: mix(ink.g, paper.g),
            b: mix(ink.b, paper.b),
        })
    }
}

/// Paragraph properties. Lengths are in twips.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ParaProps {
    pub alignment: Option<Alignment>,
    pub indent_left: Option<i32>,
    pub indent_right: Option<i32>,
    /// Signed first-line indent; negative means hanging.
    pub indent_first_line: Option<i32>,
    pub space_before: Option<u32>,
    pub space_after: Option<u32>,
    pub line_spacing: Option<LineSpacing>,
    /// Paragraph style identifier (resolved by US-201).
    pub style: Option<String>,
    pub list: Option<ListRef>,
    /// Background of the paragraph (US-243).
    pub shading: Option<ParaShading>,
    /// Borders of the paragraph, side by side (US-243).
    pub borders: ParaBorders,
    /// Uninterpreted attributes, sorted by `(namespace, name)`.
    pub opaque: Vec<OpaqueAttr>,
}

impl ParaProps {
    /// True when no property is defined.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Defines on `self` every property defined in `set`.
    pub fn overlay(&mut self, set: &ParaProps) {
        macro_rules! take {
            ($($f:ident),*) => {$( if set.$f.is_some() { self.$f = set.$f.clone(); } )*};
        }
        take!(
            alignment,
            indent_left,
            indent_right,
            indent_first_line,
            space_before,
            space_after,
            line_spacing,
            style,
            list,
            shading
        );
        let (mut mine, theirs) = (self.borders.sides(), set.borders.sides());
        for (m, t) in mine.iter_mut().zip(theirs) {
            if t.is_some() {
                *m = t;
            }
        }
        self.borders = ParaBorders::from_sides(mine);
        for a in &set.opaque {
            opaque_upsert(&mut self.opaque, a);
        }
    }

    /// Un-defines every property defined in the `mask` (values are ignored).
    pub fn clear(&mut self, mask: &ParaProps) {
        macro_rules! drop_if {
            ($($f:ident),*) => {$( if mask.$f.is_some() { self.$f = None; } )*};
        }
        drop_if!(
            alignment,
            indent_left,
            indent_right,
            indent_first_line,
            space_before,
            space_after,
            line_spacing,
            style,
            list,
            shading
        );
        let (mut mine, masked) = (self.borders.sides(), mask.borders.sides());
        for (m, k) in mine.iter_mut().zip(masked) {
            if k.is_some() {
                *m = None;
            }
        }
        self.borders = ParaBorders::from_sides(mine);
        opaque_clear(&mut self.opaque, &mask.opaque);
    }

    pub(crate) fn validate(&self) -> Result<(), RichError> {
        if self.style.as_deref() == Some("") {
            return Err(RichError::InvalidAttribute("empty style reference"));
        }
        if self.list.is_some_and(|l| l.level > MAX_LIST_LEVEL) {
            return Err(RichError::InvalidAttribute("list level above 8"));
        }
        if self.borders.sides().iter().flatten().any(|b| b.space > 31) {
            return Err(RichError::InvalidAttribute("border space above 31 pt"));
        }
        opaque_valid(&self.opaque)
    }
}
