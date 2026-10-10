# SPDX-License-Identifier: AGPL-3.0-or-later
"""Cross-check real crates with independent Python stdlib parsers; agent-only."""
from pathlib import Path
import hashlib
import io
import json
import random
import subprocess
import tempfile
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
EXE = ROOT / 'target/format-oracle.exe'

def run(args, check=True):
    result = subprocess.run([str(EXE), *map(str, args)], capture_output=True, text=True, encoding='utf-8', cwd=ROOT)
    if check and result.returncode:
        raise AssertionError((args, result.stdout, result.stderr))
    return result

def qualified(name):
    if name.startswith('{'):
        uri, local = name[1:].split('}', 1)
    else:
        uri, local = '', name
    return uri.encode().hex() + ':' + local.encode().hex()

def canonical(element):
    result = ['S ' + qualified(element.tag) + ' ' + ' '.join(sorted(qualified(k) + ':' + v.encode().hex() for k, v in element.attrib.items()))]
    if element.text:
        result.append('T ' + element.text.encode().hex())
    for child in element:
        result.extend(canonical(child))
        if child.tail:
            result.append('T ' + child.tail.encode().hex())
    result.append('E')
    return result

def merge_text(lines):
    result = []
    for line in lines:
        if line.startswith('T ') and result and result[-1].startswith('T '):
            result[-1] += line[2:]
        elif line != 'T ':
            result.append(line)
    return [line.rstrip() for line in result]

class NonSeekable(io.BytesIO):
    """A ZIP output sink whose lack of seek forces general-purpose bit 3."""
    def seekable(self):
        return False
    def seek(self, *args):
        raise io.UnsupportedOperation('non-seekable oracle sink')

def write_stream_zip(path, entries, force_zip64=False):
    sink = NonSeekable()
    with zipfile.ZipFile(sink, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
        for name, data, method in entries:
            info = zipfile.ZipInfo(name, date_time=(2020, 1, 2, 3, 4, 6))
            info.compress_type = method
            with archive.open(info, 'w', force_zip64=force_zip64) as member:
                member.write(data)
    path.write_bytes(sink.getvalue())

def remove_descriptor_signature(path, name):
    """Make a one-member Python archive's descriptor unsigned, retaining its width."""
    data = bytearray(path.read_bytes())
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        info = archive.getinfo(name)
        pos = info.header_offset
        name_len, extra_len = __import__('struct').unpack_from('<HH', data, pos + 26)
        descriptor = pos + 30 + name_len + extra_len + info.compress_size
        assert data[descriptor:descriptor + 4] == b'PK\x07\x08'
    zip64_locator = data.rfind(b'PK\x06\x07')
    zip64_eocd_offset = None
    if zip64_locator >= 0:
        zip64_eocd_offset = __import__('struct').unpack_from('<Q', data, zip64_locator + 8)[0]
    del data[descriptor:descriptor + 4]
    eocd = data.rfind(b'PK\x05\x06')
    cd_offset = __import__('struct').unpack_from('<I', data, eocd + 16)[0]
    if cd_offset != 0xffffffff:
        __import__('struct').pack_into('<I', data, eocd + 16, cd_offset - 4)
    if zip64_eocd_offset is not None:
        locator = data.rfind(b'PK\x06\x07')
        actual_zip64_offset = zip64_eocd_offset - 4
        __import__('struct').pack_into('<Q', data, locator + 8, actual_zip64_offset)
        __import__('struct').pack_into('<Q', data, actual_zip64_offset + 48,
                                       __import__('struct').unpack_from('<Q', data, actual_zip64_offset + 48)[0] - 4)
    path.write_bytes(data)

def corrupt_stored_member(path, name):
    data = bytearray(path.read_bytes())
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        info = archive.getinfo(name)
        pos = info.header_offset
        name_len, extra_len = __import__('struct').unpack_from('<HH', data, pos + 26)
        payload = pos + 30 + name_len + extra_len
        assert info.compress_type == zipfile.ZIP_STORED and info.file_size
    data[payload] ^= 0x40
    path.write_bytes(data)

def main():
    source_paths = {
        'crates/xml/src/lib.rs': ROOT/'crates/xml/src/lib.rs',
        'crates/zip/src/lib.rs': ROOT/'crates/zip/src/lib.rs',
        '_bmad-output/implementation-artifacts/format-oracle.rs': ART/'format-oracle.rs',
        '_bmad-output/implementation-artifacts/check-format-oracle.py': ART/'check-format-oracle.py',
    }
    tested_hashes = {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in source_paths.items()}
    subprocess.run(['cargo', 'build', '-p', 'atelier-xml', '-p', 'atelier-zip', '--locked', '--offline'], cwd=ROOT, check=True)
    subprocess.run(['rustc', '--edition=2024', str(ART/'format-oracle.rs'), '--extern', 'atelier_xml=target/debug/libatelier_xml.rlib', '--extern', 'atelier_zip=target/debug/libatelier_zip.rlib', '-L', 'dependency=target/debug/deps', '-o', str(EXE)], cwd=ROOT, check=True)
    evidence = {'date': '2026-10-10', 'xml_parses': 0, 'xml_round_trips': 0, 'xml_rejections': 0, 'xml_prolog_doctype_cases': 2, 'xml_comment_pi_line_ending_case': True, 'zip_reads': 0, 'zip_writes': 0, 'zip_descriptor_cases': [], 'failures': []}
    with tempfile.TemporaryDirectory(prefix='atelier-format-oracle-') as temp:
        temp = Path(temp)
        inputs = [p.read_bytes() for p in (ROOT/'tests/format-reference/xml').glob('*.xml') if not p.name.startswith(('malformed-', 'doctype-no-'))]
        # XML lexical constructs that ElementTree discards but the event parser must consume safely.
        inputs += [b"<?xml version='1.0' standalone='yes'?><!--a\r\nb\rc--><!DOCTYPE r [<!ELEMENT r (#PCDATA)>]><r><?work one\r\ntwo\rthree?>ok</r>",
                   b"<?xml version='1.0'?><!DOCTYPE r SYSTEM 'urn:unresolved'><r/> "]
        inputs += [f'<r xmlns="urn:r" xmlns:p="urn:p" a="&#13;é{n}"><p:x p:a="&quot;&amp;">🙂{n}<![CDATA[a\r\nb]]>tail</p:x><x xmlns=""/></r>'.encode() for n in range(12)]
        for i, data in enumerate(inputs):
            reference = merge_text(canonical(ET.fromstring(data)))
            for chunk in (1, 2, 3, 7, 4096):
                path, out = temp/'input.xml', temp/'roundtrip.xml'
                path.write_bytes(data)
                result = run(['xml', path, chunk, out], check=False)
                if result.returncode or merge_text(result.stdout.splitlines()) != reference:
                    evidence['failures'].append({'case': f'xml-{i}-{chunk}', 'stderr': result.stderr, 'actual': result.stdout, 'expected': reference})
                else:
                    evidence['xml_parses'] += 1
                    if merge_text(canonical(ET.fromstring(out.read_bytes()))) == reference:
                        evidence['xml_round_trips'] += 1
                    else:
                        evidence['failures'].append({'case': f'xml-roundtrip-{i}-{chunk}'})
        invalid = [b'<r a="1"b="2"/>', b'<r>]]></r>', b'<r>&#+65;</r>', b'<r>&#x+41;</r>', b'<r xmlns:p="a" xmlns:p="b"/>', b'<r xmlns:p="http://www.w3.org/2000/xmlns/"/>', b'<r\xc2\xa0a="1"/>', b'<r/><?xml version="1.0"?>', b' <?xml version="1.0"?><r/>', b'<?xml?><r/>', b'<?XML version="1.0"?><r/>', b'<?xml version="1.1"?><r/>', b'<![CDATA[outside]]><r/>', b'<r><!--x---></r>', b'<r/>tail',
                   b"<?xml version='1.0' encoding='UTF-16'?><r/>", b'<!DOCTYPE r SYSTEM "urn:x"><!DOCTYPE r><r/>',
                   b'<!DOCTYPE r [<!ENTITY x "y">]><r>&x;</r>', b'<r><!--a--b--></r>', b'<r><?p a?><?p b?></r><!--ok\r\nbad--x-->']
        for i, data in enumerate(invalid):
            path = temp/'invalid.xml'
            path.write_bytes(data)
            if run(['xml', path, 1], check=False).returncode:
                evidence['xml_rejections'] += 1
            else:
                evidence['failures'].append({'case': f'xml-invalid-{i}', 'data': data.hex()})
        rng = random.Random(223031)
        for i in range(24):
            entries = [] if i == 0 else [('ascii.txt', bytes(rng.randrange(256) for _ in range(i*211))), ('été.txt', b'alpha '*i*71), ('日本/🙂.xml', '<r>é🙂</r>'.encode()), ('empty', b'')]
            for force in (False, True):
                path = temp/'python.zip'
                with zipfile.ZipFile(path, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
                    for j, (name, data) in enumerate(entries):
                        info = zipfile.ZipInfo(name, date_time=(1980,1,1,0,0,0))
                        info.compress_type = zipfile.ZIP_STORED if j % 2 == 0 else zipfile.ZIP_DEFLATED
                        with archive.open(info, 'w', force_zip64=force) as member:
                            member.write(data)
                result = run(['zip-read', path])
                expected = ['COMPLETE true'] + [f'OK {n.encode().hex()} {0 if j%2==0 else 8} {d.hex()}' for j,(n,d) in enumerate(entries)]
                assert result.stdout.splitlines() == expected, (i, force, result.stdout[:500])
                evidence['zip_reads'] += 1
                args = ['zip-write', temp/'rust.zip', '64' if force else '32']
                for j, (name, data) in enumerate(entries):
                    source = temp/f'{j}.bin'
                    source.write_bytes(data)
                    args += [name, str(0 if j%2==0 else 8), source]
                run(args)
                first = (temp/'rust.zip').read_bytes()
                run(args)
                assert first == (temp/'rust.zip').read_bytes()
                with zipfile.ZipFile(temp/'rust.zip') as archive:
                    assert archive.testzip() is None
                    assert archive.namelist() == [n for n,_ in entries]
                    assert [(n, archive.read(n)) for n in archive.namelist()] == entries
                evidence['zip_writes'] += 1
        # Streamed Python outputs set bit 3. Cover signed 32-bit, unsigned 32-bit,
        # and signed ZIP64 descriptors forced for tiny entries.
        stream_cases = [
            ('stream-signed-32', [('first.txt', b'first payload', zipfile.ZIP_DEFLATED)], False, False),
            ('stream-unsigned-32', [('first.txt', b'unsigned payload', zipfile.ZIP_DEFLATED)], False, True),
            ('stream-signed-64', [('first.txt', b'zip64 descriptor', zipfile.ZIP_DEFLATED)], True, False),
            ('stream-unsigned-64', [('first.txt', b'zip64 unsigned descriptor', zipfile.ZIP_DEFLATED)], True, True),
        ]
        for label, members, force64, unsigned in stream_cases:
            path = temp/f'{label}.zip'
            write_stream_zip(path, members, force64)
            if unsigned:
                remove_descriptor_signature(path, members[0][0])
            raw = path.read_bytes()
            with zipfile.ZipFile(io.BytesIO(raw)) as reference:
                info = reference.getinfo(members[0][0])
                name_len, extra_len = __import__('struct').unpack_from('<HH', raw, info.header_offset + 26)
                payload_end = info.header_offset + 30 + name_len + extra_len + info.compress_size
                eocd = raw.rfind(b'PK\x05\x06')
                central_offset = __import__('struct').unpack_from('<I', raw, eocd + 16)[0]
                descriptor_length = central_offset - payload_end
                expected_length = (20 if unsigned else 24) if force64 else (12 if unsigned else 16)
                assert descriptor_length == expected_length, (label, descriptor_length, expected_length)
                assert reference.testzip() is None
            result = run(['zip-read', path])
            assert result.stdout.splitlines() == ['COMPLETE true'] + [
                f'OK {name.encode().hex()} {8 if method == zipfile.ZIP_DEFLATED else 0} {data.hex()}'
                for name, data, method in members
            ], (label, result.stdout, result.stderr)
            evidence['zip_reads'] += 1
            evidence['zip_descriptor_cases'].append({'case': label, 'nonseekable_output': True, 'force_zip64': force64, 'unsigned_descriptor': unsigned, 'descriptor_bytes': descriptor_length, 'crate_read': 'passed'})

        # Corrupt just one stored payload in an otherwise sound multi-member archive.
        damaged = temp/'one-bad-member.zip'
        write_stream_zip(damaged, [('bad.bin', b'bad data', zipfile.ZIP_STORED),
                                   ('healthy.txt', b'healthy sibling', zipfile.ZIP_DEFLATED)], False)
        # The stored entry uses a descriptor; alter its payload while preserving offsets.
        corrupt_stored_member(damaged, 'bad.bin')
        result = run(['zip-read', damaged])
        lines = result.stdout.splitlines()
        if lines != ['COMPLETE true', f'ERR {b"bad.bin".hex()} CrcMismatch',
                     f'OK {b"healthy.txt".hex()} 8 {b"healthy sibling".hex()}']:
            evidence['failures'].append({'case': 'zip-local-crc-corruption', 'stdout': result.stdout, 'stderr': result.stderr})
        else:
            evidence['zip_local_errors'] = 1
            evidence['zip_healthy_siblings'] = 1
            evidence['zip_corrupt_member'] = {'member': 'bad.bin', 'expected_error': 'CrcMismatch', 'healthy_sibling_read': True}

        # A truncated central directory should trigger bounded local recovery for
        # entries with sizes available in their local headers (no data descriptor).
        recovery = temp/'truncated-central.zip'
        with zipfile.ZipFile(recovery, 'w') as archive:
            info = zipfile.ZipInfo('recover.txt', date_time=(2020, 1, 2, 3, 4, 6))
            info.compress_type = zipfile.ZIP_STORED
            archive.writestr(info, b'recovered from local header')
        recovery_bytes = recovery.read_bytes()
        eocd = recovery_bytes.rfind(b'PK\x05\x06')
        cd_offset = __import__('struct').unpack_from('<I', recovery_bytes, eocd + 16)[0]
        recovery.write_bytes(recovery_bytes[:cd_offset])
        result = run(['zip-read', recovery])
        wanted = ['COMPLETE false', f'OK {b"recover.txt".hex()} 0 {b"recovered from local header".hex()}']
        if result.stdout.splitlines() != wanted:
            evidence['failures'].append({'case': 'zip-truncated-central-recovery', 'stdout': result.stdout, 'stderr': result.stderr})
        else:
            evidence['zip_local_recovery'] = 1
            evidence['zip_local_recovery_member'] = 'recover.txt'
        fixture_manifest = json.loads((ROOT/'tests/format-reference/manifest.json').read_text(encoding='utf-8'))
        for fixture in fixture_manifest['fixtures']:
            data = (ROOT/'tests/format-reference'/fixture['path']).read_bytes()
            assert hashlib.sha256(data).hexdigest() == fixture['sha256']
    current_hashes = {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in source_paths.items()}
    evidence['source_sha256'] = tested_hashes
    if current_hashes != tested_hashes:
        evidence['failures'].append({'case': 'source-changed-during-oracle', 'tested': tested_hashes, 'after': current_hashes})
    (ART/'format-oracle.json').write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps(evidence, ensure_ascii=False))
    return bool(evidence['failures'])

if __name__ == '__main__':
    raise SystemExit(main())
