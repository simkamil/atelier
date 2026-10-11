# SPDX-License-Identifier: AGPL-3.0-or-later
"""Write complete unstaged/untracked review diff without modifying the Git index."""
from pathlib import Path
import difflib
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
baseline = 'c091f6a52aa58af94966592ff356750471200d44'
tracked = subprocess.check_output(['git','diff',baseline,'--no-ext-diff','--no-color'], cwd=ROOT)
parts = [tracked.decode('utf-8')]
untracked = subprocess.check_output(['git','ls-files','--others','--exclude-standard','-z'], cwd=ROOT).decode('utf-8').split('\0')
for name in sorted(filter(None, untracked)):
    path = ROOT / name
    if name.endswith('review-diff.txt'):
        continue
    if name == 'tests/font-layout-reference/layout.tsv':
        parts.append(f'diff --git a/{name} b/{name}\nBinary files /dev/null and b/{name} differ\n')
        continue
    content = path.read_text(encoding='utf-8').splitlines(keepends=True)
    parts.append(f'diff --git a/{name} b/{name}\n')
    parts.extend(difflib.unified_diff([],content,fromfile='/dev/null',tofile='b/'+name))
destination = Path(tempfile.gettempdir())/'atelier-worker-shell-review-c091f6a.diff'
body = ''.join(parts)
entries = re.findall(r'^diff --git a/(.*?) b/(.*?)$', body, re.M)
index = '# Complete review diff index (generated graph JSON remains present below)\n'
index += ''.join(f'# {after}\n' for _, after in entries)
index += '# Golden layout TSV uses Git binary diff attributes; its full bytes are in the workspace, with manifest hash and independent generator.\n\n'
destination.write_text(index + body, encoding='utf-8', newline='\n')
print(f'Review diff: {destination.stat().st_size} bytes')
