# SPDX-License-Identifier: AGPL-3.0-or-later
"""Refresh the checked-in desktop WASM asset from a real optimized Cargo build."""
import hashlib
import json
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[1]
subprocess.run([
    'cargo', 'build', '-p', 'atelier-core-wasm', '--profile', 'wasm-release',
    '--target', 'wasm32-unknown-unknown', '--locked', '--offline',
], cwd=root, check=True)
artifact = root / 'target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm'
data = artifact.read_bytes()
if data[:8] != b'\0asm\x01\0\0\0':
    raise SystemExit('Optimized engine is not a WebAssembly 1 module')
sources = sorted([
    root / 'Cargo.toml', root / 'crates/core-wasm/Cargo.toml',
    *(root / 'crates/core-wasm/src').rglob('*.rs'),
])
manifest = {
    'schema_version': 1,
    'artifact': 'atelier_core_wasm.wasm',
    'target': 'wasm32-unknown-unknown',
    'profile': 'wasm-release',
    'features': [],
    'bytes': len(data),
    'sha256': hashlib.sha256(data).hexdigest(),
    'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
    'source_hash_encoding': 'UTF-8 bytes with CRLF normalized to LF',
    'sources_sha256': {
        path.relative_to(root).as_posix(): hashlib.sha256(path.read_bytes().replace(b'\r\n', b'\n')).hexdigest()
        for path in sources
    },
    'scope': 'Real current ABI/protocol engine; dispatch still echoes, document handlers pending.',
}
destination = root / 'data/engine'
destination.mkdir(parents=True, exist_ok=True)
(destination / manifest['artifact']).write_bytes(data)
(destination / 'manifest.json').write_text(
    json.dumps(manifest, indent=2) + '\n', encoding='utf-8', newline='\n',
)
print(json.dumps({'bytes': len(data), 'sha256': manifest['sha256'], 'rustc': manifest['rustc']}))
