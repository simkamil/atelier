# SPDX-License-Identifier: AGPL-3.0-or-later
"""Run scoped validation against frozen tracked product inputs."""
from pathlib import Path
import datetime
import hashlib
import json
import re
import subprocess
import time

root = Path(__file__).resolve().parents[2]
out = root / '_bmad-output/implementation-artifacts'

def git(*args):
    return subprocess.check_output(['git', *args], cwd=root).decode('utf-8').strip()

def inputs():
    paths = git('ls-files', '-z').split('\0')
    selected = sorted(p for p in paths if p and (
        p in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml') or
        p.startswith(('crates/', 'data/', 'tests/', 'web/', 'tools/', '.cargo/'))))
    return {p: hashlib.sha256((root / p).read_bytes()).hexdigest() for p in selected}

before = inputs()
commands = [
    ['cargo', 'test', '-p', 'atelier-core-wasm', '--locked', '--offline'],
    ['cargo', 'clippy', '-p', 'atelier-core-wasm', '--all-targets', '--locked', '--offline', '--', '-D', 'warnings'],
    ['cargo', 'check', '-p', 'atelier-core-wasm', '--target', 'wasm32-unknown-unknown', '--locked', '--offline'],
    ['cargo', 'fmt', '--all', '--', '--check'],
    ['node', 'tests/check-embedded-engine.mjs', '--compare-artifact'],
    ['node', 'tests/check-wasm-size.mjs'],
    ['node', 'tests/editor-protocol.mjs'],
    ['git', 'diff', '--check'],
    ['git', 'diff', '--cached', '--check'],
]
results = []
log = []
for command in commands:
    print('RUN ' + ' '.join(command), flush=True)
    started = time.monotonic()
    result = subprocess.run(command, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    output = result.stdout.decode('utf-8', errors='replace')
    tests = sum(int(n) for n in re.findall(r'test result: ok\. (\d+) passed;', output))
    results.append({'command': command, 'exit_code': result.returncode,
                    'seconds': round(time.monotonic() - started, 3), 'rust_tests_passed': tests})
    log.append('$ ' + ' '.join(command) + '\n' + output)
    print(json.dumps(results[-1]), flush=True)
    if result.returncode:
        break
after = inputs()
changed = sorted(p for p in before.keys() | after.keys() if before.get(p) != after.get(p))
receipt = {
    'created_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'branch': git('branch', '--show-current'), 'baseline_commit': git('rev-parse', 'HEAD'),
    'scope': 'retained rich session engine; unchanged TXT/preview protocols; no editable UI or DOCX save',
    'checks': results, 'all_passed': len(results) == len(commands) and all(r['exit_code'] == 0 for r in results) and not changed,
    'rust_tests_passed': sum(r['rust_tests_passed'] for r in results),
    'input_count': len(before), 'input_sha256': before, 'inputs_changed_during_validation': changed,
    'limits': ['No full workspace test pass claimed', 'No editable DOCX UI or native binary built in this lot',
               'No Word/LibreOffice save round trip', 'No hardware/IME/screen-reader qualification',
               'Embedded WASM rebuilt and compared; no new rich-session protocol opcode'],
}
(out / 'docx-edit-session-validation.json').write_text(json.dumps(receipt, indent=2) + '\n', encoding='utf-8')
(out / 'docx-edit-session-validation.log').write_text('\n'.join(log), encoding='utf-8')
print(json.dumps({k: receipt[k] for k in ('all_passed', 'rust_tests_passed', 'input_count', 'inputs_changed_during_validation')}), flush=True)
raise SystemExit(0 if receipt['all_passed'] else 1)
