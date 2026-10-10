# SPDX-License-Identifier: AGPL-3.0-or-later
"""Independent HarfBuzz golden data; Python tooling never ships in the product."""
from pathlib import Path
import hashlib
import itertools
import json
import string
import sys
import fontTools
from fontTools.ttLib import TTFont

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'target/typography-oracle/python'))
import uharfbuzz as hb


def selected_ligatures(tt):
    if 'GSUB' not in tt:
        return []
    table = tt['GSUB'].table
    scripts = {r.ScriptTag: r.Script for r in table.ScriptList.ScriptRecord}
    script = scripts.get('latn', scripts.get('DFLT'))
    if script is None or script.DefaultLangSys is None:
        return []
    lang = script.DefaultLangSys
    indices = list(lang.FeatureIndex)
    if lang.ReqFeatureIndex != 0xffff:
        indices.append(lang.ReqFeatureIndex)
    reverse = {}
    for cp, name in sorted(tt.getBestCmap().items()):
        reverse.setdefault(name, chr(cp))
    result = []
    for index in indices:
        feature = table.FeatureList.FeatureRecord[index]
        if feature.FeatureTag != 'liga':
            continue
        for lookup_index in feature.Feature.LookupListIndex:
            lookup = table.LookupList.Lookup[lookup_index]
            for subtable in lookup.SubTable:
                if lookup.LookupType == 7:
                    subtable = subtable.ExtSubTable
                if not hasattr(subtable, 'ligatures'):
                    continue
                for first, ligatures in subtable.ligatures.items():
                    for ligature in ligatures:
                        names = [first, *ligature.Component]
                        if all(name in reverse for name in names):
                            result.append(''.join(reverse[name] for name in names))
    return result


def main():
    manifest = json.loads((ROOT / 'data/fonts/manifest.json').read_text(encoding='utf-8'))
    records = []
    faces = []
    alphabet = string.ascii_letters
    base = [''.join(pair) for pair in itertools.product(alphabet, repeat=2)]
    base += ['', 'fi', 'fl', 'ffi', 'ffl', 'fffi', 'fifi', 'office', 'affliction',
             'AVATAR To WAVE', '0123456789', 'Atelier français', 'fi ffi fl ffl fi',
             'AVAVAVA', 'ToToTo', '  a  b ', 'a,b.c: d; e! f?', 'Œuvre déjà créée']
    for asset in manifest['assets']:
        relative = asset['path']
        data = (ROOT / relative).read_bytes()
        tt = TTFont(ROOT / relative)
        font = hb.Font(hb.Face(data))
        font.scale = (font.face.upem, font.face.upem)
        features = {record.FeatureTag: False for tag in ('GSUB', 'GPOS') if tag in tt
                    for record in tt[tag].table.FeatureList.FeatureRecord}
        features.update({'kern': True, 'liga': True})
        cases = sorted(set(base + selected_ligatures(tt)))
        for text in cases:
            buffer = hb.Buffer()
            buffer.add_codepoints([ord(char) for char in text])
            buffer.direction = 'ltr'
            buffer.script = 'Latn'
            buffer.language = 'und'
            buffer.cluster_level = hb.BufferClusterLevel.MONOTONE_CHARACTERS
            hb.shape(font, buffer, features)
            infos, positions = buffer.glyph_infos or [], buffer.glyph_positions or []
            glyphs = []
            for i, (info, pos) in enumerate(zip(infos, positions)):
                end = next((later.cluster for later in infos[i+1:] if later.cluster > info.cluster), len(text))
                glyphs.append(','.join(map(str, (info.codepoint, info.cluster, end,
                                                pos.x_offset, pos.y_offset, pos.x_advance, pos.y_advance))))
            records.append(relative + '\t' + ','.join(f'{ord(c):X}' for c in text) + '\t' + ';'.join(glyphs))
        faces.append({'path': relative, 'sha256': hashlib.sha256(data).hexdigest(), 'cases': len(cases)})
    corpus = ROOT / 'tests/font-layout-reference'
    corpus.mkdir(exist_ok=True)
    content = ('\n'.join(records) + '\n').encode('utf-8')
    (corpus / 'layout.tsv').write_bytes(content)
    evidence = {'schema_version': 1, 'oracle': 'HarfBuzz', 'uharfbuzz_version': hb.__version__,
                'harfbuzz_version': hb.version_string(), 'fonttools_version': fontTools.__version__,
                'features': 'Only kern/liga; all other GSUB/GPOS feature tags explicitly disabled',
                'script': 'Latn', 'language': 'und (DefaultLangSys)', 'direction': 'ltr',
                'cluster_unit': 'Unicode scalar index', 'cases': len(records), 'faces': faces,
                'file': 'layout.tsv', 'sha256': hashlib.sha256(content).hexdigest()}
    (corpus / 'manifest.json').write_text(json.dumps(evidence, ensure_ascii=False, indent=2)+'\n', encoding='utf-8', newline='\n')
    print(json.dumps({'cases': len(records), 'faces': len(faces), 'sha256': evidence['sha256']}))


if __name__ == '__main__':
    main()
