# SPDX-License-Identifier: AGPL-3.0-or-later
"""Record verified TXT editing slices without closing their full rich-text stories."""
from pathlib import Path
import hashlib
import json
import re

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
proof = ART/'editing-ergonomics-validation.json'
evidence = json.loads(proof.read_text(encoding='utf-8'))
checks = {c['name']: c for c in evidence['checks']}
assert len(checks) == 41 and all(c['exit_code'] == 0 for c in checks.values())
assert evidence['source_bytes_unchanged'] and evidence['lock_bytes_unchanged']
assert all(hashlib.sha256((ROOT/p).read_bytes()).hexdigest() == h
           for p,h in evidence['source_sha256'].items())
assert checks['metadata']['package_count'] == 32
assert checks['metadata']['no_external_dependencies']
assert json.loads((ART/'editing-ergonomics-portable.json').read_text(encoding='utf-8'))['passed']
passed = checks['windows_tests']['passed']
release = checks['shell_release_tests']['passed']
browser_count = len(checks['editor_browser']['browser']['checks'])
receipt = f'''## Édition TXT au clavier, à la souris et presse-papiers — 2026-10-11

Trois agents GPT-6 Luna moyen ont développé en parallèle la navigation, la
sélection souris et le presse-papiers, puis trois revues indépendantes ont été
triées. Les [contrats clavier](us-221-keyboard-editing.md),
[souris](us-222-mouse-selection.md) et [texte brut](us-225-plain-clipboard.md)
décrivent les comportements livrés : Ctrl+flèches par mot/paragraphe,
Ctrl+effacement par mot, colonne visuelle conservée, Ctrl+A, F8/Échap,
glisser avec défilement aux bords, double/triple-clic et copier/couper/coller.

La [validation finale](../_bmad-output/implementation-artifacts/editing-ergonomics-validation.json)
réussit **41 contrôles, {passed} tests workspace et {release} tests natifs release**.
Les {browser_count} assertions du navigateur exercent le vrai Worker/WASM ;
l’ABI réelle refuse onze SelectPoint invalides sans mutation. Les sources et le
lock sont inchangés pendant les contrôles. Le shell embarque **18 ressources**,
et la [copie isolée](../_bmad-output/implementation-artifacts/editing-ergonomics-portable.json)
fonctionne sans dossier web/data. La [revue](../_bmad-output/implementation-artifacts/editing-ergonomics-review.md)
conserve 17 constats et leurs décisions, sans nouveau report. Les anciens reports
copy/cut, mot et colonne sont résolus pour cette tranche TXT.
Un timeout d’initialisation WebView2 est conservé dans
`editing-ergonomics-retry-validation.json` ; les contrôles restants ont été repris
sur les mêmes sources, avec des dossiers de diagnostic natifs isolés.
La sonde portable a également réussi à la seconde tentative, avec le même
exécutable ; `editing-ergonomics-portable-first-attempt.json` conserve son timeout
ExecuteScript initial. Ces reprises ne prouvent pas l’absence d’instabilité du runtime.

Le backlog reste à **35/158 US complètes**. US-214/220/221/222/223/225 restent
ouvertes : objets riches, phrases/marge/bloc rectangulaire, formats HTML/interne
et images, ainsi que les essais physiques, ne sont pas tous livrés. US-111
conserve son échec strict de handles. Les événements synthétiques ne valident pas
AZERTY/IME physique ; les tests musl sont compilés localement et exécutés en CI Linux.
'''
documents = ['docs/us-221-keyboard-editing.md','docs/us-222-mouse-selection.md',
             'docs/us-225-plain-clipboard.md','docs/us-120-editor-ui.md',
             'docs/us-120-editor-engine.md','docs/us-112-embedded-resources.md']
mouse_doc = ROOT/'docs/us-222-mouse-selection.md'
mouse_text = mouse_doc.read_text(encoding='utf-8')
mouse_text = mouse_text.replace(
    "L'intégration avec la queue de commandes de l'éditeur, le vrai moteur/WASM,\nl'undo et les ressources natives reste à vérifier par la racine dans la suite\nnavigateur globale.",
    "L’intégration avec la queue de commandes, le vrai Worker/WASM, l’undo et les ressources natives est vérifiée dans la suite globale ci-dessous. Les erreurs du callback isEnabled annulent capture et RAF et sont rapportées une fois.")
mouse_doc.write_text(mouse_text,encoding='utf-8',newline='\n')
for name in documents:
    p = ROOT/name
    t = p.read_text(encoding='utf-8')
    if receipt not in t:
        p.write_text(t.rstrip()+'\n\n'+receipt,encoding='utf-8',newline='\n')
for name,marker in [('docs/DEVELOPMENT.md','## Validation\n'),
                    ('docs/PROTOTYPE-PROGRESS.md','Les preuves des lots antérieurs')]:
    p = ROOT/name
    t = p.read_text(encoding='utf-8')
    assert marker in t
    if receipt not in t:
        t = t.replace(marker, marker+'\n'+receipt+'\n',1) if name.endswith('DEVELOPMENT.md') else t.replace(marker,receipt+'\n'+marker,1)
        p.write_text(t,encoding='utf-8',newline='\n')

backlog_path = ROOT/'US-V0-V1.md'
backlog = backlog_path.read_text(encoding='utf-8')
notes = {
    '214': 'Tranche TXT validée le 2026-10-11 : colonne visuelle conservée entre lignes courtes ; voir [navigation](docs/us-221-keyboard-editing.md). Les clusters riches restent à couvrir.',
    '220': 'Tranche TXT validée le 2026-10-11 : Ctrl+Retour arrière/Suppr par mot et annulation ; voir [navigation](docs/us-221-keyboard-editing.md). Les critères de saisie riche/physique restent ouverts.',
    '221': 'Tranche TXT validée le 2026-10-11 : Ctrl+flèches par mot/paragraphe, maintien du X ; voir [contrat](docs/us-221-keyboard-editing.md). Navigation entre objets riches encore absente.',
    '222': 'Tranche TXT validée le 2026-10-11 : glisser, auto-scroll, Maj+clic, double-clic mot et triple-clic paragraphe ; voir [contrat](docs/us-222-mouse-selection.md). Phrase, marge et bloc rectangulaire restent ouverts.',
    '223': 'Tranche TXT validée le 2026-10-11 : Ctrl+A, Maj+mouvements disponibles et F8/Échap ; Ctrl+A quitte F8. Les combinaisons liées aux objets riches restent ouvertes.',
    '225': 'Tranche TXT validée le 2026-10-11 : copy/cut/paste text/plain synchrones, Ctrl+Maj+V, sélection UTF-8 et coupe annulable ; voir [contrat](docs/us-225-plain-clipboard.md). HTML/interne riche, images et options restent ouverts.',
}
for story,note in notes.items():
    start = backlog.index(f'#### US-{story} —')
    ref = backlog.index('Réf :',start)
    if note not in backlog[start:ref]:
        backlog = backlog[:ref]+note+'\n\n'+backlog[ref:]
backlog_path.write_text(backlog,encoding='utf-8',newline='\n')
headings = list(re.finditer(r'^#### US-\d+ —',backlog,re.M))
done = 0
for i,heading in enumerate(headings):
    block = backlog[heading.start():headings[i+1].start() if i+1<len(headings) else len(backlog)]
    flags = re.findall(r'^- \[([ x])\]',block,re.M)
    done += bool(flags) and all(f=='x' for f in flags)
assert len(headings)==158 and done==35
review = (ART/'editing-ergonomics-review.md').read_text(encoding='utf-8')
table = review[review.index('| ID |'):review.index('\n\nTous les constats')]
specs = ['spec-us221-keyboard-editing.md','spec-us222-mouse-selection.md','spec-us225-plain-clipboard.md']
for name in specs:
    p = ART/name
    t = p.read_text(encoding='utf-8').replace("status: 'in-review'","status: 'done'")
    if table not in t:
        t = t.replace('## Review Triage Log\n','## Review Triage Log\n\n'+table+'\n',1)
    note = f'Validation finale : 41 contrôles, {passed} tests workspace, {release} tests natifs release, {browser_count} assertions navigateur ; sources/lock inchangés. Voir editing-ergonomics-validation.json et editing-ergonomics-review.md. done concerne cette tranche TXT, pas la clôture des US parentes.'
    if note not in t:
        t = t.rstrip()+'\n\n'+note+'\n'
    p.write_text(t,encoding='utf-8',newline='\n')
(ART/'editing-ergonomics-claims.md').write_text('\n\n'.join((ART/n).read_text(encoding='utf-8') for n in specs),encoding='utf-8',newline='\n')
paths = [ROOT/n for n in documents+['docs/DEVELOPMENT.md','docs/PROTOTYPE-PROGRESS.md','README.md','US-V0-V1.md','web/README.md','tests/README.md']]
paths += [ART/n for n in specs+['editing-ergonomics-review.md','editing-ergonomics-claims.md']]
record = {'date':'2026-10-11','validated_stories':done,'total_stories':158,
          'validated_slices':['keyboard navigation','mouse selection','plain-text clipboard'],
          'in_progress':['US-214','US-220','US-221','US-222','US-223','US-225'],
          'quality_gates':41,'workspace_tests':passed,'native_release_tests':release,'browser_assertions':browser_count,
          'runtime_validation_sha256':hashlib.sha256(proof.read_bytes()).hexdigest(),
          'documentation_sha256':{p.relative_to(ROOT).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}}
(ART/'editing-ergonomics-documentation.json').write_text(json.dumps(record,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
print(json.dumps({k:v for k,v in record.items() if not k.endswith('sha256')},ensure_ascii=False))
