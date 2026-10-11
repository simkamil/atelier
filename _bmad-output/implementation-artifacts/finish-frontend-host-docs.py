# SPDX-License-Identifier: AGPL-3.0-or-later
"""Publish source-checked evidence, retaining incomplete story acceptance."""
from pathlib import Path
import hashlib
import json
import re

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
proof = ART/'frontend-host-validation.json'
evidence = json.loads(proof.read_text(encoding='utf-8'))
assert len(evidence['checks']) == 34 and all(c['exit_code'] == 0 for c in evidence['checks'])
assert evidence['source_bytes_unchanged'] and evidence['lock_bytes_unchanged']
assert all(hashlib.sha256((ROOT/p).read_bytes()).hexdigest() == h for p,h in evidence['source_sha256'].items())
checks = {c['name']:c for c in evidence['checks']}
assert checks['metadata']['package_count'] == 32 and checks['metadata']['no_external_dependencies']
for profile in ('debug','release'):
    report = checks[f'native_webview_{profile}']['report']
    assert report['functional_passed'] and report['strict_exit_code'] in (0,1)
review = (ART/'frontend-host-review.md').read_text(encoding='utf-8')
assert 'Un constat reste différé' in review and 'US-111 reste ouverte' in review
passed = checks['windows_tests']['passed']
release = checks['shell_release_tests']['passed']
receipt = f'''
Validation finale du lot : [34 contrôles fonctionnels et qualité](../_bmad-output/implementation-artifacts/frontend-host-validation.json),
**{passed} tests workspace et {release} tests shell release**, sur sources et lock inchangés.
Les tests Chromium couvrent Canvas, saisie et leur composition intégrée.
Les deux profils WebView2 accomplissent 100 cycles fonctionnels avec callbacks équilibrés ;
le rapport conserve séparément l'échec strict des handles et US-111 reste ouverte.
La [revue indépendante](../_bmad-output/implementation-artifacts/frontend-host-review.md)
conserve un point IME natif de gravité moyenne non vérifiée à trancher sur matériel.
Les tests musl sont compilés localement ; leur exécution appartient à la CI Linux.
'''
documents = ['docs/us-101-canvas-renderer.md','docs/us-103-text-input.md',
             'docs/us-111-webview2.md','docs/DEVELOPMENT.md','docs/PROTOTYPE-PROGRESS.md']
for name in documents:
    p = ROOT/name
    text = p.read_text(encoding='utf-8')
    if receipt not in text:
        p.write_text(text+receipt,encoding='utf-8',newline='\n')
specs = ['spec-us101-canvas.md','spec-us103-input.md','spec-us111-webview.md']
for name in specs:
    p = ART/name
    text = p.read_text(encoding='utf-8').replace("status: 'in-review'", "status: 'in-progress'")
    text += f'\nModule livré et revu : 34 contrôles fonctionnels/qualité, {passed} tests workspace, {release} tests shell release. US-114 validée ; US-101/103/111 restent ouvertes pour les critères explicités au backlog. Voir frontend-host-validation.json et frontend-host-review.md.\n'
    p.write_text(text,encoding='utf-8',newline='\n')
(ART/'frontend-host-claims.md').write_text('\n\n'.join((ART/name).read_text(encoding='utf-8') for name in specs),encoding='utf-8',newline='\n')
text = (ROOT/'US-V0-V1.md').read_text(encoding='utf-8')
headings = list(re.finditer(r'^#### US-\d+ —',text,re.M))
done = 0
for i,heading in enumerate(headings):
    block = text[heading.start():headings[i+1].start() if i+1<len(headings) else len(text)]
    flags = re.findall(r'^- \[([ x])\]',block,re.M)
    done += bool(flags) and all(f=='x' for f in flags)
assert len(headings) == 158 and done == 35
paths = [ROOT/name for name in documents+['README.md','US-V0-V1.md','web/README.md','tests/README.md']]
paths += [ART/name for name in specs+['frontend-host-review.md','frontend-host-claims.md','frontend-host-exploration.json']]
record = {'date':'2026-10-11','validated_stories':done,'total_stories':158,
          'validated_this_batch':['US-114'],'in_progress':['US-101','US-103','US-111'],
          'runtime_validation_sha256':hashlib.sha256(proof.read_bytes()).hexdigest(),
          'documentation_sha256':{p.relative_to(ROOT).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}}
(ART/'frontend-host-documentation.json').write_text(json.dumps(record,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
print(json.dumps({'done':done,'quality_gates':34,'workspace_tests':passed,'shell_release_tests':release,
                  'native_handle_acceptance':{p:checks[f'native_webview_{p}']['report']['handle_acceptance_passed'] for p in ('debug','release')}}))
