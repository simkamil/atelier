# SPDX-License-Identifier: AGPL-3.0-or-later
"""Reproducible Windows UI-lot gates, with immutable runtime/test input hashes."""
from pathlib import Path
from datetime import datetime, timezone
import hashlib
import json
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
LOG = ROOT / 'target/ui-validation'
LOG.mkdir(parents=True, exist_ok=True)

def inventory():
    files = subprocess.run(['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'], cwd=ROOT, check=True, capture_output=True).stdout.decode().split('\0')
    selected = {name for name in files if name and (name.startswith(('crates/', 'tools/', 'web/', 'tests/', 'data/', '.github/')) or name in ('Cargo.toml', 'Cargo.lock', '.gitattributes', '.gitignore', '.graphifyignore'))}
    selected.add(Path(__file__).relative_to(ROOT).as_posix())
    return {name: hashlib.sha256((ROOT/name).read_bytes()).hexdigest() for name in sorted(selected) if (ROOT/name).is_file()}

before = inventory()
assert sys.argv[1:] in ([], ['--reuse-engine-tests']), 'Usage: validate-ui-shell.py [--reuse-engine-tests]'
reused_tests = None
if sys.argv[1:]:
    previous = json.loads((ART/'ui-shell-validation.json').read_text(encoding='utf-8'))
    # Browser assets can change without invalidating parser/model tests. Native
    # resource tests below always rebuild and exercise the final embedded bytes.
    def engine_input(name):
        return name.startswith(('crates/', 'tools/', 'data/')) or name in ('Cargo.toml', 'Cargo.lock') or (name.startswith('tests/') and not name.endswith(('.mjs', '.html', '.css', '.md')))
    current_engine = {name: digest for name, digest in before.items() if engine_input(name)}
    prior_engine = {name: digest for name, digest in previous['inputs'].items() if engine_input(name)}
    assert current_engine == prior_engine, 'Engine/test fixtures changed; run full validation instead'
    reused_tests = next(row.copy() for row in previous['gates'] if row['gate']=='workspace-tests' and row['exit_code']==0)
    assert hashlib.sha256((LOG/'workspace-tests.log').read_bytes()).hexdigest() == reused_tests['log_sha256']
    reused_tests['reused_verified_inputs'] = current_engine
    reused_tests['verified_at'] = previous['generated_at']
    reused_tests['note'] = 'Unchanged Rust/data/native fixture inputs; final embedded bytes rechecked by desktop-resources-tests, release build and real WebView2 smoke.'
results = []
commands = [
    ('format', ['cargo', 'fmt', '--all', '--check']),
    ('clippy', ['cargo', 'clippy', '--workspace', '--all-targets', '--locked', '--offline', '--', '-D', 'warnings']),
    ('workspace-tests', ['cargo', 'test', '--workspace', '--locked', '--offline']),
    ('desktop-resources-tests', ['cargo', 'test', '-p', 'atelier-desktop-win', '--locked', '--offline']),
    ('workspace-guard', ['cargo', 'run', '-p', 'atelier-check-workspace', '--locked', '--offline', '--', '.']),
    ('licenses', ['cargo', 'run', '-p', 'atelier-check-license', '--locked', '--offline', '--', '.']),
    ('theme-node', ['node', 'tests/theme.mjs']),
    ('ui-edge', ['node', 'tests/run-ui-browser.mjs', '--screenshots']),
    ('ui-forced-colors', ['node', 'tests/run-ui-browser.mjs', '--forced-colors', '--screenshots']),
    ('editor-edge', ['node', 'tests/run-editor-browser.mjs']),
    ('frontend-edge', ['node', 'tests/run-frontend-browser.mjs']),
    ('frontend-helpers', ['node', '--test', 'tests/text-drag-drop.mjs', 'tests/french-spacing.mjs', 'tests/rich-preview.mjs', 'tests/rich-preview-protocol.mjs']),
    ('release-host', ['cargo', 'build', '-p', 'atelier-desktop-win', '--release', '--locked', '--offline']),
    ('release-embedded-origin', [str(ROOT/'target/release/atelier.exe'), '--resources-self-check', 'target/ui-validation/native-origin']),
    ('embedded-engine', ['node', 'tests/check-embedded-engine.mjs', '--compare-artifact']),
    ('wasm-size', ['node', 'tests/check-wasm-size.mjs', 'data/engine/atelier_core_wasm.wasm']),
    ('diff-whitespace', ['git', 'diff', '--check']),
]
for label, command in commands:
    if label == 'workspace-tests' and reused_tests:
        results.append(reused_tests)
        print(json.dumps({'gate':label, 'exit_code':0, 'passed':reused_tests['passed'], 'reused_verified_inputs':len(reused_tests['reused_verified_inputs'])}), flush=True)
        continue
    start = time.monotonic()
    result = subprocess.run(command, cwd=ROOT, capture_output=True, timeout=1800)
    output = result.stdout + result.stderr
    (LOG/f'{label}.log').write_bytes(output)
    row = {'gate': label, 'command': command, 'exit_code': result.returncode, 'seconds': round(time.monotonic()-start, 3), 'log_sha256': hashlib.sha256(output).hexdigest()}
    text = result.stdout.decode('utf-8', errors='replace')
    if label == 'workspace-tests':
        summaries = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored', text)
        row.update(passed=sum(int(v[0]) for v in summaries), ignored=sum(int(v[2]) for v in summaries), failed=sum(int(v[1]) for v in summaries))
    if label in ('ui-edge', 'ui-forced-colors', 'editor-edge', 'frontend-edge') and result.returncode == 0:
        report = json.loads(text.strip())
        row.update(assertions=len(report.get('checks', [])) + sum(len(suite.get('checks', [])) for suite in report.get('suites', [])), browser_report=report)
    if label == 'release-embedded-origin': row['output'] = text.strip()
    results.append(row)
    print(json.dumps({key: value for key, value in row.items() if key not in ('command', 'browser_report')}, ensure_ascii=False), flush=True)
    if result.returncode:
        print(output.decode('utf-8', errors='replace')[-12000:], flush=True)
        break
after = inventory()
changed = sorted(name for name in set(before)|set(after) if before.get(name) != after.get(name))
evidence = {
    'baseline_commit': '72874af6736729c52b715103108b77cab26ba215',
    'generated_at': datetime.now(timezone.utc).isoformat(),
    'status': 'passed' if len(results)==len(commands) and all(row['exit_code']==0 for row in results) and not changed else 'failed',
    'platform': 'Windows MSVC; installed Edge; real WebView2 embedded-origin smoke',
    'input_scope': 'All versioned/untracked production, data, test, tooling and CI files, plus this validator; excludes documentation and agent reports.',
    'input_count': len(before), 'inputs': before, 'changed_inputs': changed,
    'gates': results,
    'release_executable': {'path': 'target/release/atelier.exe', 'sha256': hashlib.sha256((ROOT/'target/release/atelier.exe').read_bytes()).hexdigest()},
    'limitations': ['Synthetic DOM keyboard/CDP media events do not establish physical AZERTY/IME or screen-reader acceptance.', 'Linux runtime and remote CI are not claimed by these local Windows gates.'],
}
(ART/'ui-shell-validation.json').write_text(json.dumps(evidence, ensure_ascii=False, indent=2)+'\n', encoding='utf-8')
print(json.dumps({'status':evidence['status'], 'gates':len(results), 'inputs':len(before), 'changed_inputs':changed}), flush=True)
raise SystemExit(0 if evidence['status']=='passed' else 1)
