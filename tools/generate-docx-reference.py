#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Generate and independently validate the deterministic US-301 DOCX fixture."""

from __future__ import annotations

import argparse
import hashlib
import json
import posixpath
import sys
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FIXTURE_DIR = ROOT / "tests" / "docx-reference"
DOCX_PATH = FIXTURE_DIR / "basic.docx"
EXPECTED_PATH = FIXTURE_DIR / "expected.json"
FONT_PATH = ROOT / "data" / "fonts" / "carlito" / "Carlito-Regular.ttf"

W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PKG_REL = "http://schemas.openxmlformats.org/package/2006/relationships"
CT = "http://schemas.openxmlformats.org/package/2006/content-types"
A = "http://schemas.openxmlformats.org/drawingml/2006/main"
OFFICE_REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument"
REL_BASE = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/"
FONT_KEY = "{00112233-4455-6677-8899-AABBCCDDEEFF}"
FONT_PART = "content/fonts/Carlito-Regular.odttf"
OPAQUE_PART = "vendor/preserved-opaque.bin"
OPAQUE_BYTES = b"US301-opaque-fixture\x00\xff\x10preserve-me\n"
FRENCH_TEXT = "Référence de titre — Français et English mix: cœurs, accents, ligatures."
ENGLISH_TEXT = (
    "This second English paragraph is deliberately long so a layout engine must wrap it across "
    "multiple visual lines. It includes punctuation, varied word lengths, and enough ordinary "
    "text to make the fixture useful without depending on any renderer or platform-specific "
    "font metrics."
)

ET.register_namespace("w", W)
ET.register_namespace("r", R)
ET.register_namespace("a", A)


def qn(namespace: str, local: str) -> str:
    return f"{{{namespace}}}{local}"


def element(parent: ET.Element, namespace: str, local: str, attrs: dict[str, str] | None = None) -> ET.Element:
    return ET.SubElement(parent, qn(namespace, local), attrs or {})


def xml_bytes(root: ET.Element) -> bytes:
    return ET.tostring(root, encoding="utf-8", xml_declaration=True, short_empty_elements=True)


def obfuscate_font(data: bytes) -> bytes:
    key = bytes.fromhex(FONT_KEY.strip("{}").replace("-", ""))[::-1]
    mask = key + key
    return bytes(value ^ mask[index] for index, value in enumerate(data[:32])) + data[32:]


def make_document() -> bytes:
    document = ET.Element(qn(W, "document"))
    body = element(document, W, "body")

    para = element(body, W, "p")
    ppr = element(para, W, "pPr")
    element(ppr, W, "pStyle", {qn(W, "val"): "Heading1"})
    run = element(para, W, "r")
    rpr = element(run, W, "rPr")
    element(rpr, W, "rStyle", {qn(W, "val"): "Strong"})
    element(rpr, W, "b", {qn(W, "val"): "0"})
    text = element(run, W, "t", {qn("http://www.w3.org/XML/1998/namespace", "space"): "preserve"})
    text.text = FRENCH_TEXT

    para = element(body, W, "p")
    run = element(para, W, "r")
    text = element(run, W, "t")
    text.text = ENGLISH_TEXT
    sect = element(body, W, "sectPr")
    element(sect, W, "pgSz", {qn(W, "w"): "12240", qn(W, "h"): "15840"})
    element(sect, W, "pgMar", {
        qn(W, "top"): "1440", qn(W, "right"): "1440", qn(W, "bottom"): "1440",
        qn(W, "left"): "1440", qn(W, "header"): "720", qn(W, "footer"): "720",
        qn(W, "gutter"): "0",
    })
    return xml_bytes(document)


def make_styles() -> bytes:
    root = ET.Element(qn(W, "styles"))
    defaults = element(root, W, "docDefaults")
    rdefaults = element(defaults, W, "rPrDefault")
    rpr = element(rdefaults, W, "rPr")
    element(rpr, W, "rFonts", {
        qn(W, "asciiTheme"): "minorHAnsi", qn(W, "hAnsiTheme"): "minorHAnsi",
        qn(W, "eastAsiaTheme"): "minorEastAsia", qn(W, "cstheme"): "minorBidi",
    })
    element(rpr, W, "sz", {qn(W, "val"): "22"})
    element(rpr, W, "szCs", {qn(W, "val"): "22"})
    pdefaults = element(defaults, W, "pPrDefault")
    element(pdefaults, W, "pPr")

    normal = element(root, W, "style", {qn(W, "type"): "paragraph", qn(W, "default"): "1", qn(W, "styleId"): "Normal"})
    element(normal, W, "name", {qn(W, "val"): "Normal"})

    heading = element(root, W, "style", {qn(W, "type"): "paragraph", qn(W, "styleId"): "Heading1"})
    element(heading, W, "name", {qn(W, "val"): "heading 1"})
    element(heading, W, "basedOn", {qn(W, "val"): "Normal"})
    h_rpr = element(heading, W, "rPr")
    element(h_rpr, W, "sz", {qn(W, "val"): "32"})
    element(h_rpr, W, "color", {qn(W, "val"): "000000", qn(W, "themeColor"): "accent1"})

    strong = element(root, W, "style", {qn(W, "type"): "character", qn(W, "styleId"): "Strong"})
    element(strong, W, "name", {qn(W, "val"): "Strong"})
    element(element(strong, W, "rPr"), W, "b")
    return xml_bytes(root)


def make_theme() -> bytes:
    theme = ET.Element(qn(A, "theme"), {"name": "US-301 Deterministic Theme"})
    elements = element(theme, A, "themeElements")
    colors = element(elements, A, "clrScheme", {"name": "US-301"})
    color_values = [
        ("dk1", "sysClr", {"val": "windowText", "lastClr": "000000"}),
        ("lt1", "sysClr", {"val": "window", "lastClr": "FFFFFF"}),
        ("dk2", "srgbClr", {"val": "1F1F1F"}),
        ("lt2", "srgbClr", {"val": "E7E6E6"}),
        ("accent1", "srgbClr", {"val": "4472C4"}),
        ("accent2", "srgbClr", {"val": "ED7D31"}),
        ("accent3", "srgbClr", {"val": "A5A5A5"}),
        ("accent4", "srgbClr", {"val": "FFC000"}),
        ("accent5", "srgbClr", {"val": "5B9BD5"}),
        ("accent6", "srgbClr", {"val": "70AD47"}),
        ("hlink", "srgbClr", {"val": "0563C1"}),
        ("folHlink", "srgbClr", {"val": "954F72"}),
    ]
    for name, child_name, attrs in color_values:
        color = element(colors, A, name)
        element(color, A, child_name, attrs)

    fonts = element(elements, A, "fontScheme", {"name": "US-301 Fonts"})
    major = element(fonts, A, "majorFont")
    element(major, A, "latin", {"typeface": "Liberation Serif"})
    element(major, A, "ea", {"typeface": ""})
    element(major, A, "cs", {"typeface": ""})
    minor = element(fonts, A, "minorFont")
    element(minor, A, "latin", {"typeface": "Carlito"})
    element(minor, A, "ea", {"typeface": ""})
    element(minor, A, "cs", {"typeface": ""})
    for font_group in (major, minor):
        for script in ("Jpan", "Hang", "Hans", "Hant", "Arab", "Hebr", "Thai", "Ethi", "Beng", "Gujr", "Khmr", "Knda", "Guru", "Cans", "Cher", "Yiii", "Tibt", "Thaa", "Deva", "Telu", "Taml", "Syrc", "Orya", "Mlym", "Laoo", "Sinh", "Mong", "Viet", "Uigh", "Geor", "Armn", "Bopo", "Brai", "Cyrl", "Grek"):
            element(font_group, A, "font", {"script": script, "typeface": ""})

    fmt = element(elements, A, "fmtScheme", {"name": "US-301 Format"})
    fills = element(fmt, A, "fillStyleLst")
    solid = element(fills, A, "solidFill")
    element(solid, A, "schemeClr", {"val": "phClr"})
    for shade in ("tint", "shade"):
        grad = element(fills, A, "gradFill", {"rotWithShape": "1"})
        stops = element(grad, A, "gsLst")
        for position, color_op in (("0", "tint"), ("100000", "shade")):
            stop = element(stops, A, "gs", {"pos": position})
            scheme = element(stop, A, "schemeClr", {"val": "phClr"})
            element(scheme, A, color_op, {"val": "50000" if shade == "tint" else "50000"})
    lines = element(fmt, A, "lnStyleLst")
    for width in (6350, 12700, 19050):
        line = element(lines, A, "ln", {"w": str(width), "cap": "flat", "cmpd": "sng", "algn": "ctr"})
        fill = element(line, A, "solidFill")
        element(fill, A, "schemeClr", {"val": "phClr"})
        element(line, A, "prstDash", {"val": "solid"})
        element(line, A, "miter", {"lim": "800000"})
        element(line, A, "headEnd", {"type": "none", "w": "med", "len": "med"})
        element(line, A, "tailEnd", {"type": "none", "w": "med", "len": "med"})
    effects = element(fmt, A, "effectStyleLst")
    for _ in range(3):
        effect = element(effects, A, "effectStyle")
        element(effect, A, "effectLst")
    bgfills = element(fmt, A, "bgFillStyleLst")
    for _ in range(3):
        fill = element(bgfills, A, "solidFill")
        element(fill, A, "schemeClr", {"val": "phClr"})
    return xml_bytes(theme)


def make_settings() -> bytes:
    root = ET.Element(qn(W, "settings"))
    element(root, W, "defaultTabStop", {qn(W, "val"): "720"})
    compat = element(root, W, "compat")
    element(compat, W, "compatSetting", {
        qn(W, "name"): "compatibilityMode",
        qn(W, "uri"): "http://schemas.microsoft.com/office/word",
        qn(W, "val"): "15",
    })
    element(compat, W, "doNotExpandShiftReturn")
    latent = element(root, W, "latentStyles", {qn(W, "defLockedState"): "0", qn(W, "latentStyleCount"): "276"})
    element(latent, W, "lsdException", {qn(W, "name"): "Normal", qn(W, "locked"): "0"})
    return xml_bytes(root)


def make_font_table() -> bytes:
    root = ET.Element(qn(W, "fonts"))
    font = element(root, W, "font", {qn(W, "name"): "Carlito"})
    element(font, W, "panose1", {qn(W, "val"): "020F0502020204030204"})
    element(font, W, "charset", {qn(W, "val"): "00"})
    element(font, W, "family", {qn(W, "val"): "swiss"})
    element(font, W, "pitch", {qn(W, "val"): "variable"})
    element(font, W, "embedRegular", {qn(R, "id"): "rIdCarlito", qn(W, "fontKey"): FONT_KEY})
    return xml_bytes(root)


def make_relationships(items: list[tuple[str, str, str]]) -> bytes:
    root = ET.Element(qn(PKG_REL, "Relationships"))
    for rel_id, rel_type, target in items:
        ET.SubElement(root, qn(PKG_REL, "Relationship"), {"Id": rel_id, "Type": rel_type, "Target": target})
    return xml_bytes(root)


def make_content_types() -> bytes:
    root = ET.Element(qn(CT, "Types"))
    ET.SubElement(root, qn(CT, "Default"), {"Extension": "rels", "ContentType": "application/vnd.openxmlformats-package.relationships+xml"})
    ET.SubElement(root, qn(CT, "Default"), {"Extension": "xml", "ContentType": "application/xml"})
    ET.SubElement(root, qn(CT, "Default"), {"Extension": "odttf", "ContentType": "application/vnd.openxmlformats-officedocument.obfuscatedFont"})
    ET.SubElement(root, qn(CT, "Default"), {"Extension": "bin", "ContentType": "application/octet-stream"})
    overrides = [
        ("/content/main.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"),
        ("/content/styles.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"),
        ("/content/theme1.xml", "application/vnd.openxmlformats-officedocument.theme+xml"),
        ("/content/settings.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"),
        ("/content/fontTable.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml"),
    ]
    for part, content_type in overrides:
        ET.SubElement(root, qn(CT, "Override"), {"PartName": part, "ContentType": content_type})
    return xml_bytes(root)


def package_parts() -> dict[str, bytes]:
    source_font = FONT_PATH.read_bytes()
    return {
        "[Content_Types].xml": make_content_types(),
        "_rels/.rels": make_relationships([("rIdOffice", OFFICE_REL, "content/main.xml")]),
        "content/main.xml": make_document(),
        "content/_rels/main.xml.rels": make_relationships([
            ("rIdStyles", REL_BASE + "styles", "styles.xml"),
            ("rIdTheme", REL_BASE + "theme", "theme1.xml"),
            ("rIdSettings", REL_BASE + "settings", "settings.xml"),
            ("rIdFontTable", REL_BASE + "fontTable", "fontTable.xml"),
        ]),
        "content/styles.xml": make_styles(),
        "content/theme1.xml": make_theme(),
        "content/settings.xml": make_settings(),
        "content/fontTable.xml": make_font_table(),
        "content/_rels/fontTable.xml.rels": make_relationships([("rIdCarlito", REL_BASE + "font", "fonts/Carlito-Regular.odttf")]),
        FONT_PART: obfuscate_font(source_font),
        OPAQUE_PART: OPAQUE_BYTES,
    }


def make_docx(parts: dict[str, bytes]) -> bytes:
    from io import BytesIO

    stream = BytesIO()
    # Stored entries make byte reproduction independent of the host zlib version.
    with zipfile.ZipFile(stream, "w", compression=zipfile.ZIP_STORED) as archive:
        for name, data in parts.items():
            info = zipfile.ZipInfo(name, date_time=(2000, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_STORED
            info.create_system = 0
            info.external_attr = 0o600 << 16
            info.flag_bits = 0
            archive.writestr(info, data, compress_type=zipfile.ZIP_STORED)
    return stream.getvalue()


def resolve_target(source_part: str | None, target: str) -> str:
    base = "" if source_part is None else posixpath.dirname(source_part)
    result = posixpath.normpath(posixpath.join(base, target))
    if result.startswith("../") or result == ".." or result.startswith("/"):
        raise ValueError(f"Relationship target escapes package: {target}")
    return result


def relationship_map(parts: dict[str, bytes], source_part: str | None) -> dict[str, str]:
    rel_path = "_rels/.rels" if source_part is None else posixpath.join(posixpath.dirname(source_part), "_rels", posixpath.basename(source_part) + ".rels")
    root = ET.fromstring(parts[rel_path])
    result: dict[str, str] = {}
    for rel in root.findall(qn(PKG_REL, "Relationship")):
        if rel.get("TargetMode") == "External":
            continue
        result[rel.attrib["Type"]] = resolve_target(source_part, rel.attrib["Target"])
    return result


def parse_independently(parts: dict[str, bytes]) -> dict[str, object]:
    root_rels = relationship_map(parts, None)
    main = root_rels[OFFICE_REL]
    document_rels = relationship_map(parts, main)
    for required in ("styles", "theme", "settings", "fontTable"):
        rel_type = REL_BASE + required
        if rel_type not in document_rels:
            raise ValueError(f"Missing document relationship: {rel_type}")

    document = ET.fromstring(parts[main])
    body = document.find(f"{qn(W, 'body')}")
    paragraphs = body.findall(qn(W, "p")) if body is not None else []
    paragraph_records = []
    for para in paragraphs:
        pstyle = para.find(f"{qn(W, 'pPr')}/{qn(W, 'pStyle')}")
        para_text = "".join(node.text or "" for node in para.iter(qn(W, "t")))
        runs = []
        for run in para.findall(qn(W, "r")):
            rpr = run.find(qn(W, "rPr"))
            rstyle = rpr.find(qn(W, "rStyle")) if rpr is not None else None
            bold = rpr.find(qn(W, "b")) if rpr is not None else None
            run_text = "".join(node.text or "" for node in run.iter(qn(W, "t")))
            runs.append({
                "text": run_text,
                "characterStyleId": rstyle.get(qn(W, "val")) if rstyle is not None else None,
                "directBold": None if bold is None else bold.get(qn(W, "val"), "true") not in ("0", "false", "off"),
            })
        paragraph_records.append({
            "text": para_text,
            "paragraphStyleId": pstyle.get(qn(W, "val")) if pstyle is not None else None,
            "runs": runs,
        })

    styles = ET.fromstring(parts[document_rels[REL_BASE + "styles"]])
    style_ids = [style.get(qn(W, "styleId")) for style in styles.findall(qn(W, "style"))]
    defaults_rfonts = styles.find(f"{qn(W, 'docDefaults')}/{qn(W, 'rPrDefault')}/{qn(W, 'rPr')}/{qn(W, 'rFonts')}")
    defaults_size = styles.find(f"{qn(W, 'docDefaults')}/{qn(W, 'rPrDefault')}/{qn(W, 'rPr')}/{qn(W, 'sz')}")
    heading = next(style for style in styles.findall(qn(W, "style")) if style.get(qn(W, "styleId")) == "Heading1")
    heading_based_on = heading.find(qn(W, "basedOn"))
    heading_size = heading.find(f"{qn(W, 'rPr')}/{qn(W, 'sz')}")
    heading_color = heading.find(f"{qn(W, 'rPr')}/{qn(W, 'color')}")

    theme = ET.fromstring(parts[document_rels[REL_BASE + "theme"]])
    theme_minor = theme.find(f"{qn(A, 'themeElements')}/{qn(A, 'fontScheme')}/{qn(A, 'minorFont')}/{qn(A, 'latin')}")
    theme_major = theme.find(f"{qn(A, 'themeElements')}/{qn(A, 'fontScheme')}/{qn(A, 'majorFont')}/{qn(A, 'latin')}")

    settings = ET.fromstring(parts[document_rels[REL_BASE + "settings"]])
    default_tab = settings.find(qn(W, "defaultTabStop"))
    compat_setting = settings.find(f"{qn(W, 'compat')}/{qn(W, 'compatSetting')}")
    compat_flag = settings.find(f"{qn(W, 'compat')}/{qn(W, 'doNotExpandShiftReturn')}")
    latent = settings.find(qn(W, "latentStyles"))

    font_table_part = document_rels[REL_BASE + "fontTable"]
    fonts = ET.fromstring(parts[font_table_part])
    font = fonts.find(qn(W, "font"))
    embed = font.find(qn(W, "embedRegular"))
    font_rels = relationship_map(parts, font_table_part)
    font_relationships_root = ET.fromstring(parts[posixpath.join(posixpath.dirname(font_table_part), "_rels", posixpath.basename(font_table_part) + ".rels")])
    font_relationship = next(rel for rel in font_relationships_root.findall(qn(PKG_REL, "Relationship")) if rel.get("Id") == embed.get(qn(R, "id")))
    font_part = resolve_target(font_table_part, font_relationship.get("Target"))
    obfuscated = parts[font_part]
    key = bytes.fromhex(embed.get(qn(W, "fontKey")).strip("{}").replace("-", ""))[::-1]
    deobfuscated = bytes(value ^ (key + key)[index] for index, value in enumerate(obfuscated[:32])) + obfuscated[32:]
    source_font = FONT_PATH.read_bytes()
    ct = ET.fromstring(parts["[Content_Types].xml"])

    return {
        "relationshipDiscovery": {
            "mainPart": main,
            "stylesPart": document_rels[REL_BASE + "styles"],
            "themePart": document_rels[REL_BASE + "theme"],
            "settingsPart": document_rels[REL_BASE + "settings"],
            "fontTablePart": font_table_part,
        },
        "paragraphs": paragraph_records,
        "styles": {
            "ids": style_ids,
            "docDefaultsFontTheme": defaults_rfonts.get(qn(W, "asciiTheme")),
            "docDefaultsSizeHalfPoints": int(defaults_size.get(qn(W, "val"))),
            "heading1BasedOn": heading_based_on.get(qn(W, "val")),
            "heading1SizeHalfPoints": int(heading_size.get(qn(W, "val"))),
            "heading1Color": heading_color.get(qn(W, "themeColor")),
            "heading1ColorFallback": heading_color.get(qn(W, "val")),
        },
        "theme": {
            "minorLatin": theme_minor.get("typeface"),
            "majorLatin": theme_major.get("typeface"),
        },
        "settings": {
            "defaultTabStopTwips": int(default_tab.get(qn(W, "val"))),
            "compatSetting": {
                "name": compat_setting.get(qn(W, "name")),
                "uri": compat_setting.get(qn(W, "uri")),
                "value": compat_setting.get(qn(W, "val")),
            },
            "compatFlagPresent": compat_flag is not None,
            "latentStyleCount": int(latent.get(qn(W, "latentStyleCount"))),
        },
        "font": {
            "faceName": font.get(qn(W, "name")),
            "fontKey": embed.get(qn(W, "fontKey")),
            "fontPart": font_part,
            "originalSha256": hashlib.sha256(source_font).hexdigest(),
            "obfuscatedSha256": hashlib.sha256(obfuscated).hexdigest(),
            "deobfuscatedSha256": hashlib.sha256(deobfuscated).hexdigest(),
            "originalFirst32Hex": source_font[:32].hex(),
            "obfuscatedFirst32Hex": obfuscated[:32].hex(),
        },
        "opaquePart": {
            "path": OPAQUE_PART,
            "sha256": hashlib.sha256(parts[OPAQUE_PART]).hexdigest(),
            "bytesHex": parts[OPAQUE_PART].hex(),
        },
        "contentTypes": {
            "mainPartOverrideFound": any(
                item.get("PartName") == f"/{main}"
                and item.get("ContentType", "").endswith("wordprocessingml.document.main+xml")
                for item in ct.findall(qn(CT, "Override"))
            ),
            "fontExtensionFound": any(
                item.get("Extension") == "odttf"
                and item.get("ContentType", "").endswith("obfuscatedFont")
                for item in ct.findall(qn(CT, "Default"))
            ),
        },
    }


def expected_payload(parts: dict[str, bytes]) -> dict[str, object]:
    observed = parse_independently(parts)
    return {
        "schemaVersion": 1,
        "fixture": "tests/docx-reference/basic.docx",
        "generator": "tools/generate-docx-reference.py (Python standard library only)",
        "paragraphText": [FRENCH_TEXT, ENGLISH_TEXT],
        "paragraphStyleIds": ["Heading1", None],
        "firstRunDirectProperties": {"characterStyleId": "Strong", "directBold": False},
        "fontHashesAndKnownVector": observed["font"],
        "reparse": observed,
    }


def check_fixture() -> None:
    generated_parts = package_parts()
    generated_docx = make_docx(generated_parts)
    generated_expected = (json.dumps(expected_payload(generated_parts), ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")

    if not DOCX_PATH.exists() or DOCX_PATH.read_bytes() != generated_docx:
        raise ValueError(f"DOCX bytes differ from deterministic output: {DOCX_PATH}")
    if not EXPECTED_PATH.exists() or EXPECTED_PATH.read_bytes() != generated_expected:
        raise ValueError(f"expected.json bytes differ from deterministic output: {EXPECTED_PATH}")

    with zipfile.ZipFile(DOCX_PATH, "r") as archive:
        actual_parts = {name: archive.read(name) for name in archive.namelist()}
    actual_expected = json.loads(EXPECTED_PATH.read_text(encoding="utf-8"))
    independently_parsed = parse_independently(actual_parts)
    if independently_parsed != actual_expected["reparse"]:
        raise ValueError("Independent XML/relationship reparse differs from expected.json")
    if independently_parsed["font"]["deobfuscatedSha256"] != independently_parsed["font"]["originalSha256"]:
        raise ValueError("The embedded obfuscated font does not recover the source font bytes")
    if not independently_parsed["contentTypes"]["mainPartOverrideFound"]:
        raise ValueError("The relocated main document part is missing its content type override")
    if not independently_parsed["contentTypes"]["fontExtensionFound"]:
        raise ValueError("The obfuscated font extension is missing its content type declaration")
    print("DOCX_REFERENCE_CHECK_OK: exact bytes match; relationships, XML, font vector, and expected JSON reparse verified")


def generate() -> None:
    FIXTURE_DIR.mkdir(parents=True, exist_ok=True)
    parts = package_parts()
    DOCX_PATH.write_bytes(make_docx(parts))
    EXPECTED_PATH.write_bytes((json.dumps(expected_payload(parts), ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8"))
    print(f"Generated {DOCX_PATH.relative_to(ROOT)} and {EXPECTED_PATH.relative_to(ROOT)}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="compare exact generated bytes and independently reparse the existing fixture")
    args = parser.parse_args()
    try:
        if args.check:
            check_fixture()
        else:
            generate()
    except (OSError, ValueError, KeyError, StopIteration, zipfile.BadZipFile, ET.ParseError) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
