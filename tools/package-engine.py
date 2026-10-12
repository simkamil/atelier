# SPDX-License-Identifier: AGPL-3.0-or-later
"""Refresh the checked-in desktop WASM asset from a real optimized Cargo build."""
import hashlib
import json
from pathlib import Path
import re
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
metadata = json.loads(subprocess.check_output([
    'cargo', 'metadata', '--format-version', '1', '--locked', '--offline',
], cwd=root, text=True, encoding='utf-8'))
packages = {p['id']: p for p in metadata['packages']}
nodes = {n['id']: n for n in metadata['resolve']['nodes']}
pending = [next(p['id'] for p in packages.values() if p['name'] == 'atelier-core-wasm')]
closure = set()
while pending:
    package_id = pending.pop()
    if package_id in closure:
        continue
    package = packages[package_id]
    if package['source'] is not None:
        raise SystemExit('The packaged engine must have only local dependencies')
    closure.add(package_id)
    pending.extend(d['pkg'] for d in nodes[package_id]['deps']
                   if any(k['kind'] in (None, 'build') for k in d['dep_kinds']))
sources = {root / 'Cargo.toml', root / 'Cargo.lock', Path(__file__).resolve()}
for package_id in closure:
    manifest_path = Path(packages[package_id]['manifest_path']).resolve()
    sources.add(manifest_path)
    sources.update((manifest_path.parent / 'src').rglob('*.rs'))
    build = manifest_path.parent / 'build.rs'
    if build.is_file():
        sources.add(build)
resources = set()
for path in list(sources):
    if path.suffix != '.rs':
        continue
    content = path.read_text(encoding='utf-8')
    for match in re.finditer(r'include_(bytes|str)!\s*\(\s*"([^"]+)"\s*\)', content):
        included = (path.parent / match[2]).resolve()
        included.relative_to(root)
        if not included.is_file():
            raise SystemExit(f'Missing embedded engine input: {included.relative_to(root)}')
        (sources if match[1] == 'str' else resources).add(included)
# unicode/build.rs regenerates and checks src/generated.rs from this pinned
# corpus. Include every corpus file conservatively, including its provenance.
if any(packages[p]['name'] == 'atelier-unicode' for p in closure):
    for path in (root / 'data/ucd/17.0.0').rglob('*'):
        if path.is_file():
            (resources if path.suffix.lower() == '.pdf' else sources).add(path)
sources = sorted(sources)
resources = sorted(resources - set(sources))
manifest = {
    'schema_version': 2,
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
    'resources_sha256': {
        path.relative_to(root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in resources
    },
    'local_packages': sorted(packages[p]['name'] for p in closure),
    'inventory_policy': 'Conservative local normal/build dependency closure, Rust sources, static include inputs and pinned Unicode corpus; includes unused cfg/test resources.',
    'scope': 'Real ABI/protocol, plain-text editor and bounded read-only DOCX preview; legacy transport probes still echo.',
}
destination = root / 'data/engine'
destination.mkdir(parents=True, exist_ok=True)
(destination / manifest['artifact']).write_bytes(data)
(destination / 'manifest.json').write_text(
    json.dumps(manifest, indent=2) + '\n', encoding='utf-8', newline='\n',
)
print(json.dumps({'bytes': len(data), 'sha256': manifest['sha256'], 'rustc': manifest['rustc']}))
