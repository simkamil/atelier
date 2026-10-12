// SPDX-License-Identifier: AGPL-3.0-or-later
//! The registry and the targets for the analyzers that read external data.
//!
//! To add a target: write `fn run(input: &[u8]) -> Outcome` that calls the
//! analyzer with its default limits, optionally `fn seeds() -> Vec<Vec<u8>>`
//! and a `&[&[u8]]` dictionary, then add one `FnTarget::new(...)` line to
//! [`registry`]. `core-wasm` and `desktop-win` are deliberately absent.

use crate::target::{FnTarget, FuzzTarget, Outcome};
use atelier_cfb::{Compound as CfbCompound, Limits as CfbLimits};
use atelier_compress::{Format, InflateState, Level, Status, compress_with_format};
use atelier_fmt_legacy::doc::import_doc;
use atelier_fmt_odf::odt::import_odt;
use atelier_fmt_ooxml::{Package, PartUri, TargetMode, import_docx};
use atelier_font::cff::CffFont;
use atelier_font::subset::{subset_cff, subset_font};
use atelier_font::{FontFace, GlyphCluster, OutlineLimits};
use atelier_geom::{Point, Rect};
use atelier_image::{
    DecodeLimits, Image, decode_bmp, decode_gif, decode_jpeg_with_metadata, decode_png,
    encode_png_rgba,
};
use atelier_model::journal::{
    Journal, JournalLimits, RecoveryId, SnapshotCodec, replay, snapshot_state,
};
use atelier_model::{IndexedTree, IndexedTreeOperation as TreeOp, Transaction};
use atelier_render::{
    Color, Command, DisplayList, DisplayPath, FillRule, LineCap, LineJoin, PageInfo, PathElement,
    StrokeStyle,
};
use atelier_unicode::{Encoding, decode, detect_encoding, encode};
use atelier_xml::{Event, Reader};
use atelier_zip::{Archive, Method, WriteOptions, Writer};
use std::io::Read;
use std::sync::Arc;

/// Every target, in the order shown by `list`.
pub fn registry() -> Vec<Arc<dyn FuzzTarget>> {
    vec![
        Arc::new(
            FnTarget::new("inflate", inflate)
                .dictionary(INFLATE_TOKENS)
                .seeds(inflate_seeds),
        ),
        Arc::new(
            FnTarget::new("zlib", zlib)
                .dictionary(ZLIB_TOKENS)
                .seeds(zlib_seeds),
        ),
        Arc::new(
            FnTarget::new("zip", zip)
                .dictionary(ZIP_TOKENS)
                .seeds(zip_seeds),
        ),
        Arc::new(
            FnTarget::new("xml", xml)
                .dictionary(XML_TOKENS)
                .seeds(xml_seeds),
        ),
        Arc::new(
            FnTarget::new("png", png)
                .dictionary(PNG_TOKENS)
                .seeds(png_seeds),
        ),
        Arc::new(
            FnTarget::new("gif", gif)
                .dictionary(GIF_TOKENS)
                .seeds(gif_seeds),
        ),
        Arc::new(
            FnTarget::new("bmp", bmp)
                .dictionary(BMP_TOKENS)
                .seeds(bmp_seeds),
        ),
        Arc::new(
            FnTarget::new("jpeg", jpeg)
                .dictionary(JPEG_TOKENS)
                .seeds(jpeg_seeds),
        ),
        Arc::new(
            FnTarget::new("opentype", opentype)
                .dictionary(SFNT_TOKENS)
                .seeds(font_seeds),
        ),
        Arc::new(
            FnTarget::new("truetype", truetype)
                .dictionary(SFNT_TOKENS)
                .seeds(truetype_seeds),
        ),
        Arc::new(
            FnTarget::new("cff", cff)
                .dictionary(CFF_TOKENS)
                .seeds(cff_seeds),
        ),
        Arc::new(
            FnTarget::new("opc", opc)
                .dictionary(OPC_TOKENS)
                .seeds(opc_seeds),
        ),
        Arc::new(
            FnTarget::new("docx-import", docx_import_target)
                .dictionary(DOCX_TOKENS)
                .seeds(docx_seeds),
        ),
        Arc::new(
            FnTarget::new("cfb", cfb)
                .dictionary(CFB_TOKENS)
                .seeds(cfb_seeds),
        ),
        Arc::new(
            FnTarget::new("doc-import", doc_import_target)
                .dictionary(CFB_TOKENS)
                .seeds(doc_seeds),
        ),
        Arc::new(
            FnTarget::new("odt-import", odt_import_target)
                .dictionary(ODT_TOKENS)
                .seeds(odt_seeds),
        ),
        Arc::new(
            FnTarget::new("subset-font", subset_font_target)
                .dictionary(SFNT_TOKENS)
                .seeds(font_seeds),
        ),
        Arc::new(
            FnTarget::new("subset-cff", subset_cff_target)
                .dictionary(CFF_TOKENS)
                .seeds(cff_seeds),
        ),
        Arc::new(
            FnTarget::new("encodings", encodings)
                .dictionary(ENCODING_TOKENS)
                .seeds(encoding_seeds),
        ),
        Arc::new(FnTarget::new("display-list", display_list).seeds(display_list_seeds)),
        Arc::new(
            FnTarget::new("recovery-journal", recovery_journal)
                .dictionary(JOURNAL_TOKENS)
                .seeds(journal_seeds),
        ),
    ]
}

pub fn find(name: &str) -> Option<Arc<dyn FuzzTarget>> {
    registry().into_iter().find(|t| t.name() == name)
}

// ---------------------------------------------------------------- inflate

/// Output budget handed to the decoder (it has no default of its own).
const INFLATE_MAX_OUTPUT: u64 = 16 * 1024 * 1024;

fn run_inflate(format: Format, input: &[u8]) -> Outcome {
    let mut state = InflateState::new(format, INFLATE_MAX_OUTPUT);
    let mut buffer = vec![0u8; 64 * 1024];
    let (mut at, mut total) = (0usize, 0u64);
    loop {
        match state.push(&input[at..], &mut buffer, true) {
            Ok(progress) => {
                at += progress.consumed;
                total += progress.produced as u64;
                match progress.status {
                    Status::Done => return Outcome::accepted(total),
                    Status::NeedInput if progress.consumed + progress.produced == 0 => {
                        return Outcome::labelled("need-input", total);
                    }
                    _ => assert!(
                        progress.consumed + progress.produced > 0,
                        "inflate made no progress with status {:?}",
                        progress.status
                    ),
                }
            }
            Err(error) => return Outcome::rejected_with(&error, total),
        }
    }
}

fn inflate(input: &[u8]) -> Outcome {
    run_inflate(Format::Deflate, input)
}

fn zlib(input: &[u8]) -> Outcome {
    run_inflate(Format::Zlib, input)
}

const INFLATE_TOKENS: &[&[u8]] = &[
    &[0x01, 0x00, 0x00, 0xFF, 0xFF],
    &[0x03, 0x00],
    &[0xED, 0xFD],
    &[0xFF, 0xFF, 0xFF, 0xFF],
];
const ZLIB_TOKENS: &[&[u8]] = &[&[0x78, 0x01], &[0x78, 0x9C], &[0x78, 0xDA], &[0x78, 0x20]];

const SAMPLE_TEXT: &[u8] = b"Atelier fuzz seed: the quick brown fox jumps over the lazy dog. \
the quick brown fox jumps over the lazy dog again, and again, and again.";

fn deflate(data: &[u8], format: Format) -> Vec<u8> {
    let mut out = vec![0u8; data.len() * 2 + 1024];
    let n = compress_with_format(data, format, Level::Normal, &mut out).expect("sized output");
    out.truncate(n);
    out
}

fn inflate_seeds() -> Vec<Vec<u8>> {
    vec![
        include_bytes!("../../../crates/compress/tests/vectors/dynamic.bin").to_vec(),
        include_bytes!("../../../crates/compress/tests/vectors/fixed.bin").to_vec(),
        include_bytes!("../../../crates/compress/tests/vectors/stored.bin").to_vec(),
        include_bytes!("../../../crates/compress/tests/vectors/empty-deflate.bin").to_vec(),
        include_bytes!("../../../crates/compress/tests/vectors/distance-before-history.bin")
            .to_vec(),
        include_bytes!("../../../crates/compress/tests/vectors/incomplete-code-tree.bin").to_vec(),
        include_bytes!("../../../crates/compress/tests/vectors/incomplete-literal-tree.bin")
            .to_vec(),
        deflate(SAMPLE_TEXT, Format::Deflate),
    ]
}

fn zlib_seeds() -> Vec<Vec<u8>> {
    vec![
        include_bytes!("../../../crates/compress/tests/vectors/zlib.bin").to_vec(),
        include_bytes!("../../../crates/compress/tests/vectors/empty.bin").to_vec(),
        deflate(SAMPLE_TEXT, Format::Zlib),
        deflate(&[0xAB; 300], Format::Zlib),
    ]
}

// -------------------------------------------------------------------- zip

fn zip(input: &[u8]) -> Outcome {
    let archive = match Archive::open(input, atelier_zip::Limits::default()) {
        Ok(archive) => archive,
        Err(error) => return Outcome::rejected(&error),
    };
    let (mut total, mut failed) = (0u64, 0u32);
    for entry in archive.entries().iter().take(64) {
        let _ = (
            entry.name().len(),
            entry.method(),
            entry.crc32(),
            entry.compressed_size(),
            entry.uncompressed_size(),
            entry.local_header_offset(),
            entry.is_name_safe(),
            entry.is_duplicate(),
        );
        match entry.read_all() {
            Ok(bytes) => total += bytes.len() as u64,
            Err(_) => failed += 1,
        }
    }
    let _ = archive.entry("[Content_Types].xml");
    Outcome::labelled(&format!("opened-failed-{}", failed.min(3)), total)
}

const ZIP_TOKENS: &[&[u8]] = &[
    b"PK\x03\x04",
    b"PK\x01\x02",
    b"PK\x05\x06",
    b"PK\x06\x06",
    b"PK\x06\x07",
    b"PK\x07\x08",
    &[0xFF, 0xFF, 0xFF, 0xFF],
];

fn zip_archive(files: &[(&str, &[u8], Method)], zip64: bool) -> Vec<u8> {
    let mut writer = Writer::new(WriteOptions {
        force_zip64: zip64,
        ..WriteOptions::default()
    });
    for (name, bytes, method) in files {
        writer.add(name, bytes, *method).expect("valid seed entry");
    }
    writer.finish().expect("valid seed archive")
}

fn zip_seeds() -> Vec<Vec<u8>> {
    vec![
        zip_archive(&[("a.txt", SAMPLE_TEXT, Method::Deflate)], false),
        zip_archive(
            &[
                ("dir/b.bin", &[7u8; 64], Method::Store),
                ("dir/c.txt", SAMPLE_TEXT, Method::Deflate),
            ],
            false,
        ),
        zip_archive(&[("z.txt", b"zip64", Method::Store)], true),
        zip_archive(&[], false),
    ]
}

// -------------------------------------------------------------------- xml

struct Drained {
    events: u64,
    text: u64,
    error: Option<atelier_xml::ErrorKind>,
}

fn drain<R: Read>(mut reader: Reader<R>, capture: bool) -> Drained {
    let mut drained = Drained {
        events: 0,
        text: 0,
        error: None,
    };
    let mut captured = !capture;
    loop {
        match reader.next_event() {
            Ok(Some(event)) => {
                drained.events += 1;
                match &event {
                    Event::Text(text) | Event::Comment(text) => drained.text += text.len() as u64,
                    Event::StartElement { attributes, .. } => {
                        drained.text += attributes.len() as u64;
                        if !captured {
                            captured = true;
                            if let Err(error) = reader.capture_opaque() {
                                drained.error = Some(error.kind.clone());
                                return drained;
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(None) => return drained,
            Err(error) => {
                drained.error = Some(error.kind.clone());
                return drained;
            }
        }
    }
}

fn xml(input: &[u8]) -> Outcome {
    let strict = drain(Reader::new(input), false);
    let tolerant = drain(Reader::new_tolerant(input), false);
    let opaque = drain(Reader::new(input), true);
    let label = format!("{:?}/{:?}/{:?}", strict.error, tolerant.error, opaque.error);
    Outcome::labelled(&label, strict.text + tolerant.events + opaque.events)
}

const XML_TOKENS: &[&[u8]] = &[
    b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
    b"<?xml version=\"1.1\"?>",
    b"<!DOCTYPE ",
    b"<!ENTITY ",
    b"<![CDATA[",
    b"]]>",
    b"<!--",
    b"-->",
    b"xmlns=\"",
    b"xmlns:",
    b"xml:space=\"preserve\"",
    b"&amp;",
    b"&lt;",
    b"&#x",
    b"&#",
    b"/>",
    b"</",
    b"\xEF\xBB\xBF",
    b"\xFF\xFE",
    b"\xFE\xFF",
];

fn xml_seeds() -> Vec<Vec<u8>> {
    [
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<root xmlns=\"urn:a\" xmlns:p=\"urn:b\"><p:item id=\"1\" p:k=\"v\">text &amp; more</p:item><!-- note --><![CDATA[raw <data>]]><empty/></root>",
        "<a><b><c/></b><?pi data?></a>",
        "<!DOCTYPE d [<!ENTITY e \"x\">]><d>&e;&#65;&#x42;</d>",
        "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t xml:space=\"preserve\"> hi </w:t></w:r></w:p></w:body></w:document>",
    ]
    .iter()
    .map(|s| s.as_bytes().to_vec())
    .collect()
}

// ----------------------------------------------------------------- images

fn image_outcome(result: Result<Image, atelier_image::ImageError>) -> Outcome {
    match result {
        Ok(image) => Outcome::accepted(image.pixels.len() as u64),
        Err(error) => Outcome::rejected(&error),
    }
}

fn png(input: &[u8]) -> Outcome {
    image_outcome(decode_png(input, DecodeLimits::default()))
}

fn gif(input: &[u8]) -> Outcome {
    image_outcome(decode_gif(input, DecodeLimits::default()))
}

fn bmp(input: &[u8]) -> Outcome {
    image_outcome(decode_bmp(input, DecodeLimits::default()))
}

fn jpeg(input: &[u8]) -> Outcome {
    image_outcome(decode_jpeg_with_metadata(input, DecodeLimits::default()).map(|d| d.image))
}

const PNG_TOKENS: &[&[u8]] = &[
    b"\x89PNG\r\n\x1a\n",
    b"IHDR",
    b"IDAT",
    b"IEND",
    b"PLTE",
    b"tRNS",
    b"gAMA",
    b"iCCP",
    b"tEXt",
    b"\x00\x00\x00\x0d",
    &[0xFF, 0xFF, 0xFF, 0xFF],
];
const GIF_TOKENS: &[&[u8]] = &[
    b"GIF87a",
    b"GIF89a",
    b"\x21\xF9\x04",
    b"\x21\xFF\x0B",
    b"\x21\xFE",
    b"NETSCAPE2.0",
    b"\x2C",
    b"\x3B",
    &[0xFF, 0xFF],
];
const BMP_TOKENS: &[&[u8]] = &[
    b"BM",
    b"\x0C\x00\x00\x00",
    b"\x28\x00\x00\x00",
    b"\x6C\x00\x00\x00",
    b"\x7C\x00\x00\x00",
    b"\x01\x00\x00\x00",
    b"\x02\x00\x00\x00",
    b"\x03\x00\x00\x00",
    &[0xFF, 0xFF, 0xFF, 0xFF],
];
const JPEG_TOKENS: &[&[u8]] = &[
    b"\xFF\xD8",
    b"\xFF\xD9",
    b"\xFF\xC0",
    b"\xFF\xC2",
    b"\xFF\xC4",
    b"\xFF\xDB",
    b"\xFF\xDA",
    b"\xFF\xDD",
    b"\xFF\xD0",
    b"\xFF\xE0\x00\x10JFIF\x00",
    b"\xFF\xE1\x00\x10Exif\x00\x00",
    b"\xFF\xEE\x00\x0EAdobe",
    b"\xFF\xFE",
];

fn png_seeds() -> Vec<Vec<u8>> {
    let gradient = |w: u32, h: u32| Image {
        width: w,
        height: h,
        pixels: (0..w * h * 4).map(|i| (i * 7) as u8).collect(),
    };
    [gradient(4, 4), gradient(1, 1), gradient(9, 3)]
        .iter()
        .map(|image| encode_png_rgba(image, DecodeLimits::default()).expect("valid seed image"))
        .collect()
}

/// The classic 1x1 GIF.
const GIF_1X1: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\xFF\xFF\xFF\x00\x00\x00\x2C\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02\x44\x01\x00\x3B";

/// A GIF89a with a 256-entry palette and one frame of literal LZW codes.
fn gif_file(width: u16, height: u16, interlace: bool, indices: &[u8]) -> Vec<u8> {
    let mut b = b"GIF89a".to_vec();
    b.extend_from_slice(&width.to_le_bytes());
    b.extend_from_slice(&height.to_le_bytes());
    b.extend_from_slice(&[0xF7, 0, 0]);
    for i in 0..=255u8 {
        b.extend_from_slice(&[i, 255 - i, i / 2]);
    }
    b.extend_from_slice(&[0x21, 0xF9, 4, 1, 0, 0, 0, 0]);
    b.extend_from_slice(&[0x2C, 0, 0, 0, 0]);
    b.extend_from_slice(&width.to_le_bytes());
    b.extend_from_slice(&height.to_le_bytes());
    b.push(if interlace { 0x40 } else { 0 });
    b.push(8);
    // Clear code, one 9-bit literal per pixel, end code; packed LSB first.
    let codes: Vec<u16> = std::iter::once(256)
        .chain(indices.iter().map(|&i| u16::from(i)))
        .chain(std::iter::once(257))
        .collect();
    let (mut packed, mut acc, mut bits) = (Vec::new(), 0u32, 0u32);
    for code in codes {
        acc |= u32::from(code) << bits;
        bits += 9;
        while bits >= 8 {
            packed.push(acc as u8);
            acc >>= 8;
            bits -= 8;
        }
    }
    if bits > 0 {
        packed.push(acc as u8);
    }
    for chunk in packed.chunks(255) {
        b.push(chunk.len() as u8);
        b.extend_from_slice(chunk);
    }
    b.extend_from_slice(&[0, 0x3B]);
    b
}

fn gif_seeds() -> Vec<Vec<u8>> {
    vec![
        GIF_1X1.to_vec(),
        gif_file(3, 2, false, &[1, 2, 3, 4, 5, 6]),
        gif_file(2, 4, true, &[9, 8, 7, 6, 5, 4, 3, 2]),
    ]
}

fn bmp_file(width: i32, height: i32, bpp: u16, palette: &[u8], pixels: &[u8]) -> Vec<u8> {
    let offset = 54 + palette.len();
    let mut b = Vec::new();
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&((offset + pixels.len()) as u32).to_le_bytes());
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(&(offset as u32).to_le_bytes());
    b.extend_from_slice(&40u32.to_le_bytes());
    b.extend_from_slice(&width.to_le_bytes());
    b.extend_from_slice(&height.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&bpp.to_le_bytes());
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(&(pixels.len() as u32).to_le_bytes());
    b.extend_from_slice(&[0; 8]);
    b.extend_from_slice(&((palette.len() / 4) as u32).to_le_bytes());
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(palette);
    b.extend_from_slice(pixels);
    b
}

fn bmp_seeds() -> Vec<Vec<u8>> {
    vec![
        bmp_file(3, 2, 24, &[], &(0..24).map(|i| i * 9).collect::<Vec<u8>>()),
        bmp_file(
            4,
            2,
            8,
            &[0, 0, 0, 0, 255, 0, 0, 0, 0, 255, 0, 0, 0, 0, 255, 0],
            &[0, 1, 2, 3, 3, 2, 1, 0],
        ),
        bmp_file(2, -2, 32, &[], &[1; 16]),
    ]
}

fn jpeg_seeds() -> Vec<Vec<u8>> {
    vec![
        include_bytes!("../../../crates/image/tests/fixtures/jpeg/size-1x1.jpg").to_vec(),
        include_bytes!("../../../crates/image/tests/fixtures/jpeg/baseline-gray.jpg").to_vec(),
        include_bytes!("../../../crates/image/tests/fixtures/jpeg/baseline-420.jpg").to_vec(),
        include_bytes!("../../../crates/image/tests/fixtures/jpeg/progressive-gray.jpg").to_vec(),
        include_bytes!("../../../crates/image/tests/fixtures/jpeg/restart-420.jpg").to_vec(),
        include_bytes!("../../../crates/image/tests/fixtures/jpeg/adobe-ycck.jpg").to_vec(),
    ]
}

// ------------------------------------------------------------------ fonts

const SAMPLE_CHARS: [char; 8] = [
    'A',
    'f',
    'i',
    '\u{E9}',
    '\u{20AC}',
    '\u{FB01}',
    '\u{1F600}',
    '\0',
];
const OTF: &[u8] = include_bytes!("../../../crates/font/tests/fixtures/font-0-4.font");
const OTF_CMAP12: &[u8] = include_bytes!("../../../crates/font/tests/fixtures/font-0-12.font");
const TTF: &[u8] = include_bytes!("../../../crates/font/tests/fixtures/font-1-4.font");
const TTF_CMAP6: &[u8] = include_bytes!("../../../crates/font/tests/fixtures/font-1-6.font");
const TTF_CMAP12: &[u8] = include_bytes!("../../../crates/font/tests/fixtures/font-1-12.font");
const COLLECTION: &[u8] = include_bytes!("../../../crates/font/tests/fixtures/two-faces.ttc");

/// Parse faces 0 and 1 (a collection may hold several) and run `body` on each.
fn with_faces(input: &[u8], body: impl Fn(&FontFace<'_>) -> u64) -> Outcome {
    let mut total = 0u64;
    let mut parsed = 0u32;
    let mut first_error = None;
    for index in 0..2 {
        match FontFace::parse_collection(input, index) {
            Ok(face) => {
                parsed += 1;
                total += body(&face);
            }
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    match (parsed, first_error) {
        (0, Some(error)) => Outcome::rejected(&error),
        _ => Outcome::labelled(&format!("faces-{parsed}"), total),
    }
}

/// Glyph ids worth visiting: the first few, the last ones, and the middle.
fn sample_glyphs(count: u16) -> Vec<u16> {
    let mut ids: Vec<u16> = (0..count.min(48)).collect();
    ids.extend([count / 2, count.saturating_sub(1), count, u16::MAX]);
    ids
}

fn opentype(input: &[u8]) -> Outcome {
    with_faces(input, |face| {
        let metrics = face.metrics();
        let mut work = face.names().len() as u64;
        let mut clusters = Vec::new();
        for (i, c) in SAMPLE_CHARS.iter().enumerate() {
            let glyph = face.glyph_index(*c);
            work += u64::from(glyph);
            clusters.push(GlyphCluster {
                glyph_id: glyph,
                cluster: i..i + 1,
            });
        }
        let _ = face.variation_glyph('A', '\u{FE00}');
        let _ = face.variation_glyph('\u{4E00}', '\u{E0100}');
        for glyph in sample_glyphs(metrics.number_of_glyphs) {
            work += u64::from(face.horizontal_metric(glyph).is_ok());
        }
        for tag in [
            *b"head", *b"hhea", *b"maxp", *b"cmap", *b"name", *b"GSUB", *b"GPOS", *b"kern",
        ] {
            work += face.table(tag).map_or(0, |t| t.len() as u64);
        }
        if let Ok(layout) = face.layout() {
            work += layout
                .apply_ligatures(&clusters)
                .map_or(0, |v| v.len() as u64);
            work += layout
                .pair_adjustments(&clusters)
                .map_or(0, |v| v.len() as u64);
        }
        work
    })
}

fn truetype(input: &[u8]) -> Outcome {
    with_faces(input, |face| {
        let mut drawn = 0u64;
        for glyph in sample_glyphs(face.metrics().number_of_glyphs) {
            if let Ok(path) = face.outline(glyph) {
                drawn += 1 + u64::from(!path.is_empty());
            }
            let _ = face.outline_with_limits(glyph, OutlineLimits::default());
        }
        drawn
    })
}

fn cff(input: &[u8]) -> Outcome {
    let font = match CffFont::parse(input) {
        Ok(font) => font,
        Err(error) => return Outcome::rejected(&error),
    };
    let count = u16::try_from(font.glyph_count()).unwrap_or(u16::MAX);
    let mut drawn = font.font_dict_count() as u64;
    let _ = (font.is_cid(), font.registry_ordering_supplement());
    for sid in [0u16, 1, 390, 391, u16::MAX] {
        drawn += font.string(sid).map_or(0, |s| s.len() as u64);
    }
    drawn += font.charset().map_or(0, |c| c.len() as u64);
    for glyph in sample_glyphs(count) {
        let _ = font.font_dict_index(glyph);
        let _ = font.glyph_id_in_charset(glyph);
        if let Ok(g) = font.glyph(glyph, OutlineLimits::default()) {
            drawn += 1 + u64::from(!g.path.is_empty());
        }
        let _ = font.outline(glyph, OutlineLimits::default());
    }
    Outcome::labelled(if font.is_cid() { "cid" } else { "name-keyed" }, drawn)
}

const SFNT_TOKENS: &[&[u8]] = &[
    b"OTTO",
    b"true",
    b"ttcf",
    &[0, 1, 0, 0],
    b"cmap",
    b"head",
    b"hhea",
    b"hmtx",
    b"maxp",
    b"name",
    b"OS/2",
    b"post",
    b"glyf",
    b"loca",
    b"CFF ",
    b"GSUB",
    b"GPOS",
    b"GDEF",
    b"kern",
    &[0xFF, 0xFF, 0xFF, 0xFF],
];
const CFF_TOKENS: &[&[u8]] = &[
    &[0x0E],
    &[0x0A],
    &[0x1D],
    &[0x13],
    &[0x14],
    &[0x1C, 0xFF, 0xFF],
    &[0xFF, 0x7F, 0xFF, 0xFF],
    &[0x01, 0x00, 0x04],
    &[0x0C, 0x1E],
];

fn font_seeds() -> Vec<Vec<u8>> {
    [OTF, OTF_CMAP12, TTF, TTF_CMAP6, COLLECTION]
        .iter()
        .map(|b| b.to_vec())
        .collect()
}

fn truetype_seeds() -> Vec<Vec<u8>> {
    [TTF, TTF_CMAP6, TTF_CMAP12, COLLECTION]
        .iter()
        .map(|b| b.to_vec())
        .collect()
}

/// Raw `CFF ` tables extracted from the OpenType fixtures.
fn cff_seeds() -> Vec<Vec<u8>> {
    [OTF, OTF_CMAP12, COLLECTION]
        .iter()
        .filter_map(|bytes| {
            FontFace::parse(bytes)
                .ok()
                .and_then(|face| face.table(*b"CFF ").map(<[u8]>::to_vec))
        })
        .collect()
}

// -------------------------------------------------------------------- opc

fn opc(input: &[u8]) -> Outcome {
    match Package::read(input) {
        Err(error) => Outcome::rejected(&error),
        Ok(package) => {
            let mut bytes = 0u64;
            for (uri, part) in package.parts() {
                bytes += part.bytes.len() as u64;
                let _ = package.relationships(Some(uri)).len();
            }
            let _ = package.relationships(None).len();
            let _ = (package.core_properties(), package.app_properties());
            match package.write() {
                Ok(written) => Outcome::labelled("read-write", bytes + written.len() as u64),
                Err(error) => Outcome::rejected_with(&error, bytes),
            }
        }
    }
}

fn docx_import_target(input: &[u8]) -> Outcome {
    match import_docx(input) {
        Err(error) => Outcome::rejected(&error),
        Ok(imported) => {
            let bytes: u64 = imported
                .package
                .parts()
                .values()
                .map(|part| part.bytes.len() as u64)
                .sum();
            Outcome::accepted(bytes + imported.paragraph_ids().len() as u64)
        }
    }
}

const DOCX_TOKENS: &[&[u8]] = &[
    b"PK\x03\x04",
    b"[Content_Types].xml",
    b"_rels/.rels",
    b"word/document.xml",
    b"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
    b"<w:document",
    b"<w:body",
    b"<w:p",
    b"<w:r",
    b"<w:t",
];

/// A compact valid DOCX package with one paragraph of text.
fn docx_seeds() -> Vec<Vec<u8>> {
    let mut package = Package::new();
    package
        .insert_part(
            PartUri::parse("/word/document.xml").expect("valid seed part name"),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
            b"<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>seed</w:t></w:r></w:p></w:body></w:document>".to_vec(),
        )
        .expect("valid seed part");
    package
        .add_relationship(
            None,
            "rId1",
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument",
            "word/document.xml",
            TargetMode::Internal,
        )
        .expect("valid seed relationship");
    vec![package.write().expect("valid DOCX seed")]
}

const OPC_TOKENS: &[&[u8]] = &[
    b"PK\x03\x04",
    b"PK\x01\x02",
    b"PK\x05\x06",
    b"[Content_Types].xml",
    b"_rels/.rels",
    b"<Relationship ",
    b"<Override ",
    b"<Default ",
    b"TargetMode=\"External\"",
    b"/word/document.xml",
    b"../",
    b"%2F",
    b"docProps/core.xml",
];

fn opc_seeds() -> Vec<Vec<u8>> {
    let uri = |s: &str| PartUri::parse(s).expect("valid seed part name");
    let mut minimal = Package::new();
    minimal
        .insert_part(
            uri("/word/document.xml"),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
            b"<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body/></w:document>".to_vec(),
        )
        .expect("valid part");
    minimal
        .add_relationship(
            None,
            "rId1",
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument",
            "word/document.xml",
            TargetMode::Internal,
        )
        .expect("valid relationship");
    let mut linked = Package::new();
    linked
        .insert_part(uri("/a/b.xml"), "application/xml", b"<x/>".to_vec())
        .expect("valid part");
    linked
        .add_relationship(
            Some(uri("/a/b.xml")),
            "rId7",
            "http://example.com/rel",
            "https://example.com/",
            TargetMode::External,
        )
        .expect("valid relationship");
    [minimal, linked]
        .iter()
        .map(|p| p.write().expect("valid seed package"))
        .collect()
}

// ---------------------------------------------------------------------- cfb

const CFB_TOKENS: &[&[u8]] = &[
    b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1",
    b"Root Entry",
    b"\xFE\xFF",
    &[0x09, 0x00],
    &[0x0C, 0x00],
    &[0xFC, 0xFF, 0xFF, 0xFF],
    &[0xFD, 0xFF, 0xFF, 0xFF],
    &[0xFE, 0xFF, 0xFF, 0xFF],
];

fn cfb(input: &[u8]) -> Outcome {
    let limits = CfbLimits {
        max_file_size: 1 << 20,
        max_entries: 4096,
        max_depth: 32,
        max_stream_size: 1 << 20,
        max_total_read: 4 << 20,
    };
    let compound = match CfbCompound::open(input, limits) {
        Ok(compound) => compound,
        Err(error) => return Outcome::rejected(&error),
    };
    let entries = compound.list();
    let mut work = entries.len() as u64 + compound.anomalies().len() as u64;
    for listed in entries {
        work = work.saturating_add(listed.path.len() as u64);
        if listed.entry.is_stream() {
            match compound.read_entry(listed.entry) {
                Ok(bytes) => work = work.saturating_add(bytes.len() as u64),
                Err(error) => return Outcome::rejected_with(&error, work),
            }
        }
    }
    Outcome::accepted(work)
}

fn cfb_seeds() -> Vec<Vec<u8>> {
    vec![
        include_bytes!("../../../tests/cfb-reference/native-v3.cfb").to_vec(),
        include_bytes!("../../../tests/cfb-reference/native-v4.cfb").to_vec(),
    ]
}

// --------------------------------------------------------------- doc import
fn doc_import_target(input: &[u8]) -> Outcome {
    match import_doc(input) {
        Err(error) => Outcome::rejected(&error),
        Ok(mut imported) => {
            let ids = imported.paragraph_ids().to_vec();
            let mut work = ids.len() as u64;
            for id in ids {
                work += u64::from(imported.resolved_paragraph(id).is_ok());
            }
            Outcome::accepted(work)
        }
    }
}

fn doc_seeds() -> Vec<Vec<u8>> {
    vec![
        include_bytes!("../../../tests/doc-reference/text-accents.doc").to_vec(),
        include_bytes!("../../../tests/doc-reference/char-formatting.doc").to_vec(),
        include_bytes!("../../../tests/doc-reference/para-formatting.doc").to_vec(),
        include_bytes!("../../../tests/doc-reference/styles.doc").to_vec(),
        include_bytes!("../../../tests/doc-reference/underlines.doc").to_vec(),
    ]
}

// --------------------------------------------------------------- odt import
fn odt_import_target(input: &[u8]) -> Outcome {
    match import_odt(input) {
        Err(error) => Outcome::rejected(&error),
        Ok(mut imported) => {
            let ids = imported.paragraph_ids().to_vec();
            let mut work = ids.len() as u64;
            for id in ids {
                work += u64::from(imported.resolved_paragraph(id).is_ok());
            }
            Outcome::accepted(work)
        }
    }
}

const ODT_TOKENS: &[&[u8]] = &[
    b"PK",
    b"mimetype",
    b"application/vnd.oasis.opendocument.text",
    b"content.xml",
    b"styles.xml",
    b"<office:text",
    b"<text:p",
    b"<text:span",
    b"<table:table",
    b"number-columns-repeated",
    b"number-rows-repeated",
];

fn odt_seeds() -> Vec<Vec<u8>> {
    vec![
        include_bytes!("../../../tests/odt-reference/text-accents.odt").to_vec(),
        include_bytes!("../../../tests/odt-reference/lists.odt").to_vec(),
        include_bytes!("../../../tests/odt-reference/table-merge.odt").to_vec(),
        include_bytes!("../../../tests/odt-reference/table-merge.fodt").to_vec(),
        include_bytes!("../../../tests/odt-reference/header-footer.odt").to_vec(),
    ]
}

// ------------------------------------------------------------ font subsetting
const SUBSET_GLYPHS: &[u16] = &[0, 1, 2, 5, 40, 0xff];

fn subset_font_target(input: &[u8]) -> Outcome {
    match subset_font(input, 0, SUBSET_GLYPHS) {
        Err(error) => Outcome::rejected(&error),
        Ok(subset) => Outcome::accepted(subset.data.len() as u64),
    }
}

fn subset_cff_target(input: &[u8]) -> Outcome {
    match subset_cff(input, SUBSET_GLYPHS) {
        Err(error) => Outcome::rejected(&error),
        // A subset the outline reader cannot parse back is a defect.
        Ok(subset) => match CffFont::parse(&subset.data) {
            Ok(_) => Outcome::accepted(subset.data.len() as u64),
            Err(error) => panic!("subset is not a valid CFF font: {error:?}"),
        },
    }
}

// -------------------------------------------------------------- encodings

const ENCODINGS: [Encoding; 6] = [
    Encoding::Utf8,
    Encoding::Utf16Le,
    Encoding::Utf16Be,
    Encoding::Windows1252,
    Encoding::Iso8859_1,
    Encoding::Cp437,
];

fn encodings(input: &[u8]) -> Outcome {
    let detected = detect_encoding(input);
    let mut total = 0u64;
    for encoding in ENCODINGS.into_iter().chain([Encoding::MacRoman]) {
        let text = decode(input, encoding);
        total += text.len() as u64;
        if let Ok(bytes) = encode(&text, encoding) {
            total += bytes.len() as u64;
        }
    }
    Outcome::labelled(&format!("{detected:?}"), total)
}

const ENCODING_TOKENS: &[&[u8]] = &[
    b"\xEF\xBB\xBF",
    b"\xFF\xFE",
    b"\xFE\xFF",
    b"\xED\xA0\x80",
    b"\xF4\x90\x80\x80",
    b"\xC0\xAF",
    &[0x00, 0xD8],
    &[0xD8, 0x00],
    &[0x00, 0xDC],
];

fn encoding_seeds() -> Vec<Vec<u8>> {
    let mut utf16 = vec![0xFF, 0xFE];
    utf16.extend(
        "caf\u{E9} \u{1F600}"
            .encode_utf16()
            .flat_map(u16::to_le_bytes),
    );
    vec![
        "Caf\u{E9} \u{20AC} na\u{EF}ve".as_bytes().to_vec(),
        vec![0xEF, 0xBB, 0xBF, b'o', b'k'],
        utf16,
        vec![b'a', 0x80, 0x9F, 0xE9, 0xFF],
    ]
}

// ----------------------------------------------------------- display list

fn display_list(input: &[u8]) -> Outcome {
    match DisplayList::decode(input) {
        Err(error) => Outcome::rejected(&error),
        Ok(list) => {
            assert!(list.validate().is_ok(), "decoded list fails validation");
            let again = list.encode().expect("a decoded list must re-encode");
            assert!(
                DisplayList::decode(&again).is_ok(),
                "re-encoded list does not decode"
            );
            Outcome::accepted(list.commands.len() as u64 + again.len() as u64)
        }
    }
}

fn display_list_seeds() -> Vec<Vec<u8>> {
    let p = |x, y| Point::try_new(x, y).expect("valid point");
    let rect = |x, y, w, h| Rect::from_xywh(x, y, w, h).expect("valid rectangle");
    let triangle = DisplayPath::new(vec![
        PathElement::MoveTo(p(0.0, 0.0)),
        PathElement::LineTo(p(10.0, 0.0)),
        PathElement::QuadraticTo {
            control: p(12.0, 6.0),
            to: p(10.0, 10.0),
        },
        PathElement::CubicTo {
            control1: p(6.0, 12.0),
            control2: p(2.0, 12.0),
            to: p(0.0, 10.0),
        },
        PathElement::Close,
    ])
    .expect("valid path");
    let color = Color::new(0.2, 0.4, 0.6, 1.0).expect("valid color");
    let page = PageInfo::new(rect(0.0, 0.0, 100.0, 200.0)).expect("valid page");
    let empty = DisplayList::new(page, Vec::new()).expect("valid list");
    let full = DisplayList::new(
        page,
        vec![
            Command::PushClip {
                bounds: rect(1.0, 2.0, 80.0, 90.0),
            },
            Command::FillPath {
                path: triangle.clone(),
                color,
                rule: FillRule::EvenOdd,
            },
            Command::StrokePath {
                path: triangle,
                color,
                style: StrokeStyle::new(1.5, LineCap::Round, LineJoin::Bevel, 4.0)
                    .expect("valid stroke"),
            },
            Command::PopClip,
            Command::Link {
                bounds: rect(5.0, 6.0, 7.0, 8.0),
                target: "https://example.com/".to_string(),
            },
            Command::Structure {
                role: "heading".to_string(),
                node_id: 9,
            },
        ],
    )
    .expect("valid list");
    [empty, full]
        .iter()
        .map(|l| l.encode().expect("valid seed list"))
        .collect()
}

// -------------------------------------------------------- recovery journal

struct TreeCodec;

impl SnapshotCodec<IndexedTree<i32>> for TreeCodec {
    type Error = &'static str;

    fn encode(&self, tree: &IndexedTree<i32>) -> Result<Vec<u8>, Self::Error> {
        let mut out = (tree.len() as u32).to_le_bytes().to_vec();
        for i in 0..tree.len() {
            out.extend_from_slice(&tree.get(i).copied().unwrap_or(0).to_le_bytes());
        }
        Ok(out)
    }

    fn restore(&self, payload: &[u8]) -> Result<IndexedTree<i32>, Self::Error> {
        let (head, body) = payload.split_first_chunk::<4>().ok_or("short snapshot")?;
        let count = u32::from_le_bytes(*head) as usize;
        if body.len() / 4 != count || body.len() % 4 != 0 {
            return Err("invalid snapshot");
        }
        let mut tree = IndexedTree::new();
        for chunk in body.chunks_exact(4) {
            let value = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            tree.insert(tree.len(), value).map_err(|_| "tree")?;
        }
        Ok(tree)
    }
}

/// Input layout: `u16` little-endian snapshot length, the snapshot envelope,
/// then the journal. Both parts are mutated; a short input is zero-padded.
fn recovery_journal(input: &[u8]) -> Outcome {
    let declared = input
        .first_chunk::<2>()
        .map_or(0, |b| usize::from(u16::from_le_bytes(*b)));
    let body = input.get(2..).unwrap_or(&[]);
    let (snapshot, journal) = body.split_at(declared.min(body.len()));
    let result = replay::<IndexedTree<i32>, TreeOp<i32>, _>(
        &TreeCodec,
        snapshot,
        journal,
        JournalLimits::default(),
    );
    match result {
        Err(error) => Outcome::rejected(&error),
        Ok(replayed) => Outcome::labelled(
            &format!("replayed-{:?}", replayed.stopped),
            replayed.state.len() as u64 + replayed.applied_entries as u64,
        ),
    }
}

const JOURNAL_TOKENS: &[&[u8]] = &[
    b"ATJS",
    b"ATJR",
    b"ATOP",
    &[1, 0],
    &[0xFF, 0xFF, 0xFF, 0xFF],
];

fn journal_seeds() -> Vec<Vec<u8>> {
    let ids = (RecoveryId([1; 16]), RecoveryId([2; 16]));
    let limits = JournalLimits::default();
    let mut tree = IndexedTree::new();
    for value in [1, 2, 3] {
        tree.insert(tree.len(), value).expect("seed tree");
    }
    let snapshot = snapshot_state(&TreeCodec, &tree, ids.0, ids.1, 0, limits).expect("snapshot");
    let mut journal = Journal::new(ids.0, ids.1, 0, limits).expect("journal");
    let mut seeds = Vec::new();
    let frame = |snapshot: &[u8], journal: &[u8]| {
        let mut out = (snapshot.len() as u16).to_le_bytes().to_vec();
        out.extend_from_slice(snapshot);
        out.extend_from_slice(journal);
        out
    };
    seeds.push(frame(&snapshot, journal.as_bytes()));
    for (position, value) in [(0usize, 7), (1, 8), (2, 9)] {
        let transaction = Transaction::new(vec![TreeOp::Insert { position, value }]).expect("tx");
        journal
            .append::<IndexedTree<i32>, TreeOp<i32>>(&transaction)
            .expect("append");
    }
    seeds.push(frame(&snapshot, journal.as_bytes()));
    seeds
}
