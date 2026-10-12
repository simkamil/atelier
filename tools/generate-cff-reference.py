# SPDX-License-Identifier: AGPL-3.0-or-later
"""Build and check the deterministic synthetic CFF oracle fixture.

Requires FontTools. Run `py tools/generate-cff-reference.py` to refresh the
fixture, manifest, and receipt. Run with `--check` to verify without writing.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from io import BytesIO
from pathlib import Path

import fontTools
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.recordingPen import DecomposingRecordingPen
from fontTools.pens.t2CharStringPen import T2CharStringPen
from fontTools.misc.psCharStrings import T2CharString
from fontTools.ttLib import TTFont


ROOT = Path(__file__).resolve().parents[1]
REFERENCE = ROOT / "tests" / "font-cff-reference"
FONT_PATH = REFERENCE / "cff-oracle.otf"
MANIFEST_PATH = REFERENCE / "manifest.json"
RECEIPT_PATH = ROOT / "_bmad-output" / "implementation-artifacts" / "cff-independent-oracle.json"
FIXED_TIMESTAMP = 3_600_000_000  # 2018-01-23T08:00:00Z in OpenType seconds since 1904.


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def make_charstring(width: int, commands: list[tuple[str, tuple]]) -> object:
    pen = T2CharStringPen(width, None)
    for op, args in commands:
        getattr(pen, op)(*args)
    return pen.getCharString()


def points_for(font: TTFont, name: str) -> list[dict[str, object]]:
    pen = DecomposingRecordingPen(font.getGlyphSet())
    font.getGlyphSet()[name].draw(pen)
    result = []
    for op, args in pen.value:
        normalized = []
        for arg in args:
            if isinstance(arg, tuple):
                normalized.append([float(arg[0]), float(arg[1])])
            else:
                normalized.append(arg)
        result.append({"op": op, "points": normalized})
    return result


def expected_glyphs() -> dict[str, list[dict[str, object]]]:
    return {
        "triangle": [
            {"op": "moveTo", "points": [[0.0, 0.0]]},
            {"op": "lineTo", "points": [[100.0, 0.0]]},
            {"op": "lineTo", "points": [[100.0, 200.0]]},
            {"op": "closePath", "points": []},
        ],
        "curve": [
            {"op": "moveTo", "points": [[0.0, 0.0]]},
            {"op": "curveTo", "points": [[20.0, 80.0], [80.0, 80.0], [100.0, 0.0]]},
            {"op": "closePath", "points": []},
        ],
        "nested_seac": [
            {"op": "moveTo", "points": [[0.0, 0.0]]},
            {"op": "lineTo", "points": [[100.0, 0.0]]},
            {"op": "lineTo", "points": [[100.0, 200.0]]},
            {"op": "closePath", "points": []},
            {"op": "moveTo", "points": [[40.0, 50.0]]},
            {"op": "lineTo", "points": [[140.0, 50.0]]},
            {"op": "lineTo", "points": [[140.0, 250.0]]},
            {"op": "closePath", "points": []},
            {"op": "moveTo", "points": [[60.0, 370.0]]},
            {"op": "lineTo", "points": [[80.0, 410.0]]},
            {"op": "closePath", "points": []},
        ],
    }


def build_font() -> bytes:
    glyph_order = [".notdef", "triangle", "curve", "A", "acute", "B", "C"]
    charstrings = {
        ".notdef": make_charstring(500, []),
        "triangle": make_charstring(
            600,
            [("moveTo", ((0, 0),)), ("lineTo", ((100, 0),)), ("lineTo", ((100, 200),)), ("closePath", ())],
        ),
        "curve": make_charstring(
            700,
            [("moveTo", ((0, 0),)), ("curveTo", ((20, 80), (80, 80), (100, 0))), ("closePath", ())],
        ),
        "A": make_charstring(
            600,
            [("moveTo", ((0, 0),)), ("lineTo", ((100, 0),)), ("lineTo", ((100, 200),)), ("closePath", ())],
        ),
        "acute": make_charstring(
            300,
            [("moveTo", ((10, 300),)), ("lineTo", ((30, 340),)), ("closePath", ())],
        ),
        "B": T2CharString(program=[10, 20, 65, 194, "endchar"]),
        "C": T2CharString(program=[40, 50, 65, 66, "endchar"]),
    }
    builder = FontBuilder(1000, isTTF=False)
    builder.setupGlyphOrder(glyph_order)
    builder.setupCharacterMap({65: "A", 66: "C"})
    builder.setupHorizontalMetrics(
        {
            ".notdef": (500, 0),
            "triangle": (600, 0),
            "curve": (700, 0),
            "A": (600, 0),
            "acute": (300, 0),
            "B": (600, 0),
            "C": (600, 0),
        }
    )
    builder.setupHorizontalHeader(ascent=800, descent=-200)
    builder.setupNameTable(
        {"familyName": "Atelier CFF Oracle", "styleName": "Regular", "psName": "AtelierCFFOracle-Regular"}
    )
    builder.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200)
    builder.setupPost()
    builder.setupCFF(
        "AtelierCFFOracle-Regular",
        {"FullName": "Atelier CFF Oracle Regular", "FamilyName": "Atelier CFF Oracle", "Weight": "Regular"},
        charstrings,
        {},
    )
    builder.setupMaxp()
    builder.font.recalcTimestamp = False
    builder.font["head"].created = FIXED_TIMESTAMP
    builder.font["head"].modified = FIXED_TIMESTAMP
    out = BytesIO()
    builder.font.save(out)
    return out.getvalue()


def artifact_bytes(font_bytes: bytes) -> tuple[bytes, bytes]:
    font = TTFont(BytesIO(font_bytes))
    glyphs = {name: points_for(font, name) for name in ("triangle", "curve", "C")}
    glyphs["nested_seac"] = glyphs.pop("C")
    if glyphs != expected_glyphs():
        raise AssertionError(f"FontTools outline differed from fixture specification: {glyphs!r}")
    cff_bytes = font.getTableData("CFF ")
    manifest = {
        "schema_version": 1,
        "oracle": "fontTools RecordingPen over TTFont.getGlyphSet()",
        "fonttools_version": fontTools.__version__,
        "fixture": "synthetic OpenType/CFF1, 1000 unitsPerEm, no downloaded fonts",
        "timestamp": {"head_created": FIXED_TIMESTAMP, "head_modified": FIXED_TIMESTAMP, "recalc": False},
        "font_file": FONT_PATH.name,
        "font_sha256": sha256(font_bytes),
        "cff_table_sha256": sha256(cff_bytes),
        "glyphs": glyphs,
    }
    return font_bytes, (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode("utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="verify committed artifacts without writing")
    args = parser.parse_args()

    font_bytes, manifest_bytes = artifact_bytes(build_font())
    if args.check:
        mismatches = []
        if not FONT_PATH.is_file() or FONT_PATH.read_bytes() != font_bytes:
            mismatches.append(str(FONT_PATH.relative_to(ROOT)))
        if not MANIFEST_PATH.is_file() or MANIFEST_PATH.read_bytes() != manifest_bytes:
            mismatches.append(str(MANIFEST_PATH.relative_to(ROOT)))
        if not RECEIPT_PATH.is_file() or RECEIPT_PATH.read_bytes() != manifest_bytes:
            mismatches.append(str(RECEIPT_PATH.relative_to(ROOT)))
        if mismatches:
            raise SystemExit("CFF oracle artifacts differ: " + ", ".join(mismatches))
        print("CFF oracle fixture, manifest, and receipt match; no files written.")
        return 0

    REFERENCE.mkdir(parents=True, exist_ok=True)
    RECEIPT_PATH.parent.mkdir(parents=True, exist_ok=True)
    FONT_PATH.write_bytes(font_bytes)
    MANIFEST_PATH.write_bytes(manifest_bytes)
    RECEIPT_PATH.write_bytes(manifest_bytes)
    print(f"Wrote {FONT_PATH.relative_to(ROOT)} and its oracle manifest/receipt.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
