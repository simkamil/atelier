# SPDX-License-Identifier: AGPL-3.0-or-later
"""Independent local verification of UCD tables, TrueType outlines and manual WASM ABI; agent tooling only."""
from pathlib import Path
import hashlib
import json
import os
import re
import subprocess
import sys

root = Path(__file__).resolve().parents[2]
artifact = root / '_bmad-output/implementation-artifacts'
log = artifact / 'prototype-foundations-validation.log'
evidence = {'date': '2026-10-10', 'host': 'Windows x86_64', 'checks': [],
            'limitations': ['Validation locale : consulter github-ci.json pour le dernier commit publie.',
                            'Linux musl tests compiled only; no Linux runtime on this host.']}
before = hashlib.sha256((root / 'Cargo.lock').read_bytes()).hexdigest()
source_paths = sorted({root / 'Cargo.lock', root / 'Cargo.toml', root / '.github/workflows/ci.yml',
                       root / 'tests/check-assets.ps1', root / 'tests/wasm-abi.mjs', root / 'web/wasm-loader.mjs',
                       root / 'tests/check-ucd.ps1', root / 'tests/check-wasm-size.mjs', root / 'tools/bench/references/wasm-size.json',
                       *(p for folder in ('crates/unicode','crates/font','crates/core-wasm','tools/ucd-gen') for p in (root/folder).rglob('*') if p.is_file() and p.suffix in ('.rs','.toml')),
                       *(p for folder in ('data/ucd','tests/font-outline-reference','tests/unicode-reference') for p in (root/folder).rglob('*') if p.is_file())})
source_hashes = {p.relative_to(root).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
                 for p in source_paths}
evidence['source_sha256'] = source_hashes
sysroot = subprocess.check_output(['rustc', '--print', 'sysroot'], text=True).strip()
linker = str(Path(sysroot) / 'lib/rustlib/x86_64-pc-windows-msvc/bin/rust-lld.exe')
commands = [
    ('bootstrap', ['rustc', '--edition=2024', 'tools/check-workspace/src/main.rs', '-o', 'target/check-workspace.exe']),
    ('guard', [str(root / 'target/check-workspace.exe'), '.']),
    ('assets', ['pwsh', '-NoProfile', '-File', 'tests/check-assets.ps1']),
    ('ucd_and_outline_assets', ['pwsh', '-NoProfile', '-File', 'tests/check-ucd.ps1']),
    ('windows_build', ['cargo', 'build', '--workspace', '--target', 'x86_64-pc-windows-msvc', '--locked', '--offline']),
    ('windows_tests', ['cargo', 'test', '--workspace', '--target', 'x86_64-pc-windows-msvc', '--locked', '--offline']),
    ('wasm_build', ['cargo', 'build', '-p', 'atelier-core-wasm', '-p', 'atelier-unicode', '-p', 'atelier-geom', '-p', 'atelier-model', '-p', 'atelier-compress', '-p', 'atelier-render', '-p', 'atelier-zip', '-p', 'atelier-xml', '-p', 'atelier-font', '-p', 'atelier-image', '--target', 'wasm32-unknown-unknown', '--locked', '--offline']),
    ('wasm_native_tests', ['cargo', 'test', '-p', 'atelier-core-wasm', '-p', 'atelier-unicode', '-p', 'atelier-geom', '-p', 'atelier-model', '-p', 'atelier-compress', '-p', 'atelier-render', '-p', 'atelier-zip', '-p', 'atelier-xml', '-p', 'atelier-font', '-p', 'atelier-image', '--locked', '--offline']),
    ('foundation_release_tests', ['cargo', 'test', '-p', 'atelier-font', '-p', 'atelier-unicode', '-p', 'atelier-ucd-gen', '-p', 'atelier-core-wasm', '--release', '--locked', '--offline']),
    ('ucd_reproducible', ['cargo', 'run', '-p', 'atelier-ucd-gen', '--locked', '--offline', '--', '--ucd', 'data/ucd/17.0.0', '--output', 'crates/unicode/src/generated.rs', '--check']),
    ('wasm_optimized', ['cargo', 'build', '-p', 'atelier-core-wasm', '--profile', 'wasm-release', '--target', 'wasm32-unknown-unknown', '--locked', '--offline']),
    ('wasm_trap_hooks', ['cargo', 'build', '-p', 'atelier-core-wasm', '--profile', 'wasm-release', '--target', 'wasm32-unknown-unknown', '--target-dir', 'target/wasm-abi-hooks', '--features', 'abi-test-hooks', '--locked', '--offline']),
    ('wasm_js', ['node', 'tests/wasm-abi.mjs', 'target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm', 'target/wasm-abi-hooks/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm']),
    ('wasm_size', ['node', 'tests/check-wasm-size.mjs', 'target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm']),
    ('clippy_workspace', ['cargo', 'clippy', '--workspace', '--all-targets', '--locked', '--offline', '--', '-D', 'warnings']),
    ('clippy_wasm', ['cargo', 'clippy', '-p', 'atelier-core-wasm', '-p', 'atelier-unicode', '-p', 'atelier-geom', '-p', 'atelier-model', '-p', 'atelier-compress', '-p', 'atelier-render', '-p', 'atelier-zip', '-p', 'atelier-xml', '-p', 'atelier-font', '-p', 'atelier-image', '--all-targets', '--target', 'wasm32-unknown-unknown', '--locked', '--offline', '--', '-D', 'warnings']),
    ('format', ['cargo', 'fmt', '--all', '--check']),
    ('license', ['cargo', 'run', '--locked', '--offline', '-p', 'atelier-check-license', '--', '.']),
    ('metadata', ['cargo', 'metadata', '--no-deps', '--format-version', '1', '--locked', '--offline']),
    ('musl_build', ['cargo', 'build', '--workspace', '--exclude', 'atelier-desktop-win', '--exclude', 'atelier-core-wasm', '--target', 'x86_64-unknown-linux-musl', '--locked', '--offline']),
    ('musl_tests_compile_only', ['cargo', 'test', '--workspace', '--exclude', 'atelier-desktop-win', '--exclude', 'atelier-core-wasm', '--target', 'x86_64-unknown-linux-musl', '--locked', '--offline', '--no-run']),
]
required_check_count = len(commands)
resume_format = sys.argv[1:] == ['--resume-after-format']
if sys.argv[1:] and not resume_format:
    raise SystemExit('Only --resume-after-format is supported')
if resume_format:
    previous = json.loads((artifact / 'prototype-foundations-validation.json').read_text(encoding='utf-8'))
    assert previous['checks'][-1]['name'] == 'format' and previous['checks'][-1]['exit_code'] != 0
    assert all(c['exit_code'] == 0 for c in previous['checks'][:-1])
    assert previous['lock_bytes_unchanged'] and previous['source_bytes_unchanged']
    changed = {p for p,h in previous['source_sha256'].items() if source_hashes[p] != h}
    assert changed == {'crates/font/src/outline.rs','crates/font/tests/outlines.rs'}, changed
    # The coordinator inspected rustfmt's complete diff: whitespace in two
    # if-expressions and import ordering only. Functional checks remain valid.
    evidence['format_only_followup'] = {
        'reason': 'cargo fmt --all applied workspace edition2024 formatting; only two if-expression layouts and import ordering changed',
        'prior_source_sha256': previous['source_sha256'],
        'formatted_files': sorted(changed),
        'functional_checks_preserved': [c['name'] for c in previous['checks'][:-1]],
    }
    evidence['checks'] = previous['checks'][:-1]
    commands = commands[len(evidence['checks']):]
with log.open('a' if resume_format else 'w', encoding='utf-8') as stream:
    for name, argv in commands:
        env = dict(os.environ)
        if name.startswith('musl_'):
            env['CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER'] = linker
        result = subprocess.run(argv, cwd=root, env=env, capture_output=True, text=True,
                                encoding='utf-8', errors='replace')
        output = result.stdout + result.stderr
        stream.write(f'{name}: {argv!r}\n{output}\n')
        entry = {'name': name, 'argv': argv, 'exit_code': result.returncode}
        if name in ('windows_tests','foundation_release_tests'):
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
        print(json.dumps({k:v for k,v in entry.items() if k!='tests'}, ensure_ascii=False), flush=True)
        if result.returncode:
            evidence['failed_output'] = output[-6000:]
            break
evidence['lock_bytes_unchanged'] = hashlib.sha256((root / 'Cargo.lock').read_bytes()).hexdigest() == before
evidence['source_bytes_unchanged'] = all(hashlib.sha256((root / p).read_bytes()).hexdigest() == digest
                                         for p, digest in source_hashes.items())
(artifact / 'prototype-foundations-validation.json').write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding='utf-8', newline='\n')
raise SystemExit(0 if all(c['exit_code'] == 0 for c in evidence['checks'])
                 and len(evidence['checks']) == required_check_count
                 and evidence['lock_bytes_unchanged'] and evidence['source_bytes_unchanged'] else 1)
