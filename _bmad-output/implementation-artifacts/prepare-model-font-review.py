# SPDX-License-Identifier: AGPL-3.0-or-later
"""Prepare context-free review of US-050, US-073 and US-074; agent only."""
from pathlib import Path
import hashlib
import json
import subprocess
import tempfile

ROOT=Path(__file__).resolve().parents[2]
ART=Path(__file__).resolve().parent
BASE='77622b0c410ff9f5686325e81bca1e71cf383642'
SPECS=['spec-us050-opentype-tables.md','spec-us073-selection.md','spec-us074-recovery-journal.md']

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
    reviewed=sorted(p for p in paths if p not in excluded and not Path(p).name.startswith('model-font-review-') and not Path(p).name.startswith('epic-0.'))
    tracked=set(git('ls-files','-z').decode().split('\0'))
    parts=[]
    for path in reviewed:
        parts.append(git('diff','--binary',BASE,'--',path) if path in tracked else git('diff','--no-index','--binary','--','/dev/null',path))
    raw=b''.join(parts)
    directory=Path(tempfile.mkdtemp(prefix='atelier-model-font-review-'))
    diff=directory/'changes.diff'
    diff.write_bytes(raw)
    claims=ART/'model-font-review-claims.md'
    claims.write_text('\n\n'.join((ART/name).read_text(encoding='utf-8') for name in SPECS),encoding='utf-8',newline='\n')
    sources=['Cargo.lock','.github/workflows/ci.yml',*[p.relative_to(ROOT).as_posix() for name in ('model','font') for p in (ROOT/f'crates/{name}').rglob('*.rs')]]
    snapshot={'baseline_commit':BASE,'status':'independent reviews pending; stories open','reviewers':'GPT-6 Luna medium; three fresh parallel agents',
              'diff_file':str(diff),'claims_file':str(claims),'diff_bytes':len(raw),'diff_sha256':hashlib.sha256(raw).hexdigest(),
              'reviewed_paths':reviewed,'source_sha256':{name:hashlib.sha256((ROOT/name).read_bytes()).hexdigest() for name in sorted(sources)}}
    (ART/'model-font-review-snapshot.json').write_text(json.dumps(snapshot,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
    print(json.dumps({key:snapshot[key] for key in ('diff_file','claims_file','diff_bytes','diff_sha256')}))

if __name__=='__main__':
    main()
