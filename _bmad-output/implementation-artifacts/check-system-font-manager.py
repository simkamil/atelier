# SPDX-License-Identifier: AGPL-3.0-or-later
"""Read installed Windows fonts without distributing their bytes; validation tooling only."""
from pathlib import Path
import hashlib
import json
import os
import subprocess

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
fonts = Path(os.environ.get('WINDIR', 'C:/Windows'))/'Fonts'
paths = [fonts/name for name in ('arial.ttf','arialbd.ttf','ariali.ttf','arialbi.ttf')]
assert all(p.is_file() for p in paths), 'This Windows evidence requires four installed Arial variants'
subprocess.run(['cargo','build','-p','atelier-font','--locked','--offline'], cwd=ROOT, check=True)
driver = ROOT/'target/font-manager-system.exe'
subprocess.run(['rustc','--edition=2024',str(ART/'font-manager-system.rs'),
                '--extern',f'atelier_font={ROOT}/target/debug/libatelier_font.rlib',
                '-L',f'dependency={ROOT}/target/debug/deps','-o',str(driver)], cwd=ROOT, check=True)
run = subprocess.run([str(driver),*[str(p) for p in paths]], cwd=ROOT, check=True,
                     capture_output=True, text=True, encoding='utf-8')
evidence = {'host':'Windows', 'system_faces':4, 'results':run.stdout.splitlines(),
            'inputs':[{'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in paths],
            'scope':'Actual system font bytes registered through the source API; native Host bridge remains US-113.'}
(ART/'text-layout-system-fonts.json').write_text(json.dumps(evidence,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
print(json.dumps(evidence,ensure_ascii=False))
