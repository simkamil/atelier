# SPDX-License-Identifier: AGPL-3.0-or-later
"""Prepare context-free review of US-032, US-072 and US-081; agent only."""
from pathlib import Path
import hashlib
import json
import subprocess
import tempfile

ROOT=Path(__file__).resolve().parents[2]
ART=Path(__file__).resolve().parent
BASE='3aa4d19bef7e8e917b90136f80252bd988e1f7b0'
SPECS=['spec-us032-opaque-xml.md','spec-us072-undo-history.md','spec-us081-svg-backend.md']

def git(*args):
    result=subprocess.run(['git',*args],cwd=ROOT,capture_output=True)
    if result.returncode not in (0,1):
        raise RuntimeError(result.stderr.decode('utf-8',errors='replace'))
    return result.stdout

def main():
    paths=set(git('diff','--name-only','-z',BASE).decode().split('\0'))
    paths.update(git('ls-files','--others','--exclude-standard','-z').decode().split('\0'))
    paths.discard('')
    excluded={f'_bmad-output/implementation-artifacts/{name}' for name in SPECS}
    reviewed=sorted(p for p in paths if p not in excluded and not Path(p).name.startswith('editing-review-') and not Path(p).name.startswith('epic-0.'))
    tracked=set(git('ls-files','-z').decode().split('\0'))
    parts=[]
    for path in reviewed:
        parts.append(git('diff','--binary',BASE,'--',path) if path in tracked else git('diff','--no-index','--binary','--','/dev/null',path))
    raw=b''.join(parts)
    directory=Path(tempfile.mkdtemp(prefix='atelier-editing-review-'))
    diff=directory/'changes.diff'
    diff.write_bytes(raw)
    claims=ART/'editing-review-claims.md'
    claims.write_text('\n\n'.join((ART/name).read_text(encoding='utf-8') for name in SPECS),encoding='utf-8',newline='\n')
    sources=['Cargo.lock','.github/workflows/ci.yml',*[p.relative_to(ROOT).as_posix() for name in ('model','xml','render') for p in (ROOT/f'crates/{name}').rglob('*.rs')]]
    snapshot={'baseline_commit':BASE,'status':'independent reviews pending; stories open','reviewers':'GPT-6 Luna medium; three fresh parallel agents',
              'diff_file':str(diff),'claims_file':str(claims),'diff_bytes':len(raw),'diff_sha256':hashlib.sha256(raw).hexdigest(),
              'reviewed_paths':reviewed,'source_sha256':{name:hashlib.sha256((ROOT/name).read_bytes()).hexdigest() for name in sorted(sources)}}
    (ART/'editing-review-snapshot.json').write_text(json.dumps(snapshot,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
    print(json.dumps({key:snapshot[key] for key in ('diff_file','claims_file','diff_bytes','diff_sha256')}))

if __name__=='__main__':
    main()
