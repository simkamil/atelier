// SPDX-License-Identifier: AGPL-3.0-or-later
//! Deterministic Latin left-to-right text shaping.

#![forbid(unsafe_code)]

use atelier_font::{FontError, FontFace, FontLayout, GlyphCluster, LayoutError, LayoutLimits};
use atelier_unicode::segmentation::grapheme_boundaries;
use core::ops::Range;

/// Shape parameters. Positions use 1/64 typographic point; the default size is 11 pt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShapeOptions {
    /// Font size in 1/64 typographic point units. Must be positive.
    pub size: i64,
    /// Extra distance between source graphemes, in output units. May be negative.
    pub letter_spacing: i64,
    /// Horizontal scale in thousandths. 1000 means 100%; must be positive.
    pub horizontal_scale: u32,
}

impl Default for ShapeOptions {
    fn default() -> Self {
        Self {
            size: 704,
            letter_spacing: 0,
            horizontal_scale: 1000,
        }
    }
}

/// A glyph placed in the shaped run. Its cluster is a half-open scalar-index range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PositionedGlyph {
    pub glyph_id: u16,
    pub cluster: Range<usize>,
    pub x_advance: i64,
    pub y_advance: i64,
    pub x_offset: i64,
    pub y_offset: i64,
}

/// Shaped glyphs, total advances and (for text input) UTF-8 grapheme boundaries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GlyphRun {
    pub glyphs: Vec<PositionedGlyph>,
    pub x_advance: i64,
    pub y_advance: i64,
    /// Byte offsets including 0 and the text length; absent for generic glyph input.
    pub grapheme_boundaries: Option<Vec<usize>>,
}

/// Errors returned by Latin shaping. Failures never return a partial run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShapeError {
    InvalidOptions(&'static str),
    LimitExceeded(&'static str),
    InvalidGlyph(u16),
    InvalidClusterRange,
    ArithmeticOverflow,
    Font(FontError),
    Layout(LayoutError),
}

impl core::fmt::Display for ShapeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidOptions(s) => write!(f, "invalid shaping options: {s}"),
            Self::LimitExceeded(s) => write!(f, "shaping limit exceeded: {s}"),
            Self::InvalidGlyph(g) => write!(f, "glyph id {g} is outside numGlyphs"),
            Self::InvalidClusterRange => f.write_str("invalid glyph cluster range"),
            Self::ArithmeticOverflow => f.write_str("shaping arithmetic overflow"),
            Self::Font(e) => e.fmt(f),
            Self::Layout(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for ShapeError {}
impl From<FontError> for ShapeError {
    fn from(value: FontError) -> Self {
        Self::Font(value)
    }
}
impl From<LayoutError> for ShapeError {
    fn from(value: LayoutError) -> Self {
        Self::Layout(value)
    }
}

/// Reusable shaping façade. Font tables are validated once when it is created.
pub struct LatinShaper<'face, 'font> {
    face: &'face FontFace<'font>,
    layout: FontLayout<'face>,
    limits: LayoutLimits,
}

impl<'face, 'font> LatinShaper<'face, 'font> {
    /// Create a shaper with default font-layout resource limits.
    pub fn new(face: &'face FontFace<'font>) -> Result<Self, ShapeError> {
        Self::with_layout_limits(face, LayoutLimits::default())
    }

    /// Create a shaper with caller-selected font-layout resource limits.
    pub fn with_layout_limits(
        face: &'face FontFace<'font>,
        limits: LayoutLimits,
    ) -> Result<Self, ShapeError> {
        let layout = face.layout_with_limits(limits)?;
        Ok(Self {
            face,
            layout,
            limits,
        })
    }

    /// Map Unicode scalars through cmap, apply optional `liga`, pair positioning,
    /// scale to output units and preserve UTF-8 source grapheme boundaries.
    pub fn shape_text(&self, text: &str, options: ShapeOptions) -> Result<GlyphRun, ShapeError> {
        validate_options(options)?;
        let scalar_count = text.chars().count();
        self.check_input_count(scalar_count)?;

        let boundary_index = grapheme_boundaries(text);
        let byte_boundaries = boundary_index.boundaries().to_vec();
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let mut glyphs = Vec::with_capacity(scalar_count);
        for (index, &(_, ch)) in chars.iter().enumerate() {
            glyphs.push(GlyphCluster {
                glyph_id: self.face.glyph_index(ch),
                cluster: index..index + 1,
            });
        }
        self.finish(glyphs, options, Some(byte_boundaries), Some(&chars))
    }

    /// Shape caller-supplied glyphs without reconstructing characters.
    /// Letter spacing is added between consecutive groups of identical source ranges.
    pub fn shape_glyphs(
        &self,
        glyphs: &[GlyphCluster],
        options: ShapeOptions,
    ) -> Result<GlyphRun, ShapeError> {
        validate_options(options)?;
        self.check_input_count(glyphs.len())?;
        for glyph in glyphs {
            if glyph.cluster.start >= glyph.cluster.end {
                return Err(ShapeError::InvalidClusterRange);
            }
            if glyph.glyph_id >= self.face.metrics().number_of_glyphs {
                return Err(ShapeError::InvalidGlyph(glyph.glyph_id));
            }
        }
        self.finish(glyphs.to_vec(), options, None, None)
    }

    fn check_input_count(&self, count: usize) -> Result<(), ShapeError> {
        if count > self.limits.input_glyphs {
            Err(ShapeError::LimitExceeded("input scalars/glyphs"))
        } else {
            Ok(())
        }
    }

    fn finish(
        &self,
        mut source: Vec<GlyphCluster>,
        options: ShapeOptions,
        boundaries: Option<Vec<usize>>,
        chars: Option<&[(usize, char)]>,
    ) -> Result<GlyphRun, ShapeError> {
        if options.letter_spacing == 0 {
            source = self.layout.apply_ligatures(&source)?;
        }
        let adjustments = self.layout.pair_adjustments(&source)?;
        let mut positioned = Vec::with_capacity(source.len());
        let mut x_total = 0i64;
        let mut y_total = 0i64;
        for (item, adjustment) in source.into_iter().zip(adjustments) {
            let metric = self.face.horizontal_metric(item.glyph_id)?;
            let x_font = i128::from(metric.advance_width)
                .checked_add(i128::from(adjustment.x_advance))
                .ok_or(ShapeError::ArithmeticOverflow)?;
            let upem = i128::from(self.face.metrics().units_per_em);
            let x_advance = scale_x(x_font, options, upem)?;
            let y_advance = scale_y(i128::from(adjustment.y_advance), options, upem)?;
            let x_offset = scale_x(i128::from(adjustment.x_placement), options, upem)?;
            let y_offset = scale_y(i128::from(adjustment.y_placement), options, upem)?;
            x_total = x_total
                .checked_add(x_advance)
                .ok_or(ShapeError::ArithmeticOverflow)?;
            y_total = y_total
                .checked_add(y_advance)
                .ok_or(ShapeError::ArithmeticOverflow)?;
            positioned.push(PositionedGlyph {
                glyph_id: item.glyph_id,
                cluster: item.cluster,
                x_advance,
                y_advance,
                x_offset,
                y_offset,
            });
        }

        if options.letter_spacing != 0 {
            let ends = if let Some(chars) = chars {
                let boundaries = boundaries
                    .as_ref()
                    .expect("text boundaries accompany chars");
                let mut scalar_ends = Vec::with_capacity(boundaries.len());
                let mut scalar = 0;
                for &byte in boundaries {
                    while scalar < chars.len() && chars[scalar].0 < byte {
                        scalar += 1;
                    }
                    scalar_ends.push(scalar);
                }
                scalar_ends
                    .into_iter()
                    .take(boundaries.len().saturating_sub(1))
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            if let Some(chars) = chars {
                let mut end_index = ends
                    .iter()
                    .copied()
                    .filter(|&end| end > 0 && end < chars.len())
                    .peekable();
                for i in 0..positioned.len() {
                    while end_index
                        .peek()
                        .is_some_and(|&end| end < positioned[i].cluster.end)
                    {
                        end_index.next();
                    }
                    if end_index.peek() == Some(&positioned[i].cluster.end)
                        && positioned
                            .get(i + 1)
                            .is_none_or(|next| next.cluster.end != positioned[i].cluster.end)
                    {
                        positioned[i].x_advance = positioned[i]
                            .x_advance
                            .checked_add(options.letter_spacing)
                            .ok_or(ShapeError::ArithmeticOverflow)?;
                        x_total = x_total
                            .checked_add(options.letter_spacing)
                            .ok_or(ShapeError::ArithmeticOverflow)?;
                        end_index.next();
                    }
                }
            } else {
                for i in 0..positioned.len().saturating_sub(1) {
                    if positioned[i].cluster != positioned[i + 1].cluster {
                        positioned[i].x_advance = positioned[i]
                            .x_advance
                            .checked_add(options.letter_spacing)
                            .ok_or(ShapeError::ArithmeticOverflow)?;
                        x_total = x_total
                            .checked_add(options.letter_spacing)
                            .ok_or(ShapeError::ArithmeticOverflow)?;
                    }
                }
            }
        }
        Ok(GlyphRun {
            glyphs: positioned,
            x_advance: x_total,
            y_advance: y_total,
            grapheme_boundaries: boundaries,
        })
    }
}

fn validate_options(options: ShapeOptions) -> Result<(), ShapeError> {
    if options.size <= 0 {
        return Err(ShapeError::InvalidOptions("size must be positive"));
    }
    if options.horizontal_scale == 0 {
        return Err(ShapeError::InvalidOptions(
            "horizontal scale must be positive",
        ));
    }
    Ok(())
}

fn rounded_ratio(numerator: i128, denominator: i128) -> Result<i64, ShapeError> {
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    let rounded = if remainder == 0 {
        quotient
    } else {
        let abs_remainder = remainder
            .checked_abs()
            .ok_or(ShapeError::ArithmeticOverflow)?;
        if abs_remainder
            .checked_mul(2)
            .ok_or(ShapeError::ArithmeticOverflow)?
            >= denominator
        {
            quotient
                .checked_add(if numerator < 0 { -1 } else { 1 })
                .ok_or(ShapeError::ArithmeticOverflow)?
        } else {
            quotient
        }
    };
    i64::try_from(rounded).map_err(|_| ShapeError::ArithmeticOverflow)
}
fn scale_x(value: i128, options: ShapeOptions, units_per_em: i128) -> Result<i64, ShapeError> {
    let numerator = value
        .checked_mul(i128::from(options.size))
        .and_then(|v| v.checked_mul(i128::from(options.horizontal_scale)))
        .ok_or(ShapeError::ArithmeticOverflow)?;
    let denominator = units_per_em
        .checked_mul(1000)
        .ok_or(ShapeError::ArithmeticOverflow)?;
    rounded_ratio(numerator, denominator)
}
fn scale_y(value: i128, options: ShapeOptions, units_per_em: i128) -> Result<i64, ShapeError> {
    let numerator = value
        .checked_mul(i128::from(options.size))
        .ok_or(ShapeError::ArithmeticOverflow)?;
    rounded_ratio(numerator, units_per_em)
}

#[cfg(test)]
mod tests {
    use super::rounded_ratio;

    #[test]
    fn signed_half_ties_round_away_from_zero() {
        assert_eq!(rounded_ratio(5, 2), Ok(3));
        assert_eq!(rounded_ratio(-5, 2), Ok(-3));
        assert_eq!(rounded_ratio(3, 2), Ok(2));
        assert_eq!(rounded_ratio(-3, 2), Ok(-2));
    }
}
