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
    total = sum(len(e["sha256"]) and e["size"] for e in manifest["files"].values())
    print(f"ok: {len(manifest['files'])} files, {total} bytes")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true",
                        help="verify hashes and independently decode the stored files")
    args = parser.parse_args()
    try:
        check() if args.check else generate()
    except (OSError, ValueError, KeyError, subprocess.SubprocessError, struct.error) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
