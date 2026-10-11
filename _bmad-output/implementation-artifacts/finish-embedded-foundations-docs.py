# SPDX-License-Identifier: AGPL-3.0-or-later
"""Publish verified modules while keeping unfinished full stories open."""
from pathlib import Path
import hashlib
import json
import re

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
proof = ART / 'embedded-foundations-validation.json'
evidence = json.loads(proof.read_text(encoding='utf-8'))
assert len(evidence['checks']) == 37 and all(c['exit_code'] == 0 for c in evidence['checks'])
assert evidence['source_bytes_unchanged'] and evidence['lock_bytes_unchanged']
assert all(hashlib.sha256((ROOT / p).read_bytes()).hexdigest() == h
           for p, h in evidence['source_sha256'].items())
checks = {c['name']: c for c in evidence['checks']}
for profile in ('debug', 'release'):
    assert checks[f'embedded_resources_{profile}']['report']['passed']
    assert checks[f'native_webview_{profile}']['report']['functional_passed']
assert checks['metadata']['package_count'] == 32
assert checks['metadata']['no_external_dependencies']
passed = checks['windows_tests']['passed']
release = checks['shell_release_tests']['passed']
receipt = f'''
Le lot ressources, fichiers et texte brut a été développé en parallèle avec trois
agents GPT-6 Luna moyen, puis corrigé après trois revues indépendantes.
La [validation finale](../_bmad-output/implementation-artifacts/embedded-foundations-validation.json)
réussit **37 contrôles, {passed} tests workspace et {release} tests natifs release**,
sur sources et lock inchangés. Les diagnostics debug/release chargent les onze
ressources, vérifient le vrai Worker/WASM par écho Ping, HEAD/404/405 et six
catégories de refus CSP avec traces natives. La [copie isolée de l’exécutable](../_bmad-output/implementation-artifacts/embedded-foundations-portable.json)
fonctionne sans dossier web/data. Les [18 constats de revue](../_bmad-output/implementation-artifacts/embedded-foundations-review.md)
sont triés ; les corrections retenues sont appliquées, aucun nouveau constat
n’est différé. Les [échecs exploratoires](../_bmad-output/implementation-artifacts/embedded-foundations-exploration.json)
restent conservés.

US-112 reste ouverte pour la journalisation des violations CSP internes aux
Workers et les scénarios service-worker. US-113 reste ouverte pour le pont
fichiers JS, SharedBuffer, les polices/URL et les essais de dialogues natifs.
US-120 reste ouverte pour la mise en page et la connexion saisie/modèle/rendu,
ainsi que ses mesures de latence et démarrage. US-111 conserve l’échec strict
de handles ; ses contrôles fonctionnels de 100 cycles et callbacks passent.
Le backlog reste à **35 US entièrement validées sur 158**. Les tests musl sont
compilés localement ; leur exécution appartient à la CI Linux du commit publié.
'''
documents = ['docs/us-112-embedded-resources.md', 'docs/us-113-file-access.md',
             'docs/us-120-plain-text-model.md', 'docs/DEVELOPMENT.md',
             'docs/PROTOTYPE-PROGRESS.md']
for name in documents:
    p = ROOT / name
    text = p.read_text(encoding='utf-8')
    if receipt not in text:
        p.write_text(text + receipt, encoding='utf-8', newline='\n')
specs = ['spec-us112-embedded-origin.md', 'spec-us113-file-access.md',
         'spec-us120-plain-text-model.md']
for name in specs:
    p = ART / name
    text = p.read_text(encoding='utf-8')
    status = 'in-progress' if name == specs[0] else 'done'
    text = text.replace("status: 'in-review'", f"status: '{status}'")
    start = text.index('**Execution:**')
    end = text.index('**Acceptance Criteria:**')
    text = text[:start] + text[start:end].replace('- [ ]', '- [x]') + text[end:]
    text += f'\nValidation après corrections : 37 contrôles, {passed} tests workspace, {release} tests natifs release ; sources et lock inchangés. Voir embedded-foundations-validation.json et embedded-foundations-review.md. Le statut done des tranches fichiers/modèle ne termine pas les US-113/120 complètes. US-112 reste in-progress pour les scénarios explicitement ouverts.\n'
    p.write_text(text, encoding='utf-8', newline='\n')
backlog = ROOT / 'US-V0-V1.md'
text = backlog.read_text(encoding='utf-8')
text = text.replace('- [ ] Ressources incorporées à la compilation,', '- [x] Ressources incorporées à la compilation,')
text = text.replace('- [ ] Les liens externes ne naviguent jamais dans la WebView.',
                    '- [x] Les liens externes ne naviguent jamais dans la WebView.')
notes = {
    'US-112': 'Avancement US-112 (2026-10-11) : [origine embarquée, table HTTP et six catégories CSP validées](docs/us-112-embedded-resources.md). La journalisation CSP interne aux Workers et les scénarios service-worker restent ouverts ; l’US complète n’est pas terminée.',
    'US-113': 'Avancement US-113 (2026-10-11) : [backend fichiers et bindings dialogues](docs/us-113-file-access.md) implémentés et testés sans UI interactive. Pont fichiers JS, SharedBuffer, polices système, URL et validation manuelle restent à développer/valider.',
    'US-120': 'Avancement US-120 (2026-10-11) : [modèle concret de texte brut](docs/us-120-plain-text-model.md) validé : UTF-8/BOM, sélection, graphèmes et annuler/rétablir. Les critères de démonstration ci-dessus restent ouverts : aucun éditeur intégré n’est encore livré.',
}
for story, note in notes.items():
    pattern = rf'(^#### {story} —.*?^Réf :[^\n]*\n)'
    assert re.search(pattern, text, re.M | re.S)
    if note not in text:
        text = re.sub(pattern, lambda m: m.group(1) + '\n' + note + '\n', text, count=1, flags=re.M | re.S)
backlog.write_text(text, encoding='utf-8', newline='\n')
headings = list(re.finditer(r'^#### US-\d+ —', text, re.M))
done = 0
for i, heading in enumerate(headings):
    block = text[heading.start():headings[i + 1].start() if i + 1 < len(headings) else len(text)]
    flags = re.findall(r'^- \[([ x])\]', block, re.M)
    done += bool(flags) and all(f == 'x' for f in flags)
assert len(headings) == 158 and done == 35
(ART / 'embedded-foundations-claims.md').write_text(
    '\n\n'.join((ART / name).read_text(encoding='utf-8') for name in specs),
    encoding='utf-8', newline='\n')
paths = [ROOT / name for name in documents + ['README.md', 'US-V0-V1.md', 'web/README.md', 'tests/README.md']]
paths += [ART / name for name in specs + ['embedded-foundations-review.md', 'embedded-foundations-claims.md']]
record = {'date': '2026-10-11', 'validated_stories': done, 'total_stories': 158,
          'validated_this_batch': [], 'validated_modules': ['embedded resources', 'native file backend', 'plain text model'],
          'in_progress': ['US-112', 'US-113', 'US-120', 'US-111'],
          'runtime_validation_sha256': hashlib.sha256(proof.read_bytes()).hexdigest(),
          'documentation_sha256': {p.relative_to(ROOT).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}}
(ART / 'embedded-foundations-documentation.json').write_text(
    json.dumps(record, ensure_ascii=False, indent=2) + '\n', encoding='utf-8', newline='\n')
print(json.dumps({'done': done, 'quality_gates': 37, 'workspace_tests': passed, 'native_release_tests': release}))
