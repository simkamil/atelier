# SPDX-License-Identifier: AGPL-3.0-or-later
"""Close US-012/054/055 only after review and immutable complete verification."""
from pathlib import Path
import hashlib
import json
import re

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
proof = ART/'text-layout-validation.json'
evidence = json.loads(proof.read_text(encoding='utf-8'))
assert len(evidence['checks']) == 26 and all(c['exit_code'] == 0 for c in evidence['checks'])
assert evidence['source_bytes_unchanged'] and evidence['lock_bytes_unchanged']
assert all(hashlib.sha256((ROOT/p).read_bytes()).hexdigest() == h for p,h in evidence['source_sha256'].items())
checks = {c['name']:c for c in evidence['checks']}
assert checks['metadata']['package_count'] == 32 and checks['metadata']['all_members_local'] and checks['metadata']['no_external_dependencies']
mutations = json.loads((ART/'text-layout-layout-mutations.json').read_text(encoding='utf-8'))
assert mutations['cases'] == 10000 and mutations['panics'] == 0
system = json.loads((ART/'text-layout-system-fonts.json').read_text(encoding='utf-8'))
assert system['system_faces'] == 4 and len(system['results']) == 4
review = (ART/'text-layout-review.md').read_text(encoding='utf-8')
assert 'Aucun constat différé' in review and len(re.findall(r'^\| B\d+ \|',review,re.M)) == 11
passed = checks['windows_tests']['passed']
release = checks['text_layout_release_tests']['passed']
stories = [('012','line-breaks','us-012-line-breaks.md'),('054','latin-shaping','us-054-latin-shaping.md'),('055','font-manager','us-055-font-manager.md')]
for number, suffix, document in stories:
    p = ART/f'spec-us{number}-{suffix}.md'
    text = p.read_text(encoding='utf-8')
    assert "status: 'in-review'" in text
    text = text.replace("status: 'in-review'","status: 'done'").replace('- [ ]','- [x]')
    text += f'\nValidation finale : 26 contrôles, {passed} tests workspace et {release} tests release. Sources et Cargo.lock identiques pendant les contrôles. Les critères sont couverts par les tests publics ciblés, les oracles indépendants et le [relevé complet](text-layout-validation.json). La [revue triée](text-layout-review.md) ne conserve aucun constat différé.\n'
    p.write_text(text,encoding='utf-8',newline='\n')
    p = ROOT/'docs'/document
    p.write_text(p.read_text(encoding='utf-8')+f'\nValidation du lot : [26 contrôles](../_bmad-output/implementation-artifacts/text-layout-validation.json), {passed} tests workspace et {release} tests release sur sources inchangées ; [revue indépendante triée](../_bmad-output/implementation-artifacts/text-layout-review.md), aucun constat différé.\n',encoding='utf-8',newline='\n')

p = ROOT/'US-V0-V1.md'
text = p.read_text(encoding='utf-8')
for number, _, document in stories:
    def close(match):
        block = match.group(1).replace('- [ ]','- [x]')
        return block.rstrip()+f'\n\nValidation US-{number} (2026-10-11) : [contrat et preuves](docs/{document}), [validation du lot](_bmad-output/implementation-artifacts/text-layout-validation.json).\n\n'
    text, count = re.subn(rf'(^#### US-{number} —.*?)(?=^#### |\Z)',close,text,flags=re.M|re.S)
    assert count == 1
headings = list(re.finditer(r'^#### US-\d+ —',text,re.M))
done = 0
for i,m in enumerate(headings):
    block = text[m.start():headings[i+1].start() if i+1<len(headings) else len(text)]
    flags = re.findall(r'^- \[([ x])\]',block,re.M)
    done += bool(flags) and all(f=='x' for f in flags)
assert len(headings) == 158 and done == 32,(len(headings),done)
text = text.replace('Validation US-055 (2026-10-11) :', 'Le registre US-055 accepte les octets système par API ; leur découverte et transfert par le pont natif restent couverts par US-113.\n\nValidation US-055 (2026-10-11) :')
p.write_text(text,encoding='utf-8',newline='\n')

p = ROOT/'docs/PROTOTYPE-PROGRESS.md'
text = p.read_text(encoding='utf-8').replace('US-010/011, US-051/053 et US-090/091','US-010/011/012, US-051/053/054/055 et US-090/091').replace('**29 US terminées sur 158**','**32 US terminées sur 158**')
start = text.index('Le démonstrateur US-120 reste à construire.')
end = text.index('\nLes [21 contrôles du lot précédent]',start)
text = text[:start]+'''Le démonstrateur US-120 reste à construire. Les coupures de ligne Unicode,
le façonnage latin, le registre de polices et l'historique sont disponibles.
Il reste le Worker (US-092), le démarrage de l'interface (US-101), le rendu Canvas
(US-103), la coque Windows et le pont natif (US-113), puis leurs interactions.
US-120 dépend de US-012/054/055/072/101/103/113 ; les quatre premières sont validées.
La connexion des fontes système au Host appartient à US-113. Il n'existe pas
encore d'interface permettant d'ouvrir et modifier un document.
''' + text[end:]
text = text.replace('Les [25 contrôles du lot]','Les [25 contrôles du lot US-011/053/091]')
text += f'''
US-012, US-054 et US-055 sont développées, relues et validées :

- [Coupures de ligne](us-012-line-breaks.md) : 19 338 cas officiels UAX14,
  les mêmes cas enrichis de marques LB9 et les largeurs EAW de tous les scalaires.
- [Façonnage latin](us-054-latin-shaping.md) : glyphes/clusters et positions
  déterministes, interlettrage signé, chasse, 54 716 références HarfBuzz sur vingt faces.
- [Gestionnaire de polices](us-055-font-manager.md) : vingt faces embarquées,
  cinq alias métriques, proximité CSS, sources/TTC, couverture et limites atomiques.
  Quatre variantes Arial réellement installées sont sélectionnées par l'API System.

Les [26 contrôles finaux](../_bmad-output/implementation-artifacts/text-layout-validation.json)
passent : **{passed} tests workspace et {release} tests release**, builds Windows/WASM/musl,
Clippy hôte/WASM, format, SPDX, reproductibilité et intégrité des ressources.
La taille WASM reste de 52 323 octets. Sources et lock sont identiques pendant
les contrôles ; les 32 paquets n'utilisent que des dépendances locales.
Les [trois revues Luna moyen](../_bmad-output/implementation-artifacts/text-layout-review.md)
ne laissent aucun constat différé. Le coordinateur a finalisé les corrections
après la limite d'usage des deux agents de développement ; les agents frais
de revue ont pu travailler. Les tests musl sont compilés localement et exécutés
par la CI Linux du commit publié.
'''
p.write_text(text,encoding='utf-8',newline='\n')

p = ROOT/'README.md'
text = p.read_text(encoding='utf-8').replace('préparent le [démonstrateur]','ainsi que les [coupures de ligne](docs/us-012-line-breaks.md), le [façonnage latin](docs/us-054-latin-shaping.md) et le [gestionnaire de polices](docs/us-055-font-manager.md) préparent le [démonstrateur]').replace('-p atelier-image --','-p atelier-image -p atelier-shape --').replace('`atelier-font` et `atelier-image` pour','`atelier-font`, `atelier-image` et `atelier-shape` pour')
p.write_text(text,encoding='utf-8',newline='\n')

p = ROOT/'docs/DEVELOPMENT.md'
text = p.read_text(encoding='utf-8')
entry = f'''### Preuve US-012, US-054 et US-055 — 2026-10-11

Les [coupures Unicode](us-012-line-breaks.md), le [façonnage latin](us-054-latin-shaping.md) et le [registre de polices](us-055-font-manager.md) portent le backlog à **32 US terminées sur 158**. Trois agents GPT-6 Luna moyen ont développé en parallèle ; le coordinateur a finalisé les corrections après leur limite d'usage, puis trois agents frais Luna moyen ont conduit les revues indépendantes.

Les [26 contrôles](../_bmad-output/implementation-artifacts/text-layout-validation.json) passent : {passed} tests workspace, {release} tests release, LineBreakTest complet et contextes enrichis, 54 716 comparaisons HarfBuzz, quatre fontes système réelles et 10 000 mutations sans panic. Les sources et le lock restent identiques pendant les contrôles, les 32 paquets restent locaux et le WASM mesure 52 323 octets. La [revue triée](../_bmad-output/implementation-artifacts/text-layout-review.md) conserve chacun des onze constats et sa résolution, sans report. Elle fournit les commandes de reproduction.

Le [suivi du démonstrateur](PROTOTYPE-PROGRESS.md) précise les travaux restants : Worker, interface, Canvas et pont natif. L'API de provenance System n'implémente pas le pont US-113. Les tests musl sont compilés sur Windows ; la CI Linux les exécute.

Le lot précédent (`6f4b0b34ab39d5a15a73d21f615d2cbf75c7edf6`) a réussi les trois jobs de la [CI GitHub](https://github.com/simkamil/mon-bureau/actions/runs/38093368641). Son [relevé archivé](../_bmad-output/implementation-artifacts/github-ci-text-bridge.json) conserve la preuve.

'''
text = text.replace('## Validation\n','## Validation\n\n'+entry,1)
p.write_text(text,encoding='utf-8',newline='\n')

p = ART/'text-layout-claims.md'
p.write_text(p.read_text(encoding='utf-8')+f'\nClôture du lot : 32 US terminées sur 158. Les 26 contrôles finaux passent, avec {passed} tests workspace et {release} tests release sur sources inchangées. La revue triée ne laisse aucun constat différé. Les contrats reproduits plus haut sont ceux reçus en revue ; leurs statuts historiques in-review précèdent cette clôture.\n',encoding='utf-8',newline='\n')
documentation_paths = [ROOT/'README.md',ROOT/'US-V0-V1.md',ROOT/'tests/README.md',ROOT/'docs/DEVELOPMENT.md',ROOT/'docs/PROTOTYPE-PROGRESS.md',
                       *(ROOT/'docs'/doc for _,_,doc in stories),*(ART/f'spec-us{number}-{suffix}.md' for number,suffix,_ in stories),ART/'text-layout-claims.md',ART/'text-layout-review.md']
record = {'date':'2026-10-11','validated_stories':32,'total_stories':158,'runtime_validation_sha256':hashlib.sha256(proof.read_bytes()).hexdigest(),
          'documentation_sha256':{p.relative_to(ROOT).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in documentation_paths}}
(ART/'text-layout-documentation.json').write_text(json.dumps(record,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
print(json.dumps({'done':done,'total':158,'checks':26,'workspace_tests':passed,'release_tests':release}))
