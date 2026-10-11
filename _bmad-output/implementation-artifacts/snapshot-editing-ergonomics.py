# SPDX-License-Identifier: AGPL-3.0-or-later
"""Refresh the patched review snapshot without touching Git's index."""
from pathlib import Path
import difflib
import hashlib
import json
import subprocess

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
metadata = ART/'editing-ergonomics-review-snapshot.json'
record = json.loads(metadata.read_text(encoding='utf-8'))
excluded = metadata.relative_to(ROOT).as_posix()
parts = [subprocess.check_output(['git','diff',record['baseline_commit'],
    '--no-ext-diff','--no-color','--binary','--','.',f':(exclude){excluded}'],cwd=ROOT).decode('utf-8')]
files = subprocess.check_output(['git','ls-files','--others','--exclude-standard','-z'],cwd=ROOT).decode('utf-8').split('\0')
for name in sorted(filter(None,files)):
    if name==excluded:
        continue
    content = (ROOT/name).read_bytes()
    assert b'\0' not in content, f'Unexpected new binary: {name}'
    parts.append(f'diff --git a/{name} b/{name}\n')
    parts.extend(difflib.unified_diff([],content.decode('utf-8').splitlines(keepends=True),
                                    fromfile='/dev/null',tofile='b/'+name))
destination = Path(record['diff_file']).with_name('patched-changes.diff')
destination.write_text(''.join(parts),encoding='utf-8',newline='\n')
record.update(patched_diff_file=str(destination),patched_bytes=destination.stat().st_size,
              patched_sha256=hashlib.sha256(destination.read_bytes()).hexdigest(),
              patched_snapshot_excludes=[excluded+' (self-referential metadata)'])
metadata.write_text(json.dumps(record,indent=2)+'\n',encoding='utf-8',newline='\n')
print(json.dumps({k:v for k,v in record.items() if k.startswith('patched_')}))
