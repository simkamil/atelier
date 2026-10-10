# SPDX-License-Identifier: AGPL-3.0-or-later
"""Compare public TrueType paths against independent FontTools geometry."""
from pathlib import Path
import hashlib
import json
import math
import re
import subprocess
from fontTools.ttLib import TTFont
from fontTools.pens.basePen import BasePen

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
WORK = ROOT / 'target/prototype-oracle'


class GeometryPen(BasePen):
    def __init__(self, glyphs):
        super().__init__(glyphs)
        self.contours = []
        self.commands = None

    def _moveTo(self, point):
        self.commands = [('M', *point)]

    def _lineTo(self, point):
        self.commands.append(('L', *point))

    def _qCurveToOne(self, control, point):
        self.commands.append(('Q', *control, *point))

    def _curveToOne(self, first, second, point):
        raise AssertionError('TrueType contour unexpectedly cubic')

    def _closePath(self):
        self.commands.append(('Z',))
        self.contours.append(self.commands)
        self.commands = None

    def _endPath(self):
        raise AssertionError('TrueType contour unexpectedly open')


def rust_debug(text):
    tokens = re.findall(r'[A-Za-z_][A-Za-z_0-9]*|[-+]?\d+(?:\.\d*)?(?:[eE][+-]?\d+)?|[{}\[\]:,]', text)
    position = 0

    def take(expected=None):
        nonlocal position
        value = tokens[position]
        position += 1
        if expected is not None:
            assert value == expected, (value, expected)
        return value

    def value():
        if tokens[position] == '[':
            take('[')
            result = []
            while tokens[position] != ']':
                result.append(value())
                if tokens[position] == ',':
                    take(',')
            take(']')
            return result
        word = take()
        if word in ('true', 'false'):
            return word == 'true'
        if re.fullmatch(r'[-+]?\d+(?:\.\d*)?(?:[eE][+-]?\d+)?', word):
            return float(word)
        take('{')
        result = {'_type': word}
        while tokens[position] != '}':
            key = take()
            take(':')
            result[key] = value()
            if tokens[position] == ',':
                take(',')
        take('}')
        return result

    result = value()
    assert position == len(tokens)
    return result


def commands_from_rust(text):
    result = []
    for contour in rust_debug(text)['subpaths']:
        point = contour['start']
        commands = [('M', point['x'], point['y'])]
        for segment in contour['segments']:
            point = segment['to']
            if segment['_type'] == 'Line':
                commands.append(('L', point['x'], point['y']))
            elif segment['_type'] == 'Quadratic':
                control = segment['control']
                commands.append(('Q', control['x'], control['y'], point['x'], point['y']))
            else:
                raise AssertionError(segment['_type'])
        assert contour['closed']
        commands.append(('Z',))
        result.append(commands)
    return result


def canonical(contours):
    result = []
    for commands in contours:
        start = current = tuple(commands[0][1:])
        segments = []
        for command in commands[1:]:
            if command[0] == 'Z':
                if current != start:
                    segments.append(('L', *current, *start))
                continue
            end = tuple(command[-2:])
            # An explicit zero length closure and an implicit closure agree.
            if command[0] != 'L' or current != end:
                segments.append((command[0], *current, *command[1:]))
            current = end
        rounded = [tuple(v if isinstance(v, str) else round(v, 6) for v in segment) for segment in segments]
        if rounded:
            rotations = [tuple(rounded[i:] + rounded[:i]) for i in range(len(rounded))]
            result.append(min(rotations))
    return sorted(result)


def encode(contours):
    return ' '.join(command[0] + (' ' + ' '.join(format(v, '.12g') for v in command[1:]) if len(command) > 1 else '')
                    for contour in contours for command in contour)


def main():
    WORK.mkdir(parents=True, exist_ok=True)
    font_manifest = json.loads((ROOT / 'data/fonts/manifest.json').read_text(encoding='utf-8'))
    fonts = [ROOT / asset['path'] for asset in font_manifest['assets']]
    compiled = subprocess.run(['cargo', 'build', '-p', 'atelier-font', '--release', '--locked', '--offline', '--message-format=json'],
                              cwd=ROOT, capture_output=True, text=True, check=True)
    artifacts = [json.loads(line) for line in compiled.stdout.splitlines() if line.startswith('{')]
    library = next(Path(filename) for event in artifacts if event.get('reason') == 'compiler-artifact' and event['target']['name'] == 'atelier_font'
                   for filename in event['filenames'] if filename.endswith('.rlib'))
    driver = WORK / 'font-outline-oracle.exe'
    dependencies = library.parent if library.parent.name == 'deps' else library.parent / 'deps'
    subprocess.run(['rustc', '--edition=2024', '-O', str(ART/'font-outline-oracle.rs'), '--extern', f'atelier_font={library}',
                    '-L', f'dependency={dependencies}', '-o', str(driver)], cwd=ROOT, check=True)
    paths = WORK / 'actual-font-paths.tsv'
    with paths.open('wb') as output:
        subprocess.run([str(driver), *map(str, fonts)], cwd=ROOT, stdout=output, check=True)
    records = []
    visual = []
    count = 0
    selected = {}
    ttfonts = [TTFont(font) for font in fonts]
    glyphsets = [font.getGlyphSet() for font in ttfonts]
    for index, font in enumerate(ttfonts):
        cmap = font.getBestCmap()
        selected[index] = {0, *[font.getGlyphID(cmap[ord(char)]) for char in ' AéŒﬁg€ÁȺjﬀÅüç±' if ord(char) in cmap]}
    with paths.open(encoding='utf-8') as stream:
        for line in stream:
            index, glyph, text = line.rstrip('\n').split('\t', 2)
            index, glyph = int(index), int(glyph)
            font = ttfonts[index]
            name = font.getGlyphName(glyph)
            pen = GeometryPen(glyphsets[index])
            glyphsets[index][name].draw(pen)
            expected = pen.contours
            actual = commands_from_rust(text)
            assert canonical(actual) == canonical(expected), (font_manifest['assets'][index]['path'], glyph, name,
                                                             canonical(actual)[:1], canonical(expected)[:1])
            count += 1
            if glyph in selected[index]:
                relative = fonts[index].relative_to(ROOT).as_posix()
                records.append(f'{relative}\t{glyph}\t{encode(expected)}')
                visual.append({'font': relative, 'glyph': glyph, 'expected': encode(expected), 'actual': encode(actual)})
    corpus = ROOT / 'tests/font-outline-reference'
    corpus.mkdir(parents=True, exist_ok=True)
    data = ('\n'.join(records)+'\n').encode('utf-8')
    (corpus/'outlines.tsv').write_bytes(data)
    manifest = {'schema_version': 1, 'oracle': 'FontTools', 'faces': 20, 'cases': len(records),
                'file': 'outlines.tsv', 'sha256': hashlib.sha256(data).hexdigest()}
    (corpus/'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n', encoding='utf-8')
    (WORK/'font-visual-input.json').write_text(json.dumps(visual), encoding='utf-8')
    evidence = {'font_faces': 20, 'compared_glyphs': count, 'reference_cases': len(records),
                'canonical_coordinate_rounding_digits': 6, 'geometry_matches': True,
                'reference_sha256': manifest['sha256']}
    (ART/'prototype-font-oracle.json').write_text(json.dumps(evidence, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(evidence))


if __name__ == '__main__':
    main()
