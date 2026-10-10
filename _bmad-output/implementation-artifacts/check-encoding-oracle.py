# SPDX-License-Identifier: AGPL-3.0-or-later
"""Independent byte-codec oracle, kept outside the product test implementation."""
from pathlib import Path
import json
import random
import subprocess
import tempfile

root = Path(__file__).resolve().parents[2]
temp = Path(tempfile.mkdtemp(prefix='atelier-encoding-oracle-'))
def literal(text):
    return '"' + ''.join('\\u{' + format(ord(c), 'x') + '}' for c in text) + '"'
lines = ['use atelier_unicode::{decode, encode, detect_encoding, Encoding};', 'fn main() {']
cases = 0
for name, codec in [('Windows1252', 'cp1252'), ('Iso8859_1', 'latin1'), ('Cp437', 'cp437'), ('MacRoman', 'mac_roman')]:
    for byte in range(256):
        expected = bytes([byte]).decode(codec, 'replace')
        lines.append(f'assert_eq!(decode(&[{byte}], Encoding::{name}), {literal(expected)}, "{name} {byte:02X}");')
        if expected != '\ufffd':
            lines.append(f'assert_eq!(encode({literal(expected)}, Encoding::{name}).unwrap(), [{byte}]);')
        cases += 1
rng = random.Random(14060)
for _ in range(4000):
    data = bytes(rng.randrange(256) for _ in range(rng.randrange(1, 9)))
    if data.startswith(bytes.fromhex('efbbbf')):
        continue
    lines.append(f'assert_eq!(decode(&{list(data)!r}, Encoding::Utf8), {literal(data.decode("utf8", "replace"))});')
    cases += 1
lines.extend([
    'assert_eq!(decode(&[0xff,0xfe,0x3d,0xd8,0x42,0xde], Encoding::Utf16Le), "🙂");',
    'assert_eq!(decode(&[0xfe,0xff,0xd8,0x3d,0xde,0x42], Encoding::Utf16Be), "🙂");',
    'assert_eq!(detect_encoding(&[0xef,0xbb,0xbf,0xff]), Encoding::Utf8);',
    'assert_eq!(encode("é🙂", Encoding::Windows1252).unwrap_err().utf8_position, 2);',
    'println!("independent encoding oracle passed");', '}'])
source = temp / 'oracle.rs'
source.write_text('\n'.join(lines), encoding='utf-8')
commands = [
    ['rustc', '--edition=2024', '--crate-name', 'atelier_unicode', '--crate-type', 'rlib', str(root/'crates/unicode/src/lib.rs'), '-o', str(temp/'libatelier_unicode.rlib')],
    ['rustc', '--edition=2024', str(source), '--extern', f'atelier_unicode={temp / "libatelier_unicode.rlib"}', '-o', str(temp/'oracle.exe')],
    [str(temp/'oracle.exe')],
]
for command in commands:
    subprocess.run(command, cwd=root, check=True)
evidence = {'reference': 'Python standard-library codecs (independent of Rust mapping tables)',
            'cases': cases + 4, 'passed': True, 'source': str(source)}
(root/'_bmad-output/implementation-artifacts/encoding-oracle.json').write_text(json.dumps(evidence, indent=2), encoding='utf-8')
print(json.dumps(evidence))
