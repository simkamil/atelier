// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-270: inline pictures in DOCX. The writer stores the original bytes in
//! `word/media`, links them and emits `w:drawing`/`wp:inline` with the drawn
//! size in EMU; the Atelier import finds them again.
mod support;

use atelier_fmt_ooxml::docx_image::{read_pictures, write_new_docx_with_pictures};
use atelier_fmt_ooxml::{DocxWriteOptions, NewDocument, Package, PartUri, WriteError, import_docx};
use atelier_model::Transaction;
use atelier_text_model::inline_image::{IMAGE_OBJECT_KIND, ImageFormat, InlineImage};
use atelier_text_model::rich::{AnchoredObject, Inline, RichOp, default_rich_document};
use std::sync::Arc;
use support::order::order_problems;
use support::parse;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR-original-bytes-are-kept";
const GIF: &[u8] = b"GIF89a-another-picture";

fn image(format: ImageFormat, bytes: &[u8], w: u32, h: u32, orientation: u8) -> InlineImage {
    InlineImage::new(format, Arc::from(bytes), w, h, orientation).unwrap()
}

/// A document with `texts` as paragraphs; `pictures` are `(paragraph index, offset, image)`.
fn build(
    texts: &[&str],
    pictures: &[(usize, usize, InlineImage)],
) -> atelier_text_model::rich::DefaultRichDocument {
    let mut d = default_rich_document();
    let section = d.document.section_ids()[0];
    let first = d.document.paragraph_ids()[0];
    let mut ops = vec![RichOp::InsertText {
        para: first,
        offset: 0,
        text: texts[0].into(),
    }];
    let mut ids = vec![first];
    for (n, text) in texts.iter().enumerate().skip(1) {
        let sub = d.document.new_paragraph(text).unwrap();
        ids.push(sub.root);
        ops.push(RichOp::InsertBlock {
            parent: Some(section),
            position: n,
            subtree: sub,
        });
    }
    for (para, offset, picture) in pictures {
        ops.push(RichOp::InsertInline {
            para: ids[*para],
            item: Inline::Anchored(AnchoredObject {
                id: d.document.alloc_id().unwrap(),
                offset: *offset,
                kind: IMAGE_OBJECT_KIND.into(),
                data: picture.to_object_data(),
            }),
        });
    }
    d.document.apply(&Transaction::new(ops).unwrap()).unwrap();
    d
}

fn document_xml(package: &Package) -> String {
    let part = package
        .part(&PartUri::parse("word/document.xml").unwrap())
        .unwrap();
    String::from_utf8(part.bytes.clone()).unwrap()
}

#[test]
fn writes_media_relationship_and_inline_drawing_with_exact_emu() {
    // 400x200 px = 300x150 pt = 3 810 000 x 1 905 000 EMU.
    let d = build(
        &["ab", "cd"],
        &[(0, 1, image(ImageFormat::Png, PNG, 400, 200, 1))],
    );
    let written =
        write_new_docx_with_pictures(&NewDocument::from_default(&d), &DocxWriteOptions::default())
            .unwrap();
    let package = Package::read(&written.bytes).unwrap();
    let media = package
        .part(&PartUri::parse("word/media/image1.png").unwrap())
        .expect("media part");
    assert_eq!(media.bytes, PNG, "original bytes kept");
    assert_eq!(media.content_type, "image/png");
    let main = PartUri::parse("word/document.xml").unwrap();
    let rel = package
        .relationships(Some(&main))
        .iter()
        .find(|r| r.id == "rIdImg1")
        .expect("relationship");
    assert!(rel.rel_type.ends_with("/relationships/image"));
    assert_eq!(rel.target, "media/image1.png");
    let xml = document_xml(&package);
    assert!(
        xml.contains("<wp:extent cx=\"3810000\" cy=\"1905000\"/>"),
        "{xml}"
    );
    assert!(xml.contains("r:embed=\"rIdImg1\""));
    assert!(xml.contains("<a:ext cx=\"3810000\" cy=\"1905000\"/>"));
    // The drawing sits in a run between "a" and "b".
    let a = xml.find("<w:t>a</w:t>").unwrap();
    let drawing = xml.find("<w:drawing>").unwrap();
    let b = xml.find("<w:t>b</w:t>").unwrap();
    assert!(a < drawing && drawing < b, "{xml}");
    assert!(order_problems(&parse(xml.as_bytes())).is_empty());
    assert!(
        written
            .regenerated_parts
            .iter()
            .any(|p| p == "word/media/image1.png")
    );
}

#[test]
fn a_picture_wider_than_the_text_area_is_written_reduced_proportionally() {
    // 2000x1000 px = 1500x750 pt; the A4 text area is 453.55 pt wide.
    let d = build(
        &["x"],
        &[(
            0,
            1,
            image(ImageFormat::Jpeg, b"\xff\xd8\xff\xe0jpeg", 2000, 1000, 1),
        )],
    );
    let written =
        write_new_docx_with_pictures(&NewDocument::from_default(&d), &DocxWriteOptions::default())
            .unwrap();
    let xml = document_xml(&Package::read(&written.bytes).unwrap());
    // The default page is 11905 twips wide with 1417 margins: 9071 twips =
    // 453.55 pt = 5 760 085 EMU; half of it (rounded) for the height.
    assert!(
        xml.contains("<wp:extent cx=\"5760085\" cy=\"2880043\"/>"),
        "{xml}"
    );
}

#[test]
fn exif_orientation_swaps_the_written_extent_and_keeps_the_original_bytes() {
    // Stored 800x600 px, orientation 6 -> shown 600x800 px = 450x600 pt.
    let jpeg = b"\xff\xd8\xff\xe1exif-bytes-stay";
    let d = build(
        &["x"],
        &[(0, 1, image(ImageFormat::Jpeg, jpeg, 800, 600, 6))],
    );
    let written =
        write_new_docx_with_pictures(&NewDocument::from_default(&d), &DocxWriteOptions::default())
            .unwrap();
    let package = Package::read(&written.bytes).unwrap();
    assert!(document_xml(&package).contains("<wp:extent cx=\"5715000\" cy=\"7620000\"/>"));
    assert_eq!(
        package
            .part(&PartUri::parse("word/media/image1.jpeg").unwrap())
            .unwrap()
            .bytes,
        jpeg
    );
}

#[test]
fn identical_pictures_share_one_media_part_and_distinct_ones_get_their_own() {
    let a = image(ImageFormat::Png, PNG, 40, 40, 1);
    let g = image(ImageFormat::Gif, GIF, 40, 40, 1);
    let d = build(&["ab"], &[(0, 0, a.clone()), (0, 1, a), (0, 2, g)]);
    let written =
        write_new_docx_with_pictures(&NewDocument::from_default(&d), &DocxWriteOptions::default())
            .unwrap();
    let package = Package::read(&written.bytes).unwrap();
    let media: Vec<_> = package
        .parts()
        .keys()
        .filter(|u| u.as_str().starts_with("word/media/"))
        .map(|u| u.as_str().to_owned())
        .collect();
    assert_eq!(media, ["word/media/image1.png", "word/media/image2.gif"]);
    let xml = document_xml(&package);
    assert_eq!(xml.matches("<w:drawing>").count(), 3);
    assert_eq!(xml.matches("r:embed=\"rIdImg1\"").count(), 2);
    assert_eq!(xml.matches("r:embed=\"rIdImg2\"").count(), 1);
    // Distinct drawing ids.
    for id in 1..=3 {
        assert!(xml.contains(&format!("<wp:docPr id=\"{id}\"")), "id {id}");
    }
}

#[test]
fn the_atelier_import_finds_the_pictures_again_with_bytes_size_and_position() {
    let a = image(ImageFormat::Png, PNG, 400, 200, 1);
    let g = image(ImageFormat::Gif, GIF, 100, 100, 1);
    let d = build(&["ab", "cd"], &[(0, 1, a), (1, 2, g)]);
    let written =
        write_new_docx_with_pictures(&NewDocument::from_default(&d), &DocxWriteOptions::default())
            .unwrap();
    let imported = import_docx(&written.bytes).unwrap();
    let pictures = read_pictures(&imported);
    assert_eq!(pictures.len(), 2);
    assert_eq!(pictures[0].format, ImageFormat::Png);
    assert_eq!(pictures[0].bytes, PNG);
    assert_eq!(
        (pictures[0].width_emu, pictures[0].height_emu),
        (3_810_000, 1_905_000)
    );
    assert_eq!(pictures[0].offset, 1);
    assert_eq!(pictures[0].part, "word/media/image1.png");
    assert_eq!(pictures[1].format, ImageFormat::Gif);
    assert_eq!(pictures[1].bytes, GIF);
    assert_eq!(pictures[1].offset, 2);
    // 100 px = 75 pt = 952 500 EMU.
    assert_eq!(
        (pictures[1].width_emu, pictures[1].height_emu),
        (952_500, 952_500)
    );
    assert_ne!(pictures[0].paragraph, pictures[1].paragraph);
    // The text is untouched.
    let texts: Vec<String> = imported
        .document
        .paragraph_ids()
        .iter()
        .map(|p| imported.document.paragraph(*p).unwrap().text.clone())
        .collect();
    assert_eq!(texts, ["ab", "cd"]);
}

#[test]
fn a_document_without_pictures_is_written_exactly_as_before() {
    let d = build(&["plain"], &[]);
    let options = DocxWriteOptions::default();
    let with = write_new_docx_with_pictures(&NewDocument::from_default(&d), &options).unwrap();
    let without =
        atelier_fmt_ooxml::write_new_docx(&NewDocument::from_default(&d), &options).unwrap();
    assert_eq!(with.bytes, without.bytes);
}

#[test]
fn a_malformed_picture_object_is_a_typed_error_not_a_panic() {
    let mut d = build(&["x"], &[]);
    let para = d.document.paragraph_ids()[0];
    let id = d.document.alloc_id().unwrap();
    d.document
        .apply(
            &Transaction::new(vec![RichOp::InsertInline {
                para,
                item: Inline::Anchored(AnchoredObject {
                    id,
                    offset: 0,
                    kind: IMAGE_OBJECT_KIND.into(),
                    data: vec![1, 2, 3],
                }),
            }])
            .unwrap(),
        )
        .unwrap();
    let result =
        write_new_docx_with_pictures(&NewDocument::from_default(&d), &DocxWriteOptions::default());
    assert!(matches!(result, Err(WriteError::Invalid { .. })));
}

#[test]
fn foreign_drawings_without_a_supported_media_part_are_skipped_by_the_reader() {
    // A real Word-style drawing pointing at an EMF is ignored; no panic.
    let d = build(&["x"], &[(0, 1, image(ImageFormat::Png, PNG, 8, 8, 1))]);
    let written =
        write_new_docx_with_pictures(&NewDocument::from_default(&d), &DocxWriteOptions::default())
            .unwrap();
    let mut package = Package::read(&written.bytes).unwrap();
    // Replace the media bytes by something that is not an image.
    let uri = PartUri::parse("word/media/image1.png").unwrap();
    let ct = package.part(&uri).unwrap().content_type.clone();
    let mut copy = Package::new();
    for (u, p) in package.parts() {
        let bytes = if *u == uri {
            b"not an image".to_vec()
        } else {
            p.bytes.clone()
        };
        copy.insert_part(u.clone(), p.content_type.clone(), bytes)
            .unwrap();
    }
    for source in [None, Some(PartUri::parse("word/document.xml").unwrap())] {
        for r in package.relationships(source.as_ref()) {
            copy.add_relationship(
                source.clone(),
                r.id.clone(),
                r.rel_type.clone(),
                r.target.clone(),
                r.mode,
            )
            .unwrap();
        }
    }
    let _ = (&mut package, ct);
    let imported = import_docx(&copy.write().unwrap()).unwrap();
    assert!(read_pictures(&imported).is_empty());
}

#[test]
fn alternative_text_is_written_escaped_and_read_back_and_never_left_empty() {
    let described =
        image(ImageFormat::Png, PNG, 40, 40, 1).with_alt("Plan <A&B> \"nord\" d'\u{e9}t\u{e9}");
    let plain = image(ImageFormat::Gif, GIF, 40, 40, 1);
    let d = build(&["ab"], &[(0, 0, described), (0, 1, plain)]);
    let written =
        write_new_docx_with_pictures(&NewDocument::from_default(&d), &DocxWriteOptions::default())
            .unwrap();
    let xml = document_xml(&Package::read(&written.bytes).unwrap());
    assert!(
        xml.contains("descr=\"Plan &lt;A&amp;B&gt; &quot;nord&quot; d&apos;\u{e9}t\u{e9}\""),
        "{xml}"
    );
    assert!(
        xml.contains("descr=\"Image 2\""),
        "default text is never empty"
    );
    assert!(!xml.contains("descr=\"\""));
    let pictures = read_pictures(&import_docx(&written.bytes).unwrap());
    assert_eq!(pictures[0].alt, "Plan <A&B> \"nord\" d'\u{e9}t\u{e9}");
    assert_eq!(pictures[1].alt, "Image 2");
}
