# SPDX-License-Identifier: AGPL-3.0-or-later
"""Bounded deterministic mutations through public FontFace layout API."""
from pathlib import Path
import json
import subprocess

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
WORK = ROOT / 'target/typography-oracle'
WORK.mkdir(parents=True, exist_ok=True)
result = subprocess.run(['cargo','build','-p','atelier-font','--release','--locked','--offline','--message-format=json'],
                        cwd=ROOT, capture_output=True, text=True, check=True)
events = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
library = next(Path(name) for e in events if e.get('reason') == 'compiler-artifact' and e['target']['name'] == 'atelier_font'
               for name in e['filenames'] if name.endswith('.rlib'))
dependencies = library.parent if library.parent.name == 'deps' else library.parent / 'deps'
driver = WORK / 'layout-mutations.exe'
subprocess.run(['rustc','--edition=2024','-O',str(ART/'layout-mutations.rs'),'--extern',f'atelier_font={library}',
                '-L',f'dependency={dependencies}','-o',str(driver)], cwd=ROOT, check=True)
assets = json.loads((ROOT/'data/fonts/manifest.json').read_text(encoding='utf-8'))['assets']
result = subprocess.run([str(driver), *[str(ROOT/a['path']) for a in assets]], cwd=ROOT,
                        capture_output=True, text=True, check=True)
evidence = json.loads(result.stdout)
evidence['validation_units_per_face'] = 100_000
evidence['work_units_per_operation'] = 20_000
(ART/'text-bridge-layout-mutations.json').write_text(json.dumps(evidence, indent=2)+'\n', encoding='utf-8', newline='\n')
print(json.dumps(evidence))
