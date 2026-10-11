# SPDX-License-Identifier: AGPL-3.0-or-later
"""Publish the verified editor slice without closing unfinished full stories."""
from pathlib import Path
import hashlib
import json
import re

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
proof = ART / 'editor-integration-validation.json'
evidence = json.loads(proof.read_text(encoding='utf-8'))
assert len(evidence['checks']) == 40 and all(c['exit_code'] == 0 for c in evidence['checks'])
assert evidence['source_bytes_unchanged'] and evidence['lock_bytes_unchanged']
assert all(hashlib.sha256((ROOT / p).read_bytes()).hexdigest() == h
           for p, h in evidence['source_sha256'].items())
checks = {c['name']: c for c in evidence['checks']}
assert checks['metadata']['package_count'] == 32 and checks['metadata']['no_external_dependencies']
for profile in ('debug', 'release'):
    assert checks[f'embedded_resources_{profile}']['report']['passed']
    assert checks[f'native_webview_{profile}']['report']['functional_passed']
portable = json.loads((ART/'editor-integration-portable.json').read_text(encoding='utf-8'))
assert portable['passed']
passed = checks['windows_tests']['passed']
release = checks['shell_release_tests']['passed']
receipt = f'''## Validation du premier éditeur intégré — 2026-10-11

Trois agents GPT-6 Luna moyen ont développé en parallèle le moteur, l’interface
et le pont fichiers, puis corrigé le lot après trois revues indépendantes.
La [validation finale](../_bmad-output/implementation-artifacts/editor-integration-validation.json)
réussit **40 contrôles, {passed} tests workspace et {release} tests natifs release**,
avec sources et lock inchangés. Le navigateur utilise le vrai Worker/WASM/Canvas
et un Host factice : saisie/sélection, pagination, composition, annulations,
sauvegarde obsolète, raccourcis de la vraie page et miroir textuel accessible.
L’ABI réelle vérifie aussi le rollback d’un candidat lorsque le registre de
128 Mio ne peut réserver sa réponse.

Les diagnostics WebView2 debug/release chargent les **17 ressources** et vérifient
saisie accentuée, pixels de glyphes, undo/redo, HTTP et six catégories CSP.
La [copie isolée](../_bmad-output/implementation-artifacts/editor-integration-portable.json)
fonctionne sans dossier web/data. Les tests de routage natif injectent des résultats
de dialogue contrôlés et passent par le broker réel ; ils n’ouvrent aucun dialogue.
La [revue](../_bmad-output/implementation-artifacts/editor-integration-review.md)
conserve les 12 constats séparément : cinq corrections retenues et trois fonctions
V1 reportées (presse-papiers, navigation par mot, colonne préférée), avec les rejets
motivés. Les limites de wrapping et de Tab sont également décrites dans le contrat UI.

Le premier parcours TXT est testable avec `cargo run -p atelier-desktop-win --release --locked --offline`. La tranche reste limitée à
128 Kio, latin gauche à droite et mise en page complète à chaque modification.
Les dialogues manuels, AZERTY/IME physique et mesures de 300 pages/démarrage restent
ouverts pour US-120. US-113 conserve SharedBuffer, polices/URL ; US-111 conserve
l’échec strict de handles. Les **35/158 US complètes** sont inchangées. Les tests
musl sont compilés localement ; leur exécution appartient à la CI Linux publiée.
'''
documents = ['docs/us-120-editor-engine.md', 'docs/us-120-editor-ui.md',
             'docs/us-113-file-bridge.md', 'docs/us-120-plain-text-model.md',
             'docs/us-113-file-access.md', 'docs/us-112-embedded-resources.md']
for name in documents:
    p = ROOT/name
    t = p.read_text(encoding='utf-8')
    if receipt not in t:
        p.write_text(t.rstrip()+'\n\n'+receipt, encoding='utf-8', newline='\n')
for name, marker in [('docs/DEVELOPMENT.md', '## Validation\n'),
                     ('docs/PROTOTYPE-PROGRESS.md', 'Les preuves des lots antérieurs')]:
    p = ROOT/name
    t = p.read_text(encoding='utf-8')
    if receipt not in t:
        if name.endswith('DEVELOPMENT.md'):
            t = t.replace(marker, marker+'\n'+receipt+'\n', 1)
        else:
            t = t.replace(marker, receipt+'\n'+marker, 1)
        p.write_text(t, encoding='utf-8', newline='\n')
specs = ['spec-us120-editor-engine.md', 'spec-us120-editor-ui.md', 'spec-us113-file-bridge.md']
for name in specs:
    p = ART/name
    t = p.read_text(encoding='utf-8').replace("status: 'in-review'", "status: 'done'")
    if name == specs[0]:
        t = t.replace('(17 tests)', '(suite complétée après revue)')
    t += f'\nValidation finale du lot : 40 contrôles, {passed} tests workspace, {release} tests natifs release ; sources et lock inchangés. Voir editor-integration-validation.json et editor-integration-review.md. done concerne cette tranche ; les US-113/120 complètes restent ouvertes.\n'
    p.write_text(t, encoding='utf-8', newline='\n')
backlog = (ROOT/'US-V0-V1.md').read_text(encoding='utf-8')
headings = list(re.finditer(r'^#### US-\d+ —', backlog, re.M))
done = 0
for i, heading in enumerate(headings):
    block = backlog[heading.start():headings[i+1].start() if i+1<len(headings) else len(backlog)]
    flags = re.findall(r'^- \[([ x])\]', block, re.M)
    done += bool(flags) and all(f == 'x' for f in flags)
assert len(headings) == 158 and done == 35
(ART/'editor-integration-claims.md').write_text('\n\n'.join((ART/n).read_text(encoding='utf-8') for n in specs), encoding='utf-8', newline='\n')
paths = [ROOT/n for n in documents+['docs/DEVELOPMENT.md','docs/PROTOTYPE-PROGRESS.md',
         'README.md','US-V0-V1.md','web/README.md','tests/README.md']]
paths += [ART/n for n in specs+['editor-integration-review.md','editor-integration-claims.md']]
record = {'date':'2026-10-11','validated_stories':done,'total_stories':158,
          'validated_modules':['plain-text WASM editor','Canvas editor UI','native TXT bridge'],
          'in_progress':['US-092','US-101','US-103','US-111','US-112','US-113','US-120'],
          'quality_gates':40,'workspace_tests':passed,'native_release_tests':release,
          'runtime_validation_sha256':hashlib.sha256(proof.read_bytes()).hexdigest(),
          'documentation_sha256':{p.relative_to(ROOT).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}}
(ART/'editor-integration-documentation.json').write_text(json.dumps(record,ensure_ascii=False,indent=2)+'\n', encoding='utf-8', newline='\n')
print(json.dumps({k:v for k,v in record.items() if not k.endswith('sha256')},ensure_ascii=False))
