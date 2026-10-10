# SPDX-License-Identifier: AGPL-3.0-or-later
"""Independent local verification of PNG, GIF/BMP and embedded fonts; agent tooling only."""
from pathlib import Path
import hashlib
import json
import os
import re
import subprocess

root = Path(__file__).resolve().parents[2]
artifact = root / '_bmad-output/implementation-artifacts'
log = artifact / 'images-fonts-validation.log'
evidence = {'date': '2026-10-10', 'host': 'Windows x86_64', 'checks': [],
            'limitations': ['Validation locale : consulter github-ci.json pour le dernier commit publie.',
                            'Linux musl tests compiled only; no Linux runtime on this host.']}
before = hashlib.sha256((root / 'Cargo.lock').read_bytes()).hexdigest()
source_paths = sorted({root / 'Cargo.lock', root / '.github/workflows/ci.yml',
                       root / 'tests/check-assets.ps1',
                       *(root / 'crates/image').rglob('*.rs'), *(root / 'crates/font').rglob('*.rs'),
                       *(root / f'crates/{name}/Cargo.toml' for name in ('image','font')),
                       *(p for folder in ('data/fonts','tests/image-reference') for p in (root/folder).rglob('*') if p.is_file())})
source_hashes = {p.relative_to(root).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
                 for p in source_paths}
evidence['source_sha256'] = source_hashes
sysroot = subprocess.check_output(['rustc', '--print', 'sysroot'], text=True).strip()
linker = str(Path(sysroot) / 'lib/rustlib/x86_64-pc-windows-msvc/bin/rust-lld.exe')
commands = [
    ('bootstrap', ['rustc', '--edition=2024', 'tools/check-workspace/src/main.rs', '-o', 'target/check-workspace.exe']),
    ('guard', [str(root / 'target/check-workspace.exe'), '.']),
    ('assets', ['pwsh', '-NoProfile', '-File', 'tests/check-assets.ps1']),
    ('windows_build', ['cargo', 'build', '--workspace', '--target', 'x86_64-pc-windows-msvc', '--locked', '--offline']),
    ('windows_tests', ['cargo', 'test', '--workspace', '--target', 'x86_64-pc-windows-msvc', '--locked', '--offline']),
    ('wasm_build', ['cargo', 'build', '-p', 'atelier-core-wasm', '-p', 'atelier-unicode', '-p', 'atelier-geom', '-p', 'atelier-model', '-p', 'atelier-compress', '-p', 'atelier-render', '-p', 'atelier-zip', '-p', 'atelier-xml', '-p', 'atelier-font', '-p', 'atelier-image', '--target', 'wasm32-unknown-unknown', '--locked', '--offline']),
    ('wasm_native_tests', ['cargo', 'test', '-p', 'atelier-core-wasm', '-p', 'atelier-unicode', '-p', 'atelier-geom', '-p', 'atelier-model', '-p', 'atelier-compress', '-p', 'atelier-render', '-p', 'atelier-zip', '-p', 'atelier-xml', '-p', 'atelier-font', '-p', 'atelier-image', '--locked', '--offline']),
    ('image_font_release_tests', ['cargo', 'test', '-p', 'atelier-image', '-p', 'atelier-font', '--release', '--locked', '--offline']),
    ('clippy_workspace', ['cargo', 'clippy', '--workspace', '--all-targets', '--locked', '--offline', '--', '-D', 'warnings']),
    ('clippy_wasm', ['cargo', 'clippy', '-p', 'atelier-core-wasm', '-p', 'atelier-unicode', '-p', 'atelier-geom', '-p', 'atelier-model', '-p', 'atelier-compress', '-p', 'atelier-render', '-p', 'atelier-zip', '-p', 'atelier-xml', '-p', 'atelier-font', '-p', 'atelier-image', '--all-targets', '--target', 'wasm32-unknown-unknown', '--locked', '--offline', '--', '-D', 'warnings']),
    ('format', ['cargo', 'fmt', '--all', '--check']),
    ('license', ['cargo', 'run', '--locked', '--offline', '-p', 'atelier-check-license', '--', '.']),
    ('metadata', ['cargo', 'metadata', '--no-deps', '--format-version', '1', '--locked', '--offline']),
    ('musl_build', ['cargo', 'build', '--workspace', '--exclude', 'atelier-desktop-win', '--exclude', 'atelier-core-wasm', '--target', 'x86_64-unknown-linux-musl', '--locked', '--offline']),
    ('musl_tests_compile_only', ['cargo', 'test', '--workspace', '--exclude', 'atelier-desktop-win', '--exclude', 'atelier-core-wasm', '--target', 'x86_64-unknown-linux-musl', '--locked', '--offline', '--no-run']),
]
with log.open('w', encoding='utf-8') as stream:
    for name, argv in commands:
        env = dict(os.environ)
        if name.startswith('musl_'):
            env['CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER'] = linker
        result = subprocess.run(argv, cwd=root, env=env, capture_output=True, text=True,
                                encoding='utf-8', errors='replace')
        output = result.stdout + result.stderr
        stream.write(f'{name}: {argv!r}\n{output}\n')
        entry = {'name': name, 'argv': argv, 'exit_code': result.returncode}
        if name in ('windows_tests','image_font_release_tests'):
            entry['passed'] = sum(int(n) for n in re.findall(r'test result: ok\. (\d+) passed', output))
            entry['tests'] = re.findall(r'^test (.+) \.\.\. ok$', output, re.M)
        if name == 'metadata' and result.returncode == 0:
            metadata = json.loads(result.stdout)
            members = set(metadata['workspace_members'])
            entry['package_count'] = len(metadata['packages'])
            entry['all_members_local'] = all(p['id'] in members and p['source'] is None for p in metadata['packages'])
            entry['no_external_dependencies'] = all(d['source'] is None for p in metadata['packages'] for d in p['dependencies'])
            assert entry['all_members_local'] and entry['no_external_dependencies']
        evidence['checks'].append(entry)
        print(json.dumps(entry, ensure_ascii=False), flush=True)
        if result.returncode:
            evidence['failed_output'] = output[-6000:]
            break
evidence['lock_bytes_unchanged'] = hashlib.sha256((root / 'Cargo.lock').read_bytes()).hexdigest() == before
evidence['source_bytes_unchanged'] = all(hashlib.sha256((root / p).read_bytes()).hexdigest() == digest
                                         for p, digest in source_hashes.items())
(artifact / 'images-fonts-validation.json').write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding='utf-8', newline='\n')
raise SystemExit(0 if all(c['exit_code'] == 0 for c in evidence['checks'])
                 and len(evidence['checks']) == len(commands)
                 and evidence['lock_bytes_unchanged'] and evidence['source_bytes_unchanged'] else 1)
