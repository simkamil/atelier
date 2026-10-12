# SPDX-License-Identifier: AGPL-3.0-or-later
"""Capture source and generated changes, including untracked binary fixtures."""
from pathlib import Path
import hashlib
import json
import subprocess
import tempfile

root = Path(__file__).resolve().parents[2]
artifact = Path(__file__).resolve().parent
baseline = '39c928983092644f029f2054d96e1b55976ac8b4'
metadata = artifact / 'foundation-integration-review-snapshot.json'
metadata_exclude = ':(exclude)' + metadata.relative_to(root).as_posix()
directory = Path(tempfile.mkdtemp(prefix='atelier-foundation-integration-review-'))
untracked = subprocess.check_output(['git','ls-files','--others','--exclude-standard','-z'],cwd=root).decode().split('\0')
untracked = [n for n in untracked if n and (root/n) != metadata]
complete = [subprocess.check_output(['git','diff',baseline,'--binary','--no-textconv','--no-ext-diff','--no-color','--','.',metadata_exclude],cwd=root)]
sources = [subprocess.check_output(['git','diff',baseline,'--binary','--no-textconv','--no-ext-diff','--no-color','--','.',metadata_exclude,':(exclude)graphify-out'],cwd=root)]
for name in sorted(untracked):
    result = subprocess.run(['git','diff','--no-index','--binary','--no-textconv','--no-ext-diff','--no-color','--','NUL',name],cwd=root,capture_output=True)
    if result.returncode not in (0,1):
        raise RuntimeError(result.stderr.decode())
    complete.append(result.stdout)
    if not name.startswith('graphify-out/'):
        sources.append(result.stdout)
record = {'baseline_commit': baseline, 'snapshot_excludes': [metadata.relative_to(root).as_posix()+' (self-referential metadata)'],
          'generated_review_policy': 'Complete diff retains Graphify; source diff excludes regenerated graph artifacts. Graph integrity is checked separately by deterministic tooling.'}
for kind, parts in [('complete',complete),('sources',sources)]:
    path = directory / (kind+'.diff')
    path.write_bytes(b''.join(parts))
    record[kind+'_diff_file'] = str(path)
    record[kind+'_bytes'] = path.stat().st_size
    record[kind+'_sha256'] = hashlib.sha256(path.read_bytes()).hexdigest()
metadata.write_text(json.dumps(record,indent=2)+'\n',encoding='utf-8',newline='\n')
print(json.dumps(record))
