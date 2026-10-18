#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Generate the deterministic US-218 DOCX fixtures: tracked revisions and comments."""

import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "tests" / "docx-reference"
W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/"
DOC_CT = "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"
COMMENTS_CT = "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"
A = 'w:id="{}" w:author="Auteur" w:date="2026-01-01T00:00:00Z"'


def package(name, body, comments=None):
    ct = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        f'<Override PartName="/word/document.xml" ContentType="{DOC_CT}"/>'
        + (f'<Override PartName="/word/comments.xml" ContentType="{COMMENTS_CT}"/>' if comments else "")
        + "</Types>"
    )
    root_rels = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        f'<Relationship Id="rId1" Type="{REL}officeDocument" Target="word/document.xml"/></Relationships>'
    )
    doc_rels = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        + (f'<Relationship Id="rId2" Type="{REL}comments" Target="comments.xml"/>' if comments else "")
        + "</Relationships>"
    )
    document = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
        f'<w:document xmlns:w="{W}"><w:body>{body}<w:sectPr/></w:body></w:document>'
    )
    parts = [
        ("[Content_Types].xml", ct),
        ("_rels/.rels", root_rels),
        ("word/document.xml", document),
        ("word/_rels/document.xml.rels", doc_rels),
    ]
    if comments:
        parts.append(("word/comments.xml", comments))
    with zipfile.ZipFile(OUT / name, "w", zipfile.ZIP_DEFLATED) as z:
        for part, text in parts:
            info = zipfile.ZipInfo(part, (1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, text.encode("utf-8"))


def run(text):
    return f"<w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r>"


def deleted(text):
    return f"<w:r><w:delText xml:space=\"preserve\">{text}</w:delText></w:r>"


# Accepted text: "Début inséré fin." / "déplacé" / "gras" / "centré" / "suite".
revisions = (
    "<w:p>" + run("Début ") + f"<w:ins {A.format(1)}>" + run("inséré ") + "</w:ins>"
    + f"<w:del {A.format(2)}>" + deleted("supprimé ") + "</w:del>" + run("fin.") + "</w:p>"
    + "<w:p>" + f'<w:moveFromRangeStart w:id="3" w:name="m1" {A.format(3)[A.format(3).index("w:author"):]}/>'
    + f"<w:moveFrom {A.format(4)}>" + deleted("ailleurs") + "</w:moveFrom>"
    + '<w:moveFromRangeEnd w:id="3"/>' + "</w:p>"
    + "<w:p>" + f'<w:moveToRangeStart w:id="5" w:name="m1" {A.format(5)[A.format(5).index("w:author"):]}/>'
    + f"<w:moveTo {A.format(6)}>" + run("déplacé") + "</w:moveTo>"
    + '<w:moveToRangeEnd w:id="5"/>' + "</w:p>"
    + "<w:p><w:r><w:rPr><w:b/>"
    + f'<w:rPrChange {A.format(7)}><w:rPr/></w:rPrChange></w:rPr><w:t>gras</w:t></w:r></w:p>'
    + '<w:p><w:pPr><w:jc w:val="center"/>'
    + f'<w:pPrChange {A.format(8)}><w:pPr/></w:pPrChange></w:pPr>' + run("centré") + "</w:p>"
    + "<w:p>" + run("suite") + "</w:p>"
)
package("revisions.docx", revisions)

comments = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
    f'<w:comments xmlns:w="{W}"><w:comment w:id="0" w:author="Relecteur" w:initials="R">'
    "<w:p><w:r><w:t>À revoir</w:t></w:r></w:p></w:comment></w:comments>"
)
body = (
    "<w:p>" + run("Avant ") + '<w:commentRangeStart w:id="0"/>' + run("commenté")
    + '<w:commentRangeEnd w:id="0"/>'
    + '<w:r><w:commentReference w:id="0"/></w:r>' + run(" après.") + "</w:p>"
    + "<w:p>" + run("Fin.") + "</w:p>"
)
package("comments.docx", body, comments)
