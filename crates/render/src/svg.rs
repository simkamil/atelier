// SPDX-License-Identifier: AGPL-3.0-or-later
//! Deterministic, bounded SVG serialization for validated display lists.

use crate::{
    Color, Command, DisplayList, DisplayPath, DisplayTransform, FillRule, LineCap, LineJoin,
    PathElement, limits,
};
use atelier_geom::Rect;
use core::fmt;
use std::collections::HashMap;
use std::fmt::Write as _;

/// A borrowed image resource. MIME values must be one of the canonical values
/// `image/png`, `image/jpeg`, `image/gif`, or `image/bmp`.
#[derive(Clone, Copy, Debug)]
pub struct SvgImage<'a> {
    pub mime_type: &'a str,
    pub bytes: &'a [u8],
}

/// Synchronous, in-memory resource lookup supplied by the caller.
pub trait SvgResourceProvider {
    /// Returns a glyph outline in page units relative to the glyph origin.
    fn glyph_outline(&self, font_id: u64, glyph_id: u32) -> Option<&DisplayPath>;

    /// Returns an image's canonical MIME type and encoded bytes.
    fn image(&self, image_id: u64) -> Option<SvgImage<'_>>;
}

/// Per-render memory and output budgets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SvgLimits {
    /// Maximum UTF-8 bytes in the completed SVG (default 64 MiB).
    pub max_output_bytes: usize,
    /// Maximum encoded bytes for any one image (default 8 MiB).
    pub max_image_bytes: usize,
    /// Maximum raw image bytes embedded across the page (default 32 MiB).
    pub max_embedded_bytes: usize,
    /// Maximum unique glyph outlines resolved (default 1,000,000).
    pub max_resolved_glyphs: usize,
    /// Maximum path elements across unique supplied glyph outlines (default 1,000,000).
    pub max_resolved_path_elements: usize,
}

impl Default for SvgLimits {
    fn default() -> Self {
        Self {
            max_output_bytes: 64 * 1024 * 1024,
            max_image_bytes: 8 * 1024 * 1024,
            max_embedded_bytes: 32 * 1024 * 1024,
            max_resolved_glyphs: limits::MAX_GLYPHS,
            max_resolved_path_elements: limits::MAX_PATH_ELEMENTS,
        }
    }
}

/// Errors returned without exposing a partial SVG document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgError {
    MissingResource,
    InvalidBytes,
    InvalidMime,
    InvalidList,
    LimitExceeded,
}

impl fmt::Display for SvgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingResource => "SVG resource is missing",
            Self::InvalidBytes => "SVG resource bytes are invalid",
            Self::InvalidMime => "SVG resource MIME type is not supported",
            Self::InvalidList => "display list or resource outline is invalid",
            Self::LimitExceeded => "SVG render limit exceeded",
        })
    }
}

impl std::error::Error for SvgError {}

struct BoundedString {
    value: String,
    limit: usize,
}

impl BoundedString {
    fn new(limit: usize) -> Self {
        Self {
            value: String::new(),
            limit,
        }
    }

    fn push(&mut self, text: &str) -> Result<(), SvgError> {
        let required = self
            .value
            .len()
            .checked_add(text.len())
            .ok_or(SvgError::LimitExceeded)?;
        if required > self.limit {
            return Err(SvgError::LimitExceeded);
        }
        self.value
            .try_reserve(text.len())
            .map_err(|_| SvgError::LimitExceeded)?;
        self.value.push_str(text);
        Ok(())
    }
}

impl fmt::Write for BoundedString {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.push(s).map_err(|_| fmt::Error)
    }
}

#[derive(Clone, Copy)]
struct GlyphDef<'a> {
    id: usize,
    path: &'a DisplayPath,
}

#[derive(Clone, Copy)]
struct ImageUse<'a> {
    command_index: usize,
    image: SvgImage<'a>,
}

#[derive(Clone, Copy)]
struct ClipDef {
    command_index: usize,
    bounds: Rect,
}

/// Renders one validated display list to a complete, deterministic SVG.
pub fn render_svg<P: SvgResourceProvider + ?Sized>(
    list: &DisplayList,
    provider: &P,
    render_limits: SvgLimits,
) -> Result<String, SvgError> {
    list.validate().map_err(|_| SvgError::InvalidList)?;

    // Resolve and validate every resource before building output. Vector sizes
    // are bounded by the validated command/glyph budgets and explicit limits.
    let mut glyph_defs = Vec::<GlyphDef<'_>>::new();
    let mut glyph_ids = HashMap::<(u64, u32), usize>::new();
    let mut resolved_path_elements = 0usize;
    let mut image_uses = Vec::<ImageUse<'_>>::new();
    let mut embedded_bytes = 0usize;
    for (command_index, command) in list.commands.iter().enumerate() {
        match command {
            Command::DrawGlyphRun {
                font_id, glyphs, ..
            } => {
                for glyph in glyphs {
                    let key = (*font_id, glyph.glyph_id);
                    if glyph_ids.contains_key(&key) {
                        continue;
                    }
                    let next_count = glyph_defs
                        .len()
                        .checked_add(1)
                        .ok_or(SvgError::LimitExceeded)?;
                    if next_count > render_limits.max_resolved_glyphs {
                        return Err(SvgError::LimitExceeded);
                    }
                    let path = provider
                        .glyph_outline(*font_id, glyph.glyph_id)
                        .ok_or(SvgError::MissingResource)?;
                    let next_elements = resolved_path_elements
                        .checked_add(path.elements.len())
                        .ok_or(SvgError::LimitExceeded)?;
                    if path.elements.len() > limits::MAX_PATH_ELEMENTS
                        || next_elements > render_limits.max_resolved_path_elements
                        || next_elements > limits::MAX_PATH_ELEMENTS
                    {
                        return Err(SvgError::LimitExceeded);
                    }
                    path.validate().map_err(|_| SvgError::InvalidList)?;
                    glyph_defs
                        .try_reserve(1)
                        .map_err(|_| SvgError::LimitExceeded)?;
                    glyph_ids
                        .try_reserve(1)
                        .map_err(|_| SvgError::LimitExceeded)?;
                    let id = glyph_defs.len();
                    glyph_defs.push(GlyphDef { id, path });
                    glyph_ids.insert(key, id);
                    resolved_path_elements = next_elements;
                }
            }
            Command::DrawImage { image_id, .. } => {
                let image = provider.image(*image_id).ok_or(SvgError::MissingResource)?;
                validate_image(image, render_limits.max_image_bytes)?;
                embedded_bytes = embedded_bytes
                    .checked_add(image.bytes.len())
                    .ok_or(SvgError::LimitExceeded)?;
                if embedded_bytes > render_limits.max_embedded_bytes {
                    return Err(SvgError::LimitExceeded);
                }
                image_uses
                    .try_reserve(1)
                    .map_err(|_| SvgError::LimitExceeded)?;
                image_uses.push(ImageUse {
                    command_index,
                    image,
                });
            }
            Command::Link { target, .. } => validate_xml_text(target)?,
            Command::Structure { role, .. } => validate_xml_text(role)?,
            _ => {}
        }
    }

    let mut clip_defs = Vec::<ClipDef>::new();
    let mut clip_stack: Vec<Option<Rect>> = vec![None];
    let mut clip_empty = vec![false];
    let mut transform_stack = vec![DisplayTransform::identity()];
    for (command_index, command) in list.commands.iter().enumerate() {
        match command {
            Command::PushClip { bounds } => {
                let parent = *clip_stack.last().ok_or(SvgError::InvalidList)?;
                let empty = *clip_empty.last().ok_or(SvgError::InvalidList)?;
                let intersection = if empty {
                    None
                } else {
                    parent.map_or(Some(*bounds), |r| r.intersection(*bounds))
                };
                clip_stack.push(intersection);
                clip_empty.push(empty || (parent.is_some() && intersection.is_none()));
            }
            Command::PopClip => {
                clip_stack.pop().ok_or(SvgError::InvalidList)?;
                clip_empty.pop().ok_or(SvgError::InvalidList)?;
            }
            Command::PushTransform { transform } => {
                let current = *transform_stack.last().ok_or(SvgError::InvalidList)?;
                transform_stack.push(
                    current
                        .then(*transform)
                        .map_err(|_| SvgError::InvalidList)?,
                );
            }
            Command::PopTransform => {
                if transform_stack.len() <= 1 {
                    return Err(SvgError::InvalidList);
                }
                transform_stack.pop();
            }
            Command::FillPath { .. }
            | Command::StrokePath { .. }
            | Command::DrawGlyphRun { .. }
            | Command::DrawImage { .. }
                if !*clip_empty.last().ok_or(SvgError::InvalidList)? =>
            {
                if let Some(bounds) = *clip_stack.last().ok_or(SvgError::InvalidList)? {
                    clip_defs
                        .try_reserve(1)
                        .map_err(|_| SvgError::LimitExceeded)?;
                    clip_defs.push(ClipDef {
                        command_index,
                        bounds,
                    });
                }
            }
            _ => {}
        }
    }

    let bounds = list.page.bounds;
    let mut out = BoundedString::new(render_limits.max_output_bytes);
    write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" version=\"1.1\" width=\"{}\" height=\"{}\" viewBox=\"{} {} {} {}\">",
        bounds.width(),
        bounds.height(),
        bounds.left(),
        bounds.top(),
        bounds.width(),
        bounds.height()
    )
    .map_err(|_| SvgError::LimitExceeded)?;
    if !glyph_defs.is_empty() || !clip_defs.is_empty() {
        out.push("<defs>")?;
        for glyph in &glyph_defs {
            write!(
                out,
                "<path id=\"glyph-{}\" fill-rule=\"nonzero\" d=\"",
                glyph.id
            )
            .map_err(|_| SvgError::LimitExceeded)?;
            write_path_data(&mut out, glyph.path)?;
            out.push("\"/>")?;
        }
        for (index, clip) in clip_defs.iter().enumerate() {
            let r = clip.bounds;
            write!(out, "<clipPath id=\"clip-{index}\" clipPathUnits=\"userSpaceOnUse\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath>", r.left(), r.top(), r.width(), r.height())
                .map_err(|_| SvgError::LimitExceeded)?;
        }
        out.push("</defs>")?;
    }

    let mut image_cursor = 0usize;
    let mut clip_cursor = 0usize;
    clip_stack = vec![None];
    clip_empty = vec![false];
    transform_stack = vec![DisplayTransform::identity()];
    for (command_index, command) in list.commands.iter().enumerate() {
        match command {
            Command::PushClip { bounds } => {
                let parent = *clip_stack.last().ok_or(SvgError::InvalidList)?;
                let empty = *clip_empty.last().ok_or(SvgError::InvalidList)?;
                let intersection = if empty {
                    None
                } else {
                    parent.map_or(Some(*bounds), |r| r.intersection(*bounds))
                };
                clip_stack.push(intersection);
                clip_empty.push(empty || (parent.is_some() && intersection.is_none()));
            }
            Command::PopClip => {
                clip_stack.pop().ok_or(SvgError::InvalidList)?;
                clip_empty.pop().ok_or(SvgError::InvalidList)?;
            }
            Command::PushTransform { transform } => {
                let current = *transform_stack.last().ok_or(SvgError::InvalidList)?;
                transform_stack.push(
                    current
                        .then(*transform)
                        .map_err(|_| SvgError::InvalidList)?,
                );
            }
            Command::PopTransform => {
                if transform_stack.len() <= 1 {
                    return Err(SvgError::InvalidList);
                }
                transform_stack.pop();
            }
            Command::FillPath { path, color, rule } => {
                if !is_empty_clip(&clip_empty)? {
                    let clip_id = active_clip_id(&clip_defs, &mut clip_cursor, command_index);
                    begin_draw(
                        &mut out,
                        clip_id,
                        *transform_stack.last().ok_or(SvgError::InvalidList)?,
                    )?;
                    write!(
                        out,
                        "<path fill=\"rgb({}% {}% {}%)\" fill-opacity=\"{}\" fill-rule=\"{}\" d=\"",
                        color_value(color.red),
                        color_value(color.green),
                        color_value(color.blue),
                        color.alpha,
                        match rule {
                            FillRule::NonZero => "nonzero",
                            FillRule::EvenOdd => "evenodd",
                        }
                    )
                    .map_err(|_| SvgError::LimitExceeded)?;
                    write_path_data(&mut out, path)?;
                    out.push("\"/>")?;
                    end_draw(&mut out, clip_id)?;
                }
            }
            Command::StrokePath { path, color, style } => {
                if !is_empty_clip(&clip_empty)? {
                    let clip_id = active_clip_id(&clip_defs, &mut clip_cursor, command_index);
                    begin_draw(
                        &mut out,
                        clip_id,
                        *transform_stack.last().ok_or(SvgError::InvalidList)?,
                    )?;
                    out.push("<path fill=\"none\" stroke=\"")?;
                    write_color_rgb(&mut out, color)?;
                    write!(out, "\" stroke-opacity=\"{}\" stroke-width=\"{}\" stroke-linecap=\"{}\" stroke-linejoin=\"{}\" stroke-miterlimit=\"{}\" d=\"", color.alpha, style.width, cap_name(style.cap), join_name(style.join), style.miter_limit)
                        .map_err(|_| SvgError::LimitExceeded)?;
                    write_path_data(&mut out, path)?;
                    out.push("\"/>")?;
                    end_draw(&mut out, clip_id)?;
                }
            }
            Command::DrawGlyphRun {
                font_id,
                color,
                glyphs,
            } => {
                if !is_empty_clip(&clip_empty)? {
                    let clip_id = active_clip_id(&clip_defs, &mut clip_cursor, command_index);
                    begin_draw(
                        &mut out,
                        clip_id,
                        *transform_stack.last().ok_or(SvgError::InvalidList)?,
                    )?;
                    out.push("<g fill=\"")?;
                    write_color_rgb(&mut out, color)?;
                    write!(out, "\" fill-opacity=\"{}\">", color.alpha)
                        .map_err(|_| SvgError::LimitExceeded)?;
                    for glyph in glyphs {
                        let id = glyph_ids
                            .get(&(*font_id, glyph.glyph_id))
                            .ok_or(SvgError::InvalidList)?;
                        write!(
                            out,
                            "<use xlink:href=\"#glyph-{}\" x=\"{}\" y=\"{}\"/>",
                            id,
                            glyph.position.x(),
                            glyph.position.y()
                        )
                        .map_err(|_| SvgError::LimitExceeded)?;
                    }
                    out.push("</g>")?;
                    end_draw(&mut out, clip_id)?;
                }
            }
            Command::DrawImage { bounds, .. } => {
                let image = image_uses.get(image_cursor).ok_or(SvgError::InvalidList)?;
                if image.command_index != command_index {
                    return Err(SvgError::InvalidList);
                }
                image_cursor += 1;
                if !is_empty_clip(&clip_empty)? {
                    let clip_id = active_clip_id(&clip_defs, &mut clip_cursor, command_index);
                    begin_draw(
                        &mut out,
                        clip_id,
                        *transform_stack.last().ok_or(SvgError::InvalidList)?,
                    )?;
                    let encoded_len =
                        base64_len(image.image.bytes.len()).ok_or(SvgError::LimitExceeded)?;
                    let r = *bounds;
                    write!(out, "<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" xlink:href=\"data:{};base64,", r.left(), r.top(), r.width(), r.height(), image.image.mime_type)
                        .map_err(|_| SvgError::LimitExceeded)?;
                    if encoded_len
                        > render_limits
                            .max_output_bytes
                            .saturating_sub(out.value.len())
                    {
                        return Err(SvgError::LimitExceeded);
                    }
                    write_base64(&mut out, image.image.bytes)?;
                    out.push("\"/>")?;
                    end_draw(&mut out, clip_id)?;
                }
            }
            Command::Link { bounds, target } => {
                let r = *bounds;
                out.push(
                    "<metadata><atelier:link xmlns:atelier=\"urn:atelier:display-list\" target=\"",
                )?;
                write_escaped(&mut out, target)?;
                write!(
                    out,
                    "\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></metadata>",
                    r.left(),
                    r.top(),
                    r.width(),
                    r.height()
                )
                .map_err(|_| SvgError::LimitExceeded)?;
            }
            Command::Structure { role, node_id } => {
                out.push("<metadata><atelier:structure xmlns:atelier=\"urn:atelier:display-list\" role=\"")?;
                write_escaped(&mut out, role)?;
                write!(out, "\" node-id=\"{}\"/></metadata>", node_id)
                    .map_err(|_| SvgError::LimitExceeded)?;
            }
        }
    }
    if image_cursor != image_uses.len() {
        return Err(SvgError::InvalidList);
    }
    out.push("</svg>")?;
    Ok(out.value)
}

// An image is never decoded: these short signatures only establish that the
// declared MIME and the encoded stream's initial bytes agree.
fn validate_image(image: SvgImage<'_>, max_image_bytes: usize) -> Result<(), SvgError> {
    if image.bytes.len() > max_image_bytes {
        return Err(SvgError::LimitExceeded);
    }
    let signature: &[u8] = match image.mime_type {
        "image/png" => b"\x89PNG\r\n\x1a\n",
        "image/jpeg" => b"\xff\xd8\xff",
        "image/gif" => {
            return if image.bytes.starts_with(b"GIF87a") || image.bytes.starts_with(b"GIF89a") {
                Ok(())
            } else {
                Err(SvgError::InvalidBytes)
            };
        }
        "image/bmp" => b"BM",
        _ => return Err(SvgError::InvalidMime),
    };
    if image.bytes.starts_with(signature) {
        Ok(())
    } else {
        Err(SvgError::InvalidBytes)
    }
}

fn validate_xml_text(text: &str) -> Result<(), SvgError> {
    if text.chars().all(|ch| {
        matches!(ch, '\u{9}' | '\u{A}' | '\u{D}')
            || ('\u{20}'..='\u{D7FF}').contains(&ch)
            || ('\u{E000}'..='\u{FFFD}').contains(&ch)
            || ('\u{10000}'..='\u{10FFFF}').contains(&ch)
    }) {
        Ok(())
    } else {
        Err(SvgError::InvalidList)
    }
}

fn write_escaped(out: &mut BoundedString, text: &str) -> Result<(), SvgError> {
    for ch in text.chars() {
        match ch {
            '&' => out.push("&amp;")?,
            '<' => out.push("&lt;")?,
            '"' => out.push("&quot;")?,
            '\'' => out.push("&apos;")?,
            '\t' => out.push("&#9;")?,
            '\n' => out.push("&#10;")?,
            '\r' => out.push("&#13;")?,
            _ => {
                let mut buf = [0; 4];
                out.push(ch.encode_utf8(&mut buf))?;
            }
        }
    }
    Ok(())
}

fn write_path_data(out: &mut BoundedString, path: &DisplayPath) -> Result<(), SvgError> {
    let mut first = true;
    for element in &path.elements {
        if !first {
            out.push(" ")?;
        }
        first = false;
        match *element {
            PathElement::MoveTo(p) => write!(out, "M {} {}", p.x(), p.y()),
            PathElement::LineTo(p) => write!(out, "L {} {}", p.x(), p.y()),
            PathElement::QuadraticTo { control, to } => write!(
                out,
                "Q {} {} {} {}",
                control.x(),
                control.y(),
                to.x(),
                to.y()
            ),
            PathElement::CubicTo {
                control1,
                control2,
                to,
            } => write!(
                out,
                "C {} {} {} {} {} {}",
                control1.x(),
                control1.y(),
                control2.x(),
                control2.y(),
                to.x(),
                to.y()
            ),
            PathElement::Close => out.write_str("Z"),
        }
        .map_err(|_| SvgError::LimitExceeded)?;
    }
    Ok(())
}

fn color_value(value: f32) -> f64 {
    f64::from(value) * 100.0
}
fn write_color_rgb(out: &mut BoundedString, color: &Color) -> Result<(), SvgError> {
    write!(
        out,
        "rgb({}% {}% {}%)",
        color_value(color.red),
        color_value(color.green),
        color_value(color.blue)
    )
    .map_err(|_| SvgError::LimitExceeded)
}
fn cap_name(cap: LineCap) -> &'static str {
    match cap {
        LineCap::Butt => "butt",
        LineCap::Round => "round",
        LineCap::Square => "square",
    }
}
fn join_name(join: LineJoin) -> &'static str {
    match join {
        LineJoin::Miter => "miter",
        LineJoin::Round => "round",
        LineJoin::Bevel => "bevel",
    }
}

fn begin_draw(
    out: &mut BoundedString,
    clip_id: Option<usize>,
    transform: DisplayTransform,
) -> Result<(), SvgError> {
    if let Some(id) = clip_id {
        write!(out, "<g clip-path=\"url(#clip-{id})\">").map_err(|_| SvgError::LimitExceeded)?;
    }
    write!(
        out,
        "<g transform=\"matrix({} {} {} {} {} {})\">",
        transform.a, transform.b, transform.c, transform.d, transform.e, transform.f
    )
    .map_err(|_| SvgError::LimitExceeded)
}
fn end_draw(out: &mut BoundedString, clip_id: Option<usize>) -> Result<(), SvgError> {
    out.push("</g>")?;
    if clip_id.is_some() {
        out.push("</g>")?;
    }
    Ok(())
}
fn active_clip_id(clips: &[ClipDef], cursor: &mut usize, command_index: usize) -> Option<usize> {
    if clips
        .get(*cursor)
        .is_some_and(|clip| clip.command_index == command_index)
    {
        let id = *cursor;
        *cursor += 1;
        Some(id)
    } else {
        None
    }
}
fn is_empty_clip(stack: &[bool]) -> Result<bool, SvgError> {
    stack.last().copied().ok_or(SvgError::InvalidList)
}

fn base64_len(n: usize) -> Option<usize> {
    n.checked_add(2)?.checked_div(3)?.checked_mul(4)
}
const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
fn write_base64(out: &mut BoundedString, bytes: &[u8]) -> Result<(), SvgError> {
    let encoded_len = base64_len(bytes.len()).ok_or(SvgError::LimitExceeded)?;
    if encoded_len > out.limit.saturating_sub(out.value.len()) {
        return Err(SvgError::LimitExceeded);
    }
    out.value
        .try_reserve(encoded_len)
        .map_err(|_| SvgError::LimitExceeded)?;
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        out.value.push(BASE64[(a >> 2) as usize] as char);
        out.value
            .push(BASE64[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        out.value.push(if chunk.len() > 1 {
            BASE64[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        out.value.push(if chunk.len() > 2 {
            BASE64[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PageInfo, PositionedGlyph};
    use atelier_geom::{Point, Rect};
    use atelier_xml::{Event, Reader};
    use std::collections::HashMap;

    #[derive(Default)]
    struct Resources {
        glyphs: HashMap<(u64, u32), DisplayPath>,
        images: HashMap<u64, (&'static str, Vec<u8>)>,
    }
    impl SvgResourceProvider for Resources {
        fn glyph_outline(&self, font_id: u64, glyph_id: u32) -> Option<&DisplayPath> {
            self.glyphs.get(&(font_id, glyph_id))
        }
        fn image(&self, image_id: u64) -> Option<SvgImage<'_>> {
            self.images
                .get(&image_id)
                .map(|(mime_type, bytes)| SvgImage { mime_type, bytes })
        }
    }
    fn p(x: f64, y: f64) -> Point {
        Point::try_new(x, y).unwrap()
    }
    fn page() -> PageInfo {
        PageInfo::new(Rect::from_xywh(0.0, 0.0, 100.0, 80.0).unwrap()).unwrap()
    }
    fn glyph_path() -> DisplayPath {
        DisplayPath::new(vec![
            PathElement::MoveTo(p(0.0, 0.0)),
            PathElement::LineTo(p(4.0, 0.0)),
            PathElement::QuadraticTo {
                control: p(5.0, 2.0),
                to: p(4.0, 4.0),
            },
            PathElement::CubicTo {
                control1: p(3.0, 5.0),
                control2: p(1.0, 5.0),
                to: p(0.0, 4.0),
            },
            PathElement::Close,
        ])
        .unwrap()
    }
    fn parse(svg: &str) -> Vec<Event> {
        let mut reader = Reader::new(svg.as_bytes());
        let mut events = Vec::new();
        while let Some(event) = reader.next_event().unwrap() {
            events.push(event);
        }
        events
    }
    fn tag_count(events: &[Event], name: &str) -> usize {
        events
            .iter()
            .filter(|event| matches!(event, Event::StartElement { name: n, .. } if n.local == name))
            .count()
    }
    fn attribute_values(events: &[Event], tag: &str, attribute: &str) -> Vec<String> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::StartElement {
                    name, attributes, ..
                } if name.local == tag => attributes
                    .iter()
                    .find(|item| item.name.local == attribute)
                    .map(|item| item.value.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn glyphs_are_deduplicated_and_output_is_repeatable_and_parseable() {
        let mut resources = Resources::default();
        resources.glyphs.insert((7, 3), glyph_path());
        let list = DisplayList::new(
            page(),
            vec![Command::DrawGlyphRun {
                font_id: 7,
                color: Color::new(0.2, 0.3, 0.4, 0.8).unwrap(),
                glyphs: vec![
                    PositionedGlyph {
                        glyph_id: 3,
                        position: p(4.0, 5.0),
                        advance: p(5.0, 0.0),
                    },
                    PositionedGlyph {
                        glyph_id: 3,
                        position: p(9.0, 5.0),
                        advance: p(5.0, 0.0),
                    },
                ],
            }],
        )
        .unwrap();
        let one = render_svg(&list, &resources, SvgLimits::default()).unwrap();
        assert_eq!(
            one,
            render_svg(&list, &resources, SvgLimits::default()).unwrap()
        );
        let events = parse(&one);
        assert_eq!(tag_count(&events, "path"), 1);
        assert_eq!(tag_count(&events, "use"), 2);
        assert_eq!(attribute_values(&events, "use", "x"), ["4", "9"]);
        assert_eq!(attribute_values(&events, "use", "y"), ["5", "5"]);
    }

    #[test]
    fn base64_images_and_escaped_metadata_are_inert_and_valid_xml() {
        let mut resources = Resources::default();
        resources
            .images
            .insert(2, ("image/png", b"\x89PNG\r\n\x1a\nxyz".to_vec()));
        let target = "a&<\"'😀\t\n\r";
        let role = "r&<\"'😀\t\n\r";
        let list = DisplayList::new(
            page(),
            vec![
                Command::DrawImage {
                    image_id: 2,
                    bounds: Rect::from_xywh(1.0, 2.0, 10.0, 12.0).unwrap(),
                },
                Command::Link {
                    bounds: Rect::from_xywh(1.0, 2.0, 3.0, 4.0).unwrap(),
                    target: target.into(),
                },
                Command::Structure {
                    role: role.into(),
                    node_id: 9,
                },
            ],
        )
        .unwrap();
        let svg = render_svg(&list, &resources, SvgLimits::default()).unwrap();
        assert!(svg.contains("data:image/png;base64,iVBORw0KGgp4eXo="));
        assert!(svg.contains("a&amp;&lt;&quot;&apos;😀&#9;&#10;&#13;"));
        assert!(svg.contains("r&amp;&lt;&quot;&apos;😀&#9;&#10;&#13;"));
        let events = parse(&svg);
        assert_eq!(tag_count(&events, "image"), 1);
        assert_eq!(attribute_values(&events, "image", "x"), ["1"]);
        assert_eq!(attribute_values(&events, "image", "y"), ["2"]);
        assert_eq!(attribute_values(&events, "image", "width"), ["10"]);
        assert_eq!(attribute_values(&events, "image", "height"), ["12"]);
        assert_eq!(tag_count(&events, "link"), 1);
        assert_eq!(tag_count(&events, "structure"), 1);
        for (tag, attribute, expected) in [("link", "target", target), ("structure", "role", role)]
        {
            let actual = events.iter().find_map(|event| match event {
                Event::StartElement {
                    name, attributes, ..
                } if name.local == tag => attributes
                    .iter()
                    .find(|attribute_value| attribute_value.name.local == attribute)
                    .map(|attribute_value| attribute_value.value.as_str()),
                _ => None,
            });
            assert_eq!(actual, Some(expected));
        }
    }

    #[test]
    fn clips_are_outer_to_transform_and_transforms_compose_in_order() {
        let list = DisplayList::new(
            page(),
            vec![
                Command::PushClip {
                    bounds: Rect::from_xywh(0.0, 0.0, 20.0, 20.0).unwrap(),
                },
                Command::PushTransform {
                    transform: DisplayTransform::new(1.0, 0.0, 0.0, 1.0, 5.0, 0.0).unwrap(),
                },
                Command::PushClip {
                    bounds: Rect::from_xywh(10.0, 0.0, 20.0, 20.0).unwrap(),
                },
                Command::PushTransform {
                    transform: DisplayTransform::new(2.0, 0.0, 0.0, 2.0, 0.0, 0.0).unwrap(),
                },
                Command::FillPath {
                    path: DisplayPath::new(vec![
                        PathElement::MoveTo(p(1.0, 1.0)),
                        PathElement::LineTo(p(2.0, 1.0)),
                    ])
                    .unwrap(),
                    color: Color::new(0.0, 0.0, 0.0, 1.0).unwrap(),
                    rule: FillRule::NonZero,
                },
                Command::PopTransform,
                Command::PopClip,
                Command::PopTransform,
                Command::PopClip,
            ],
        )
        .unwrap();
        let svg = render_svg(&list, &Resources::default(), SvgLimits::default()).unwrap();
        assert!(svg.contains("<clipPath id=\"clip-0\""));
        assert!(svg.contains("x=\"10\" y=\"0\" width=\"10\" height=\"20\""));
        assert!(svg.contains("transform=\"matrix(2 0 0 2 10 0)\""));
        assert!(svg.find("clip-path=").unwrap() < svg.find("transform=").unwrap());
    }

    #[test]
    fn empty_nested_clip_does_not_shift_following_clip_ids() {
        let drawing = || Command::FillPath {
            path: DisplayPath::new(vec![
                PathElement::MoveTo(p(1.0, 1.0)),
                PathElement::LineTo(p(2.0, 1.0)),
            ])
            .unwrap(),
            color: Color::new(0.0, 0.0, 0.0, 1.0).unwrap(),
            rule: FillRule::NonZero,
        };
        let outer = Rect::from_xywh(1.0, 2.0, 30.0, 40.0).unwrap();
        let list = DisplayList::new(
            page(),
            vec![
                Command::PushClip { bounds: outer },
                drawing(),
                Command::PushClip {
                    bounds: Rect::from_xywh(50.0, 50.0, 10.0, 10.0).unwrap(),
                },
                drawing(),
                Command::PopClip,
                drawing(),
                Command::PopClip,
            ],
        )
        .unwrap();
        let svg = render_svg(&list, &Resources::default(), SvgLimits::default()).unwrap();
        let events = parse(&svg);
        assert_eq!(
            attribute_values(&events, "clipPath", "id"),
            ["clip-0", "clip-1"]
        );
        assert_eq!(attribute_values(&events, "rect", "x"), ["1", "1"]);
        assert_eq!(attribute_values(&events, "rect", "y"), ["2", "2"]);
        assert_eq!(
            attribute_values(&events, "g", "clip-path"),
            ["url(#clip-0)", "url(#clip-1)"]
        );
        assert_eq!(tag_count(&events, "path"), 2);
    }

    #[test]
    fn malformed_resources_and_budgets_return_typed_errors() {
        let image_list = DisplayList::new(
            page(),
            vec![Command::DrawImage {
                image_id: 1,
                bounds: Rect::from_xywh(0.0, 0.0, 2.0, 2.0).unwrap(),
            }],
        )
        .unwrap();
        assert_eq!(
            render_svg(&image_list, &Resources::default(), SvgLimits::default()),
            Err(SvgError::MissingResource)
        );
        let mut unknown = Resources::default();
        unknown
            .images
            .insert(1, ("image/svg+xml", b"<svg/>".to_vec()));
        assert_eq!(
            render_svg(&image_list, &unknown, SvgLimits::default()),
            Err(SvgError::InvalidMime)
        );
        unknown.images.insert(1, ("image/png", b"not png".to_vec()));
        assert_eq!(
            render_svg(&image_list, &unknown, SvgLimits::default()),
            Err(SvgError::InvalidBytes)
        );
        let mut valid = Resources::default();
        valid
            .images
            .insert(1, ("image/png", b"\x89PNG\r\n\x1a\n".to_vec()));
        let limits = SvgLimits {
            max_image_bytes: 7,
            ..SvgLimits::default()
        };
        assert_eq!(
            render_svg(&image_list, &valid, limits),
            Err(SvgError::LimitExceeded)
        );
        let limits = SvgLimits {
            max_output_bytes: 16,
            ..SvgLimits::default()
        };
        assert_eq!(
            render_svg(&image_list, &valid, limits),
            Err(SvgError::LimitExceeded)
        );
    }

    #[test]
    fn glyph_outline_is_revalidated_and_path_budget_checked_before_validation() {
        let list = DisplayList::new(
            page(),
            vec![Command::DrawGlyphRun {
                font_id: 1,
                color: Color::new(0.0, 0.0, 0.0, 1.0).unwrap(),
                glyphs: vec![PositionedGlyph {
                    glyph_id: 1,
                    position: p(0.0, 0.0),
                    advance: p(1.0, 0.0),
                }],
            }],
        )
        .unwrap();
        let mut resources = Resources::default();
        resources.glyphs.insert(
            (1, 1),
            DisplayPath {
                elements: vec![PathElement::LineTo(p(1.0, 1.0))],
            },
        );
        assert_eq!(
            render_svg(&list, &resources, SvgLimits::default()),
            Err(SvgError::InvalidList)
        );
        resources.glyphs.insert((1, 1), glyph_path());
        let limits = SvgLimits {
            max_resolved_path_elements: 1,
            ..SvgLimits::default()
        };
        assert_eq!(
            render_svg(&list, &resources, limits),
            Err(SvgError::LimitExceeded)
        );
    }
}
