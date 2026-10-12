# SPDX-License-Identifier: AGPL-3.0-or-later
"""Verify DOCX import and rich layout on an immutable source inventory."""
from pathlib import Path
import hashlib
import json
import os
import platform
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
REPORT = ART / 'docx-rich-layout-validation.json'
LOG = ART / 'docx-rich-layout-validation.log'


def inventory():
    paths = {ROOT / name for name in ('Cargo.lock', 'Cargo.toml', '.github/workflows/ci.yml',
                                      'tools/package-engine.py', 'tools/word-line-reference.ps1')}
    paths.add(Path(__file__).resolve())
    paths.add(Path(__file__).with_name('check-word-availability.py'))
    for folder in ('crates', 'tools', 'data/engine', 'tests/text-layout-reference', 'tests/docx-reference'):
        for path in (ROOT / folder).rglob('*'):
            if path.is_file() and 'target' not in path.relative_to(ROOT).parts:
                paths.add(path)
    return {p.relative_to(ROOT).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(paths) if p.is_file()}


wasm_packages = ['atelier-core-wasm', 'atelier-text-model', 'atelier-unicode', 'atelier-geom',
                 'atelier-model', 'atelier-compress', 'atelier-render', 'atelier-zip',
                 'atelier-xml', 'atelier-font', 'atelier-image', 'atelier-shape',
                 'atelier-raster', 'atelier-fmt-ooxml', 'atelier-text-layout']
package_args = [item for name in wasm_packages for item in ('-p', name)]
locked = ['--locked', '--offline']
commands = [
    ('bootstrap', ['rustc', '--edition=2024', 'tools/check-workspace/src/main.rs', '-o', 'target/check-workspace.exe']),
    ('guard', [str(ROOT/'target/check-workspace.exe'), '.']),
    ('docx_reference_reproducible', ['py', 'tools/generate-docx-reference.py', '--check']),
    ('windows_build', ['cargo', 'build', '--workspace', '--target', 'x86_64-pc-windows-msvc', *locked]),
    ('windows_tests', ['cargo', 'test', '--workspace', '--target', 'x86_64-pc-windows-msvc', *locked]),
    ('wasm_build', ['cargo', 'build', *package_args, '--target', 'wasm32-unknown-unknown', *locked]),
    ('clippy_workspace', ['cargo', 'clippy', '--workspace', '--all-targets', *locked, '--', '-D', 'warnings']),
    ('clippy_wasm', ['cargo', 'clippy', *package_args, '--all-targets', '--target', 'wasm32-unknown-unknown', *locked, '--', '-D', 'warnings']),
    ('format', ['cargo', 'fmt', '--all', '--check']),
    ('license', ['cargo', 'run', *locked, '-p', 'atelier-check-license', '--', '.']),
    ('metadata', ['cargo', 'metadata', '--format-version', '1', *locked]),
    ('packaged_engine', ['node', 'tests/check-embedded-engine.mjs', '--compare-artifact']),
    ('word_com_registration', ['py', '_bmad-output/implementation-artifacts/check-word-availability.py']),
    ('musl_build', ['cargo', 'build', '--workspace', '--exclude', 'atelier-desktop-win', '--exclude', 'atelier-core-wasm', '--target', 'x86_64-unknown-linux-musl', *locked]),
    ('musl_tests_compile_only', ['cargo', 'test', '--workspace', '--exclude', 'atelier-desktop-win', '--exclude', 'atelier-core-wasm', '--target', 'x86_64-unknown-linux-musl', *locked, '--no-run']),
]
before = inventory()
report = {'date': '2026-10-12', 'host': platform.platform(), 'checks': [],
          'source_sha256': before,
          'limitations': ['No Microsoft Word here: US-210 95% line-break criterion remains open.',
                          'Local Linux musl tests compile only; execution belongs to remote Linux CI.',
                          'These library APIs are not yet connected to the rich editor UI.']}
sysroot = subprocess.check_output(['rustc', '--print', 'sysroot'], text=True).strip()
linker = Path(sysroot)/'lib/rustlib/x86_64-pc-windows-msvc/bin/rust-lld.exe'
with LOG.open('w', encoding='utf-8', newline='\n') as stream:
    for name, argv in commands:
        env = dict(os.environ)
        if name.startswith('musl_'):
            env['CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER'] = str(linker)
        result = subprocess.run(argv, cwd=ROOT, env=env, capture_output=True,
                                text=True, encoding='utf-8', errors='replace')
        output = result.stdout + result.stderr
        stream.write(f'{name}: {argv!r}\n{output}\n')
        stream.flush()
        entry = {'name': name, 'argv': argv, 'exit_code': result.returncode}
        if name == 'windows_tests':
            entry['passed'] = sum(int(n) for n in re.findall(r'test result: ok\. (\d+) passed', output))
            entry['tests'] = re.findall(r'^test (.+) \.\.\. ok$', output, re.M)
        if name == 'metadata' and result.returncode == 0:
            metadata = json.loads(result.stdout)
            members = set(metadata['workspace_members'])
            entry['package_count'] = len(metadata['packages'])
            entry['all_members_local'] = all(p['id'] in members and p['source'] is None
                                             for p in metadata['packages'])
            entry['no_external_dependencies'] = all(d['source'] is None for p in metadata['packages']
                                                    for d in p['dependencies'])
            if not (entry['all_members_local'] and entry['no_external_dependencies']):
                entry['exit_code'] = 1
        report['checks'].append(entry)
        print(json.dumps({k: v for k, v in entry.items() if k != 'tests'}), flush=True)
        if entry['exit_code']:
            report['failed_output'] = output[-8000:]
            break
report['source_bytes_unchanged'] = before == inventory()
report['lock_bytes_unchanged'] = before['Cargo.lock'] == inventory()['Cargo.lock']
report['passed'] = (len(report['checks']) == len(commands)
                    and all(c['exit_code'] == 0 for c in report['checks'])
                    and report['source_bytes_unchanged'] and report['lock_bytes_unchanged'])
REPORT.write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n', encoding='utf-8', newline='\n')
raise SystemExit(0 if report['passed'] else 1)
