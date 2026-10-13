# SPDX-License-Identifier: AGPL-3.0-or-later
"""Extracts text and an application's own statistics from .docx / .odt files.

Writes one `<name>.stats.txt` per input in the format read by
`crates/text-model/tests/statistics_oracle.rs`:

    @producer <application that wrote the file>
    @expected words=N chars=N chars_spaces=N paragraphs=N
    <tag> <one paragraph per line; \\t tab, \\v line break, \\\\ backslash>

Each line is "<tag> <text>": B body (tables included), H header or footer (ODT
only), N note body (ODT only). Text boxes and frames are left out.
Usage: python -I tools/extract-statistics-oracle.py OUT_DIR FILE...
"""
import re
import sys
import zipfile
import xml.etree.ElementTree as ET

W = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}"
T = "{urn:oasis:names:tc:opendocument:xmlns:text:1.0}"
O = "{urn:oasis:names:tc:opendocument:xmlns:office:1.0}"
M = "{urn:oasis:names:tc:opendocument:xmlns:meta:1.0}"


def esc(s):
    return s.replace("\\", "\\\\").replace("\t", "\\t").replace("\x0b", "\\v").replace("\n", " ")


def docx(path):
    z = zipfile.ZipFile(path)
    try:
        app = z.read("docProps/app.xml").decode("utf-8", "replace")
    except KeyError:
        app = ""

    def g(tag):
        m = re.search("<%s>([^<]*)" % tag, app)
        return m.group(1) if m else ""

    producer = g("Application") or "unknown"
    exp = {"words": g("Words"), "chars": g("Characters"),
           "chars_spaces": g("CharactersWithSpaces"), "paragraphs": g("Paragraphs")}
    body = ET.fromstring(z.read("word/document.xml")).find(W + "body")
    paras = []

    skipped = (W + "txbxContent", W + "drawing", W + "pict", W + "object")

    def run_text(el):
        out = []
        for ch in el:
            if ch.tag in skipped:
                continue
            if ch.tag == W + "t":
                out.append(ch.text or "")
            elif ch.tag == W + "tab":
                out.append("	")
            elif ch.tag == W + "br" and ch.get(W + "type") in (None, "textWrapping"):
                out.append("")
            elif ch.tag == W + "noBreakHyphen":
                out.append("‑")
            else:
                out.append(run_text(ch))
        return "".join(out)

    def walk(node):
        for ch in node:
            if ch.tag == W + "p":
                paras.append("B " + run_text(ch))
            elif ch.tag in (W + "tbl", W + "tr", W + "tc", W + "sdt", W + "sdtContent"):
                walk(ch)

    walk(body)
    return producer, exp, paras


def odt(path):
    z = zipfile.ZipFile(path)
    meta = z.read("meta.xml").decode("utf-8")
    gen = re.search("<meta:generator>([^<]*)", meta)
    st = ET.fromstring(meta).find(".//" + M + "document-statistic")
    exp = {"words": st.get(M + "word-count"),
           "chars": st.get(M + "non-whitespace-character-count"),
           "chars_spaces": st.get(M + "character-count"),
           "paragraphs": st.get(M + "paragraph-count")}
    body = ET.fromstring(z.read("content.xml")).find(O + "body").find(O + "text")
    paras = []
    notes = []

    def text_of(el):
        out = [el.text or ""]
        for ch in el:
            if ch.tag == T + "s":
                out.append(" " * int(ch.get(T + "c", "1")))
            elif ch.tag == T + "tab":
                out.append("\t")
            elif ch.tag == T + "line-break":
                out.append("\x0b")
            elif ch.tag == T + "note":
                for part in ch:
                    if part.tag == T + "note-citation":
                        out.append("".join(part.itertext()))
                    elif part.tag == T + "note-body":
                        for q in part:
                            if q.tag in (T + "p", T + "h"):
                                notes.append("N " + text_of(q))
            elif ch.tag in (T + "bookmark", T + "bookmark-start", T + "bookmark-end") or ch.tag.endswith("}frame"):
                pass
            else:
                out.append(text_of(ch))
            out.append(ch.tail or "")
        return "".join(out)

    def walk(node):
        for ch in node:
            if ch.tag in (T + "p", T + "h"):
                paras.append("B " + text_of(ch))
            elif ch.tag in (T + "list", T + "list-item", T + "section") or ch.tag.rsplit("}", 1)[-1] in (
                    "table", "table-row", "table-cell", "table-header-rows", "table-rows"):
                walk(ch)

    walk(body)
    styles = ET.fromstring(z.read("styles.xml"))
    for el in styles.iter():
        if el.tag.rsplit("}", 1)[-1] in ("header", "footer", "header-left", "footer-left",
                                         "header-first", "footer-first"):
            for q in el:
                if q.tag in (T + "p", T + "h"):
                    paras.append("H " + text_of(q))
    paras.extend(notes)
    return (gen.group(1) if gen else "unknown"), exp, paras


def main():
    out_dir = sys.argv[1]
    for path in sys.argv[2:]:
        name = re.sub(r"[^A-Za-z0-9._-]", "_", path.replace("\\", "/").split("/")[-1])
        producer, exp, paras = (docx if path.endswith(".docx") else odt)(path)
        with open("%s/%s.stats.txt" % (out_dir, name), "w", encoding="utf-8", newline="\n") as f:
            f.write("@producer %s\n" % producer)
            f.write("@expected " + " ".join("%s=%s" % kv for kv in exp.items()) + "\n")
            for p in paras:
                f.write(esc(p) + "\n")


main()
