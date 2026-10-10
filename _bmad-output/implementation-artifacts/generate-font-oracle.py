# SPDX-License-Identifier: AGPL-3.0-or-later
"""Agent-only independent OpenType fixtures, generated and read by FontTools."""
from pathlib import Path
from io import BytesIO
import hashlib
import json

from fontTools.fontBuilder import FontBuilder
from fontTools.ttLib import TTFont, TTCollection
from fontTools.ttLib.tables._c_m_a_p import CmapSubtable
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.pens.t2CharStringPen import T2CharStringPen

ROOT = Path(__file__).resolve().parents[2]
WORK = ROOT / 'target/model-font-oracle'
GLYPHS = ['.notdef', 'A', 'C', 'B', 'B.alt', 'smile']
MAP = {0x41: 'A', 0x42: 'B', 0x43: 'C'}


def font(ttf, cmap_format):
    builder = FontBuilder(1000, isTTF=ttf)
    builder.setupGlyphOrder(GLYPHS)
    builder.setupCharacterMap(MAP)
    builder.setupHorizontalMetrics({name: (600 if i == 0 else 500, i * 7 - 14)
                                    for i, name in enumerate(GLYPHS)})
    builder.setupHorizontalHeader(ascent=800, descent=-200, lineGap=30)
    builder.setupNameTable({'familyName': 'Atelier Oracle Épreuve',
                           'styleName': 'Regular', 'uniqueFontIdentifier': 'Atelier050',
                           'fullName': 'Atelier Oracle Épreuve Regular',
                           'psName': 'AtelierOracle-Regular', 'version': 'Version 1.0'})
    builder.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800,
                     usWinDescent=200)
    builder.setupPost()
    if ttf:
        outlines = {}
        for name in GLYPHS:
            pen = TTGlyphPen(None)
            pen.moveTo((0, 0)); pen.lineTo((100, 0)); pen.lineTo((100, 100)); pen.closePath()
            outlines[name] = pen.glyph()
        builder.setupGlyf(outlines)
        builder.setupMaxp()
    else:
        outlines = {}
        for name in GLYPHS:
            pen = T2CharStringPen(600 if name == '.notdef' else 500, None)
            outlines[name] = pen.getCharString()
        builder.setupCFF('AtelierOracle-Regular', {}, outlines, {})
        builder.setupMaxp()
    subtable = CmapSubtable.newSubtable(cmap_format)
    subtable.platformID = 0
    subtable.platEncID = 4 if cmap_format == 12 else 3
    subtable.language = 0
    subtable.cmap = dict(MAP)
    if cmap_format == 12:
        subtable.cmap[0x1F600] = 'smile'
    variation = CmapSubtable.newSubtable(14)
    variation.platformID = 0
    variation.platEncID = 5
    variation.language = 0
    variation.cmap = {}
    variation.uvsDict = {0xFE0F: [(0x41, None), (0x42, 'B.alt')]}
    builder.font['cmap'].tables = [subtable, variation]
    builder.font['head'].created = builder.font['head'].modified = 3800000000
    return builder.font


def expected(face):
    cmap = face.getBestCmap()
    return {'glyphs': {str(cp): face.getGlyphID(cmap.get(cp, '.notdef'))
                       for cp in [0, 0x20, 0x40, 0x41, 0x42, 0x43, 0x44, 0xFFFF, 0x1F600, 0x10FFFF]},
            'metrics': [list(face['hmtx'].metrics[name]) for name in face.getGlyphOrder()],
            'units_per_em': face['head'].unitsPerEm,
            'glyph_count': face['maxp'].numGlyphs,
            'hhea': [face['hhea'].ascent, face['hhea'].descent, face['hhea'].lineGap],
            'variations': {'65:65039': ['default', 1], '66:65039': ['non-default', 4],
                           '67:65039': ['unsupported', 0]},
            'unicode_names': [[n.platformID, n.platEncID, n.langID, n.nameID, n.toUnicode()]
                              for n in face['name'].names if n.platformID == 0 or n.platformID == 3 and n.platEncID in (1, 10)]}


def main():
    WORK.mkdir(parents=True, exist_ok=True)
    cases = []
    for is_ttf in [True, False]:
        for format_number in [4, 6, 12]:
            path = WORK / f'font-{int(is_ttf)}-{format_number}.font'
            font(is_ttf, format_number).save(path)
            reader = TTFont(path)
            cases.append({'file': str(path.relative_to(ROOT)), 'face': 0,
                          'sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
                          'expected': expected(reader)})
    collection = TTCollection()
    collection.fonts = [font(True, 4), font(False, 12)]
    path = WORK / 'two-faces.ttc'
    collection.save(path)
    readers = TTCollection(path).fonts
    for index, reader in enumerate(readers):
        cases.append({'file': str(path.relative_to(ROOT)), 'face': index,
                      'sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
                      'expected': expected(reader)})
    (WORK / 'fonts.json').write_text(json.dumps(cases, ensure_ascii=False, indent=2),
                                     encoding='utf-8', newline='\n')
    print(json.dumps({'generated_font_faces': len(cases), 'files': len(set(c['file'] for c in cases))}))


if __name__ == '__main__':
    main()
