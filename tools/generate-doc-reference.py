#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Generate and independently validate the US-312 Word 97-2003 reference files.

Each document is described once in DOCS (text, character and paragraph
properties). The generator writes a flat ODT (`.fodt`) for every document,
asks LibreOffice to save it as "MS Word 97" (`.doc`), converts the resulting
`.doc` back to text with LibreOffice and records everything in
`tests/doc-reference/expected.json`. Microsoft Word was not available when the
fixtures were created; LibreOffice is the producer and an independent reader.

`--check` does not need LibreOffice: it verifies the SHA-256 and size of the
stored files, parses their compound file, FIB and piece table with the small
reader below (no Rust code) and compares the decoded main text with the text
expected by DOCS and by `expected.json`.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path
from xml.sax.saxutils import escape

ROOT = Path(__file__).resolve().parents[1]
FIXTURE_DIR = ROOT / "tests" / "doc-reference"
EXPECTED_PATH = FIXTURE_DIR / "expected.json"

NS = (
    'xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
    'xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" '
    'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" '
    'xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" '
    'xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" '
    'xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" '
    'xmlns:dc="http://purl.org/dc/elements/1.1/"'
)

# ---------------------------------------------------------------------------
# Document descriptions. A paragraph is {"runs": [(text, props)], "props": {},
# "style": name}. Run props: bold, italic, underline (single|double), strike,
# vertical_align (superscript|subscript), color (RRGGBB), size (points), font,
# highlight (RRGGBB), style (character style). Paragraph props: alignment
# (left|center|right|justify), indent_left/indent_right/indent_first_line
# (points), space_before/space_after (points), line (("auto", percent) |
# ("exact", points) | ("at_least", points)).
# ---------------------------------------------------------------------------

STYLES = [
    # (name, display name, family, parent, paragraph props, text props)
    ("Heading_20_1", "Heading 1", "paragraph", "Standard",
     {"space_before": 12, "space_after": 6}, {"size": 20, "bold": True}),
    ("Heading_20_2", "Heading 2", "paragraph", "Standard",
     {"space_before": 10, "space_after": 4}, {"size": 16, "bold": True, "italic": True}),
    ("Mon_20_Style", "Mon Style", "paragraph", "Standard",
     {"alignment": "center"}, {"color": "336699", "italic": True}),
    ("Emphase_20_forte", "Emphase forte", "text", None, {}, {"bold": True, "color": "CC0000"}),
]

# Underline descriptions: name -> (ODF style, ODF type, ODF width, model value).
UL = {
    "single": ("solid", "single", "auto", "single"),
    "double": ("solid", "double", "auto", "double"),
    "thick": ("solid", "single", "bold", "thick"),
    "dotted": ("dotted", "single", "auto", "dotted"),
    "dash": ("dash", "single", "auto", "dashed"),
    "long-dash": ("long-dash", "single", "auto", "dashed"),
    "dot-dash": ("dot-dash", "single", "auto", "dashed"),
    "dot-dot-dash": ("dot-dot-dash", "single", "auto", "dashed"),
    "wave": ("wave", "single", "auto", "wave"),
    "dotted-bold": ("dotted", "single", "bold", "dotted"),
    "dash-bold": ("dash", "single", "bold", "dashed"),
    "long-dash-bold": ("long-dash", "single", "bold", "dashed"),
    "dot-dash-bold": ("dot-dash", "single", "bold", "dashed"),
    "dot-dot-dash-bold": ("dot-dot-dash", "single", "bold", "dashed"),
    "wave-bold": ("wave", "single", "bold", "wave"),
    "wave-double": ("wave", "double", "auto", "wave"),
}


DOCS = {
    "text-accents": {
        "meta": {
            "title": "Référence US-312 : accents",
            "subject": "Texte accentué",
            "initial_creator": "Atelier",
            "creator": "Révision",
            "keywords": "doc; accents; Unicode",
            "description": "Fichier de référence généré par LibreOffice.",
            "creation_date": "2024-03-05T10:20:30",
            "date": "2024-03-06T11:21:31",
        },
        "paragraphs": [
            {"runs": [("Café crème – « guillemets » … 5 € ™", {})]},
            {"runs": [("Œuvre ÿ œ Ç à ü ñ ß", {})]},
            {"runs": [("日本語 et ελληνικά", {})]},
            {"runs": [("Émoji 😀 fin", {})]},
            {"runs": [("Un", {}), ("\t", {}), ("deux", {})]},
        ],
    },
    "char-formatting": {
        "meta": {"title": "Mise en forme de caractère", "initial_creator": "Atelier"},
        "paragraphs": [
            {"runs": [
                ("Normal ", {}),
                ("gras", {"bold": True}),
                (" ", {}),
                ("italique", {"italic": True}),
                (" ", {}),
                ("souligné", {"underline": "single"}),
                (" ", {}),
                ("double", {"underline": "double"}),
                (" ", {}),
                ("barré", {"strike": True}),
            ]},
            {"runs": [
                ("E=mc", {}),
                ("2", {"vertical_align": "superscript"}),
                (" H", {}),
                ("2", {"vertical_align": "subscript"}),
                ("O", {}),
            ]},
            {"runs": [
                ("rouge", {"color": "FF0000"}),
                (" ", {}),
                ("bleu-gris", {"color": "336699"}),
                (" ", {}),
                ("surligné", {"highlight": "FFFF00"}),
            ]},
            {"runs": [
                ("grand", {"size": 18}),
                (" ", {}),
                ("petit", {"size": 8}),
                (" ", {}),
                ("Arial", {"font": "Arial"}),
                (" ", {}),
                ("Courier", {"font": "Courier New"}),
                (" ", {}),
                ("tout", {"bold": True, "italic": True, "underline": "single", "size": 14,
                          "color": "00AA00", "font": "Arial"}),
            ]},
        ],
    },
    "underlines": {
        "meta": {"title": "Soulignements", "initial_creator": "Atelier"},
        "paragraphs": [
            {"runs": [(name, {"underline": name})]} for name in UL
        ],
    },
    "para-formatting": {
        "meta": {"title": "Mise en forme de paragraphe", "initial_creator": "Atelier"},
        "paragraphs": [
            {"runs": [("Gauche", {})], "props": {"alignment": "left"}},
            {"runs": [("Centré", {})], "props": {"alignment": "center"}},
            {"runs": [("Droite", {})], "props": {"alignment": "right"}},
            {"runs": [("Justifié", {})], "props": {"alignment": "justify"}},
            {"runs": [("Retraits", {})],
             "props": {"indent_left": 36, "indent_right": 18, "indent_first_line": -18}},
            {"runs": [("Première ligne", {})], "props": {"indent_left": 0, "indent_first_line": 28.35}},
            {"runs": [("Espacements", {})], "props": {"space_before": 12, "space_after": 6}},
            {"runs": [("Interligne 150 %", {})], "props": {"line": ("auto", 150)}},
            {"runs": [("Interligne exact 14 pt", {})], "props": {"line": ("exact", 14)}},
            {"runs": [("Interligne au moins 20 pt", {})], "props": {"line": ("at_least", 20)}},
            {"runs": [("Fin", {})]},
        ],
    },
    "styles": {
        "meta": {"title": "Styles", "initial_creator": "Atelier"},
        "paragraphs": [
            {"runs": [("Titre de niveau 1", {})], "style": "Heading 1"},
            {"runs": [("Texte courant", {})]},
            {"runs": [("Titre de niveau 2", {})], "style": "Heading 2"},
            {"runs": [("Style personnalisé hérité", {})], "style": "Mon Style"},
            {"runs": [("Avec ", {}), ("emphase forte", {"style": "Emphase forte"}), (" au milieu", {})]},
            {"runs": [("Élément de liste", {})], "style": "List"},
        ],
    },
}

# Resolved properties expected by the Rust test, per paragraph style.
EXPECTED_STYLES = {
    "Heading 1": {
        "id": "Heading1",
        "paragraph": {"space_before": 240, "space_after": 120},
        "character": {"bold": True, "size_half_points": 40},
    },
    "Heading 2": {
        "id": "Heading2",
        "paragraph": {"space_before": 200, "space_after": 80},
        "character": {"bold": True, "italic": True, "size_half_points": 32},
    },
    "Mon Style": {
        "id": "MonStyle",
        "paragraph": {"alignment": "center"},
        "character": {"italic": True, "color": "336699"},
    },
    # LibreOffice's built-in "List" style is based on its built-in "Body Text"
    # (line spacing 115 %, 7 pt after): the inheritance below comes from the
    # producer, not from this description.
    "List": {
        "id": "List",
        "paragraph": {"line_spacing": {"rule": "auto", "value": 276}, "space_after": 140},
        "character": {},
    },
}

# ---------------------------------------------------------------------------
# Flat ODT writer.
# ---------------------------------------------------------------------------

FONTS = ["Liberation Serif", "Arial", "Courier New"]


def pt(v: float) -> str:
    return f"{v:g}pt"


def text_props_xml(p: dict) -> str:
    a = []
    if p.get("bold"):
        a.append('fo:font-weight="bold"')
    if p.get("italic"):
        a.append('fo:font-style="italic"')
    if "underline" in p:
        style, kind, width, _ = UL[p["underline"]]
        a.append(f'style:text-underline-style="{style}" style:text-underline-type="{kind}" '
                 f'style:text-underline-width="{width}" style:text-underline-color="font-color"')
    if p.get("strike"):
        a.append('style:text-line-through-style="solid"')
    if p.get("vertical_align") == "superscript":
        a.append('style:text-position="super 58%"')
    if p.get("vertical_align") == "subscript":
        a.append('style:text-position="sub 58%"')
    if "color" in p:
        a.append(f'fo:color="#{p["color"]}"')
    if "highlight" in p:
        a.append(f'fo:background-color="#{p["highlight"]}"')
    if "size" in p:
        a.append(f'fo:font-size="{pt(p["size"])}"')
    if "font" in p:
        a.append(f'style:font-name="{p["font"]}"')
    return f"<style:text-properties {' '.join(a)}/>" if a else ""


def para_props_xml(p: dict) -> str:
    a = []
    align = {"left": "start", "center": "center", "right": "end", "justify": "justify"}
    if "alignment" in p:
        a.append(f'fo:text-align="{align[p["alignment"]]}"')
    for key, attr in (("indent_left", "fo:margin-left"), ("indent_right", "fo:margin-right"),
                      ("indent_first_line", "fo:text-indent"),
                      ("space_before", "fo:margin-top"), ("space_after", "fo:margin-bottom")):
        if key in p:
            a.append(f'{attr}="{pt(p[key])}"')
    if "line" in p:
        kind, v = p["line"]
        if kind == "auto":
            a.append(f'fo:line-height="{v}%"')
        elif kind == "exact":
            a.append(f'fo:line-height="{pt(v)}"')
        else:
            a.append(f'style:line-height-at-least="{pt(v)}"')
    return f"<style:paragraph-properties {' '.join(a)}/>" if a else ""


def build_fodt(doc: dict) -> str:
    auto = []
    body = []
    pcount = tcount = 0
    for para in doc["paragraphs"]:
        pprops = para.get("props", {})
        attrs = f'text:style-name="{para.get("style", "Standard").replace(" ", "_20_")}"'
        if pprops:
            pcount += 1
            name = f"P{pcount}"
            parent = para.get("style", "Standard").replace(" ", "_20_")
            auto.append(f'<style:style style:name="{name}" style:family="paragraph" '
                        f'style:parent-style-name="{parent}">{para_props_xml(pprops)}</style:style>')
            attrs = f'text:style-name="{name}"'
        spans = []
        for text, props in para["runs"]:
            inner = []
            for i, chunk in enumerate(text.split("\t")):
                if i:
                    inner.append("<text:tab/>")
                inner.append(escape(chunk))
            content = "".join(inner)
            direct = {k: v for k, v in props.items() if k != "style"}
            if "style" in props:
                content = (f'<text:span text:style-name="{props["style"].replace(" ", "_20_")}">'
                           f"{content}</text:span>")
            if direct:
                tcount += 1
                name = f"T{tcount}"
                auto.append(f'<style:style style:name="{name}" style:family="text">'
                            f"{text_props_xml(direct)}</style:style>")
                content = f'<text:span text:style-name="{name}">{content}</text:span>'
            spans.append(content)
        body.append(f"<text:p {attrs}>{''.join(spans)}</text:p>")
    styles = []
    for name, display, family, parent, pp, tp in STYLES:
        par = f' style:parent-style-name="{parent}"' if parent else ""
        fam = "paragraph" if family == "paragraph" else "text"
        extra = ' style:next-style-name="Standard"' if family == "paragraph" else ""
        styles.append(f'<style:style style:name="{name}" style:display-name="{display}" '
                      f'style:family="{fam}"{par}{extra}>{para_props_xml(pp)}'
                      f"{text_props_xml(tp)}</style:style>")
    m = doc["meta"]
    meta = "".join(f"<{tag}>{escape(m[key])}</{tag}>" for key, tag in (
        ("title", "dc:title"), ("subject", "dc:subject"), ("initial_creator", "meta:initial-creator"),
        ("creator", "dc:creator"), ("keywords", "meta:keyword"), ("description", "dc:description"),
        ("creation_date", "meta:creation-date"), ("date", "dc:date")) if key in m)
    faces = "".join(f'<style:font-face style:name="{f}" svg:font-family="{f}"/>' for f in FONTS)
    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        f'<office:document {NS} office:version="1.3" '
        'office:mimetype="application/vnd.oasis.opendocument.text">'
        f"<office:meta>{meta}</office:meta>"
        f"<office:font-face-decls>{faces}</office:font-face-decls>"
        "<office:styles>"
        '<style:default-style style:family="paragraph">'
        '<style:text-properties style:font-name="Liberation Serif" fo:font-size="12pt"/>'
        "</style:default-style>"
        '<style:style style:name="Standard" style:family="paragraph" style:class="text"/>'
        f"{''.join(styles)}</office:styles>"
        f"<office:automatic-styles>{''.join(auto)}</office:automatic-styles>"
        f"<office:body><office:text>{''.join(body)}</office:text></office:body>"
        "</office:document>\n"
    )


# ---------------------------------------------------------------------------
# Expected values.
# ---------------------------------------------------------------------------

ALIGN = {"left": "left", "center": "center", "right": "right", "justify": "justify"}


def twips(points: float) -> int:
    return round(points * 20)


def expected_paragraph(para: dict) -> dict:
    out = {"text": "".join(t for t, _ in para["runs"])}
    style = para.get("style")
    if style in EXPECTED_STYLES:
        out["style"] = EXPECTED_STYLES[style]["id"]
        out["style_effective"] = EXPECTED_STYLES[style]
    p = para.get("props", {})
    props = {}
    if "alignment" in p:
        props["alignment"] = ALIGN[p["alignment"]]
    for key in ("indent_left", "indent_right", "indent_first_line", "space_before", "space_after"):
        if key in p:
            props[key] = twips(p[key])
    if "line" in p:
        kind, v = p["line"]
        props["line_spacing"] = (
            {"rule": "auto", "value": round(v * 240 / 100)} if kind == "auto"
            else {"rule": kind, "value": twips(v)}
        )
    if props:
        out["props"] = props
    runs = []
    for text, rp in para["runs"]:
        e = {}
        for key in ("bold", "italic", "strike", "vertical_align", "color", "highlight", "font"):
            if key in rp:
                e[key] = rp[key]
        if "underline" in rp:
            e["underline"] = UL[rp["underline"]][3]
        if "size" in rp:
            e["size_half_points"] = round(rp["size"] * 2)
        if "style" in rp:
            e["char_style"] = "Emphaseforte"
            e["bold"] = True
            e["color"] = "CC0000"
        if e:
            runs.append({"text": text, **e})
    if runs:
        out["runs"] = runs
    return out


def expected_meta(meta: dict) -> dict:
    """Text properties promised by the description (dates depend on the producer)."""
    keys = {"title": "title", "subject": "subject", "initial_creator": "author",
            "creator": "last_author", "keywords": "keywords", "description": "comments"}
    return {keys[k]: v for k, v in meta.items() if k in keys}


def read_summary(doc_bytes: bytes) -> dict:
    """Independent reader of the summary information property set."""
    data = read_cfb(doc_bytes)["\x05SummaryInformation"]
    base = struct.unpack_from("<I", data, 44)[0]
    count = struct.unpack_from("<II", data, base)[1]
    names = {2: "title", 3: "subject", 4: "author", 5: "keywords", 6: "comments", 8: "last_author"}
    codepage, out = 1252, {}
    for i in range(count):
        pid, off = struct.unpack_from("<II", data, base + 8 + 8 * i)
        if pid == 1:
            codepage = struct.unpack_from("<H", data, base + off + 4)[0]
    for i in range(count):
        pid, off = struct.unpack_from("<II", data, base + 8 + 8 * i)
        ty = struct.unpack_from("<H", data, base + off)[0] & 0xFFF
        at = base + off + 4
        if pid in names and ty == 0x1E:
            n = struct.unpack_from("<I", data, at)[0]
            raw = data[at + 4:at + 4 + n].split(bytes(1))[0]
            out[names[pid]] = raw.decode("utf-8" if codepage == 65001 else f"cp{codepage}")
        elif pid in (12, 13) and ty == 0x40:
            ft = struct.unpack_from("<Q", data, at)[0]
            if ft:
                out["created_unix" if pid == 12 else "modified_unix"] = ft // 10_000_000 - 11644473600
    return out


# ---------------------------------------------------------------------------
# Independent reader: CFB, FIB, piece table (no Rust code involved).
# ---------------------------------------------------------------------------

def read_cfb(data: bytes) -> dict[str, bytes]:
    if data[:8] != bytes.fromhex("d0cf11e0a1b11ae1"):
        raise ValueError("not a compound file")
    shift = struct.unpack_from("<H", data, 30)[0]
    sector = 1 << shift
    mini_sector = 1 << struct.unpack_from("<H", data, 32)[0]
    n_fat, dir_start, _, cutoff, minifat_start, n_minifat, difat_start, n_difat = (
        struct.unpack_from("<IIIIIIII", data, 44))

    def sec(i: int) -> bytes:
        return data[(i + 1) * sector:(i + 2) * sector]

    difat = list(struct.unpack_from("<109I", data, 76))
    s = difat_start
    for _ in range(n_difat):
        vals = struct.unpack("<%dI" % (sector // 4), sec(s))
        difat.extend(vals[:-1])
        s = vals[-1]
    fat: list[int] = []
    for i in difat[:n_fat]:
        fat.extend(struct.unpack("<%dI" % (sector // 4), sec(i)))

    def chain(start: int) -> list[int]:
        out = []
        while start < 0xFFFFFFFA:
            out.append(start)
            start = fat[start]
            if len(out) > len(fat):
                raise ValueError("FAT cycle")
        return out

    def read_chain(start: int) -> bytes:
        return b"".join(sec(i) for i in chain(start))

    directory = read_chain(dir_start)
    entries = []
    for off in range(0, len(directory), 128):
        e = directory[off:off + 128]
        nlen = struct.unpack_from("<H", e, 64)[0]
        name = e[:max(nlen - 2, 0)].decode("utf-16-le")
        kind = e[66]
        start, size = struct.unpack_from("<I", e, 116)[0], struct.unpack_from("<Q", e, 120)[0]
        entries.append((name, kind, start, size))
    root = entries[0]
    ministream = read_chain(root[2])[:root[3]]
    minifat = list(struct.unpack("<%dI" % (len(read_chain(minifat_start)) // 4),
                                 read_chain(minifat_start))) if n_minifat else []
    out = {}
    for name, kind, start, size in entries[1:]:
        if kind != 2:
            continue
        if size < cutoff:
            buf, s = b"", start
            while s < 0xFFFFFFFA:
                buf += ministream[s * mini_sector:(s + 1) * mini_sector]
                s = minifat[s]
            out[name] = buf[:size]
        else:
            out[name] = read_chain(start)[:size]
    return out


CP1252_HIGH = {
    0x80: "\u20ac", 0x82: "\u201a", 0x83: "\u0192", 0x84: "\u201e", 0x85: "\u2026",
    0x86: "\u2020", 0x87: "\u2021", 0x88: "\u02c6", 0x89: "\u2030", 0x8A: "\u0160",
    0x8B: "\u2039", 0x8C: "\u0152", 0x8E: "\u017d", 0x91: "\u2018", 0x92: "\u2019",
    0x93: "\u201c", 0x94: "\u201d", 0x95: "\u2022", 0x96: "\u2013", 0x97: "\u2014",
    0x98: "\u02dc", 0x99: "\u2122", 0x9A: "\u0161", 0x9B: "\u203a", 0x9C: "\u0153",
    0x9E: "\u017e", 0x9F: "\u0178",
}


def main_text(doc_bytes: bytes) -> tuple[str, dict]:
    streams = read_cfb(doc_bytes)
    word = streams["WordDocument"]
    ident, nfib = struct.unpack_from("<HH", word, 0)
    flags = struct.unpack_from("<H", word, 10)[0]
    if ident != 0xA5EC or nfib < 0xC1 or flags & 0x100:
        raise ValueError("not a readable Word 97+ document")
    csw = struct.unpack_from("<H", word, 32)[0]
    lw_at = 34 + csw * 2
    cslw = struct.unpack_from("<H", word, lw_at)[0]
    ccp_text = struct.unpack_from("<I", word, lw_at + 2 + 4 * 3)[0]
    pairs_at = lw_at + 2 + cslw * 4 + 2
    fc_clx, lcb_clx = struct.unpack_from("<II", word, pairs_at + 33 * 8)
    table = streams["1Table" if flags & 0x200 else "0Table"]
    clx = table[fc_clx:fc_clx + lcb_clx]
    pos = 0
    while clx[pos] == 1:
        pos += 3 + struct.unpack_from("<H", clx, pos + 1)[0]
    assert clx[pos] == 2
    size = struct.unpack_from("<I", clx, pos + 1)[0]
    plc = clx[pos + 5:pos + 5 + size]
    n = (len(plc) - 4) // 12
    cps = struct.unpack_from("<%dI" % (n + 1), plc, 0)
    text = []
    for i in range(n):
        fc_raw = struct.unpack_from("<I", plc, 4 * (n + 1) + 8 * i + 2)[0]
        count = cps[i + 1] - cps[i]
        if fc_raw & 0x40000000:
            off = (fc_raw & 0x3FFFFFFF) // 2
            raw = word[off:off + count]
            text.append("".join(CP1252_HIGH.get(b, chr(b)) for b in raw))
        else:
            text.append(word[fc_raw:fc_raw + 2 * count].decode("utf-16-le", "surrogatepass"))
    full = "".join(text)
    # CPs are UTF-16 units: measure in units.
    units = len(full.encode("utf-16-le", "surrogatepass")) // 2
    if units < ccp_text:
        raise ValueError("piece table shorter than ccpText")
    cut = full.encode("utf-16-le", "surrogatepass")[:2 * ccp_text].decode("utf-16-le", "surrogatepass")
    return cut, {"n_fib": nfib, "ccp_text": ccp_text, "pieces": n,
                 "table": "1Table" if flags & 0x200 else "0Table"}


def read_kuls(doc_bytes: bytes) -> list[int]:
    """sprmCKul (0x2A3E) operands in file order, read from the CHPX pages."""
    streams = read_cfb(doc_bytes)
    word = streams["WordDocument"]
    flags = struct.unpack_from("<H", word, 10)[0]
    table = streams["1Table" if flags & 0x200 else "0Table"]
    csw = struct.unpack_from("<H", word, 32)[0]
    lw_at = 34 + csw * 2
    cslw = struct.unpack_from("<H", word, lw_at)[0]
    pairs_at = lw_at + 2 + cslw * 4 + 2
    fc, lcb = struct.unpack_from("<II", word, pairs_at + 12 * 8)
    n = (lcb - 4) // 8
    found = []
    for i in range(n):
        pn = struct.unpack_from("<I", table, fc + 4 * (n + 1) + 4 * i)[0] & 0x3FFFFF
        page = word[pn * 512:(pn + 1) * 512]
        crun = page[511]
        for k in range(crun):
            start = struct.unpack_from("<I", page, 4 * k)[0]
            b = page[4 * (crun + 1) + k]
            if not b:
                continue
            cb = page[b * 2]
            g = page[b * 2 + 1:b * 2 + 1 + cb]
            pos = 0
            while pos + 2 <= len(g):
                op = struct.unpack_from("<H", g, pos)[0]
                spra = op >> 13
                size = {0: 1, 1: 1, 2: 2, 3: 4, 4: 2, 5: 2, 7: 3}.get(spra)
                if size is None:
                    size = g[pos + 2] + 1
                if op == 0x2A3E:
                    found.append((start, g[pos + 2]))
                pos += 2 + size
    return [v for _, v in sorted(found)]


def paragraphs_of(text: str) -> list[str]:
    assert text.endswith("\r"), "main text must end with a paragraph mark"
    return text[:-1].split("\r")


# ---------------------------------------------------------------------------
# US-313 structure documents: tables, lists, sections, headers, fields,
# bookmarks. Each document is described by a flat ODT (written by hand below)
# and by `expect`: what the description promises, in two views. "file" is
# checked against the independent reader of this script; "model" is what the
# Rust importer must produce (checked by crates/fmt-legacy/tests/
# reference_structure.rs). Widths are in twips (1 cm = 567 twips).
# ---------------------------------------------------------------------------

STRUCT_NS = NS + (
    ' xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"'
    ' xmlns:xlink="http://www.w3.org/1999/xlink"'
)


def struct_fodt(styles: str, auto: str, master: str, body: str) -> str:
    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        f'<office:document {STRUCT_NS} office:version="1.3" '
        'office:mimetype="application/vnd.oasis.opendocument.text">'
        '<office:meta><dc:title>US-313</dc:title></office:meta>'
        f"<office:styles>{styles}</office:styles>"
        f"<office:automatic-styles>{auto}</office:automatic-styles>"
        f"<office:master-styles>{master}</office:master-styles>"
        f"<office:body><office:text>{body}</office:text></office:body></office:document>\n"
    )


def cell(text: str, style: str = "", extra: str = "") -> str:
    st = f' table:style-name="{style}"' if style else ""
    return (f'<table:table-cell{st} office:value-type="string"{extra}>'
            f"<text:p>{text}</text:p></table:table-cell>")


def cell_open(style: str = "") -> str:
    st = f' table:style-name="{style}"' if style else ""
    return f'<table:table-cell{st} office:value-type="string">'


TABLES_FODT = struct_fodt(
    "",
    '<style:style style:name="TA" style:family="table"><style:table-properties style:width="12cm" table:align="left"/></style:style>'
    '<style:style style:name="TA.A" style:family="table-column"><style:table-column-properties style:column-width="4cm"/></style:style>'
    '<style:style style:name="Cb" style:family="table-cell"><style:table-cell-properties fo:border="0.5pt solid #000000" fo:padding="0.1cm"/></style:style>'
    '<style:style style:name="TB" style:family="table"><style:table-properties style:width="12cm" table:align="center" fo:margin-left="0cm"/></style:style>'
    '<style:style style:name="TB.A" style:family="table-column"><style:table-column-properties style:column-width="6cm"/></style:style>'
    '<style:style style:name="RowH" style:family="table-row"><style:table-row-properties style:row-height="1.5cm" fo:keep-together="always"/></style:style>'
    '<style:style style:name="RowE" style:family="table-row"><style:table-row-properties style:min-row-height="1cm"/></style:style>'
    '<style:style style:name="Cx" style:family="table-cell"><style:table-cell-properties fo:border-top="1pt double #FF0000" fo:border-left="1pt dotted #00FF00" fo:border-bottom="1pt dashed #0000FF" fo:border-right="none" style:vertical-align="middle" fo:background-color="#FFFF00"/></style:style>'
    '<style:style style:name="Cy" style:family="table-cell"><style:table-cell-properties style:vertical-align="bottom" fo:border="none"/></style:style>',
    "",
    "<text:p>Avant</text:p>"
    '<table:table table:name="A" table:style-name="TA">'
    '<table:table-column table:style-name="TA.A" table:number-columns-repeated="3"/>'
    f'<table:table-row>{cell("A1-2", "Cb", " table:number-columns-spanned=" + chr(34) + "2" + chr(34))}<table:covered-table-cell/>{cell("A3", "Cb")}</table:table-row>'
    f'<table:table-row>{cell("B1", "Cb", " table:number-rows-spanned=" + chr(34) + "2" + chr(34))}{cell("B2", "Cb")}{cell("B3", "Cb")}</table:table-row>'
    f'<table:table-row><table:covered-table-cell/>{cell("C2", "Cb")}{cell("C3", "Cb")}</table:table-row>'
    "</table:table>"
    "<text:p>Entre</text:p>"
    '<table:table table:name="B" table:style-name="TB">'
    '<table:table-column table:style-name="TB.A" table:number-columns-repeated="2"/>'
    '<table:table-header-rows><table:table-row table:style-name="RowH">'
    f'{cell("H1", "Cx")}{cell("H2", "Cy")}</table:table-row></table:table-header-rows>'
    '<table:table-row table:style-name="RowE">'
    f'{cell_open()}<text:p>Ext 1</text:p>'
    '<table:table table:name="C"><table:table-column table:number-columns-repeated="2"/>'
    f'<table:table-row>{cell("In a")}{cell("In b")}</table:table-row></table:table>'
    f'<text:p>Apres interne</text:p></table:table-cell>{cell("Ext 2")}</table:table-row>'
    "</table:table>"
    "<text:p>Fin</text:p>",
)

_LIST_LEVEL = (
    '<text:list-level-style-{kind} text:level="{n}" {attrs}><style:list-level-properties '
    'text:list-level-position-and-space-mode="label-alignment"><style:list-level-label-alignment '
    'text:label-followed-by="listtab" text:list-tab-stop-position="{pos}cm" fo:text-indent="-0.5cm" '
    'fo:margin-left="{pos}cm"/></style:list-level-properties></text:list-level-style-{kind}>'
)


def list_style(name: str, levels: list) -> str:
    out = []
    for n, (kind, attrs) in enumerate(levels, start=1):
        out.append(_LIST_LEVEL.format(kind=kind, n=n, attrs=attrs, pos=n))
    return f'<text:list-style style:name="{name}">{"".join(out)}</text:list-style>'


def item(text: str, inner: str = "") -> str:
    return f"<text:list-item><text:p>{text}</text:p>{inner}</text:list-item>"


def lst(style: str, items: str) -> str:
    return f'<text:list text:style-name="{style}">{items}</text:list>'


def sub(items: str) -> str:
    return f"<text:list>{items}</text:list>"


LISTS_FODT = struct_fodt(
    list_style("LBul", [("bullet", 'text:bullet-char="•"'), ("bullet", 'text:bullet-char="◦"')])
    + list_style("LNum", [
        ("number", 'style:num-suffix="." style:num-format="1"'),
        ("number", 'style:num-suffix=")" style:num-format="a"'),
        ("number", 'style:num-suffix="." style:num-format="i" text:display-levels="1"'),
    ])
    + list_style("LUp", [
        ("number", 'style:num-suffix="." style:num-format="I"'),
        ("number", 'style:num-suffix=")" style:num-format="A"'),
    ]),
    "", "",
    lst("LBul", item("Puce 1", sub(item("Sous-puce"))) + item("Puce 2"))
    + "<text:p>Intercalaire</text:p>"
    + lst("LNum", item("Num 1", sub(item("Sous a", sub(item("Sous i"))))) + item("Num 2"))
    + lst("LUp", item("Romain", sub(item("Lettre"))))
    + lst("LNum", '<text:list-item text:start-value="5"><text:p>Redemarre 5</text:p></text:list-item>'),
)

SECTIONS_FODT = struct_fodt(
    "",
    '<style:page-layout style:name="pmP"><style:page-layout-properties fo:page-width="21cm" fo:page-height="29.7cm" style:print-orientation="portrait" fo:margin-top="2cm" fo:margin-bottom="2cm" fo:margin-left="2.5cm" fo:margin-right="2.5cm"/></style:page-layout>'
    '<style:page-layout style:name="pmL"><style:page-layout-properties fo:page-width="29.7cm" fo:page-height="21cm" style:print-orientation="landscape" fo:margin-top="1.5cm" fo:margin-bottom="1.5cm" fo:margin-left="3cm" fo:margin-right="3cm"/></style:page-layout>'
    '<style:page-layout style:name="pmR"><style:page-layout-properties fo:page-width="21cm" fo:page-height="29.7cm" style:print-orientation="portrait" style:num-format="i" fo:margin-top="2cm" fo:margin-bottom="2cm" fo:margin-left="2cm" fo:margin-right="2cm"/></style:page-layout>'
    '<style:style style:name="PBL" style:family="paragraph" style:master-page-name="Land"><style:paragraph-properties/></style:style>'
    '<style:style style:name="PBR" style:family="paragraph" style:master-page-name="Rom"><style:paragraph-properties style:page-number="5"/></style:style>'
    '<style:style style:name="Sect2" style:family="section"><style:section-properties><style:columns fo:column-count="2" fo:column-gap="1cm"><style:column style:rel-width="1*" fo:start-color="#000000"/><style:column style:rel-width="1*"/></style:columns></style:section-properties></style:style>',
    '<style:master-page style:name="Standard" style:page-layout-name="pmP"/>'
    '<style:master-page style:name="Land" style:page-layout-name="pmL"/>'
    '<style:master-page style:name="Rom" style:page-layout-name="pmR"/>',
    "<text:p>Portrait</text:p>"
    '<text:p text:style-name="PBL">Paysage</text:p>'
    '<text:p text:style-name="PBR">Romain</text:p>'
    '<text:section text:style-name="Sect2" text:name="Cols"><text:p>Deux colonnes</text:p></text:section>'
    "<text:p>Apres</text:p>",
)

HEADERS_FODT = struct_fodt(
    "",
    '<style:page-layout style:name="pmH"><style:page-layout-properties fo:page-width="21cm" fo:page-height="29.7cm" style:print-orientation="portrait" fo:margin-top="2cm" fo:margin-bottom="2cm" fo:margin-left="2.5cm" fo:margin-right="2.5cm"/>'
    '<style:header-style><style:header-footer-properties fo:min-height="0.5cm" fo:margin-bottom="0.3cm"/></style:header-style>'
    '<style:footer-style><style:header-footer-properties fo:min-height="0.5cm" fo:margin-top="0.3cm"/></style:footer-style></style:page-layout>'
    '<style:style style:name="PB2" style:family="paragraph" style:master-page-name="Deux"><style:paragraph-properties/></style:style>',
    '<style:master-page style:name="Standard" style:page-layout-name="pmH">'
    '<style:header><text:p>Entete <text:page-number>1</text:page-number></text:p></style:header>'
    '<style:header-first><text:p>Premiere entete</text:p></style:header-first>'
    '<style:footer><text:p>Pied <text:page-count>1</text:page-count></text:p></style:footer></style:master-page>'
    '<style:master-page style:name="Deux" style:page-layout-name="pmH">'
    '<style:header><text:p>Entete deux</text:p></style:header>'
    '<style:footer><text:p>Pied deux</text:p></style:footer></style:master-page>',
    "<text:p>Page un</text:p>"
    '<text:p text:style-name="PB2">Section deux</text:p>',
)

FIELDS_FODT = struct_fodt(
    "", "", "",
    '<text:p>Un <text:bookmark text:name="Point"/>texte <text:bookmark-start text:name="Plage"/>avec plage<text:bookmark-end text:name="Plage"/> fin</text:p>'
    '<text:p>Debut <text:bookmark-start text:name="Deux"/>de plage</text:p>'
    '<text:p>suite<text:bookmark-end text:name="Deux"/> du texte</text:p>'
    '<text:p>Lien <text:a xlink:href="http://example.com/x?a=1" office:title="Infobulle">externe</text:a>'
    ' et <text:a xlink:href="#Point">interne</text:a>'
    ' date <text:date style:data-style-name="N1" text:date-value="2024-01-02">02/01/2024</text:date>'
    ' heure <text:time>10:00:00</text:time></text:p>'
    "<text:p>Page <text:page-number>1</text:page-number> sur <text:page-count>1</text:page-count></text:p>",
)

# "file": checked against read_structure() of this script.
# "model": checked against the Rust importer.
STRUCT_DOCS = {
    "struct-tables": {
        "fodt": TABLES_FODT,
        "expect": {
            "file": {
                # Rows in end order: inner rows end before their outer row.
                # Cells: (text, width, vertical merge, vertical alignment).
                "rows": [
                    {"itap": 1, "cells": [("A1-2", 4536, None, 0), ("A3", 2268, None, 0)]},
                    {"itap": 1, "cells": [("B1", 2268, "restart", 0), ("B2", 2268, None, 0), ("B3", 2268, None, 0)]},
                    {"itap": 1, "cells": [("", 2268, "cont", 0), ("C2", 2268, None, 0), ("C3", 2268, None, 0)]},
                    {"itap": 1, "cells": [("H1", 3402, None, 1), ("H2", 3402, None, 2)],
                     "header": True, "height": -850, "cant_split": True, "jc": 1},
                    {"itap": 2, "cells": [("In a", 1701, None, 0), ("In b", 1701, None, 0)]},
                    {"itap": 1, "cells": [("Ext 1\nIn a\nIn b\nApres interne".replace("\nIn a\nIn b", ""), 3402, None, 0),
                                          ("Ext 2", 3402, None, 0)],
                     "height": 567, "jc": 1},
                ],
                "borders": {"H1": {"top": ["double", "FF0000"], "left": ["dotted", "00FF00"], "bottom": ["dashed", "0000FF"]}},
                "shading": {"H1": "FFFF00"},
            },
            "model": {
                "tables": [
                    {"depth": 0, "columns": 3, "grid": [2268, 2268, 2268],
                     "cells": [[0, 0, 1, 2, "A1-2"], [0, 2, 1, 1, "A3"], [1, 0, 2, 1, "B1"], [1, 1, 1, 1, "B2"],
                               [1, 2, 1, 1, "B3"], [2, 1, 1, 1, "C2"], [2, 2, 1, 1, "C3"]]},
                    {"depth": 0, "columns": 2, "grid": [3402, 3402], "align": "center",
                     "cells": [[0, 0, 1, 1, "H1"], [0, 1, 1, 1, "H2"], [1, 0, 1, 1, "Ext 1"], [1, 1, 1, 1, "Ext 2"]],
                     "rows": [{"header": True, "height": [850, "exact"], "cant_split": True},
                              {"header": None, "height": [567, "at_least"], "cant_split": False}],
                     "cell_props": {
                         "H1": {"valign": "center", "fill": "FFFF00",
                                "top": ["double", "FF0000"], "left": ["dotted", "00FF00"], "bottom": ["dashed", "0000FF"]},
                         "H2": {"valign": "bottom"}}},
                    {"depth": 1, "columns": 2, "grid": [1701, 1701],
                     "cells": [[0, 0, 1, 1, "In a"], [0, 1, 1, 1, "In b"]]},
                ],
                "body_texts": ["Avant", "A1-2", "A3", "B1", "B2", "B3", "C2", "C3", "Entre", "H1", "H2",
                               "Ext 1", "In a", "In b", "Apres interne", "Ext 2", "Fin"],
            },
        },
    },
    "struct-lists": {
        "fodt": LISTS_FODT,
        "expect": {
            "file": {
                # (text, group, level); groups number the distinct LFOs used.
                "paragraphs": [("Puce 1", 0, 0), ("Sous-puce", 0, 1), ("Puce 2", 0, 0),
                               ("Num 1", 1, 0), ("Sous a", 1, 1), ("Sous i", 1, 2), ("Num 2", 1, 0),
                               ("Romain", 2, 0), ("Lettre", 2, 1), ("Redemarre 5", 3, 0)],
                # First levels of each group: (nfc, text, start); bullets: (23, text).
                "groups": [
                    [[23, ""], [23, "◦"]],
                    [[0, "%1.", 1], [4, "%2)", 1], [2, "%3.", 1]],
                    [[1, "%1.", 1], [3, "%2)", 1]],
                    [[0, "%1.", 5], [4, "%2)", 1], [2, "%3.", 1]],
                ],
            },
            "model": {
                "paragraphs": [("Puce 1", 0, 0), ("Sous-puce", 0, 1), ("Puce 2", 0, 0),
                               ("Num 1", 1, 0), ("Sous a", 1, 1), ("Sous i", 1, 2), ("Num 2", 1, 0),
                               ("Romain", 2, 0), ("Lettre", 2, 1), ("Redemarre 5", 3, 0)],
                "groups": [
                    [["bullet", ""], ["bullet", "◦"]],
                    [["Decimal", "%1.", 1], ["LowerLetter", "%2)", 1], ["LowerRoman", "%3.", 1]],
                    [["UpperRoman", "%1.", 1], ["UpperLetter", "%2)", 1]],
                    [["Decimal", "%1.", 5], ["LowerLetter", "%2)", 1], ["LowerRoman", "%3.", 1]],
                ],
            },
        },
    },
    "struct-sections": {
        "fodt": SECTIONS_FODT,
        "expect": {
            "file": {
                # width, height, landscape, margins left/right/top/bottom, columns,
                # break (0 continuous, 2 new page), nfcPgn, restart, start
                "sections": [
                    {"text": "Portrait", "width": 11906, "height": 16838, "landscape": False,
                     "margins": [1417, 1417, 1134, 1134], "columns": 1, "bkc": 2, "nfc": 0},
                    {"text": "Paysage", "width": 16838, "height": 11906, "landscape": True,
                     "margins": [1701, 1701, 850, 850], "columns": 1, "bkc": 2, "nfc": 0},
                    {"text": "Romain", "width": 11906, "height": 16838, "landscape": False,
                     "margins": [1134, 1134, 1134, 1134], "columns": 1, "bkc": 2, "nfc": 2,
                     "restart": True, "start": 5},
                    {"text": "Deux colonnes", "columns": 2, "column_spacing": 567, "bkc": 0},
                    {"text": "Apres", "columns": 1, "bkc": 0, "nfc": 2},
                ],
            },
            "model": {
                "sections": [
                    {"text": "Portrait", "width": 11906, "height": 16838, "landscape": False,
                     "margins": [1417, 1417, 1134, 1134], "columns": 1, "break": "NewPage", "format": "Decimal"},
                    {"text": "Paysage", "width": 16838, "height": 11906, "landscape": True,
                     "margins": [1701, 1701, 850, 850], "columns": 1, "break": "NewPage", "format": "Decimal"},
                    {"text": "Romain", "width": 11906, "height": 16838, "landscape": False,
                     "margins": [1134, 1134, 1134, 1134], "columns": 1, "break": "NewPage", "format": "LowerRoman",
                     "restart": True, "start": 5},
                    {"text": "Deux colonnes", "columns": 2, "column_spacing": 567, "break": "Continuous"},
                    {"text": "Apres", "columns": 1, "break": "Continuous", "format": "LowerRoman"},
                ],
            },
        },
    },
    "struct-headers": {
        "fodt": HEADERS_FODT,
        "expect": {
            "file": {
                # (section, slot, text); slots: 0 even header, 1 odd header, 2 even
                # footer, 3 odd footer, 4 first header, 5 first footer. Empty
                # stories are not listed. The extra final mark is not content.
                "stories": [(0, 1, "Entete {PAGE}"), (0, 3, "Pied {NUMPAGES}"), (0, 4, "Premiere entete"),
                            (1, 1, "Entete deux"), (1, 3, "Pied deux")],
                "title_page": [True, False],
            },
            "model": {
                "sections": [
                    {"title_page": True,
                     "slots": [["Header", "Default", "Entete {PAGE}"], ["Footer", "Default", "Pied {NUMPAGES}"],
                               ["Header", "First", "Premiere entete"]]},
                    {"title_page": False,
                     "slots": [["Header", "Default", "Entete deux"], ["Footer", "Default", "Pied deux"]]},
                ],
            },
        },
    },
    "struct-fields": {
        "fodt": FIELDS_FODT,
        "expect": {
            "file": {
                "bookmarks": [("Point", ""), ("Plage", "avec plage"), ("Deux", "de plage\rsuite")],
                "fields": [("HYPERLINK", "externe"), ("HYPERLINK", "interne"), ("DATE", None),
                           ("TIME", None), ("PAGE", None), ("NUMPAGES", None)],
                "hyperlinks": [("http://example.com/x?a=1", None), (None, "Point")],
            },
            "model": {
                # LibreOffice does not export the link title (no \o switch): no tooltip.
                "bookmarks": [["Point", "Un texte avec plage fin", 3, 3, False],
                              ["Plage", "Un texte avec plage fin", 9, 19, False],
                              ["Deux", "Debut de plage", 6, 14, True]],
                "links": [["externe", "External", "http://example.com/x?a=1", None],
                          ["interne", "Bookmark", "Point", None]],
                "fields": ["DATE", "TIME", "PAGE", "NUMPAGES"],
            },
        },
    },
}


# ---------------------------------------------------------------------------
# Independent structure reader (no Rust code): FKP paragraph exceptions,
# sprms, tables, lists, sections, header stories, fields, bookmarks.
# ---------------------------------------------------------------------------

def u16(b, o): return struct.unpack_from("<H", b, o)[0]
def i16(b, o): return struct.unpack_from("<h", b, o)[0]
def u32(b, o): return struct.unpack_from("<I", b, o)[0]


def sprm_list(g: bytes) -> list:
    pos, out = 0, []
    while pos + 2 <= len(g):
        op = u16(g, pos)
        spra = op >> 13
        if op == 0xD608:
            size = max(u16(g, pos + 2) + 1, 2)
        elif spra in (0, 1):
            size = 1
        elif spra in (2, 4, 5):
            size = 2
        elif spra == 3:
            size = 4
        elif spra == 7:
            size = 3
        else:
            size = g[pos + 2] + 1
        out.append((op, g[pos + 2:pos + 2 + size]))
        pos += 2 + size
    return out


class LoadedDoc:
    def __init__(self, data: bytes):
        st = read_cfb(data)
        self.word = st["WordDocument"]
        flags = u16(self.word, 10)
        self.table = st["1Table" if flags & 0x200 else "0Table"]
        csw = u16(self.word, 32)
        lw_at = 34 + csw * 2
        cslw = u16(self.word, lw_at)
        self.lw = struct.unpack_from("<%dI" % cslw, self.word, lw_at + 2)
        at = lw_at + 2 + cslw * 4
        n = u16(self.word, at)
        self.pairs = [struct.unpack_from("<II", self.word, at + 2 + 8 * i) for i in range(n)]
        self.ccp_text, self.ccp_ftn, self.ccp_hdd = self.lw[3], self.lw[4], self.lw[5]
        fc, lcb = self.pairs[33]
        clx = self.table[fc:fc + lcb]
        pos = 0
        while clx[pos] == 1:
            pos += 3 + u16(clx, pos + 1)
        size = u32(clx, pos + 1)
        plc = clx[pos + 5:pos + 5 + size]
        k = (len(plc) - 4) // 12
        cps = struct.unpack_from("<%dI" % (k + 1), plc, 0)
        self.pieces, parts = [], []
        for i in range(k):
            raw = u32(plc, 4 * (k + 1) + 8 * i + 2)
            comp = bool(raw & 0x40000000)
            off = (raw & 0x3FFFFFFF) // 2 if comp else raw
            self.pieces.append((cps[i], cps[i + 1], off, comp))
            cnt = cps[i + 1] - cps[i]
            if comp:
                parts.append("".join(CP1252_HIGH.get(x, chr(x)) for x in self.word[off:off + cnt]))
            else:
                parts.append(self.word[off:off + 2 * cnt].decode("utf-16-le", "surrogatepass"))
        self.text = "".join(parts)
        self.papx = self._fkps(self.pairs[13])

    def _fkps(self, pair):
        fc, lcb = pair
        n = (lcb - 4) // 8
        out = []
        for i in range(n):
            pn = u32(self.table, fc + 4 * (n + 1) + 4 * i) & 0x3FFFFF
            page = self.word[pn * 512:(pn + 1) * 512]
            crun = page[511]
            for k in range(crun):
                s, e = struct.unpack_from("<II", page, 4 * k)
                b = page[4 * (crun + 1) + 13 * k]
                if not b:
                    continue
                off = b * 2
                cb = page[off]
                content = (page[off + 2:off + 2 + page[off + 1] * 2] if cb == 0
                           else page[off + 1:off + 1 + cb * 2 - 1])
                out.append((s, e, u16(content, 0), content[2:]))
        out.sort()
        return out

    def fc_of(self, cp):
        for a, b, off, comp in self.pieces:
            if a <= cp < b:
                return off + (cp - a) * (1 if comp else 2)
        raise ValueError(cp)

    def para_sprms(self, cp) -> dict:
        fc = self.fc_of(cp)
        for s, e, _, g in self.papx:
            if s <= fc < e:
                return dict(sprm_list(g))
        return {}

    def plc(self, idx, cb):
        fc, lcb = self.pairs[idx]
        d = self.table[fc:fc + lcb]
        n = (lcb - 4) // (4 + cb) if lcb >= 4 else 0
        cps = struct.unpack_from("<%dI" % (n + 1), d, 0) if lcb >= 4 else ()
        return cps, [d[4 * (n + 1) + cb * i:4 * (n + 1) + cb * (i + 1)] for i in range(n)]


def read_tables(d: LoadedDoc) -> list:
    main = d.text[:d.ccp_text]
    rows, cur, buf, start = [], {}, {}, 0
    for cp, ch in enumerate(main):
        if ch not in "\r\x07\x0c":
            continue
        ops = d.para_sprms(cp)
        text = main[start:cp]
        start = cp + 1
        itap = u32(ops[0x6649], 0) if 0x6649 in ops else (1 if 0x2416 in ops else 0)
        if not itap:
            continue
        if 0x2417 in ops or 0x244C in ops:
            row = {"itap": itap, "cells": cur.pop(itap, [])}
            defn = ops.get(0xD608)
            if defn:
                n = defn[2]
                centers = struct.unpack_from("<%dh" % (n + 1), defn, 3)
                tcs = [u16(defn, 3 + 2 * (n + 1) + 20 * i) for i in range(n)]
                row["bounds"], row["tcgrf"] = list(centers), tcs
                brcs = []
                for i in range(n):
                    o = 3 + 2 * (n + 1) + 20 * i + 4
                    brcs.append([defn[o + 4 * j:o + 4 * j + 4].hex() for j in range(4)])
                row["brc80"] = brcs
            row["header"] = bool(ops[0x3404][0]) if 0x3404 in ops else False
            row["height"] = i16(ops[0x9407], 0) if 0x9407 in ops else None
            row["cant_split"] = bool(ops[0x3466][0]) if 0x3466 in ops else False
            row["jc"] = u16(ops[0x548A], 0) if 0x548A in ops else None
            row["shd"] = ops.get(0xD612, b"").hex()
            rows.append(row)
            continue
        buf.setdefault(itap, []).append(text)
        if ch == "\x07" or 0x244B in ops:
            cur.setdefault(itap, []).append("\n".join(buf.pop(itap)))
    return rows


def file_view_tables(rows: list) -> list:
    out = []
    for r in rows:
        cells = []
        for i, text in enumerate(r["cells"]):
            width = r["bounds"][i + 1] - r["bounds"][i]
            grf = r["tcgrf"][i]
            vm = "restart" if grf & 0x40 else ("cont" if grf & 0x20 else None)
            cells.append([text, width, vm, (grf >> 7) & 3])
        e = {"itap": r["itap"], "cells": cells}
        if r["header"]:
            e["header"] = True
        if r["height"] is not None:
            e["height"] = r["height"]
        if r["cant_split"]:
            e["cant_split"] = True
        if r["jc"] is not None:
            e["jc"] = r["jc"]
        out.append(e)
    return out


def read_lists(d: LoadedDoc) -> dict:
    t = d.table
    fc, lcb = d.pairs[73]
    lstf = []
    if lcb:
        n = u16(t, fc)
        heads = [(u32(t, fc + 2 + 28 * i), t[fc + 2 + 28 * i + 26] & 1) for i in range(n)]
        at = fc + lcb
        for lsid, simple in heads:
            levels = []
            for _ in range(1 if simple else 9):
                h = t[at:at + 28]
                cb_chpx, cb_papx = h[24], h[25]
                xst = at + 28 + cb_papx + cb_chpx
                cch = u16(t, xst)
                units = struct.unpack_from("<%dH" % cch, t, xst + 2)
                text = "".join("%%%d" % (u + 1) if u < 9 else chr(u) for u in units)
                levels.append({"nfc": h[4], "start": struct.unpack_from("<i", h, 0)[0], "text": text,
                               "follow": h[15]})
                at = xst + 2 + 2 * cch
            lstf.append((lsid, levels))
    fc, lcb = d.pairs[74]
    lfos = []
    if lcb:
        n = u32(t, fc)
        lfos = [u32(t, fc + 4 + 16 * i) for i in range(n)]
    main = d.text[:d.ccp_text]
    paras, start = [], 0
    for cp, ch in enumerate(main):
        if ch in "\r\x07\x0c":
            ops = d.para_sprms(cp)
            if 0x460B in ops:
                paras.append((main[start:cp], u16(ops[0x460B], 0), ops[0x260A][0] if 0x260A in ops else 0))
            start = cp + 1
    groups, out_paras = [], []
    for text, ilfo, ilvl in paras:
        if ilfo not in groups:
            groups.append(ilfo)
        out_paras.append([text, groups.index(ilfo), ilvl])
    levels = []
    for ilfo in groups:
        lsid = lfos[ilfo - 1]
        defn = dict(lstf)[lsid]
        levels.append(defn)
    return {"paragraphs": out_paras, "levels": levels}


def read_sections(d: LoadedDoc) -> list:
    cps, items = d.plc(6, 12)
    main = d.text[:d.ccp_text]
    out = []
    for i, it in enumerate(items):
        fcs = struct.unpack_from("<I", it, 2)[0]
        ops = {}
        if fcs not in (0, 0xFFFFFFFF):
            ops = dict(sprm_list(d.word[fcs + 2:fcs + 2 + u16(d.word, fcs)]))
        g = lambda op, f=u16: f(ops[op], 0) if op in ops else None
        gs = lambda op: i16(ops[op], 0) if op in ops else None
        text = main[cps[i]:cps[i + 1]].replace("\x0c", "").split("\r")[0]
        out.append({
            "text": text, "width": g(0xB01F), "height": g(0xB020),
            "landscape": ops.get(0x301D, b"\x01")[0] == 2,
            "margins": [g(0xB021), g(0xB022), g(0x9023), g(0x9024)],
            "columns": (g(0x500B) or 0) + 1, "column_spacing": g(0x900C),
            "bkc": ops[0x3009][0] if 0x3009 in ops else 2,
            "nfc": ops[0x300E][0] if 0x300E in ops else 0,
            "restart": bool(ops.get(0x3011, b"\0")[0]), "start": g(0x501C),
            "title_page": bool(ops.get(0x300A, b"\0")[0]),
        })
    return out


def clean_field(s: str) -> str:
    return "".join(c for c in s if c >= " ").strip()


def render_story(s: str) -> str:
    """Text of a story with each field shown as its {CODE} marker."""
    out, stack = [], []
    for c in s:
        if c == "":
            stack.append(["code", ""])
        elif c == "" and stack:
            stack[-1][0] = "result"
            out.append("{%s}" % clean_field(stack[-1][1]).split(" ")[0])
        elif c == "" and stack:
            stack.pop()
        elif stack:
            if stack[-1][0] == "code":
                stack[-1][1] += c
        else:
            out.append(c)
    return "".join(out)


def read_headers(d: LoadedDoc, sections: list) -> list:
    cps, _ = d.plc(11, 0)
    base = d.ccp_text + d.ccp_ftn
    stories = []
    for k in range(len(sections)):
        for slot in range(6):
            i = 6 + 6 * k + slot
            if i + 1 >= len(cps):
                continue
            a, b = cps[i], cps[i + 1]
            if b - a <= 2:
                continue
            raw = d.text[base + a:base + b]
            assert raw.endswith("\r"), "extra story mark expected"
            stories.append([k, slot, render_story(raw[:-1]).rstrip("\r")])
    return stories


def read_fields_bookmarks(d: LoadedDoc) -> dict:
    main = d.text[:d.ccp_text]
    fields, i, stack = [], 0, []
    flt = {}
    cps, items = d.plc(16, 2)
    for cp, it in zip(cps, items):
        flt[cp] = (it[0] & 0x1F, it[1])
    for cp, ch in enumerate(main):
        if ch == "\x13":
            assert flt[cp][0] == 0x13
            stack.append({"cp": cp, "code": "", "result": "", "sep": False, "flt": flt[cp][1]})
        elif ch == "\x14" and stack:
            assert flt[cp][0] == 0x14
            stack[-1]["sep"] = True
        elif ch == "\x15" and stack:
            assert flt[cp][0] == 0x15
            f = stack.pop()
            f["code"] = clean_field(f["code"])
            fields.append(f)
        elif stack:
            stack[-1]["result" if stack[-1]["sep"] else "code"] += ch
    t = d.table
    fc, lcb = d.pairs[21]
    names = []
    if lcb:
        n = u16(t, fc + 2)
        pos = fc + 6
        for _ in range(n):
            cch = u16(t, pos)
            names.append(t[pos + 2:pos + 2 + 2 * cch].decode("utf-16-le"))
            pos += 2 + 2 * cch
    bkf_cps, bkf = d.plc(22, 4)
    bkl_cps, _ = d.plc(23, 0)
    bks = []
    for i, nm in enumerate(names):
        ibkl = u16(bkf[i], 0)
        s, e = bkf_cps[i], bkl_cps[ibkl]
        bks.append([nm, main[s:e].replace("\r", "\r")])
    return {"fields": fields, "bookmarks": bks}


def read_structure(data: bytes) -> dict:
    d = LoadedDoc(data)
    secs = read_sections(d)
    return {
        "tables": file_view_tables(read_tables(d)),
        "lists": read_lists(d),
        "sections": secs,
        "headers": read_headers(d, secs) if d.ccp_hdd else [],
        "fields": read_fields_bookmarks(d),
    }


def norm(v):
    """JSON-comparable form (tuples as lists)."""
    return json.loads(json.dumps(v, ensure_ascii=False))


def close(a, b, tol=3):
    return abs(a - b) <= tol


def check_structure(name: str, data: bytes, expect: dict) -> dict:
    """Compares the independent reader with the description; returns the
    observed facts (stored in expected.json for the record)."""
    s = read_structure(data)
    f = expect["file"]
    if name == "struct-tables":
        rows = s["tables"]
        want = norm(f["rows"])
        if len(rows) != len(want):
            raise ValueError(f"{name}: {len(rows)} rows, expected {len(want)}")
        for got, exp in zip(rows, want):
            for key in ("itap", "header", "height", "cant_split", "jc"):
                if got.get(key) != exp.get(key):
                    raise ValueError(f"{name}: row {key}: {got.get(key)!r} != {exp.get(key)!r} in {exp}")
            if len(got["cells"]) != len(exp["cells"]):
                raise ValueError(f"{name}: cell count differs in {exp}")
            for gc, ec in zip(got["cells"], exp["cells"]):
                if gc[0] != ec[0] or not close(gc[1], ec[1]) or gc[2] != ec[2] or gc[3] != ec[3]:
                    raise ValueError(f"{name}: cell {gc} != {ec}")
        # Borders and shading of H1 (row 3, cell 0), straight from the bytes.
        raw = read_tables(LoadedDoc(data))[3]
        top, left, bottom, right = raw["brc80"][0]
        for brc, (style, color) in ((top, (3, 6)), (left, (6, 4)), (bottom, (7, 2))):
            if int(brc[2:4], 16) != style or int(brc[4:6], 16) != color:
                raise ValueError(f"{name}: border {brc} != type {style} ico {color}")
        if right != "00000000" and right != "00000000":
            raise ValueError(f"{name}: right border expected none, got {right}")
        if raw["shd"][2 + 8:2 + 8 + 6] != "ffff00":
            raise ValueError(f"{name}: shading {raw['shd']}")
    elif name == "struct-lists":
        got = s["lists"]
        want = norm(f["paragraphs"])
        if got["paragraphs"] != want:
            raise ValueError(f"{name}: paragraphs {got['paragraphs']} != {want}")
        for lv, exp in zip(got["levels"], f["groups"]):
            for i, e in enumerate(exp):
                g = lv[i]
                if g["nfc"] != e[0] or g["text"] != e[1] or (len(e) > 2 and g["start"] != e[2]):
                    raise ValueError(f"{name}: level {g} != {e}")
    elif name == "struct-sections":
        want = norm(f["sections"])
        if len(s["sections"]) != len(want):
            raise ValueError(f"{name}: {len(s['sections'])} sections")
        for got, exp in zip(s["sections"], want):
            for key, val in exp.items():
                if key == "margins":
                    if not all(close(a, b) for a, b in zip(got[key], val)):
                        raise ValueError(f"{name}: margins {got[key]} != {val}")
                elif key in ("width", "height", "column_spacing"):
                    if not close(got[key], val):
                        raise ValueError(f"{name}: {key} {got[key]} != {val}")
                elif got.get(key) != val:
                    raise ValueError(f"{name}: {key} {got.get(key)!r} != {val!r} in {exp['text']}")
    elif name == "struct-headers":
        if s["headers"] != norm(f["stories"]):
            raise ValueError(f"{name}: stories {s['headers']} != {f['stories']}")
        if [x["title_page"] for x in s["sections"]] != f["title_page"]:
            raise ValueError(f"{name}: title pages {[x['title_page'] for x in s['sections']]}")
    elif name == "struct-fields":
        fb = s["fields"]
        want_b = [list(b) for b in f["bookmarks"]]
        if fb["bookmarks"] != want_b:
            raise ValueError(f"{name}: bookmarks {fb['bookmarks']} != {want_b}")
        got_f = [(x["code"].split(" ")[0], x["result"] if x["code"].startswith("HYPERLINK") else None)
                 for x in fb["fields"]]
        want_f = [tuple(x) for x in f["fields"]]
        if got_f != want_f:
            raise ValueError(f"{name}: fields {got_f} != {want_f}")
        links = [x["code"] for x in fb["fields"] if x["code"].startswith("HYPERLINK")]
        if 'HYPERLINK "http://example.com/x?a=1"' not in links[0] or '\\l "Point"' not in links[1]:
            raise ValueError(f"{name}: hyperlink codes {links}")
        if {x["flt"] for x in fb["fields"] if x["code"].startswith("HYPERLINK")} != {88}:
            raise ValueError(f"{name}: HYPERLINK field type is not 88")
        flts = {x["code"].split(" ")[0]: x["flt"] for x in fb["fields"]}
        if (flts["DATE"], flts["TIME"], flts["PAGE"], flts["NUMPAGES"]) != (31, 32, 33, 26):
            raise ValueError(f"{name}: field types {flts}")
    return s


# ---------------------------------------------------------------------------
# Generation and checking.
# ---------------------------------------------------------------------------

def find_soffice() -> str:
    candidates = [os.environ.get("SOFFICE", ""), shutil.which("soffice") or "",
                  r"C:\Program Files\LibreOffice\program\soffice.exe",
                  "/usr/bin/soffice", "/Applications/LibreOffice.app/Contents/MacOS/soffice"]
    for c in candidates:
        if c and Path(c).exists():
            return c
    raise FileNotFoundError("LibreOffice (soffice) not found; set SOFFICE")


def sha256(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()


def run_soffice(args: list[str]) -> None:
    """Run soffice with output sent to a file: on Windows `soffice.exe` can keep
    inherited pipes open, which would block `capture_output`."""
    with tempfile.TemporaryFile() as log:
        proc = subprocess.run(args, stdout=log, stderr=subprocess.STDOUT,
                              stdin=subprocess.DEVNULL, timeout=300)
        if proc.returncode != 0:
            log.seek(0)
            raise subprocess.SubprocessError(log.read().decode("utf-8", "replace"))


def producer_of(doc_bytes: bytes) -> str:
    """Application string LibreOffice stores in the summary information."""
    summary = read_cfb(doc_bytes).get("\x05SummaryInformation", b"")
    match = re.search(rb"LibreOffice/[0-9.]+[^\x00]*", summary)
    return match.group(0).decode("ascii", "replace") if match else "LibreOffice"


def write_manifest(manifest: dict) -> None:
    EXPECTED_PATH.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + chr(10),
                             encoding="utf-8", newline=chr(10))


def generate_structure(manifest: dict | None = None) -> dict:
    soffice = find_soffice()
    out_files = {}
    with tempfile.TemporaryDirectory() as tmp_s:
        tmp = Path(tmp_s)
        profile = (tmp / "profile").as_uri()
        for name, doc in STRUCT_DOCS.items():
            src = tmp / f"{name}.fodt"
            src.write_text(doc["fodt"], encoding="utf-8")
            out = tmp / "out"
            run_soffice([soffice, f"-env:UserInstallation={profile}", "--headless", "--norestore",
                         "--convert-to", "doc:MS Word 97", "--outdir", str(out), str(src)])
            data = (out / f"{name}.doc").read_bytes()
            observed = check_structure(name, data, doc["expect"])
            (FIXTURE_DIR / f"{name}.doc").write_bytes(data)
            out_files[f"{name}.doc"] = {
                "size": len(data), "sha256": sha256(data), "producer": producer_of(data),
                "expect": norm(doc["expect"]), "observed": norm(observed),
            }
    return out_files


def generate() -> None:
    soffice = find_soffice()
    version = ""
    FIXTURE_DIR.mkdir(parents=True, exist_ok=True)
    files = {}
    with tempfile.TemporaryDirectory() as tmp_s:
        tmp = Path(tmp_s)
        profile = (tmp / "profile").as_uri()
        for name, doc in DOCS.items():
            src = tmp / f"{name}.fodt"
            src.write_text(build_fodt(doc), encoding="utf-8")
            out = tmp / "out"
            run_soffice([soffice, f"-env:UserInstallation={profile}", "--headless", "--norestore",
                         "--convert-to", "doc:MS Word 97", "--outdir", str(out), str(src)])
            doc_bytes = (out / f"{name}.doc").read_bytes()
            version = producer_of(doc_bytes)
            (FIXTURE_DIR / f"{name}.doc").write_bytes(doc_bytes)
            back = tmp / "back"
            run_soffice([soffice, f"-env:UserInstallation={profile}", "--headless", "--norestore",
                         "--convert-to", "txt:Text (encoded):UTF8", "--outdir", str(back),
                         str(out / f"{name}.doc")])
            lo_text = (back / f"{name}.txt").read_text(encoding="utf-8-sig")
            text, info = main_text(doc_bytes)
            expected_paras = [expected_paragraph(p) for p in doc["paragraphs"]]
            files[f"{name}.doc"] = {
                "size": len(doc_bytes),
                "sha256": sha256(doc_bytes),
                "fib": info,
                "properties": read_summary(doc_bytes),
                "libreoffice_text_lines": lo_text.replace("\r\n", "\n").rstrip("\n").split("\n"),
                "paragraphs": expected_paras,
            }
            if name == "underlines":
                files[f"{name}.doc"]["kul_observed"] = dict(zip(UL, read_kuls(doc_bytes)))
            for key, value in expected_meta(doc["meta"]).items():
                if files[f"{name}.doc"]["properties"].get(key) != value:
                    raise ValueError(f"{name}: property {key} differs from the description")
            got = paragraphs_of(text)
            want = [p["text"] for p in expected_paras]
            if got != want:
                raise ValueError(f"{name}: decoded text differs from the description: {got!r}")
    manifest = {
        "schema_version": 1,
        "generator": "tools/generate-doc-reference.py",
        "provenance": f"{version}, filter 'MS Word 97', from flat ODT sources described in the generator",
        "verification": ("each .doc was converted back to text by LibreOffice and decoded by the "
                         "generator's own CFB/FIB/piece-table reader; both agree with the description"),
        "files": files,
        "structure": generate_structure(),
    }
    EXPECTED_PATH.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
                             encoding="utf-8", newline="\n")
    print(f"generated {len(files)} files with {version}")


def check() -> None:
    manifest = json.loads(EXPECTED_PATH.read_text(encoding="utf-8"))
    if sorted(manifest["files"]) != sorted(f"{n}.doc" for n in DOCS):
        raise ValueError("expected.json lists different files than the generator")
    for fname, entry in manifest["files"].items():
        name = fname[:-4]
        data = (FIXTURE_DIR / fname).read_bytes()
        if len(data) != entry["size"] or sha256(data) != entry["sha256"]:
            raise ValueError(f"{fname}: size or SHA-256 differs from expected.json")
        text, info = main_text(data)
        if info != entry["fib"]:
            raise ValueError(f"{fname}: FIB summary differs: {info} != {entry['fib']}")
        want = [expected_paragraph(p) for p in DOCS[name]["paragraphs"]]
        if want != entry["paragraphs"]:
            raise ValueError(f"{fname}: expected paragraphs differ from the generator description")
        if paragraphs_of(text) != [p["text"] for p in want]:
            raise ValueError(f"{fname}: decoded text differs from the expected text")
        if "kul_observed" in entry and dict(zip(UL, read_kuls(data))) != entry["kul_observed"]:
            raise ValueError(f"{fname}: sprmCKul values differ from expected.json")
        props = read_summary(data)
        if props != entry["properties"]:
            raise ValueError(f"{fname}: summary information differs from expected.json")
        for key, value in expected_meta(DOCS[name]["meta"]).items():
            if props.get(key) != value:
                raise ValueError(f"{fname}: property {key} differs from the description")
    structure = manifest.get("structure", {})
    if sorted(structure) != sorted(f"{n}.doc" for n in STRUCT_DOCS):
        raise ValueError("expected.json lists different structure files than the generator")
    for fname, entry in structure.items():
        name = fname[:-4]
        data = (FIXTURE_DIR / fname).read_bytes()
        if len(data) != entry["size"] or sha256(data) != entry["sha256"]:
            raise ValueError(f"{fname}: size or SHA-256 differs from expected.json")
        if entry["expect"] != norm(STRUCT_DOCS[name]["expect"]):
            raise ValueError(f"{fname}: expectations differ from the generator description")
        if norm(check_structure(name, data, STRUCT_DOCS[name]["expect"])) != entry["observed"]:
            raise ValueError(f"{fname}: independent reader output differs from expected.json")
    total = sum(e["size"] for e in manifest["files"].values()) + sum(
        e["size"] for e in structure.values())
    print(f"ok: {len(manifest['files'])} files, {total} bytes")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--only-structure", action="store_true",
                        help="regenerate only the US-313 structure files, keep the others")
    parser.add_argument("--check", action="store_true",
                        help="verify hashes and independently decode the stored files")
    args = parser.parse_args()
    try:
        if args.check:
            check()
        elif args.only_structure:
            manifest = json.loads(EXPECTED_PATH.read_text(encoding="utf-8"))
            manifest["structure"] = generate_structure()
            write_manifest(manifest)
            print(f"generated {len(manifest['structure'])} structure files")
        else:
            generate()
    except (OSError, ValueError, KeyError, subprocess.SubprocessError, struct.error) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
