// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded, simplified A4 text pagination for the plain-text editor.

use atelier_font::FontFace;
use atelier_geom::{FlattenedSubpath, Point, Rect};
use atelier_render::{Color, Command, DisplayList, DisplayPath, FillRule, PageInfo, PathElement};
use atelier_shape::{LatinShaper, ShapeOptions};
use atelier_unicode::segmentation::grapheme_boundaries;
use std::collections::BTreeMap;

const PAGE_WIDTH: f64 = 595.276;
const PAGE_HEIGHT: f64 = 841.89;
const MARGIN: f64 = 54.0;
const FONT_SIZE: f64 = 11.0;
const LINE_HEIGHT: f64 = 14.0;
const MAX_PAGES: usize = 1024;
const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
const MAX_ATDL_BYTES: usize = 26 * 1024 * 1024;
const FLATNESS_POINTS: f64 = 0.02;
const BLACK: Color = Color {
    red: 0.0,
    green: 0.0,
    blue: 0.0,
    alpha: 1.0,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CaretPoint {
    pub page: usize,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct LineMap {
    pub page: usize,
    pub top: f64,
    pub baseline: f64,
    pub start: usize,
    pub end: usize,
    pub slots: Vec<(usize, f64)>,
}

#[derive(Clone, Debug)]
pub(crate) struct Layout {
    pub pages: Vec<Vec<u8>>,
    pub lines: Vec<LineMap>,
    pub selection_rects: Vec<(usize, f64, f64, f64, f64)>,
}

#[derive(Debug)]
pub(crate) enum LayoutError {
    Font,
    Shape,
    Outline,
    DisplayList,
    TooManyPages,
    TooLarge,
}

impl Layout {
    pub fn new(text: &str, anchor: usize, focus: usize) -> Result<Self, LayoutError> {
        let font_bytes = include_bytes!("../../../data/fonts/carlito/Carlito-Regular.ttf");
        let face = FontFace::parse(font_bytes).map_err(|_| LayoutError::Font)?;
        let shaper = LatinShaper::new(&face).map_err(|_| LayoutError::Shape)?;
        let mut commands: Vec<Vec<Command>> = vec![Vec::new()];
        let mut lines = Vec::new();
        let mut explicit_breaks = Vec::new();
        let mut outlines: BTreeMap<u16, Vec<FlattenedSubpath>> = BTreeMap::new();
        let mut estimated_atdl = 44usize;
        let mut page_path_elements = vec![0usize];
        let mut page = 0usize;
        let mut baseline = MARGIN + FONT_SIZE;
        let available = PAGE_WIDTH - 2.0 * MARGIN;
        let mut paragraph_start = 0usize;
        let mut i = 0usize;
        while i < text.len() {
            let ch = text[i..].chars().next().expect("char boundary");
            if matches!(ch, '\r' | '\n') {
                let break_len = if ch == '\r' && text[i..].starts_with("\r\n") {
                    2
                } else {
                    ch.len_utf8()
                };
                let first_line = lines.len();
                Self::layout_paragraph(
                    text,
                    paragraph_start..i,
                    &face,
                    &shaper,
                    available,
                    &mut commands,
                    &mut lines,
                    &mut page,
                    &mut baseline,
                    &mut outlines,
                    &mut estimated_atdl,
                    &mut page_path_elements,
                )?;
                if lines.len() > first_line {
                    explicit_breaks.push((i..i + break_len, lines.len() - 1));
                }
                paragraph_start = i + break_len;
                i += break_len;
            } else {
                i += ch.len_utf8();
            }
        }
        Self::layout_paragraph(
            text,
            paragraph_start..text.len(),
            &face,
            &shaper,
            available,
            &mut commands,
            &mut lines,
            &mut page,
            &mut baseline,
            &mut outlines,
            &mut estimated_atdl,
            &mut page_path_elements,
        )?;

        let range_start = anchor.min(focus);
        let range_end = anchor.max(focus);
        let mut selection_rects = Vec::new();
        if range_start != range_end {
            for line in &lines {
                for pair in line.slots.windows(2) {
                    if pair[0].0 < range_end && pair[1].0 > range_start {
                        selection_rects.push((
                            line.page,
                            pair[0].1,
                            line.top,
                            (pair[1].1 - pair[0].1).max(0.5),
                            LINE_HEIGHT,
                        ));
                    }
                }
            }
            for (break_range, line_index) in explicit_breaks {
                if break_range.start < range_end && break_range.end > range_start {
                    let line = &lines[line_index];
                    let x = line.slots.last().map_or(MARGIN, |slot| slot.1);
                    selection_rects.push((line.page, x, line.top, 1.5, LINE_HEIGHT));
                }
            }
        }
        let mut pages = Vec::with_capacity(commands.len());
        for page_commands in commands {
            let bounds = Rect::from_xywh(0.0, 0.0, PAGE_WIDTH, PAGE_HEIGHT)
                .map_err(|_| LayoutError::DisplayList)?;
            let display = DisplayList::new(
                PageInfo::new(bounds).map_err(|_| LayoutError::DisplayList)?,
                page_commands,
            )
            .map_err(|_| LayoutError::DisplayList)?;
            pages.push(display.encode().map_err(|_| LayoutError::DisplayList)?);
        }
        let total = pages
            .iter()
            .try_fold(0usize, |sum, page| sum.checked_add(page.len()))
            .ok_or(LayoutError::TooLarge)?;
        if total > MAX_RESPONSE_BYTES {
            return Err(LayoutError::TooLarge);
        }
        if total > MAX_ATDL_BYTES {
            return Err(LayoutError::TooLarge);
        }
        let response_bytes = 64usize
            .checked_add(text.len())
            .and_then(|n| n.checked_add(pages.len().checked_mul(4)?))
            .and_then(|n| n.checked_add(total))
            .and_then(|n| n.checked_add(selection_rects.len().checked_mul(36)?))
            .ok_or(LayoutError::TooLarge)?;
        if response_bytes > MAX_RESPONSE_BYTES {
            return Err(LayoutError::TooLarge);
        }
        Ok(Self {
            pages,
            lines,
            selection_rects,
        })
    }

    #[allow(clippy::too_many_arguments)] // These mutable outputs form one bounded pagination pass.
    fn layout_paragraph(
        text: &str,
        range: core::ops::Range<usize>,
        face: &FontFace<'_>,
        shaper: &LatinShaper<'_, '_>,
        available: f64,
        commands: &mut Vec<Vec<Command>>,
        lines: &mut Vec<LineMap>,
        page: &mut usize,
        baseline: &mut f64,
        outlines: &mut BTreeMap<u16, Vec<FlattenedSubpath>>,
        estimated_atdl: &mut usize,
        page_path_elements: &mut Vec<usize>,
    ) -> Result<(), LayoutError> {
        let paragraph = &text[range.clone()];
        let bounds = grapheme_boundaries(paragraph);
        let mut chunks: Vec<(usize, usize, String, f64)> = Vec::new();
        for g in bounds.segments() {
            let source = &paragraph[g.clone()];
            let display = if source == "\t" {
                "    ".to_owned()
            } else {
                source.to_owned()
            };
            let shaped = shaper
                .shape_text(&display, ShapeOptions::default())
                .map_err(|_| LayoutError::Shape)?;
            let width = shaped.x_advance as f64 / 64.0;
            chunks.push((range.start + g.start, range.start + g.end, display, width));
        }
        let mut rows: Vec<Vec<(usize, usize, String, f64)>> = Vec::new();
        let mut row = Vec::new();
        let mut width = 0.0;
        for chunk in chunks {
            if !row.is_empty() && width + chunk.3 > available {
                rows.push(row);
                row = Vec::new();
                width = 0.0;
            }
            width += chunk.3;
            row.push(chunk);
        }
        if row.is_empty() {
            rows.push(Vec::new());
        } else {
            rows.push(row);
        }
        for row in rows {
            if *baseline > PAGE_HEIGHT - MARGIN {
                *page += 1;
                if *page >= MAX_PAGES {
                    return Err(LayoutError::TooManyPages);
                }
                let next_estimate = estimated_atdl
                    .checked_add(44)
                    .ok_or(LayoutError::TooLarge)?;
                if next_estimate > MAX_ATDL_BYTES {
                    return Err(LayoutError::TooLarge);
                }
                commands.push(Vec::new());
                page_path_elements.push(0);
                *estimated_atdl = next_estimate;
                *baseline = MARGIN + FONT_SIZE;
            }
            let top = *baseline - FONT_SIZE;
            let mut x = MARGIN;
            let start = row.first().map_or(range.start, |c| c.0);
            let end = row.last().map_or(range.end, |c| c.1);
            let mut slots = vec![(start, x)];
            for (from, to, display, _) in row {
                let shaped = shaper
                    .shape_text(&display, ShapeOptions::default())
                    .map_err(|_| LayoutError::Shape)?;
                let mut pen = x;
                for glyph in shaped.glyphs {
                    let gx = pen + glyph.x_offset as f64 / 64.0;
                    let gy = *baseline - glyph.y_offset as f64 / 64.0;
                    let scale = FONT_SIZE / f64::from(face.metrics().units_per_em);
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        outlines.entry(glyph.glyph_id)
                    {
                        let outline = face
                            .outline(glyph.glyph_id)
                            .map_err(|_| LayoutError::Outline)?;
                        entry.insert(
                            outline
                                .flatten(FLATNESS_POINTS / scale)
                                .map_err(|_| LayoutError::Outline)?,
                        );
                    }
                    let flat = outlines.get(&glyph.glyph_id).expect("outline cached");
                    let (path_elements, encoded_path_size) = flat
                        .iter()
                        .try_fold((0usize, 22usize), |(element_total, byte_total), subpath| {
                            let points = subpath.points.len();
                            let close = usize::from(subpath.closed && points > 0);
                            Some((
                                element_total.checked_add(points)?.checked_add(close)?,
                                byte_total
                                    .checked_add(points.checked_mul(17)?)?
                                    .checked_add(close)?,
                            ))
                        })
                        .ok_or(LayoutError::TooLarge)?;
                    let (next_page_elements, next_total_bytes) = project_path_budget(
                        *estimated_atdl,
                        page_path_elements[*page],
                        path_elements,
                        encoded_path_size,
                        MAX_ATDL_BYTES,
                        atelier_render::limits::MAX_PATH_ELEMENTS,
                    )
                    .ok_or(LayoutError::TooLarge)?;
                    let mut elements = Vec::new();
                    for subpath in flat {
                        if let Some(first) = subpath.points.first() {
                            elements.push(PathElement::MoveTo(
                                Point::try_new(gx + first.x() * scale, gy - first.y() * scale)
                                    .map_err(|_| LayoutError::DisplayList)?,
                            ));
                            for point in subpath.points.iter().skip(1) {
                                elements.push(PathElement::LineTo(
                                    Point::try_new(gx + point.x() * scale, gy - point.y() * scale)
                                        .map_err(|_| LayoutError::DisplayList)?,
                                ));
                            }
                            if subpath.closed {
                                elements.push(PathElement::Close);
                            }
                        }
                    }
                    if !elements.is_empty() {
                        let path =
                            DisplayPath::new(elements).map_err(|_| LayoutError::DisplayList)?;
                        *estimated_atdl = next_total_bytes;
                        page_path_elements[*page] = next_page_elements;
                        commands[*page].push(Command::FillPath {
                            path,
                            color: BLACK,
                            rule: FillRule::NonZero,
                        });
                    }
                    pen += glyph.x_advance as f64 / 64.0;
                }
                x += shaped.x_advance as f64 / 64.0;
                slots.push((to, x));
                let _ = from;
            }
            if slots.len() == 1 {
                slots.push((end, x));
            }
            lines.push(LineMap {
                page: *page,
                top,
                baseline: *baseline,
                start,
                end,
                slots,
            });
            *baseline += LINE_HEIGHT;
        }
        Ok(())
    }

    pub fn caret(&self, offset: usize) -> CaretPoint {
        let index = self.line_index_for_offset(offset);
        let line = &self.lines[index];
        let (_, x) = line
            .slots
            .iter()
            .min_by_key(|(at, _)| at.abs_diff(offset))
            .copied()
            .unwrap_or((line.start, MARGIN));
        CaretPoint {
            page: line.page,
            x,
            y: line.top,
        }
    }

    pub fn hit_test(&self, page: usize, x: f64, y: f64) -> Option<usize> {
        let line = self
            .lines
            .iter()
            .filter(|line| line.page == page)
            .min_by(|a, b| (a.baseline - y).abs().total_cmp(&(b.baseline - y).abs()))?;
        line.slots
            .iter()
            .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
            .map(|slot| slot.0)
    }

    pub fn move_offset(&self, offset: usize, direction: u8) -> usize {
        self.move_offset_at_x(offset, direction, self.caret(offset).x)
    }

    pub fn move_offset_at_x(&self, offset: usize, direction: u8, preferred_x: f64) -> usize {
        let line_index = self.line_index_for_offset(offset);
        let line = &self.lines[line_index];
        match direction {
            0 => self
                .lines
                .iter()
                .flat_map(|line| line.slots.iter())
                .filter(|slot| slot.0 < offset)
                .map(|slot| slot.0)
                .max()
                .unwrap_or(offset),
            1 => self
                .lines
                .iter()
                .flat_map(|line| line.slots.iter())
                .filter(|slot| slot.0 > offset)
                .map(|slot| slot.0)
                .min()
                .unwrap_or(offset),
            4 => line.start,
            5 => line.end,
            6 => self.lines.first().map_or(0, |l| l.start),
            7 => self.lines.last().map_or(offset, |l| l.end),
            2 | 3 | 8 | 9 => {
                let target = if direction == 2 {
                    line_index.checked_sub(1)
                } else if direction == 3 {
                    Some((line_index + 1).min(self.lines.len() - 1))
                } else if direction == 8 {
                    self.lines.iter().rposition(|l| l.page < line.page)
                } else {
                    self.lines.iter().position(|l| l.page > line.page)
                };
                let Some(target) = target else {
                    return offset;
                };
                self.lines[target]
                    .slots
                    .iter()
                    .min_by(|a, b| {
                        (a.1 - preferred_x)
                            .abs()
                            .total_cmp(&(b.1 - preferred_x).abs())
                    })
                    .map_or(offset, |s| s.0)
            }
            _ => offset,
        }
    }

    fn line_index_for_offset(&self, offset: usize) -> usize {
        self.lines
            .iter()
            .position(|line| line.start == offset)
            .or_else(|| {
                self.lines
                    .iter()
                    .position(|line| line.start <= offset && offset < line.end)
            })
            .or_else(|| {
                self.lines
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, line)| {
                        offset.abs_diff(line.start).min(offset.abs_diff(line.end))
                    })
                    .map(|(index, _)| index)
            })
            .unwrap_or(0)
    }
}

fn project_path_budget(
    current_bytes: usize,
    current_page_elements: usize,
    added_elements: usize,
    added_bytes: usize,
    byte_limit: usize,
    page_element_limit: usize,
) -> Option<(usize, usize)> {
    let bytes = current_bytes.checked_add(added_bytes)?;
    let elements = current_page_elements.checked_add(added_elements)?;
    (bytes <= byte_limit && elements <= page_element_limit).then_some((elements, bytes))
}

#[cfg(test)]
mod tests {
    use super::project_path_budget;

    #[test]
    fn path_budget_rejects_before_adding_elements_or_bytes() {
        assert_eq!(project_path_budget(44, 20, 10, 40, 100, 40), Some((30, 84)));
        assert_eq!(project_path_budget(84, 30, 5, 17, 100, 40), None);
        assert_eq!(project_path_budget(44, 39, 2, 10, 100, 40), None);
    }
}
