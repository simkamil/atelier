# SPDX-License-Identifier: AGPL-3.0-or-later
"""Create an exhaustive tracked/untracked unified review diff without staging Git."""
from pathlib import Path
import difflib, hashlib, json, subprocess, tempfile
ROOT=Path(__file__).resolve().parents[2]
ART=Path(__file__).resolve().parent
BASELINE='b72b0377832358f590e313602cbef9f55a4ecc6d'
parts=[subprocess.check_output(['git','diff',BASELINE,'--no-ext-diff','--no-color'],cwd=ROOT).decode('utf-8')]
files=subprocess.check_output(['git','ls-files','--others','--exclude-standard','-z'],cwd=ROOT).decode('utf-8').split('\0')
for name in sorted(filter(None,files)):
    if name.endswith('review-diff.txt'): continue
    content=(ROOT/name).read_bytes()
    parts.append(f'diff --git a/{name} b/{name}\n')
    if b'\0' in content or name=='data/webview2/include/WebView2.h':
        parts.append(f'Binary files /dev/null and b/{name} differ\n')
        parts.append(f'# Verified upstream bytes: {len(content)} SHA-256 {hashlib.sha256(content).hexdigest()}\n')
    else:
        parts.extend(difflib.unified_diff([],content.decode('utf-8').splitlines(keepends=True),fromfile='/dev/null',tofile='b/'+name))
path=Path(tempfile.gettempdir())/'atelier-frontend-host-b72b037.diff'
body=''.join(parts)
path.write_text(body,encoding='utf-8',newline='\n')
claims=ART/'frontend-host-claims.md'
claims.write_text('\n\n'.join((ART/name).read_text(encoding='utf-8') for name in ('spec-us101-canvas.md','spec-us103-input.md','spec-us111-webview.md')),encoding='utf-8',newline='\n')
print(json.dumps({'diff_file':str(path),'bytes':path.stat().st_size,'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'claims_file':str(claims)}))
