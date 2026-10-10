# SPDX-License-Identifier: AGPL-3.0-or-later
"""Agent-only cross-implementation checks; Python/FontTools are not product dependencies."""
from pathlib import Path
import hashlib
import json
import random
import struct
import subprocess
import zlib

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
WORK = ROOT / 'target/model-font-oracle'
DRIVER = WORK / 'model-font-oracle.exe'


def run(*args):
    result = subprocess.run([str(DRIVER), *map(str, args)], cwd=ROOT,
                            capture_output=True, text=True, encoding='utf-8', check=True)
    return json.loads(result.stdout)


def crc(data):
    return struct.pack('<I', zlib.crc32(data) & 0xffffffff)


def frame(sequence, values=(1,), atop=None):
    if atop is None:
        atop = b'ATOP' + struct.pack('<HI', 1, len(values))
        for value in values:
            atop += struct.pack('<IQ', 8, value)
    body = struct.pack('<QI', sequence, len(atop)) + atop
    body += crc(body)
    return struct.pack('<I', len(body)) + body


def main():
    WORK.mkdir(parents=True, exist_ok=True)
    names = ['Cargo.lock', '.github/workflows/ci.yml',
             *[p.relative_to(ROOT).as_posix() for name in ('model', 'font') for p in (ROOT/f'crates/{name}').rglob('*.rs')],
             *[f'_bmad-output/implementation-artifacts/{name}' for name in ('model-font-oracle.rs', 'check-model-font-oracle.py', 'generate-font-oracle.py')],
             *[p.relative_to(ROOT).as_posix() for p in (ROOT/'crates/font/tests/fixtures').rglob('*') if p.is_file()]]
    hashes = {name: hashlib.sha256((ROOT/name).read_bytes()).hexdigest() for name in sorted(names)}
    subprocess.run(['py', str(ART/'generate-font-oracle.py')], cwd=ROOT, check=True, capture_output=True)
    for path in (ROOT/'crates/font/tests/fixtures').iterdir():
        if path.suffix in ('.font', '.ttc'):
            assert path.read_bytes() == (WORK/path.name).read_bytes(), ('fixture regeneration', path.name)
    build = subprocess.run(['cargo', 'build', '-p', 'atelier-font', '-p', 'atelier-model', '--locked', '--offline', '--message-format=json'],
                           cwd=ROOT, capture_output=True, text=True, check=True)
    libraries = {}
    for line in build.stdout.splitlines():
        message = json.loads(line)
        if message.get('reason') == 'compiler-artifact':
            for filename in message.get('filenames', []):
                if filename.endswith('.rlib'):
                    libraries[message['target']['name']] = filename
    command = ['rustc', '--edition=2024', str(ART/'model-font-oracle.rs'), '-L', 'dependency=target/debug/deps', '-o', str(DRIVER)]
    for name in ('font', 'model'):
        command.extend(['--extern', f'atelier_{name}={libraries[f"atelier_{name}"]}'])
    subprocess.run(command, cwd=ROOT, check=True)
    report = {'date': '2026-10-10', 'source_sha256': hashes, 'font_faces': 0,
              'selection_cases': 0, 'journal_cases': 0, 'font_mutations': 0, 'failures': []}
    for case in json.loads((WORK/'fonts.json').read_text(encoding='utf-8')):
        actual = run('font', case['file'], case['face'])
        if actual != case['expected']:
            report['failures'].append({'font': case['file'], 'face': case['face'], 'actual': actual, 'expected': case['expected']})
        report['font_faces'] += 1
    # Fixed seed and separate Python mapping provide a reference for both directed endpoints.
    rng = random.Random(73050)
    lines, expected = [], []
    for i in range(12000):
        kind, a, b = i % 5, rng.randrange(20), rng.randrange(20)
        if kind == 1:
            a, b = sorted((a, b))
        if kind == 4:
            b %= 2
        endpoints = [(rng.randrange(3), rng.randrange(32), rng.randrange(2)) for _ in range(2)]
        out = []
        for node, offset, affinity in endpoints:
            if kind == 0 and node == 0 and (offset > a or offset == a and affinity):
                offset += b
            elif kind == 1 and node == 0:
                offset = offset if offset < a else a if offset <= b else offset - (b-a)
            elif kind == 2 and node == 0 and (offset > a or offset == a and affinity):
                node, offset = 1, offset - a
            elif kind == 3 and node == 1:
                node, offset = 0, offset + a
            elif kind == 4 and node == 0:
                node, offset, affinity = 2, a, b
            out.extend((node, offset, affinity))
        lines.append(' '.join(map(str, (kind, a, b, *endpoints[0], *endpoints[1]))))
        expected.append(' '.join(map(str, out)))
    # usize overflow must leave both ranges unchanged, as asserted inside the driver.
    lines.append(f'0 0 {2**64-1} 0 1 1 2 7 1')
    expected.append('error')
    result = subprocess.run([str(DRIVER), 'selection'], input='\n'.join(lines)+'\n', cwd=ROOT,
                            text=True, capture_output=True, check=True)
    assert result.stdout.splitlines() == expected
    report['selection_cases'] = len(lines)

    snapshot, journal = WORK/'snapshot.bin', WORK/'journal.bin'
    run('emit', snapshot, journal)
    resource = b'embedded\0resource'
    payload = struct.pack('<Q', 0)+resource
    s = b'ATJS'+struct.pack('<H', 1)+bytes([1])*16+bytes([2])*16+struct.pack('<QQ', 7, len(payload))+payload
    s += crc(s)
    header = b'ATJR'+struct.pack('<H', 1)+bytes([1])*16+bytes([2])*16+struct.pack('<Q', 7)
    records = [frame(8+i, (i+1,)) for i in range(3)]
    j = header+b''.join(records)
    assert snapshot.read_bytes() == s
    assert journal.read_bytes() == j

    def check(sb, jb, value=None, applied=None, stop=None, error=None):
        snapshot.write_bytes(sb); journal.write_bytes(jb)
        actual = run('replay', snapshot, journal)
        if error:
            assert error in actual.get('error', ''), actual
        else:
            assert actual['value'] == value and actual['applied'] == applied, actual
            assert actual['sequence'] == 7 + applied and actual['resource'] == resource.hex(), actual
            assert stop in actual['stopped'], actual
        report['journal_cases'] += 1

    check(s, j, 3, 3, 'None')
    ends = [len(header)]
    for record in records:
        ends.append(ends[-1]+len(record))
    for cut in range(len(j)+1):
        if cut < len(header):
            check(s, j[:cut], error='InvalidHeader')
        else:
            applied = sum(end <= cut for end in ends[1:])
            check(s, j[:cut], applied, applied, 'None' if cut in ends else 'Truncated')
    for cut in range(len(s)):
        check(s[:cut], j, error='InvalidHeader')
    for offset, error in [(4, 'UnknownVersion'), (6, 'DocumentMismatch'), (22, 'GenerationMismatch'), (38, 'BaseSequenceMismatch')]:
        bad = bytearray(j); bad[offset] ^= 1
        check(s, bad, error=error)
    bad = bytearray(s); bad[-5] ^= 1
    check(bad, j, error='SnapshotChecksumMismatch')
    check(s, header+records[0]+frame(10, (9,))+records[2], 1, 1, 'UnexpectedSequence')
    check(s, header+records[0]+frame(9, atop=b'invalid ATOP')+records[2], 1, 1, 'InvalidTransaction')
    check(s, header+records[0]+frame(9, (20,999))+records[2], 1, 1, 'ApplicationFailed')
    bad = bytearray(records[1]); bad[-1] ^= 1
    check(s, header+records[0]+bad+records[2], 1, 1, 'ChecksumMismatch')
    check(s, header+records[0]+struct.pack('<I', 0xffffffff), 1, 1, 'Truncated')
    for path in [*sorted(WORK.glob('font-*.font')), WORK/'two-faces.ttc']:
        report['font_mutations'] += run('font-fuzz', path)['font_mutations']
    report['source_bytes_unchanged'] = all(hashlib.sha256((ROOT/name).read_bytes()).hexdigest() == digest for name, digest in hashes.items())
    (ART/'model-font-oracle.json').write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n', encoding='utf-8', newline='\n')
    print(json.dumps({key: value for key, value in report.items() if key != 'source_sha256'}, ensure_ascii=False))
    assert report['source_bytes_unchanged'] and not report['failures']


if __name__ == '__main__':
    main()
