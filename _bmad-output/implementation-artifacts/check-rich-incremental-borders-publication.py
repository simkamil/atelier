# SPDX-License-Identifier: AGPL-3.0-or-later
"""Compare the tested inventory to the Git index, allowing only EOL normalization."""
from pathlib import Path
import hashlib
import json
import subprocess

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
report = json.loads((ART/'rich-incremental-borders-validation.json').read_text(encoding='utf-8'))
assert report['passed'] and report['source_bytes_unchanged'] and report['lock_bytes_unchanged']
paths = sorted(report['source_sha256'])
result = subprocess.run(['git','cat-file','--batch'],input=''.join(f':{p}\n' for p in paths).encode(),
                        cwd=ROOT,capture_output=True,check=True)
offset = 0
normalized = []
published = {}
for path in paths:
    end = result.stdout.index(b'\n',offset)
    header = result.stdout[offset:end].decode('utf-8')
    assert not header.endswith(' missing'), f'Validated input not staged: {path}'
    _, kind, count = header.split()
    assert kind == 'blob'
    start = end + 1
    content = result.stdout[start:start+int(count)]
    offset = start + int(count) + 1
    working = (ROOT/path).read_bytes()
    working_hash = hashlib.sha256(working).hexdigest()
    assert working_hash == report['source_sha256'][path], f'Changed after validation: {path}'
    published[path] = hashlib.sha256(content).hexdigest()
    if content != working:
        assert content == working.replace(b'\r\n',b'\n'), f'Staged source differs: {path}'
        normalized.append({'path':path,'validated_working_sha256':working_hash,
                           'published_sha256':published[path],'change':'Git CRLF-to-LF normalization only'})
assert offset == len(result.stdout)
record = {'validated_inventory':'rich-incremental-borders-validation.json',
          'validated_input_count':len(paths),'all_validated_inputs_staged':True,
          'source_semantics_unchanged':True,'normalizations':normalized,
          'published_inventory_sha256':hashlib.sha256(json.dumps(published,sort_keys=True).encode()).hexdigest(),
          'note':'Staged inputs equal the immutable tested inventory, except recorded Git EOL normalization.'}
(ART/'rich-incremental-borders-publication.json').write_text(json.dumps(record,indent=2)+'\n',encoding='utf-8',newline='\n')
print(json.dumps({k:v for k,v in record.items() if k!='normalizations'}))
