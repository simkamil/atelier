#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""US-328: make LibreOffice read back a docx whose properties Atelier edited.

1. (--regenerate) rebuild tests/docx-reference/libreoffice/properties.docx from properties.fodt;
2. run the Rust test that edits the file and writes it under target/tmp;
3. convert the result (the file exactly as written, untouched) to .fodt with LibreOffice (headless) and compare the office:meta it shows.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / "tests" / "docx-reference" / "libreoffice"
NS = {
    "office": "urn:oasis:names:tc:opendocument:xmlns:office:1.0",
    "meta": "urn:oasis:names:tc:opendocument:xmlns:meta:1.0",
    "dc": "http://purl.org/dc/elements/1.1/",
}
EXPECTED = {
    "title": "Titre modifi\u00e9 \u2014 \u00ab \u00e9t\u00e9 \u00bb & <b>\"x\"</b>",
    "subject": "Sujet \u00ab sp\u00e9cial \u00bb \u2014 \u0153uvre",
    "creator": "Zo\u00eb Martin",  # lastModifiedBy
    "initial-creator": "\u00c9lodie Dupont",
    "description": "Premi\u00e8re ligne avec accents : \u00e0\u00e9\u00ee\u00f5\u00fc, \u00e7, \u0153.\n"
    "Deuxi\u00e8me ligne : \"guillemets\" & <balises>.",
    "keywords": "caf\u00e9 na\u00eff R&D",
}


def soffice() -> str:
    for candidate in (
        os.environ.get("ATELIER_SOFFICE"),
        shutil.which("soffice"),
        r"C:\Program Files\LibreOffice\program\soffice.exe",
    ):
        if candidate and Path(candidate).exists():
            return candidate
    sys.exit("LibreOffice (soffice) not found; set ATELIER_SOFFICE")


def convert(exe: str, source: Path, fmt: str, outdir: Path) -> Path:
    subprocess.run([exe, "--headless", "--convert-to", fmt, "--outdir", str(outdir), str(source)],
                   check=True, capture_output=True)
    return outdir / (source.stem + "." + fmt.split(":")[0])


CREATED = {
    "title": EXPECTED["title"],
    "subject": "Créé à l'enregistrement",
    "creator": "Zoë Martin",
    "initial-creator": None,
    "description": "Ligne 1\nLigne 2 & é",
    "keywords": "un deux",
}


def read_back(exe: str, docx: Path, expected: dict) -> int:
    with tempfile.TemporaryDirectory() as tmp:
        # The package exactly as the project wrote it: no rewriting of any part.
        fodt = convert(exe, docx, "fodt", Path(tmp))
        meta = ET.parse(fodt).getroot().find("office:meta", NS)
        seen = {
            "title": meta.findtext("dc:title", namespaces=NS),
            "subject": meta.findtext("dc:subject", namespaces=NS),
            "creator": meta.findtext("dc:creator", namespaces=NS),
            "initial-creator": meta.findtext("meta:initial-creator", namespaces=NS),
            "description": meta.findtext("dc:description", namespaces=NS),
            "keywords": " ".join(k.text or "" for k in meta.findall("meta:keyword", NS)),
        }
        print(f"LibreOffice read {docx.name}:")
        for key, value in seen.items():
            print(f"  {key}: {value!r}")
        bad = [k for k, v in expected.items() if v is not None and seen.get(k) != v]
        if bad:
            print("MISMATCH:", ", ".join(bad), file=sys.stderr)
            return 1
        print("OK: LibreOffice sees every expected value")
    return 0


def main() -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    ap = argparse.ArgumentParser()
    ap.add_argument("--regenerate", action="store_true", help="rebuild properties.docx from properties.fodt")
    args = ap.parse_args()
    exe = soffice()
    if args.regenerate:
        convert(exe, CORPUS / "properties.fodt", "docx:MS Word 2007 XML", CORPUS)
    subprocess.run(["cargo", "test", "-p", "atelier-fmt-ooxml", "--locked", "--offline",
                    "--test", "document_properties_libreoffice"], cwd=ROOT, check=True)
    status = 0
    for name, expected in (("us328-properties-modified.docx", EXPECTED), ("us328-properties-created.docx", CREATED)):
        docx = ROOT / "target" / "tmp" / name
        if not docx.exists():
            sys.exit(f"missing {docx}")
        status |= read_back(exe, docx, expected)
    return status


if __name__ == "__main__":
    sys.exit(main())
