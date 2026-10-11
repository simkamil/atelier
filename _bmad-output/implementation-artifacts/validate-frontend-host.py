# SPDX-License-Identifier: AGPL-3.0-or-later
"""Independent local verification of Canvas, input and WebView2; agent tooling only."""
from pathlib import Path
import hashlib
import json
import os
import platform
import re
import subprocess
import sys

root = Path(__file__).resolve().parents[2]
artifact = root / '_bmad-output/implementation-artifacts'
log = artifact / 'frontend-host-validation.log'
evidence = {'date': '2026-10-11', 'host': platform.platform(), 'checks': [],
            'limitations': ['Validation locale : consulter github-ci.json pour le dernier commit publie.',
                            'Linux musl tests compiled only; no Linux runtime on this host.',
                            'US-111 strict handle acceptance remains open; inspect handle_acceptance_passed in native check reports.',
                            'Real WebView2 tested locally; physical multi-monitor and native AZERTY/IME remain manual acceptance.',
                            'Canvas benchmark in headless software Edge is not a physical 60 fps guarantee.']}
before = hashlib.sha256((root / 'Cargo.lock').read_bytes()).hexdigest()
source_paths = sorted({root / 'Cargo.lock', root / 'Cargo.toml', root / '.github/workflows/ci.yml',
                       Path(__file__).resolve(), *(p for p in (root/'tests').glob('*.html')),
                       *(p for p in (root/'data/webview2').rglob('*') if p.is_file()),
                       *(p for folder in ('web','tests') for p in (root/folder).glob('*.mjs')), root / 'tests/wasm-protocol.mjs', root / 'tests/wasm-protocol-boundaries.mjs', root / 'web/wasm-protocol.mjs', root / '.gitattributes', root / 'tests/check-assets.ps1', root / 'tests/wasm-abi.mjs', root / 'web/wasm-loader.mjs',
                       root / 'tests/check-ucd.ps1', root / 'tests/check-webview-assets.ps1', root / 'tests/check-webview-host.ps1', root / 'tests/check-wasm-size.mjs', root / 'tools/bench/references/wasm-size.json',
                       *(p for folder in ('crates','tools') for p in (root/folder).rglob('*') if p.is_file() and p.suffix in ('.rs','.toml')),
                       *(p for folder in ('data/ucd','tests/font-outline-reference','tests/unicode-reference','tests/font-layout-reference') for p in (root/folder).rglob('*') if p.is_file())})
source_hashes = {p.relative_to(root).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
                 for p in source_paths}
evidence['source_sha256'] = source_hashes
sysroot = subprocess.check_output(['rustc', '--print', 'sysroot'], text=True).strip()
linker = str(Path(sysroot) / 'lib/rustlib/x86_64-pc-windows-msvc/bin/rust-lld.exe')
commands = [
    ('bootstrap', ['rustc', '--edition=2024', 'tools/check-workspace/src/main.rs', '-o', 'target/check-workspace.exe']),
    ('guard', [str(root / 'target/check-workspace.exe'), '.']),
    ('webview_assets', ['pwsh','-NoProfile','-File','tests/check-webview-assets.ps1']),
    ('assets', ['pwsh', '-NoProfile', '-File', 'tests/check-assets.ps1']),
    ('ucd_and_outline_assets', ['pwsh', '-NoProfile', '-File', 'tests/check-ucd.ps1']),
    ('windows_build', ['cargo', 'build', '--workspace', '--target', 'x86_64-pc-windows-msvc', '--locked', '--offline']),
    ('windows_tests', ['cargo', 'test', '--workspace', '--target', 'x86_64-pc-windows-msvc', '--locked', '--offline']),
    ('wasm_build', ['cargo', 'build', '-p', 'atelier-core-wasm', '-p', 'atelier-unicode', '-p', 'atelier-geom', '-p', 'atelier-model', '-p', 'atelier-compress', '-p', 'atelier-render', '-p', 'atelier-zip', '-p', 'atelier-xml', '-p', 'atelier-font', '-p', 'atelier-image', '-p', 'atelier-shape', '--target', 'wasm32-unknown-unknown', '--locked', '--offline']),
    ('wasm_native_tests', ['cargo', 'test', '-p', 'atelier-core-wasm', '-p', 'atelier-unicode', '-p', 'atelier-geom', '-p', 'atelier-model', '-p', 'atelier-compress', '-p', 'atelier-render', '-p', 'atelier-zip', '-p', 'atelier-xml', '-p', 'atelier-font', '-p', 'atelier-image', '-p', 'atelier-shape', '--locked', '--offline']),
    ('shell_release_tests', ['cargo', 'test', '-p', 'atelier-desktop-win', '--release', '--locked', '--offline']),
    ('ucd_reproducible', ['cargo', 'run', '-p', 'atelier-ucd-gen', '--locked', '--offline', '--', '--ucd', 'data/ucd/17.0.0', '--output', 'crates/unicode/src/generated.rs', '--check']),
    ('wasm_optimized', ['cargo', 'build', '-p', 'atelier-core-wasm', '--profile', 'wasm-release', '--target', 'wasm32-unknown-unknown', '--locked', '--offline']),
    ('wasm_trap_hooks', ['cargo', 'build', '-p', 'atelier-core-wasm', '--profile', 'wasm-release', '--target', 'wasm32-unknown-unknown', '--target-dir', 'target/wasm-abi-hooks', '--features', 'abi-test-hooks', '--locked', '--offline']),
    ('wasm_js', ['node', 'tests/wasm-abi.mjs', 'target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm', 'target/wasm-abi-hooks/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm']),
    ('protocol_oracle', ['cargo','build','-p','atelier-core-wasm','--example','protocol_oracle','--locked','--offline']),
    ('protocol_cross', ['node','tests/wasm-protocol.mjs','target/debug/examples/protocol_oracle.exe','target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm']),
    ('protocol_capacity', ['node','tests/wasm-protocol-boundaries.mjs']),
    ('worker_real_transfer', ['node','tests/wasm-worker.mjs','target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm']),
    ('worker_browser', ['node','tests/run-worker-browser.mjs']),
    ('native_hidden_window', [str(root/'target/x86_64-pc-windows-msvc/debug/atelier.exe'),'--self-check',str(root/'target/shell-check/window-state.txt')]),
    ('render_oracle', ['cargo','build','-p','atelier-render','--example','display_list_oracle','--locked','--offline']),
    ('display_list_js', ['node','tests/display-list.mjs']),
    ('frontend_browser', ['node','tests/run-frontend-browser.mjs']),
    ('native_webview_debug', ['pwsh','-NoProfile','-File','tests/check-webview-host.ps1','-Executable',str(root/'target/x86_64-pc-windows-msvc/debug/atelier.exe'),'-TargetDirectory',str(root/'target/webview-check'),'-AllowKnownRuntimeHandleGrowth']),
    ('native_release_build', ['cargo','build','-p','atelier-desktop-win','--release','--locked','--offline']),
    ('native_webview_release', ['pwsh','-NoProfile','-File','tests/check-webview-host.ps1','-Executable',str(root/'target/release/atelier.exe'),'-TargetDirectory',str(root/'target/webview-release-check'),'-AllowKnownRuntimeHandleGrowth']),
    ('wasm_size', ['node', 'tests/check-wasm-size.mjs', 'target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm']),
    ('clippy_workspace', ['cargo', 'clippy', '--workspace', '--all-targets', '--locked', '--offline', '--', '-D', 'warnings']),
    ('clippy_wasm', ['cargo', 'clippy', '-p', 'atelier-core-wasm', '-p', 'atelier-unicode', '-p', 'atelier-geom', '-p', 'atelier-model', '-p', 'atelier-compress', '-p', 'atelier-render', '-p', 'atelier-zip', '-p', 'atelier-xml', '-p', 'atelier-font', '-p', 'atelier-image', '-p', 'atelier-shape', '--all-targets', '--target', 'wasm32-unknown-unknown', '--locked', '--offline', '--', '-D', 'warnings']),
    ('format', ['cargo', 'fmt', '--all', '--check']),
    ('license', ['cargo', 'run', '--locked', '--offline', '-p', 'atelier-check-license', '--', '.']),
    ('metadata', ['cargo', 'metadata', '--no-deps', '--format-version', '1', '--locked', '--offline']),
    ('musl_build', ['cargo', 'build', '--workspace', '--exclude', 'atelier-desktop-win', '--exclude', 'atelier-core-wasm', '--target', 'x86_64-unknown-linux-musl', '--locked', '--offline']),
    ('musl_tests_compile_only', ['cargo', 'test', '--workspace', '--exclude', 'atelier-desktop-win', '--exclude', 'atelier-core-wasm', '--target', 'x86_64-unknown-linux-musl', '--locked', '--offline', '--no-run']),
]
required_check_count = len(commands)
if sys.argv[1:]:
    raise SystemExit('No arguments supported')
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
        if name in ('windows_tests','shell_release_tests'):
            entry['passed'] = sum(int(n) for n in re.findall(r'test result: ok\. (\d+) passed', output))
            entry['tests'] = re.findall(r'^test (.+) \.\.\. ok$', output, re.M)
        if name == 'metadata' and result.returncode == 0:
            metadata = json.loads(result.stdout)
            members = set(metadata['workspace_members'])
            entry['package_count'] = len(metadata['packages'])
            entry['all_members_local'] = all(p['id'] in members and p['source'] is None for p in metadata['packages'])
            entry['no_external_dependencies'] = all(d['source'] is None for p in metadata['packages'] for d in p['dependencies'])
            assert entry['all_members_local'] and entry['no_external_dependencies']
        if name == 'worker_browser' and result.returncode == 0:
            browser = json.loads(result.stdout)
            assert browser['status'] == 'passed' and len(browser['checks']) == 5
            assert browser['crossOriginIsolated'] is False
            entry['browser'] = browser
        if name == 'frontend_browser' and result.returncode == 0:
            browser = json.loads(result.stdout)
            assert browser['status'] == 'passed' and [s['name'] for s in browser['suites']] == ['canvas','input','composition-integration']
            entry['browser'] = browser
            (artifact/'frontend-host-browser.json').write_text(json.dumps(browser,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
        if name.startswith('native_webview_') and result.returncode == 0:
            entry['report'] = json.loads(result.stdout)
            expected_profile = 'DevTools=false' if name.endswith('release') else 'DevTools=true'
            if entry['report'].get('functional_passed') is not True or expected_profile not in entry['report'].get('result',''):
                result.returncode = entry['exit_code'] = 1
        evidence['checks'].append(entry)
        print(json.dumps({k:v for k,v in entry.items() if k!='tests'}, ensure_ascii=False), flush=True)
        if result.returncode:
            evidence['failed_output'] = output[-6000:]
            break
evidence['open_acceptance_criteria'] = ['US-111: strict process handle count', 'US-101: real resource transport and physical 60fps', 'US-103: engine integration and native keyboard/IME']
evidence['lock_bytes_unchanged'] = hashlib.sha256((root / 'Cargo.lock').read_bytes()).hexdigest() == before
evidence['source_bytes_unchanged'] = all(hashlib.sha256((root / p).read_bytes()).hexdigest() == digest
                                         for p, digest in source_hashes.items())
(artifact / 'frontend-host-validation.json').write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding='utf-8', newline='\n')
raise SystemExit(0 if all(c['exit_code'] == 0 for c in evidence['checks'])
                 and len(evidence['checks']) == required_check_count
                 and evidence['lock_bytes_unchanged'] and evidence['source_bytes_unchanged'] else 1)
