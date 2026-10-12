// SPDX-License-Identifier: AGPL-3.0-or-later
//! Explicit tab-stop settings and decorations for paragraph layout.

use crate::{LayoutError, LayoutOptions, ParagraphLayout};
use atelier_font::manager::FontManager;
use atelier_text_model::rich::{Paragraph, ResolvedProperties};

/// How text following a tab aligns to its stop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TabAlignment {
    Left,
    Center,
    Right,
    Decimal,
    Bar,
}

/// Repeated decoration drawn across a tab's consumed advance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TabLeader {
    Dots,
    Dashes,
    Line,
}

/// A tab stop position from the content area's left margin, in 1/64 pt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TabStop {
    pub position: i64,
    pub alignment: TabAlignment,
    pub leader: Option<TabLeader>,
}

/// Tab policy. Stops must be sorted and unique; interval must be positive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TabSettings {
    pub default_interval: i64,
    pub stops: Vec<TabStop>,
}

impl Default for TabSettings {
    fn default() -> Self {
        Self {
            default_interval: 2268,
            stops: Vec::new(),
        }
    }
}

/// A bar or leader decoration, positioned in paragraph coordinates.
///
/// Bars are emitted for every eligible paragraph line, including lines with no
/// U+0009. A bar has `width == 0`; consumers render it as a vertical stroke
/// whose y and height come from the referenced [`crate::LayoutLine`] metrics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TabDecoration {
    pub line: usize,
    pub x: i64,
    pub width: i64,
    pub kind: TabDecorationKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TabDecorationKind {
    Bar,
    Leader(TabLeader),
}

/// Layout plus decorations that render independently from text glyphs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TabLayout {
    pub layout: ParagraphLayout,
    pub decorations: Vec<TabDecoration>,
}

/// Lay out using explicit tab policy. `layout_paragraph` uses default settings.
///
/// Callers can reuse one `TabSettings` reference for successive paragraphs to
/// apply a document-wide tab policy. Custom non-bar stops are used before
/// automatic stops resume beyond the last such custom stop.
pub fn layout_paragraph_with_tabs(
    paragraph: &Paragraph,
    defaults: &ResolvedProperties,
    fonts: &FontManager<'_>,
    options: LayoutOptions,
    settings: &TabSettings,
) -> Result<TabLayout, LayoutError> {
    crate::layout_paragraph_impl(paragraph, defaults, fonts, options, settings)
}
