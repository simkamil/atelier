// SPDX-License-Identifier: AGPL-3.0-or-later
//! Inline pictures in DOCX (US-270): writing and reading `w:drawing`/`wp:inline`.
//!
//! The rich model holds a picture as an `AnchoredObject` of kind
//! [`IMAGE_OBJECT_KIND`] (see `atelier_text_model::inline_image`). The writer
//! turns each one into a `w:drawing` with an inline picture whose extent is the
//! natural size at 96 dpi reduced to the text area, stores the original bytes
//! in `word/media/imageN.<ext>` (same bytes for identical pictures) and links
//! it with a relationship of the main part. The reader finds those drawings in
//! an imported document and returns the media with its drawn size.

use crate::docx::{ImportedDocx, NewDocument};
use crate::{DocxWriteOptions, OpcError, Package, PartUri, TargetMode, WriteError, WrittenDocx};
use atelier_text_model::inline_image::{
    IMAGE_OBJECT_KIND, ImageFormat, InlineImage, fit_within, natural_size_points,
};
use atelier_text_model::rich::{AnchoredObject, Inline, NodeId, RichOp};

const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const REL_OFFICE_DOCUMENT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const WP_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const PIC_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
/// English Metric Units per point.
pub const EMU_PER_POINT: f64 = 12_700.0;
/// Most pictures one document may carry (matches the editor store).
pub const MAX_PICTURES: usize = 4096;

/// The `w:drawing` element of an inline picture of `width_emu` x `height_emu`.
pub fn drawing_xml(
    relationship_id: &str,
    drawing_id: u32,
    file_name: &str,
    width_emu: u64,
    height_emu: u64,
) -> Vec<u8> {
    format!(
        "<w:drawing><wp:inline xmlns:wp=\"{WP_NS}\" distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
<wp:extent cx=\"{width_emu}\" cy=\"{height_emu}\"/>\
<wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/>\
<wp:docPr id=\"{drawing_id}\" name=\"Image {drawing_id}\" descr=\"\"/>\
<wp:cNvGraphicFramePr><a:graphicFrameLocks xmlns:a=\"{A_NS}\" noChangeAspect=\"1\"/></wp:cNvGraphicFramePr>\
<a:graphic xmlns:a=\"{A_NS}\"><a:graphicData uri=\"{PIC_NS}\">\
<pic:pic xmlns:pic=\"{PIC_NS}\"><pic:nvPicPr><pic:cNvPr id=\"{drawing_id}\" name=\"{file_name}\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"{relationship_id}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{width_emu}\" cy=\"{height_emu}\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>\
</a:graphicData></a:graphic></wp:inline></w:drawing>"
    )
    .into_bytes()
}

fn invalid(detail: &'static str) -> WriteError {
    WriteError::Invalid {
        part: "word/document.xml".into(),
        detail,
    }
}

struct Media {
    image: InlineImage,
    uri: String,
    relationship: String,
}

/// Like [`crate::write_new_docx`], also writing every picture object of the
/// document (kind [`IMAGE_OBJECT_KIND`]) as an inline `w:drawing`.
pub fn write_new_docx_with_pictures(
    input: &NewDocument<'_>,
    options: &DocxWriteOptions,
) -> Result<WrittenDocx, WriteError> {
    let mut found: Vec<(NodeId, AnchoredObject)> = Vec::new();
    for para in input.document.paragraph_ids() {
        let Ok(paragraph) = input.document.paragraph(para) else {
            continue;
        };
        for object in paragraph
            .anchored
            .iter()
            .filter(|a| a.kind == IMAGE_OBJECT_KIND)
        {
            found.push((para, object.clone()));
        }
    }
    if found.is_empty() {
        return crate::write_new_docx(input, options);
    }
    if found.len() > MAX_PICTURES {
        return Err(WriteError::Limit("too many pictures"));
    }
    let text_width_pt = f64::from(
        input
            .page
            .width
            .saturating_sub(input.page.margin_left)
            .saturating_sub(input.page.margin_right)
            .max(1000),
    ) / 20.0;
    let text_height_pt = f64::from(
        input
            .page
            .height
            .saturating_sub(input.page.margin_top)
            .saturating_sub(input.page.margin_bottom)
            .max(1000),
    ) / 20.0;
    let mut media: Vec<Media> = Vec::new();
    let mut ops = Vec::new();
    for (index, (para, object)) in found.iter().enumerate() {
        let image = InlineImage::from_object_data(&object.data)
            .map_err(|_| invalid("picture object data is not a valid image"))?;
        let slot = match media
            .iter()
            .position(|m| m.image.format == image.format && m.image.bytes[..] == image.bytes[..])
        {
            Some(slot) => slot,
            None => {
                let n = media.len() + 1;
                media.push(Media {
                    uri: format!("word/media/image{n}.{}", image.format.extension()),
                    relationship: format!("rIdImg{n}"),
                    image: image.clone(),
                });
                media.len() - 1
            }
        };
        let (w, h) = natural_size_points(image.width, image.height, image.orientation);
        let (w, h) = fit_within(w, h, text_width_pt, text_height_pt)
            .ok_or_else(|| invalid("picture has no drawable size"))?;
        let emu = |points: f64| (points * EMU_PER_POINT).round().max(1.0) as u64;
        let entry = &media[slot];
        let file_name = entry.uri.rsplit('/').next().unwrap_or("image");
        let drawing_id = u32::try_from(index + 1).map_err(|_| invalid("too many pictures"))?;
        ops.push(RichOp::ModifyInline {
            para: *para,
            item: Inline::Anchored(AnchoredObject {
                id: object.id,
                offset: object.offset,
                kind: "w:drawing".into(),
                data: drawing_xml(&entry.relationship, drawing_id, file_name, emu(w), emu(h)),
            }),
        });
    }
    let mut document = input.document.clone();
    let transaction = atelier_model::Transaction::new(ops)
        .map_err(|_| invalid("picture replacement is not a valid transaction"))?;
    document
        .apply(&transaction)
        .map_err(|_| invalid("picture replacement failed"))?;
    let rewritten = NewDocument {
        document: &document,
        styles: input.styles,
        style_names: input.style_names,
        next_styles: input.next_styles,
        default_style: input.default_style,
        page: input.page,
        default_tab_stop: input.default_tab_stop,
    };
    let mut written = crate::write_new_docx(&rewritten, options)?;
    let mut package = Package::read(&written.bytes)?;
    let main = PartUri::parse("word/document.xml")?;
    for entry in &media {
        let uri = PartUri::parse(&entry.uri)?;
        package.insert_part(uri, entry.image.format.mime(), entry.image.bytes.to_vec())?;
        package.add_relationship(
            Some(main.clone()),
            entry.relationship.clone(),
            REL_IMAGE,
            entry.uri.trim_start_matches("word/"),
            TargetMode::Internal,
        )?;
        written.regenerated_parts.push(entry.uri.clone());
    }
    written.bytes = package.write().map_err(|e: OpcError| WriteError::Opc(e))?;
    Ok(written)
}

/// A picture found in an imported document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedPicture {
    pub paragraph: NodeId,
    /// UTF-8 offset of the picture in the paragraph text.
    pub offset: usize,
    /// Drawn size from `wp:extent`.
    pub width_emu: u64,
    pub height_emu: u64,
    pub format: ImageFormat,
    /// The media part, byte for byte as stored.
    pub bytes: Vec<u8>,
    /// Name of the media part.
    pub part: String,
}

/// Maximum size of one drawing element examined.
const MAX_DRAWING_BYTES: usize = 64 * 1024;

/// The value of attribute `attr` (compared by local name) on the first element
/// whose local name is `element`, found by a bounded scan of `xml`.
fn element_attr(xml: &str, element: &str, attr: &str) -> Option<String> {
    let bytes = xml.as_bytes();
    let mut at = 0;
    while let Some(found) = xml[at..].find('<') {
        let start = at + found + 1;
        let tag_end = xml[start..].find('>').map(|e| start + e)?;
        let tag = &xml[start..tag_end];
        let name_end = tag
            .find(|c: char| c.is_ascii_whitespace() || c == '/')
            .unwrap_or(tag.len());
        let name = &tag[..name_end];
        if !name.starts_with('/') && name.rsplit(':').next() == Some(element) {
            let mut rest = &tag[name_end..];
            while let Some(eq) = rest.find('=') {
                let key = rest[..eq].trim().rsplit(':').next().unwrap_or("");
                let after = rest[eq + 1..].trim_start();
                let quote = after.chars().next()?;
                if quote != '"' && quote != '\'' {
                    return None;
                }
                let value_end = after[1..].find(quote)?;
                let value = &after[1..1 + value_end];
                if key == attr {
                    return Some(value.to_owned());
                }
                rest = &after[value_end + 2..];
            }
        }
        at = tag_end + 1;
        if at >= bytes.len() {
            break;
        }
    }
    None
}

/// Every picture of `imported` that is an inline or anchored `w:drawing`
/// holding a PNG, JPEG, GIF or BMP media part.
pub fn read_pictures(imported: &ImportedDocx) -> Vec<ImportedPicture> {
    let package = &imported.package;
    let Some(main) = package
        .relationships(None)
        .iter()
        .find(|r| r.rel_type == REL_OFFICE_DOCUMENT)
        .and_then(|r| r.resolved_target.clone())
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for para in imported.document.paragraph_ids() {
        let Ok(paragraph) = imported.document.paragraph(para) else {
            continue;
        };
        for object in &paragraph.anchored {
            if object.kind.rsplit(':').next() != Some("drawing")
                || object.data.len() > MAX_DRAWING_BYTES
            {
                continue;
            }
            let Ok(xml) = core::str::from_utf8(&object.data) else {
                continue;
            };
            let Some(embed) = element_attr(xml, "blip", "embed") else {
                continue;
            };
            let parse = |attr: &str| {
                element_attr(xml, "extent", attr)
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0)
            };
            let Some(target) = package
                .relationships(Some(&main))
                .iter()
                .find(|r| r.id == embed && r.rel_type == REL_IMAGE)
                .and_then(|r| r.resolved_target.clone())
            else {
                continue;
            };
            let Some(part) = package.part(&target) else {
                continue;
            };
            let Ok(format) = ImageFormat::sniff(&part.bytes) else {
                continue;
            };
            out.push(ImportedPicture {
                paragraph: para,
                offset: object.offset,
                width_emu: parse("cx"),
                height_emu: parse("cy"),
                format,
                bytes: part.bytes.clone(),
                part: target.to_string(),
            });
        }
    }
    out
}
