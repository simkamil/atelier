#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Generate and independently validate the US-310 ODT reference corpus.

The corpus is produced by a real ODF producer: hand-written flat ODT sources are
converted by LibreOffice (``soffice --headless``) into genuine ``.odt`` packages,
and the page count of each file is taken from the PDF LibreOffice renders from
that very package. Everything else (texts, headings, counts) is re-derived from
the produced package by a separate Python reader that shares no code with the
Rust importer.

    py tools/generate-odt-reference.py            # needs LibreOffice
    py tools/generate-odt-reference.py --check    # checksums + independent reparse only

LibreOffice output embeds dates and a generator string, so the produced bytes
are not reproducible; the versioned files are the reference, pinned by SHA-256
in ``tests/odt-reference/manifest.json``.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
import zipfile
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / "tests" / "odt-reference"
MANIFEST = CORPUS / "manifest.json"
EXPECTED = CORPUS / "expected.json"

NS = {
    "office": "urn:oasis:names:tc:opendocument:xmlns:office:1.0",
    "style": "urn:oasis:names:tc:opendocument:xmlns:style:1.0",
    "text": "urn:oasis:names:tc:opendocument:xmlns:text:1.0",
    "table": "urn:oasis:names:tc:opendocument:xmlns:table:1.0",
    "draw": "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0",
    "fo": "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0",
    "svg": "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0",
    "meta": "urn:oasis:names:tc:opendocument:xmlns:meta:1.0",
    "dc": "http://purl.org/dc/elements/1.1/",
    "xlink": "http://www.w3.org/1999/xlink",
}
XMLNS = " ".join(f'xmlns:{k}="{v}"' for k, v in NS.items())
MIME = "application/vnd.oasis.opendocument.text"


def q(prefix: str, local: str) -> str:
    return f"{{{NS[prefix]}}}{local}"


def png_bytes() -> bytes:
    """A deterministic 24x16 RGB gradient PNG."""
    w, h = 24, 16
    raw = b"".join(
        b"\x00" + bytes(c for x in range(w) for c in (x * 10 % 256, y * 16 % 256, 128))
        for y in range(h)
    )

    def chunk(tag: bytes, data: bytes) -> bytes:
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def fodt(body: str, styles: str = "", auto: str = "", masters: str = "", meta: str = "", fonts: str = "") -> str:
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<office:document {XMLNS} office:version="1.3" office:mimetype="{MIME}">
<office:meta>{meta}</office:meta>
<office:font-face-decls>{fonts}</office:font-face-decls>
<office:styles>{styles}</office:styles>
<office:automatic-styles>{auto}</office:automatic-styles>
<office:master-styles>{masters}</office:master-styles>
<office:body><office:text>{body}</office:text></office:body>
</office:document>
"""


A4 = '<style:page-layout style:name="pmA4"><style:page-layout-properties fo:page-width="21cm" fo:page-height="29.7cm" style:print-orientation="portrait" fo:margin-top="2cm" fo:margin-bottom="2cm" fo:margin-left="2cm" fo:margin-right="2cm"/></style:page-layout>'
LAND = '<style:page-layout style:name="pmLand"><style:page-layout-properties fo:page-width="29.7cm" fo:page-height="21cm" style:print-orientation="landscape" fo:margin-top="1.5cm" fo:margin-bottom="1.5cm" fo:margin-left="1.5cm" fo:margin-right="1.5cm"/></style:page-layout>'
LONG = (
    "Le renard brun et rapide saute par-dessus le chien paresseux; cette phrase accentuée — éàùçœ — "
    "est répétée afin de remplir des lignes entières et de produire une pagination stable. "
)


def sources() -> dict[str, tuple[str, str]]:
    """name -> (flat ODT source, description)."""
    docs: dict[str, tuple[str, str]] = {}
    docs["text-accents"] = (
        fodt(
            "<text:p>Éléphant à l’œuvre — «ça» ñandú, Ελλάδα, 日本語 😀.</text:p>"
            '<text:p>Espaces<text:s text:c="3"/>multiples, tab<text:tab/>ulation et<text:line-break/>saut de ligne.</text:p>'
            "<text:p>Deuxième paragraphe, avec texte normal.</text:p>",
            meta="<dc:title>Texte accentué</dc:title><dc:creator>Atelier</dc:creator><meta:keyword>odt</meta:keyword><meta:keyword>accents</meta:keyword><dc:subject>Sujet</dc:subject>",
        ),
        "accented text, repeated spaces, tab, line break, metadata",
    )
    docs["char-format"] = (
        fodt(
            '<text:p>Normal <text:span text:style-name="B">gras</text:span> <text:span text:style-name="I">italique</text:span> '
            '<text:span text:style-name="U">souligné</text:span> <text:span text:style-name="S">barré</text:span> '
            'x<text:span text:style-name="Sup">2</text:span> H<text:span text:style-name="Sub">2</text:span>O '
            '<text:span text:style-name="C">rouge</text:span> <text:span text:style-name="H">surligné</text:span> '
            '<text:span text:style-name="F">police</text:span> <text:span text:style-name="Big">grand</text:span> '
            '<text:span text:style-name="SC">petites capitales</text:span></text:p>',
            auto=(
                '<style:style style:name="B" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>'
                '<style:style style:name="I" style:family="text"><style:text-properties fo:font-style="italic"/></style:style>'
                '<style:style style:name="U" style:family="text"><style:text-properties style:text-underline-style="solid" style:text-underline-width="auto" style:text-underline-color="font-color"/></style:style>'
                '<style:style style:name="S" style:family="text"><style:text-properties style:text-line-through-style="solid"/></style:style>'
                '<style:style style:name="Sup" style:family="text"><style:text-properties style:text-position="super 58%"/></style:style>'
                '<style:style style:name="Sub" style:family="text"><style:text-properties style:text-position="sub 58%"/></style:style>'
                '<style:style style:name="C" style:family="text"><style:text-properties fo:color="#cc0000"/></style:style>'
                '<style:style style:name="H" style:family="text"><style:text-properties fo:background-color="#ffff00"/></style:style>'
                '<style:style style:name="F" style:family="text"><style:text-properties style:font-name="Liberation Mono" fo:font-size="10pt"/></style:style>'
                '<style:style style:name="Big" style:family="text"><style:text-properties fo:font-size="20pt"/></style:style>'
                '<style:style style:name="SC" style:family="text"><style:text-properties fo:font-variant="small-caps" fo:letter-spacing="0.05cm"/></style:style>'
            ),
            fonts='<style:font-face style:name="Liberation Mono" svg:font-family="&apos;Liberation Mono&apos;" style:font-family-generic="modern" style:font-pitch="fixed"/>',
        ),
        "character formatting through automatic text styles",
    )
    docs["para-format"] = (
        fodt(
            '<text:p text:style-name="Centre">Centré</text:p>'
            '<text:p text:style-name="Droite">À droite</text:p>'
            '<text:p text:style-name="Just">Justifié: ' + LONG * 3 + "</text:p>"
            '<text:p text:style-name="Retrait">Retrait gauche, première ligne et espacement.</text:p>'
            '<text:p text:style-name="Interligne">Interligne 150 pour cent: ' + LONG + "</text:p>"
            '<text:p text:style-name="Tabs">A<text:tab/>B<text:tab/>C</text:p>'
            '<text:p text:style-name="Saut">Après un saut de page.</text:p>',
            auto=(
                '<style:style style:name="Centre" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>'
                '<style:style style:name="Droite" style:family="paragraph"><style:paragraph-properties fo:text-align="end"/></style:style>'
                '<style:style style:name="Just" style:family="paragraph"><style:paragraph-properties fo:text-align="justify" fo:orphans="2" fo:widows="2"/></style:style>'
                '<style:style style:name="Retrait" style:family="paragraph"><style:paragraph-properties fo:margin-left="1.5cm" fo:margin-right="1cm" fo:text-indent="0.8cm" fo:margin-top="0.4cm" fo:margin-bottom="0.3cm" fo:keep-with-next="always"/></style:style>'
                '<style:style style:name="Interligne" style:family="paragraph"><style:paragraph-properties fo:line-height="150%"/></style:style>'
                '<style:style style:name="Tabs" style:family="paragraph"><style:paragraph-properties><style:tab-stops><style:tab-stop style:position="3cm"/><style:tab-stop style:position="8cm" style:type="right"/></style:tab-stops></style:paragraph-properties></style:style>'
                '<style:style style:name="Saut" style:family="paragraph"><style:paragraph-properties fo:break-before="page"/></style:style>'
            ),
        ),
        "paragraph alignment, indents, spacing, line height, tab stops, page break",
    )
    docs["headings-styles"] = (
        fodt(
            '<text:p text:style-name="Title">Titre du document</text:p>'
            '<text:p text:style-name="Subtitle">Sous-titre</text:p>'
            '<text:h text:style-name="Heading_20_1" text:outline-level="1">Chapitre un</text:h>'
            '<text:p text:style-name="Text_20_body">Corps de texte du chapitre.</text:p>'
            '<text:h text:style-name="Heading_20_2" text:outline-level="2">Section 1.1</text:h>'
            '<text:p text:style-name="Citation">Un style personnalisé hérité de Corps de texte, <text:span text:style-name="Accent">avec un style de caractère</text:span>.</text:p>'
            '<text:h text:style-name="Heading_20_3" text:outline-level="3">Sous-section 1.1.1</text:h>'
            '<text:p text:style-name="Standard">Paragraphe standard.</text:p>',
            styles=(
                '<style:style style:name="Citation" style:display-name="Citation perso" style:family="paragraph" style:parent-style-name="Text_20_body" style:next-style-name="Standard">'
                '<style:paragraph-properties fo:margin-left="1cm" fo:text-align="justify"/><style:text-properties fo:font-style="italic" fo:color="#336699"/></style:style>'
                '<style:style style:name="Accent" style:display-name="Accent" style:family="text"><style:text-properties fo:font-weight="bold" fo:color="#aa3300"/></style:style>'
            ),
        ),
        "title, subtitle, three heading levels, custom inherited style, character style",
    )
    docs["lists"] = (
        fodt(
            '<text:list text:style-name="Puces">'
            "<text:list-item><text:p>Puce un</text:p>"
            '<text:list><text:list-item><text:p>Sous-puce a</text:p>'
            "<text:list><text:list-item><text:p>Sous-sous-puce</text:p></text:list-item></text:list></text:list-item>"
            "<text:list-item><text:p>Sous-puce b</text:p></text:list-item></text:list></text:list-item>"
            "<text:list-item><text:p>Puce deux</text:p></text:list-item></text:list>"
            "<text:p>Entre deux listes.</text:p>"
            '<text:list text:style-name="Numeros">'
            "<text:list-item><text:p>Premier</text:p></text:list-item>"
            '<text:list-item><text:p>Deuxième</text:p><text:list><text:list-item><text:p>Item a</text:p></text:list-item><text:list-item><text:p>Item b</text:p></text:list-item></text:list></text:list-item>'
            '<text:list-item text:start-value="10"><text:p>Dixième</text:p></text:list-item></text:list>',
            auto=(
                '<text:list-style style:name="Puces">'
                '<text:list-level-style-bullet text:level="1" text:bullet-char="•"><style:list-level-properties text:list-level-position-and-space-mode="label-alignment"><style:list-level-label-alignment text:label-followed-by="listtab" text:list-tab-stop-position="1cm" fo:text-indent="-0.5cm" fo:margin-left="1cm"/></style:list-level-properties></text:list-level-style-bullet>'
                '<text:list-level-style-bullet text:level="2" text:bullet-char="◦"><style:list-level-properties text:list-level-position-and-space-mode="label-alignment"><style:list-level-label-alignment text:label-followed-by="listtab" text:list-tab-stop-position="2cm" fo:text-indent="-0.5cm" fo:margin-left="2cm"/></style:list-level-properties></text:list-level-style-bullet>'
                '<text:list-level-style-bullet text:level="3" text:bullet-char="▪"><style:list-level-properties text:list-level-position-and-space-mode="label-alignment"><style:list-level-label-alignment text:label-followed-by="listtab" text:list-tab-stop-position="3cm" fo:text-indent="-0.5cm" fo:margin-left="3cm"/></style:list-level-properties></text:list-level-style-bullet></text:list-style>'
                '<text:list-style style:name="Numeros">'
                '<text:list-level-style-number text:level="1" style:num-suffix="." style:num-format="1"><style:list-level-properties text:list-level-position-and-space-mode="label-alignment"><style:list-level-label-alignment text:label-followed-by="listtab" text:list-tab-stop-position="1cm" fo:text-indent="-0.6cm" fo:margin-left="1cm"/></style:list-level-properties></text:list-level-style-number>'
                '<text:list-level-style-number text:level="2" style:num-suffix=")" style:num-format="a"><style:list-level-properties text:list-level-position-and-space-mode="label-alignment"><style:list-level-label-alignment text:label-followed-by="listtab" text:list-tab-stop-position="2cm" fo:text-indent="-0.6cm" fo:margin-left="2cm"/></style:list-level-properties></text:list-level-style-number></text:list-style>'
            ),
        ),
        "nested bullet list (3 levels), numbered list with nested level and start value",
    )
    docs["table-merge"] = (
        fodt(
            '<text:p>Avant le tableau.</text:p>'
            '<table:table table:name="Tableau1" table:style-name="Tbl">'
            '<table:table-column table:style-name="Col" table:number-columns-repeated="3"/>'
            '<table:table-header-rows><table:table-row>'
            '<table:table-cell table:style-name="Cell" table:number-columns-spanned="2" office:value-type="string"><text:p>En-tête fusionné</text:p></table:table-cell><table:covered-table-cell/>'
            '<table:table-cell table:style-name="Cell" office:value-type="string"><text:p>Col 3</text:p></table:table-cell></table:table-row></table:table-header-rows>'
            '<table:table-row><table:table-cell table:style-name="Cell" table:number-rows-spanned="2" office:value-type="string"><text:p>Haut</text:p></table:table-cell>'
            '<table:table-cell table:style-name="Cell" office:value-type="string"><text:p>b2</text:p></table:table-cell>'
            '<table:table-cell table:style-name="Cell" office:value-type="string"><text:p>c2</text:p><text:p>c2 bis</text:p></table:table-cell></table:table-row>'
            '<table:table-row><table:covered-table-cell/>'
            '<table:table-cell table:style-name="Cell" office:value-type="string"><text:p>b3</text:p></table:table-cell>'
            '<table:table-cell table:style-name="Cell" office:value-type="string"><text:p>c3</text:p></table:table-cell></table:table-row>'
            "</table:table>"
            "<text:p>Après le tableau.</text:p>",
            auto=(
                '<style:style style:name="Tbl" style:family="table"><style:table-properties style:width="15cm" table:align="left"/></style:style>'
                '<style:style style:name="Col" style:family="table-column"><style:table-column-properties style:column-width="5cm"/></style:style>'
                '<style:style style:name="Cell" style:family="table-cell"><style:table-cell-properties fo:padding="0.1cm" fo:border="0.5pt solid #000000"/></style:style>'
            ),
        ),
        "table with a merged header, a row span, repeated columns and a header row",
    )
    png = base64.b64encode(png_bytes()).decode()
    docs["image"] = (
        fodt(
            '<text:p>Image dans le texte: <draw:frame draw:name="Logo" text:anchor-type="as-char" svg:width="4.8cm" svg:height="3.2cm">'
            f"<draw:image><office:binary-data>{png}</office:binary-data></draw:image>"
            "<svg:title>Dégradé</svg:title><svg:desc>Un dégradé de test</svg:desc></draw:frame> puis du texte.</text:p>"
            '<text:p text:style-name="P1"><draw:frame draw:name="Marge" text:anchor-type="paragraph" svg:x="0cm" svg:y="1cm" svg:width="2cm" svg:height="2cm" draw:z-index="0">'
            f"<draw:image><office:binary-data>{png}</office:binary-data></draw:image></draw:frame>Paragraphe avec image ancrée.</text:p>",
            auto='<style:style style:name="P1" style:family="paragraph"/>',
        ),
        "an as-char picture with alternative text and a paragraph-anchored picture",
    )
    docs["header-footer"] = (
        fodt(
            "".join(f"<text:p>Paragraphe {i}. {LONG * 4}</text:p>" for i in range(1, 15)),
            auto=A4,
            masters=(
                '<style:master-page style:name="Standard" style:page-layout-name="pmA4">'
                "<style:header><text:p>En-tête du document</text:p></style:header>"
                '<style:footer><text:p>Page <text:page-number text:select-page="current">1</text:page-number> sur <text:page-count>1</text:page-count></text:p></style:footer>'
                "</style:master-page>"
            ),
        ),
        "running header, footer with page number and count, several pages",
    )
    docs["links-bookmarks"] = (
        fodt(
            '<text:p>Un <text:a xlink:type="simple" xlink:href="https://example.org/page?a=1&amp;b=2" office:name="Info bulle">lien externe</text:a>, '
            'un <text:a xlink:type="simple" xlink:href="#Cible">lien interne</text:a> et un signet ponctuel<text:bookmark text:name="Point"/>.</text:p>'
            '<text:p><text:bookmark-start text:name="Cible"/>Texte signalé<text:bookmark-end text:name="Cible"/> par un signet.</text:p>',
        ),
        "external hyperlink, internal hyperlink, point and range bookmarks",
    )
    docs["fields-notes"] = (
        fodt(
            '<text:p>Date: <text:date style:data-style-name="N1" text:date-value="2026-10-12">12/10/2026</text:date>, '
            'heure: <text:time text:time-value="10:30:00">10:30:00</text:time>.</text:p>'
            '<text:p>Texte avec note<text:note text:id="ftn1" text:note-class="footnote"><text:note-citation>1</text:note-citation><text:note-body><text:p>Contenu de la note.</text:p></text:note-body></text:note> et suite.</text:p>'
            '<text:p>Auteur: <text:initial-creator>Moi</text:initial-creator>.</text:p>',
        ),
        "date and time fields, a footnote, another field",
    )
    docs["sections-pages"] = (
        fodt(
            "<text:p>Page portrait.</text:p>"
            '<text:p text:style-name="PL">Début du mode paysage.</text:p>'
            '<text:section text:name="Zone1"><text:p>Dans une section.</text:p></text:section>'
            '<text:p text:style-name="PP">Retour au portrait.</text:p>',
            auto=(
                A4
                + LAND
                + '<style:style style:name="PL" style:family="paragraph" style:master-page-name="Paysage"/>'
                + '<style:style style:name="PP" style:family="paragraph" style:master-page-name="Standard"/>'
            ),
            masters=(
                '<style:master-page style:name="Standard" style:page-layout-name="pmA4"/>'
                '<style:master-page style:name="Paysage" style:page-layout-name="pmLand"/>'
            ),
        ),
        "master page changes (portrait, landscape, portrait) and a text section",
    )
    docs["multi-page"] = (
        fodt(
            "<text:h text:outline-level=\"1\">Long document</text:h>"
            + "".join(f"<text:p>Paragraphe numéro {i}. {LONG * 5}</text:p>" for i in range(1, 31)),
            auto=A4,
            masters='<style:master-page style:name="Standard" style:page-layout-name="pmA4"/>',
        ),
        "a long document of several pages",
    )
    docs["nested-table"] = (
        fodt(
            '<table:table table:name="Ext"><table:table-column table:number-columns-repeated="2"/>'
            '<table:table-row><table:table-cell office:value-type="string"><text:p>Cellule A</text:p></table:table-cell>'
            '<table:table-cell office:value-type="string"><table:table table:name="Int"><table:table-column table:number-columns-repeated="2"/>'
            '<table:table-row><table:table-cell office:value-type="string"><text:p>i1</text:p></table:table-cell><table:table-cell office:value-type="string"><text:p>i2</text:p></table:table-cell></table:table-row>'
            "</table:table><text:p>Après le tableau imbriqué.</text:p></table:table-cell></table:table-row></table:table>"
            "<text:p>Fin.</text:p>",
        ),
        "a table nested in a table cell",
    )
    return docs


# --------------------------------------------------------------------------
# Independent reader (Python stdlib only; shares nothing with the Rust code)


def odf_text(elem: ET.Element, state: dict) -> str:
    """Text of a text:p / text:h following the ODF white-space rules."""
    out: list[str] = []

    def add(s: str) -> None:
        for ch in s:
            if ch in " \t\r\n":
                if not state["prev"]:
                    out.append(" ")
                    state["prev"] = True
            else:
                out.append(ch)
                state["prev"] = False

    def walk(e: ET.Element) -> None:
        if e.text:
            add(e.text)
        for c in e:
            tag = c.tag
            if tag == q("text", "s"):
                out.append(" " * int(c.get(q("text", "c"), "1")))
                state["prev"] = False
            elif tag == q("text", "tab"):
                out.append("\t")
                state["prev"] = False
            elif tag == q("text", "line-break"):
                out.append("\n")
                state["prev"] = False
            elif tag == q("text", "note"):
                cit = c.find(q("text", "note-citation"))
                if cit is not None:
                    out.append("".join(cit.itertext()))
                    state["prev"] = False
            elif tag in (q("office", "annotation"), q("text", "ruby-text")):
                pass
            elif tag == q("draw", "frame"):
                state["prev"] = False
            else:
                walk(c)
            if c.tail:
                add(c.tail)

    walk(elem)
    return "".join(out)


def body_paragraphs(root: ET.Element) -> list[tuple[str, str, int | None]]:
    """(tag, text, outline level) of every body paragraph, in document order."""
    found: list[tuple[str, str, int | None]] = []
    skip = {q("text", "note"), q("text", "tracked-changes")}

    def visit(e: ET.Element) -> None:
        for c in e:
            if c.tag in skip:
                continue
            if c.tag in (q("text", "p"), q("text", "h")):
                level = c.get(q("text", "outline-level"))
                found.append(
                    (
                        "h" if c.tag == q("text", "h") else "p",
                        odf_text(c, {"prev": True}),
                        int(level) if level else None,
                    )
                )
                # Frames may contain nested paragraphs; they are not body text here.
            else:
                visit(c)

    text = root.find(f"{q('office', 'body')}/{q('office', 'text')}")
    assert text is not None
    visit(text)
    return found


def master_texts(styles_root: ET.Element) -> dict[str, list[str]]:
    result: dict[str, list[str]] = {}
    for mp in styles_root.iter(q("style", "master-page")):
        texts = []
        for kind in ("header", "footer"):
            for el in mp.findall(q("style", kind)):
                for p in el.iter(q("text", "p")):
                    texts.append(odf_text(p, {"prev": True}))
        result[mp.get(q("style", "name"))] = texts
    return result


def analyse(path: Path) -> dict:
    with zipfile.ZipFile(path) as z:
        names = z.namelist()
        mimetype = z.read("mimetype").decode()
        content = ET.fromstring(z.read("content.xml"))
        styles = ET.fromstring(z.read("styles.xml"))
        meta = ET.fromstring(z.read("meta.xml"))
        pictures = sorted(n for n in names if n.startswith("Pictures/"))
        picture_sizes = {n: len(z.read(n)) for n in pictures}
    paras = body_paragraphs(content)
    metaroot = meta.find(q("office", "meta"))
    stat = metaroot.find(q("meta", "document-statistic"))
    keywords = [k.text for k in metaroot.findall(q("meta", "keyword"))]
    links = [a.get(q("xlink", "href")) for a in content.iter(q("text", "a"))]
    bookmarks = sorted(
        {
            b.get(q("text", "name"))
            for tag in ("bookmark", "bookmark-start")
            for b in content.iter(q("text", tag))
        }
    )
    used_masters = [
        s.get(q("style", "master-page-name"))
        for s in content.iter(q("style", "style"))
        if s.get(q("style", "master-page-name"))
    ]
    return {
        "mimetype": mimetype,
        "paragraphs": [t for _, t, _ in paras],
        "headings": [[lvl, t] for tag, t, lvl in paras if tag == "h"],
        "tables": sum(1 for _ in content.iter(q("table", "table"))),
        "listItems": sum(1 for _ in content.iter(q("text", "list-item"))),
        "images": len(list(content.iter(q("draw", "image")))),
        "pictureParts": picture_sizes,
        "links": links,
        "bookmarks": bookmarks,
        "masterPageChanges": used_masters,
        "masterPages": master_texts(styles),
        "title": (metaroot.findtext(q("dc", "title"))),
        "creator": (metaroot.findtext(q("dc", "creator"))),
        "keywords": keywords,
        "generator": metaroot.findtext(q("meta", "generator")),
        "statisticsPageCount": int(stat.get(q("meta", "page-count"))) if stat is not None and stat.get(q("meta", "page-count")) else None,
    }


# --------------------------------------------------------------------------


def find_soffice() -> Path:
    for base in (os.environ.get("ProgramFiles", r"C:\Program Files"), r"C:\Program Files"):
        p = Path(base) / "LibreOffice" / "program" / "soffice.exe"
        if p.exists():
            return p
    found = shutil.which("soffice")
    if found:
        return Path(found)
    raise SystemExit("LibreOffice (soffice) not found; use --check to validate the existing corpus")


def pdf_page_count(pdf: bytes) -> int:
    return len(re.findall(rb"/Type\s*/Page(?![a-zA-Z])", pdf))


def run_soffice(soffice: Path, profile: Path, args: list[str]) -> None:
    cmd = [str(soffice), f"-env:UserInstallation={profile.as_uri()}", "--headless", "--norestore", *args]
    subprocess.run(cmd, check=True, timeout=600, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def generate() -> None:
    soffice = find_soffice()
    version = subprocess.run([str(soffice), "--version"], capture_output=True, text=True, timeout=120).stdout.strip()
    docs = sources()
    CORPUS.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        src_dir, out_dir, pdf_dir = tmpdir / "src", tmpdir / "odt", tmpdir / "pdf"
        for d in (src_dir, out_dir, pdf_dir):
            d.mkdir()
        sources_paths = []
        for name, (xml, _) in docs.items():
            p = src_dir / f"{name}.fodt"
            p.write_bytes(xml.encode("utf-8"))
            sources_paths.append(str(p))
        profile = tmpdir / "profile"
        run_soffice(soffice, profile, ["--convert-to", "odt:writer8", "--outdir", str(out_dir), *sources_paths])
        odts = [str(out_dir / f"{n}.odt") for n in docs]
        run_soffice(soffice, profile, ["--convert-to", "pdf", "--outdir", str(pdf_dir), *odts])
        # One flat document produced by LibreOffice itself.
        run_soffice(soffice, profile, ["--convert-to", "fodt", "--outdir", str(out_dir), str(out_dir / "table-merge.odt")])
        expected = {}
        manifest_files = {}
        for name, (_, description) in docs.items():
            target = CORPUS / f"{name}.odt"
            shutil.copyfile(out_dir / f"{name}.odt", target)
            info = analyse(target)
            info["pageCount"] = pdf_page_count((pdf_dir / f"{name}.pdf").read_bytes())
            info["description"] = description
            expected[f"{name}.odt"] = info
        flat = CORPUS / "table-merge.fodt"
        shutil.copyfile(out_dir / "table-merge.fodt", flat)
    for f in sorted(CORPUS.glob("*.od*")) + [CORPUS / "table-merge.fodt"]:
        if f.name in manifest_files:
            continue
        manifest_files[f.name] = {"bytes": f.stat().st_size, "sha256": sha256(f)}
    payload = {
        "producer": version,
        "generator": "tools/generate-odt-reference.py",
        "note": "Genuine LibreOffice output; bytes are not reproducible, the SHA-256 values pin the versioned files.",
        "files": manifest_files,
    }
    for path, data in ((MANIFEST, payload), (EXPECTED, expected)):
        text = json.dumps(data, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
        path.write_bytes(text.encode("utf-8"))  # LF endings on every host
    print(f"wrote {len(manifest_files)} files, {sum(v['bytes'] for v in manifest_files.values())} bytes")


def check() -> None:
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    expected = json.loads(EXPECTED.read_text(encoding="utf-8"))
    problems = []
    for name, meta in manifest["files"].items():
        path = CORPUS / name
        if not path.exists():
            problems.append(f"missing {name}")
        elif sha256(path) != meta["sha256"] or path.stat().st_size != meta["bytes"]:
            problems.append(f"checksum mismatch {name}")
    for name, want in expected.items():
        got = analyse(CORPUS / name)
        for key, value in got.items():
            if want.get(key) != value:
                problems.append(f"{name}: {key} differs: {want.get(key)!r} != {value!r}")
        if want["pageCount"] < 1:
            problems.append(f"{name}: invalid page count")
    if problems:
        raise SystemExit("\n".join(problems))
    print(f"ok: {len(manifest['files'])} files, {len(expected)} expectations")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--check", action="store_true", help="verify checksums and re-derive expectations; no LibreOffice needed")
    args = parser.parse_args()
    check() if args.check else generate()
    return 0


if __name__ == "__main__":
    sys.exit(main())
