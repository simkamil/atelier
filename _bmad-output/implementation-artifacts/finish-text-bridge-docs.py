# SPDX-License-Identifier: AGPL-3.0-or-later
"""Close only stories proved by the immutable final validation snapshot."""
from pathlib import Path
import hashlib
import json
import re

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
evidence = json.loads((ART/'text-bridge-validation.json').read_text(encoding='utf-8'))
assert len(evidence['checks']) == 25 and all(c['exit_code'] == 0 for c in evidence['checks'])
assert evidence['source_bytes_unchanged'] and evidence['lock_bytes_unchanged']
assert all(hashlib.sha256((ROOT/p).read_bytes()).hexdigest() == h for p,h in evidence['source_sha256'].items())
checks = {c['name']: c for c in evidence['checks']}
assert checks['metadata']['package_count'] == 32
assert checks['metadata']['all_members_local'] and checks['metadata']['no_external_dependencies']
mutations = json.loads((ART/'text-bridge-layout-mutations.json').read_text(encoding='utf-8'))
assert mutations['cases'] == 10000 and mutations['panics'] == 0
reference = json.loads((ROOT/'tests/font-layout-reference/manifest.json').read_text(encoding='utf-8'))
assert reference['cases'] == 54716 and len(reference['faces']) == 20
review = (ART/'text-bridge-review.md').read_text(encoding='utf-8')
assert 'Aucun constat différé' in review
triage_rows = [line for line in review.splitlines() if re.match(r'^\| [BE]\d+ \|', line)]
assert len(triage_rows) == 13
triage_ids = {'011':{'B7','B9'}, '053':{'B1','B2','B3','B4','B5','B6','B8','B9','B10','E1','E2'}, '091':{'B9','E3'}}
passed = checks['windows_tests']['passed']
release = checks['text_bridge_release_tests']['passed']
for number, filename in [('011','spec-us011-segmentation.md'),('053','spec-us053-kerning-ligatures.md'),('091','spec-us091-binary-protocol.md')]:
    path = ART/filename
    text = path.read_text(encoding='utf-8')
    assert "status: 'in-review'" in text
    text = text.replace("status: 'in-review'", "status: 'done'").replace('- [ ]', '- [x]')
    selected_rows = [row for row in triage_rows if row.split('|')[1].strip() in triage_ids[number]]
    triage = '| ID | Constat | Verdict | Vérification et décision |\n|---|---|---|---|\n'+'\n'.join(selected_rows)
    text = text.replace('## Review Triage Log\n\n', '## Review Triage Log\n\n'+triage+'\n\nChaque constat est classé avant regroupement ; les patches internes sont validés par les contrôles finaux. La [revue commune](text-bridge-review.md) conserve les treize constats et les contrôles supplémentaires du coordinateur.\n\n')
    text += f'\nValidation finale : 25 contrôles, {passed} tests workspace, {release} tests release. Les sources et le lock sont restés identiques pendant les contrôles. Voir [relevé](text-bridge-validation.json) et [revue triée](text-bridge-review.md). Aucun constat différé.\n'
    path.write_text(text, encoding='utf-8', newline='\n')
backlog = ROOT/'US-V0-V1.md'
text = backlog.read_text(encoding='utf-8')
for number in ('011','053','091'):
    text, count = re.subn(rf'(^#### US-{number} —.*?)(?=^#### |\Z)', lambda m:m.group(1).replace('- [ ]','- [x]'), text, flags=re.M|re.S)
    assert count == 1
headings = list(re.finditer(r'^#### US-\d+ —', text, re.M))
done = 0
for i, heading in enumerate(headings):
    block = text[heading.start():headings[i+1].start() if i+1<len(headings) else len(text)]
    flags = re.findall(r'^- \[([ x])\]', block, re.M)
    done += bool(flags) and all(f == 'x' for f in flags)
assert len(headings) == 158 and done == 29, (len(headings), done)
backlog.write_text(text, encoding='utf-8', newline='\n')

progress = ROOT/'docs/PROTOTYPE-PROGRESS.md'
text = progress.read_text(encoding='utf-8')
text = text.replace('US-010, US-051 et US-090 ont été développées en parallèle avec GPT-6 Luna,', 'US-010/011, US-051/053 et US-090/091 ont été développées en parallèle avec GPT-6 Luna,')
text = text.replace('# Préparation du démonstrateur — 2026-10-10', '# Préparation du démonstrateur — 2026-10-11')
text = text.replace('**26 US terminées sur 158**', '**29 US terminées sur 158**')
text = text.replace('Les [21 contrôles finaux]', 'Les [21 contrôles du lot précédent]')
start = text.index('Le démonstrateur US-120 reste à construire.')
end = text.index('\nLes [21 contrôles finaux]', start)
text = text[:start]+'''Le démonstrateur US-120 reste à construire. Les dépendances restantes sont
les coupures de ligne Unicode (US-012), le façonnage latin (US-054),
le gestionnaire de polices (US-055), le Worker (US-092), puis la coque Windows,
les fichiers, les entrées clavier/IME et le rendu Canvas.
US-120 dépend de US-012/054/055/072/101/103/113 ; US-072 est déjà validée.
Il n’existe pas encore d’interface permettant d’ouvrir et modifier un document.
''' + text[end:]
text += f'''
US-011, US-053 et US-091 ont été développées en parallèle avec GPT-6 Luna moyen,
puis relues et validées :

- [Segmentation](us-011-segmentation.md) : 766 cas GraphemeBreakTest et
  1 944 cas WordBreakTest officiels, navigation à chaque position UTF-8,
  itération inverse et longues suites de contexte en temps linéaire.
- [Crénage et ligatures](us-053-kerning-ligatures.md) : GSUB liga,
  GPOS kern et repli kern historique, clusters conservés et budgets cumulés.
  Les 54 716 [références HarfBuzz](../tests/font-layout-reference/README.md)
  concordent sur vingt faces ; 10 000 mutations ne déclenchent aucune panique.
- [Protocole binaire](us-091-binary-protocol.md) : dix messages versionnés,
  IDs u64/BigInt, codecs Rust/JS croisés, golden frames, erreurs et capacité
  maximale exacte. L'ABI transporte les trames par écho ; ses handlers
  métier et le Worker restent à développer.

Les [25 contrôles du lot](../_bmad-output/implementation-artifacts/text-bridge-validation.json)
passent, avec **{passed} tests workspace et {release} tests release**,
builds Windows/WASM/musl, Clippy hôte/WASM, format, SPDX et ressources.
La taille WASM reste de 52 323 octets. Les sources et le lock restent identiques
pendant les contrôles, et les 32 paquets gardent des dépendances locales.
La [revue complète](../_bmad-output/implementation-artifacts/text-bridge-review.md)
ne laisse aucun constat différé. Les tests musl sont compilés localement ;
leur exécution appartient à la CI Linux du commit publié.
'''
progress.write_text(text, encoding='utf-8', newline='\n')
development = ROOT/'docs/DEVELOPMENT.md'
text = development.read_text(encoding='utf-8')
text = text.replace('## Validation\n', '## Validation\n\n'+f'''### Preuve US-011, US-053 et US-091 — 2026-10-11

La [segmentation Unicode](us-011-segmentation.md), le [crénage et les ligatures](us-053-kerning-ligatures.md) et le [protocole Rust/JS](us-091-binary-protocol.md) sont développés et validés. Le backlog compte **29 US terminées sur 158**. Les trois implémentations et trois revues indépendantes utilisent GPT-6 Luna moyen.

Les [25 contrôles](../_bmad-output/implementation-artifacts/text-bridge-validation.json) passent : {passed} tests workspace et {release} tests release, corpus Unicode complets, 54 716 sorties HarfBuzz, échanges croisés des dix types, limites et 10 000 mutations sans panic. Les sources et le lock restent identiques pendant la validation. Les 32 paquets restent locaux, et l'ABI optimisée mesure toujours 52 323 octets. La [revue triée](../_bmad-output/implementation-artifacts/text-bridge-review.md) conserve chaque constat et sa résolution.

Le [suivi du démonstrateur](PROTOTYPE-PROGRESS.md) précise les dépendances encore ouvertes. La CI Linux exécute les tests musl, compilés seulement sur l'hôte Windows local.

Le lot précédent (`76efdfee2124112eef5ceee21d67422b062efa80`) a réussi les trois jobs de la [CI GitHub](https://github.com/simkamil/mon-bureau/actions/runs/38089712085), dont l'exécution musl. Son [relevé archivé](../_bmad-output/implementation-artifacts/github-ci-prototype-foundations.json) en conserve la preuve.

''', 1)
development.write_text(text, encoding='utf-8', newline='\n')
readme = ROOT/'README.md'
text = readme.read_text(encoding='utf-8').replace('préparent le [démonstrateur]', 'ainsi que la [segmentation](docs/us-011-segmentation.md), le [crénage/ligatures](docs/us-053-kerning-ligatures.md) et le [protocole Rust/JS](docs/us-091-binary-protocol.md) préparent le [démonstrateur]')
readme.write_text(text, encoding='utf-8', newline='\n')
claims = ART/'text-bridge-claims.md'
text = claims.read_text(encoding='utf-8').replace(
    'The backlog remains\nat 26 validated stories until this batch finishes review and full verification.',
    'The backlog now contains 29 validated stories out of 158. All 25 local\nchecks passed after the corrections, with no deferred review findings.')
text = text.replace('Final status requires the complete\nvalidation snapshot, not only targeted development tests.',
                    'The complete validation snapshot confirms the final\nimplementation on unchanged sources, beyond the targeted development tests.')
claims.write_text(text, encoding='utf-8', newline='\n')
documentation_paths = [ROOT/'README.md', ROOT/'US-V0-V1.md', ROOT/'tests/README.md', ROOT/'web/README.md',
                       ROOT/'docs/DEVELOPMENT.md', ROOT/'docs/PROTOTYPE-PROGRESS.md',
                       *(ROOT/'docs'/name for name in ('us-011-segmentation.md', 'us-053-kerning-ligatures.md', 'us-091-binary-protocol.md')),
                       *(ART/name for name in ('spec-us011-segmentation.md', 'spec-us053-kerning-ligatures.md', 'spec-us091-binary-protocol.md', 'text-bridge-claims.md', 'text-bridge-review.md'))]
documentation_evidence = {'date':'2026-10-11', 'validated_stories':29, 'total_stories':158,
                          'runtime_validation_sha256':hashlib.sha256((ART/'text-bridge-validation.json').read_bytes()).hexdigest(),
                          'documentation_sha256':{p.relative_to(ROOT).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in documentation_paths}}
(ART/'text-bridge-documentation.json').write_text(json.dumps(documentation_evidence, ensure_ascii=False, indent=2)+'\n', encoding='utf-8', newline='\n')
print(json.dumps({'done':done,'total':len(headings),'workspace_tests':passed,'release_tests':release,'checks':25}))
