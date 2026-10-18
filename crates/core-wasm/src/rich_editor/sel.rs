// SPDX-License-Identifier: AGPL-3.0-or-later
//! Selection highlight, picture bitmaps and word or paragraph selection of the
//! rich editor session (R7b-1). Messages: `rich_protocol/sel.rs`.

use atelier_text_model::inline_image::{IMAGE_OBJECT_KIND, InlineImage};
use atelier_text_model::rich::flat::{FlatIndex, LayoutContext};

use super::Session;
use crate::images::{self, ImageInsertError};
use crate::rich_layout::{PageDelta, RichPageCache};
use crate::rich_protocol::{
    self as proto, DEFAULT_LAYOUT_BUDGET, ErrorCode, MAX_SELECTION_RECTS, Response, SelectionRect,
    SelectionUnit,
};

/// The selection after a unit selection of `target`, from `base` (the unit
/// selected by the click that began the gesture, or the selection that a
/// shift-click extends): the span from `base` to `target`, anchored on the
/// side of `base` that `target` does not reach (the rule of the v1 editor).
fn extend_by_units(base: (usize, usize), target: (usize, usize)) -> (usize, usize) {
    if base.0 == base.1 && target.0 <= base.0 && base.0 <= target.1 {
        // A caret inside the unit: the unit itself.
        target
    } else if target.1 <= base.0 {
        (base.1, target.0)
    } else if target.0 >= base.1 {
        (base.0, target.1)
    } else {
        base
    }
}

impl Session {
    /// Rectangles of the selection on the pages `first .. first + count`,
    /// clamped to the page count; at most `MAX_SELECTION_RECTS` (the rest of
    /// the selection is not highlighted, the client asks for fewer pages).
    pub(super) fn selection_rects(
        &mut self,
        first: u32,
        count: u32,
    ) -> Result<Response, ErrorCode> {
        let page_count = self.cache.page_count();
        if first >= page_count {
            return Err(ErrorCode::PageRange);
        }
        let end = first.saturating_add(count).min(page_count);
        let (start, stop) = self.selection_range();
        let mut rects = Vec::new();
        if start != stop {
            'pages: for page in first..end {
                let Some(found) = self.ready(|c| c.selection_rects(page, start, stop))? else {
                    return Err(ErrorCode::NotReady);
                };
                for r in found {
                    if rects.len() >= MAX_SELECTION_RECTS {
                        break 'pages;
                    }
                    rects.push(SelectionRect {
                        page: r.page,
                        x: r.x,
                        y: r.y,
                        width: r.width,
                        height: r.height,
                    });
                }
            }
        }
        Ok(Response::SelectionRects {
            revision: self.revision(),
            rects,
        })
    }

    /// The display bitmap of the picture `object_id`.
    pub(super) fn image_bitmap(&mut self, object_id: u64) -> Result<Response, ErrorCode> {
        let index = self
            .cache
            .picture_paragraph(object_id)
            .ok_or(ErrorCode::UnknownObject)?;
        let object = LayoutContext::paragraph(&self.doc, index)
            .and_then(|p| {
                p.anchored
                    .iter()
                    .find(|o| o.id.value() == object_id && o.kind == IMAGE_OBJECT_KIND)
            })
            .ok_or(ErrorCode::UnknownObject)?;
        let image =
            InlineImage::from_object_data(&object.data).map_err(|_| ErrorCode::UnknownObject)?;
        let bitmap = images::render_bitmap(&image).map_err(|error| match error {
            ImageInsertError::TooLarge | ImageInsertError::StoreFull => ErrorCode::Limit,
            ImageInsertError::Unsupported | ImageInsertError::Corrupt => ErrorCode::ImageInvalid,
        })?;
        Ok(Response::ImageBitmap {
            object_id,
            width: bitmap.width,
            height: bitmap.height,
            rgba: bitmap.rgba,
        })
    }

    /// Bounds of the word or paragraph around `at`. A paragraph includes the
    /// break that ends it, except the last one.
    fn unit_bounds(&self, at: usize, unit: SelectionUnit) -> Result<(usize, usize), ErrorCode> {
        match unit {
            SelectionUnit::Word => Ok(self.doc.word_at(at)?.unwrap_or((at, at))),
            SelectionUnit::Paragraph => {
                let paragraph = self.doc.locate(at)?.paragraph;
                let start = self.doc.paragraph_start(paragraph)?;
                let end = if paragraph + 1 < self.doc.paragraph_count() {
                    self.doc.paragraph_start(paragraph + 1)?
                } else {
                    self.doc.len()
                };
                Ok((start, end))
            }
        }
    }

    /// Double and triple click. `extend` keeps growing the selection by units
    /// from the unit of the click that began the gesture (`unit_base`), or from
    /// the current selection when no unit gesture is running (shift-click).
    pub(super) fn select_unit(
        &mut self,
        position: u64,
        unit: SelectionUnit,
        extend: bool,
    ) -> Result<Response, ErrorCode> {
        let at = proto::to_position(position)?;
        self.doc.check_position(at)?;
        let target = self.unit_bounds(at, unit)?;
        let base = if extend {
            self.unit_base.unwrap_or_else(|| self.selection_range())
        } else {
            target
        };
        let (anchor, focus) = if extend {
            extend_by_units(base, target)
        } else {
            target
        };
        let (anchor, focus) = (self.snap(anchor), self.snap(focus));
        self.select(anchor, focus, false, None);
        self.unit_base = Some(base);
        self.ensure_caret();
        self.finish(PageDelta::default(), DEFAULT_LAYOUT_BUDGET, true)
    }
}
