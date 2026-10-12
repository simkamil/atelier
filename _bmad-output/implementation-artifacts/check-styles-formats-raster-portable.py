# SPDX-License-Identifier: AGPL-3.0-or-later
"""Exercise a release executable copied alone, with no repository web/data tree."""
from pathlib import Path
import hashlib
import json
import shutil
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
directory = Path(tempfile.mkdtemp(prefix='editor-portable-', dir=ROOT/'target')).resolve()
assert directory.parent == (ROOT/'target').resolve()
assert directory.name.startswith('editor-portable-')
try:
    executable = directory/'atelier.exe'
    shutil.copyfile(ROOT/'target/release/atelier.exe', executable)
    assert list(directory.iterdir()) == [executable]
    result = subprocess.run([str(executable), '--resources-self-check', str(directory/'check')],
                            cwd=directory, capture_output=True, text=True, encoding='utf-8',
                            errors='replace', timeout=90)
    output = result.stdout.strip()
    passed = result.returncode == 0 and output.startswith(
        'embedded resources: assets=18; worker-ping=echo; editor=typed+ink+undo+redo;')
    passed = passed and output.endswith('; callback objects 0->0') and not result.stderr.strip()
    record = {'passed':passed,'exit_code':result.returncode,'result':output,
              'stderr':result.stderr.strip(),'executable_sha256':hashlib.sha256(executable.read_bytes()).hexdigest(),
              'cwd_contains_web_or_data':False,'only_copied_input':'atelier.exe'}
    (ROOT/'_bmad-output/implementation-artifacts/styles-formats-raster-portable.json').write_text(
        json.dumps(record,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
    print(json.dumps(record,ensure_ascii=False))
    if not passed:
        raise SystemExit(1)
finally:
    for attempt in range(10):
        try:
            shutil.rmtree(directory)
            break
        except OSError:
            if attempt == 9:
                raise
            time.sleep(0.2)
