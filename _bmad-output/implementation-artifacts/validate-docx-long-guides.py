# SPDX-License-Identifier: AGPL-3.0-or-later
"""Reproducible checks for DOCX long guides and exact curves, with an immutable source inventory."""
from pathlib import Path
import datetime
import hashlib
import json
import subprocess
import sys
import time

root = Path(__file__).resolve().parents[2]
out = root / '_bmad-output/implementation-artifacts'
log_path = out / 'docx-long-guides-validation.log'
receipt_path = out / 'docx-long-guides-validation.json'
commands = [
    ['cargo', 'fmt', '--all', '--check'],
    ['cargo', 'clippy', '-p', 'atelier-geom', '-p', 'atelier-render', '-p', 'atelier-text-layout', '-p', 'atelier-text-model', '-p', 'atelier-fmt-ooxml', '-p', 'atelier-core-wasm', '--all-targets', '--locked', '--offline', '--', '-D', 'warnings'],
    ['cargo', 'test', '-p', 'atelier-geom', '-p', 'atelier-render', '-p', 'atelier-text-layout', '-p', 'atelier-text-model', '-p', 'atelier-fmt-ooxml', '-p', 'atelier-core-wasm', '--locked', '--offline'],
    ['cargo', 'clippy', '-p', 'atelier-geom', '-p', 'atelier-render', '-p', 'atelier-text-layout', '-p', 'atelier-text-model', '-p', 'atelier-fmt-ooxml', '-p', 'atelier-core-wasm', '--target', 'wasm32-unknown-unknown', '--all-targets', '--locked', '--offline', '--', '-D', 'warnings'],
    ['cargo', 'run', '-p', 'atelier-check-workspace', '--locked', '--offline', '--', '.'],
    ['cargo', 'run', '-p', 'atelier-check-license', '--locked', '--offline', '--', '.'],
    ['node', '--test', 'tests/rich-preview.mjs', 'tests/rich-preview-protocol.mjs', 'tests/editor-protocol.mjs', 'tests/editor-abi.mjs', 'tests/display-list.mjs'],
    ['node', 'tests/check-embedded-engine.mjs', '--compare-artifact'],
    ['node', 'tests/check-wasm-size.mjs', 'data/engine/atelier_core_wasm.wasm'],
    ['node', 'tests/run-editor-browser.mjs'],
    ['cargo', 'build', '-p', 'atelier-desktop-win', '--release', '--locked', '--offline'],
    [str(root / 'target/release/atelier.exe'), '--resources-self-check', str(root / 'target/docx-long-guides-native-check')],
    ['git', 'diff', '--check'],
]

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def inventory():
    listing = subprocess.check_output(['git', 'ls-files', '-co', '--exclude-standard', '-z'], cwd=root)
    names = {name.decode('utf-8') for name in listing.split(b'\0') if name}
    selected = [name for name in names if (name.startswith(('crates/', 'tools/', 'web/', 'tests/', 'data/', '.github/'))
        or name in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'rustfmt.toml', 'clippy.toml', '.gitattributes', '.graphifyignore')
        or name == Path(__file__).relative_to(root).as_posix()) and (root/name).is_file()]
    return {name: digest(root/name) for name in sorted(selected)}

before = inventory()
receipt = {'schema': 1, 'story': 'DOCX long guides and exact curves', 'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'baseline_commit': 'b3525756b0cf7bbf4732c9ea4ba8ef20d11e485d', 'platform': sys.platform,
    'scope': 'Targeted DOCX long guides validation: affected geom/render/text-layout/text-model/OOXML/core-WASM packages, host/WASM lint, packaged bytes, editor browser, native resources. No full workspace pass is claimed. No earlier test result reused. Windows browser events are synthetic; physical keyboard and screen-reader certification are excluded.',
    'inputs_sha256': before, 'checks': [], 'status': 'running'}
with log_path.open('w', encoding='utf-8', newline='\n') as log:
    for command in commands:
        print('RUN ' + ' '.join(command), flush=True)
        start = time.monotonic()
        process = subprocess.run(command, cwd=root, capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=300)
        output = process.stdout + process.stderr
        log.write('$ ' + ' '.join(command) + '\n' + (output.rstrip('\n') + '\n' if output else ''))
        log.flush()
        entry = {'command': command, 'exit_code': process.returncode, 'seconds': round(time.monotonic()-start, 3)}
        for line in process.stdout.splitlines():
            try:
                report = json.loads(line)
            except (ValueError, TypeError):
                continue
            if isinstance(report, dict):
                entry['report'] = report
        receipt['checks'].append(entry)
        receipt_path.write_text(json.dumps(receipt, ensure_ascii=False, indent=2)+'\n', encoding='utf-8', newline='\n')
        if process.returncode:
            receipt['status'] = 'failed'
            receipt_path.write_text(json.dumps(receipt, ensure_ascii=False, indent=2)+'\n', encoding='utf-8', newline='\n')
            print(output[-8000:], flush=True)
            raise SystemExit(process.returncode)
if inventory() != before:
    receipt['status'] = 'failed'
    receipt['failure'] = 'source inputs changed during validation'
    receipt_path.write_text(json.dumps(receipt, ensure_ascii=False, indent=2)+'\n', encoding='utf-8', newline='\n')
    raise SystemExit('Validation source inputs changed during the run; rerun after freezing the sources.')
receipt['status'] = 'passed'
receipt['completed_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
receipt['log_sha256'] = digest(log_path)
receipt_path.write_text(json.dumps(receipt, ensure_ascii=False, indent=2)+'\n', encoding='utf-8', newline='\n')
print(json.dumps({'status': 'passed', 'gates': len(commands), 'immutable_inputs': len(before)}), flush=True)
