# SPDX-License-Identifier: AGPL-3.0-or-later
"""Independent bidirectional interoperability checks against Python's stdlib zlib."""
from pathlib import Path
import json
import hashlib
import random
import struct
import subprocess
import tempfile
import sys
import zlib

root = Path(__file__).resolve().parents[2]
work = Path(tempfile.mkdtemp(prefix="atelier-compression-oracle-"))
source = r'''
#[path = "__SOURCE__"] mod codec;
use std::io::{Read, Write};
fn decode(data: &[u8], format: codec::Format, ic: usize, oc: usize, limit: u64) -> Result<Vec<u8>, String> {
    let mut state = codec::InflateState::new(format, limit);
    let mut cursor = 0usize;
    let mut result = Vec::new();
    let mut output = vec![0u8; oc];
    for _ in 0..2_000_000 {
        let end = (cursor + ic).min(data.len());
        let p = state.push(&data[cursor..end], &mut output, end == data.len()).map_err(|e| e.to_string())?;
        assert!(p.consumed <= end - cursor && p.produced <= output.len());
        cursor += p.consumed;
        result.extend_from_slice(&output[..p.produced]);
        if p.status == codec::Status::Done {
            if cursor != data.len() { return Err("trailing bytes left by inflater".into()); }
            return Ok(result);
        }
        if p.consumed == 0 && p.produced == 0 && p.status == codec::Status::NeedInput {
            return Err("decoder made no progress with available input".into());
        }
    }
    Err("iteration budget exhausted".into())
}
fn main() {
    let mut input = Vec::new(); std::io::stdin().read_to_end(&mut input).unwrap();
    let mut cursor = 0;
    let mut out = std::io::BufWriter::new(std::io::stdout());
    while cursor < input.len() {
        let mode=input[cursor]; let wrapped=input[cursor+1]; let level=input[cursor+2]; cursor+=3;
        let ic=u32::from_le_bytes(input[cursor..cursor+4].try_into().unwrap()) as usize; cursor+=4;
        let oc=u32::from_le_bytes(input[cursor..cursor+4].try_into().unwrap()) as usize; cursor+=4;
        let limit=u64::from_le_bytes(input[cursor..cursor+8].try_into().unwrap()); cursor+=8;
        let len=u32::from_le_bytes(input[cursor..cursor+4].try_into().unwrap()) as usize; cursor+=4;
        let data=&input[cursor..cursor+len]; cursor+=len;
        let format=if wrapped==0 {codec::Format::Deflate} else {codec::Format::Zlib};
        let result=if mode==0 {
            decode(data,format,ic,oc,limit)
        } else {
            let mut buffer=vec![0;data.len()*2+8192];
            let level=if level==0 {codec::Level::Fast} else {codec::Level::Normal};
            let n=if wrapped==0 {codec::compress(data,level,&mut buffer)} else {codec::compress_zlib(data,level,&mut buffer)};
            n.map(|n|{buffer.truncate(n);buffer}).map_err(|e|e.to_string())
        };
        let (status,payload)=match result {Ok(v)=>(0u8,v),Err(e)=>(1,e.into_bytes())};
        out.write_all(&[status]).unwrap(); out.write_all(&(payload.len() as u32).to_le_bytes()).unwrap(); out.write_all(&payload).unwrap();
    }
}
'''.replace("__SOURCE__", (root / "crates/compress/src/lib.rs").as_posix())
(work / "oracle.rs").write_text(source, encoding="utf-8")
subprocess.run(["rustc", "--edition=2024", "-A", "dead_code", "-O", str(work / "oracle.rs"), "-o", str(work / "oracle.exe")], check=True, cwd=root)
rng = random.Random(0xA7E11E2)
fixtures = [b"", b"a", b"hello world", b"abc" * 1000, bytes(range(256)) * 257,
            ("Atelier — géométrie, modèles et texte.\n" * 1600).encode(), bytes(rng.randrange(256) for _ in range(70000))]
for i in range(64):
    alphabet = (2, 7, 31, 256)[i % 4]
    fixtures.append(bytes(rng.randrange(alphabet) for _ in range(rng.randrange(1, 8193))))
corpus = {name: (root / "crates/compress/tests/corpus" / name).read_bytes()
          for name in ("repetitive.txt", "mixed-utf8.txt", "binary-repeat.bin", "incompressible.bin")}
fixtures.extend(corpus.values())
corpus_names = {data: name for name, data in corpus.items()}
tasks = []
for data in fixtures:
    for wrapped in (0, 1):
        for level in (0, 1):
            tasks.append((1, wrapped, level, 1, 1, len(data), data, data))
        for level, strategy in ((0, zlib.Z_DEFAULT_STRATEGY), (6, zlib.Z_FIXED), (6, zlib.Z_DEFAULT_STRATEGY)):
            compressor = zlib.compressobj(level, zlib.DEFLATED, 15 if wrapped else -15, 8, strategy)
            packed = compressor.compress(data) + compressor.flush()
            layouts = ((1, 1), (7, 31), (max(len(packed), 1), 4096)) if len(data) < 9000 else ((13, 71),)
            for ic, oc in layouts:
                tasks.append((0, wrapped, 0, ic, oc, len(data), packed, data))
            if data:
                tasks.append((0, wrapped, 0, 11, 23, len(data) - 1, packed, None))
            if len(data) < 100:
                for end in range(len(packed)):
                    tasks.append((0, wrapped, 0, 1, 1, len(data), packed[:end], None))
            if wrapped:
                bad = packed[:-1] + bytes([packed[-1] ^ 1])
                tasks.append((0, wrapped, 0, 3, 17, len(data), bad, None))
wire = bytearray()
for mode, wrapped, level, ic, oc, limit, data, expected in tasks:
    wire.extend(struct.pack("<BBBIIQI", mode, wrapped, level, ic, oc, limit, len(data)))
    wire.extend(data)
(work / "input.bin").write_bytes(wire)
result = subprocess.run([str(work / "oracle.exe")], input=wire, capture_output=True, cwd=root, timeout=180)
if result.returncode:
    raise RuntimeError(f"Rust oracle failed ({result.returncode}); reproducer in {work}; {result.stderr.decode(errors='replace')[-2000:]}")
cursor = 0
counts = {"encode_interop": 0, "decode_interop": 0, "reject_invalid_or_limit": 0}
wire_fixtures = {}
corpus_sizes = {}
for i, (mode, wrapped, level, ic, oc, limit, data, expected) in enumerate(tasks):
    status, size = struct.unpack_from("<BI", result.stdout, cursor)
    cursor += 5
    payload = result.stdout[cursor:cursor + size]
    cursor += size
    if expected is None:
        assert status == 1, (i, "invalid stream accepted", wrapped, data[:40].hex())
        counts["reject_invalid_or_limit"] += 1
        continue
    assert status == 0, (i, "valid stream rejected", payload.decode(errors="replace"), wrapped, ic, oc, data[:40].hex())
    if mode == 1:
        decoder = zlib.decompressobj(15 if wrapped else -15)
        decoded = decoder.decompress(payload) + decoder.flush()
        assert decoder.eof and not decoder.unused_data and decoded == expected, (i, "zlib could not decode product output")
        assert (payload[2 if wrapped else 0] >> 1) & 3 == 2, (i, "encoder did not emit a dynamic Huffman block")
        counts["encode_interop"] += 1
        if data in corpus_names:
            name = corpus_names[data]
            label = f"{name}-{'fast' if level == 0 else 'normal'}-{'raw' if wrapped == 0 else 'zlib'}.bin"
            wire_fixtures[label] = payload
            if not wrapped:
                corpus_sizes.setdefault(name, {"source": len(data)})["fast" if level == 0 else "normal"] = len(payload)
    else:
        assert payload == expected, (i, "decoded data differs")
        counts["decode_interop"] += 1
assert cursor == len(result.stdout)
fixture_root = root / "crates/compress/tests/encoder-wire"
if "--update-wire-fixtures" in sys.argv[1:]:
    fixture_root.mkdir(parents=True, exist_ok=True)
    for name, payload in wire_fixtures.items():
        (fixture_root / name).write_bytes(payload)
else:
    for name, payload in wire_fixtures.items():
        assert (fixture_root / name).read_bytes() == payload, (name, "wire fixture differs; validate before explicit regeneration")
report = {"oracle": "Python stdlib zlib", "zlib_version": zlib.ZLIB_VERSION,
          "fixtures": len(fixtures), "cases": len(tasks), "checks": counts,
          "corpus_sizes": corpus_sizes,
          "known_wire_sha256": {name: hashlib.sha256(data).hexdigest() for name, data in wire_fixtures.items()},
          "result": "passed", "reproducer": str(work)}
(root / "_bmad-output/implementation-artifacts/compression-oracle.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
print(json.dumps(report))
