# SPDX-License-Identifier: AGPL-3.0-or-later
"""Check collector syntax without starting Word; preserve unavailable tooling evidence."""
from pathlib import Path
import hashlib
import json
import shutil
import subprocess

root = Path(__file__).resolve().parents[2]
script = root / 'tools/word-line-reference.ps1'
pwsh = shutil.which('pwsh')
command = r"""+$tokens = $null
$errors = $null
$null = [System.Management.Automation.Language.Parser]::ParseFile('C:\mon-bureau\tools\word-line-reference.ps1', [ref]$tokens, [ref]$errors)
if ($errors.Count) { $errors | ConvertTo-Json -Depth 4; exit 1 }
Write-Output 'syntax-ok'
"""
record = {'script_sha256': hashlib.sha256(script.read_bytes()).hexdigest(),
          'word_capture_executed': False, 'checks': []}
for label, argv in [('syntax', [pwsh, '-NoProfile', '-NonInteractive', '-Command', command]),
                    ('availability', [pwsh, '-NoProfile', '-NonInteractive', '-File', str(script), '-AvailabilityOnly'])]:
    if not pwsh:
        record['checks'].append({'name': label, 'status': 'unavailable', 'reason': 'pwsh not found'})
        continue
    try:
        result = subprocess.run(argv, cwd=root, capture_output=True, text=True,
                                encoding='utf-8', errors='replace', timeout=15)
        record['checks'].append({'name': label, 'status': 'passed' if result.returncode == 0 else 'failed',
                                 'exit_code': result.returncode, 'output': result.stdout + result.stderr})
    except subprocess.TimeoutExpired:
        record['checks'].append({'name': label, 'status': 'unavailable',
                                 'reason': 'PowerShell startup exceeded 15 seconds; this child was terminated.'})
(Path(__file__).parent / 'docx-rich-layout-collector-check.json').write_text(
    json.dumps(record, ensure_ascii=False, indent=2)+'\n', encoding='utf-8', newline='\n')
print(json.dumps(record, ensure_ascii=False))
raise SystemExit(1 if any(c['status'] == 'failed' for c in record['checks']) else 0)
