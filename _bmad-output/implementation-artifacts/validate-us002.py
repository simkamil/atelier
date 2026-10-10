# SPDX-License-Identifier: AGPL-3.0-or-later
"""Independent local verification of the CI story; agent tooling only."""
from pathlib import Path
import hashlib
import json
import os
import re
import subprocess

root = Path(__file__).resolve().parents[2]
artifact = root / '_bmad-output/implementation-artifacts'
log = artifact / 'us002-validation.log'
evidence = {'date': '2026-10-10', 'host': 'Windows x86_64', 'checks': [],
            'limitations': ['GitHub Actions not executed: no remote repository configured.',
                            'Linux musl tests compiled only; no Linux runtime on this host.']}
before = hashlib.sha256((root / 'Cargo.lock').read_bytes()).hexdigest()
sysroot = subprocess.check_output(['rustc', '--print', 'sysroot'], text=True).strip()
linker = str(Path(sysroot) / 'lib/rustlib/x86_64-pc-windows-msvc/bin/rust-lld.exe')
commands = [
    ('bootstrap', ['rustc', '--edition=2024', 'tools/check-workspace/src/main.rs', '-o', 'target/check-workspace.exe']),
    ('guard', [str(root / 'target/check-workspace.exe'), '.']),
    ('windows_build', ['cargo', 'build', '--workspace', '--target', 'x86_64-pc-windows-msvc', '--locked', '--offline']),
    ('windows_tests', ['cargo', 'test', '--workspace', '--target', 'x86_64-pc-windows-msvc', '--locked', '--offline']),
    ('wasm_build', ['cargo', 'build', '-p', 'atelier-core-wasm', '--target', 'wasm32-unknown-unknown', '--locked', '--offline']),
    ('wasm_native_tests', ['cargo', 'test', '-p', 'atelier-core-wasm', '--locked', '--offline']),
    ('clippy_workspace', ['cargo', 'clippy', '--workspace', '--all-targets', '--locked', '--offline', '--', '-D', 'warnings']),
    ('clippy_wasm', ['cargo', 'clippy', '-p', 'atelier-core-wasm', '--all-targets', '--target', 'wasm32-unknown-unknown', '--locked', '--offline', '--', '-D', 'warnings']),
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
        if name == 'windows_tests':
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
(artifact / 'us002-validation.json').write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding='utf-8')
raise SystemExit(0 if all(c['exit_code'] == 0 for c in evidence['checks']) and len(evidence['checks']) == len(commands) else 1)
