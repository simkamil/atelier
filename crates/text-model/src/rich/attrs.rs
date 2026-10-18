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
    /// bytes in flat text. Resolution and DOCX mapping are R1's (US-204).
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
            highlight
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
            list
        );
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
            list
        );
        opaque_clear(&mut self.opaque, &mask.opaque);
    }

    pub(crate) fn validate(&self) -> Result<(), RichError> {
        if self.style.as_deref() == Some("") {
            return Err(RichError::InvalidAttribute("empty style reference"));
        }
        if self.list.is_some_and(|l| l.level > MAX_LIST_LEVEL) {
            return Err(RichError::InvalidAttribute("list level above 8"));
        }
        opaque_valid(&self.opaque)
    }
}
