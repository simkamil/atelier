# SPDX-License-Identifier: AGPL-3.0-or-later
"""Close only the three stories supported by the final validation snapshot."""
from pathlib import Path
import hashlib
import json
import re

ROOT=Path(__file__).resolve().parents[2]
ART=Path(__file__).resolve().parent
evidence=json.loads((ART/'prototype-foundations-validation.json').read_text(encoding='utf-8'))
assert len(evidence['checks'])==21 and all(c['exit_code']==0 for c in evidence['checks'])
assert evidence['source_bytes_unchanged'] and evidence['lock_bytes_unchanged']
assert all(hashlib.sha256((ROOT/p).read_bytes()).hexdigest()==h for p,h in evidence['source_sha256'].items())
checks={c['name']:c for c in evidence['checks']}
assert checks['metadata']['package_count']==32
assert checks['metadata']['all_members_local'] and checks['metadata']['no_external_dependencies']
passed=checks['windows_tests']['passed']
release=checks['foundation_release_tests']['passed']
assert json.loads((ART/'prototype-ucd-oracle.json').read_text(encoding='utf-8'))['all_equal']
assert json.loads((ART/'prototype-font-oracle.json').read_text(encoding='utf-8'))['geometry_matches']
assert json.loads((ART/'prototype-outline-raster.json').read_text(encoding='utf-8'))['maximum_channel_difference']==0
assert json.loads((ART/'prototype-outline-mutations.json').read_text(encoding='utf-8'))['panics']==0
assert json.loads((ART/'prototype-ucd-negative.json').read_text(encoding='utf-8'))['all_passed']

review_path=ART/'prototype-foundations-review.md'
review=review_path.read_text(encoding='utf-8').replace('Les corrections et leur validation finale restent à confirmer dans le relevé\n`prototype-foundations-validation.json`. Aucun constat n’est différé.',f'Les corrections passent les 21 contrôles finaux, dont {passed} tests workspace\net {release} tests release, dans `prototype-foundations-validation.json`. Aucun constat\nn’est différé. Les tests musl sont compilés localement ; leur exécution est assurée\npar le job Linux de la CI du commit publié.')
review_path.write_text(review,encoding='utf-8',newline='\n')
rows=[line for line in review.splitlines() if re.match(r'\| (?:B\d+|E\d+|V\d+|R\d+) \|',line)]
selected={10:{'B1','B2','B3','B4','B5','B6','B7','E1','V1'},51:{'B8','B9'},90:{'B10','R1','R2'}}
for number,filename in [(10,'spec-us010-ucd-tables.md'),(51,'spec-us051-truetype-outlines.md'),(90,'spec-us090-wasm-abi.md')]:
    path=ART/filename
    text=path.read_text(encoding='utf-8').replace("status: 'in-review'","status: 'done'")
    assert "status: 'done'" in text
    text=text.replace('- [ ]','- [x]')
    selected_rows=[line for line in rows if line.split('|')[1].strip() in selected[number]]
    text += '\n\n| ID | Constat | Verdict et preuve | Disposition |\n|---|---|---|---|\n'+'\n'.join(selected_rows)+'\n\n'
    text += f'Validation finale : 21 contrôles, {passed} tests workspace et {release} tests release. Sources et lock inchangés durant les contrôles. Voir [relevé](prototype-foundations-validation.json) et [revue complète](prototype-foundations-review.md). Aucun constat différé.\n'
    path.write_text(text,encoding='utf-8',newline='\n')

backlog=ROOT/'US-V0-V1.md'
text=backlog.read_text(encoding='utf-8')
for number in ('010','051','090'):
    pattern=rf'(^#### US-{number} —.*?)(?=^#### |\Z)'
    text,count=re.subn(pattern,lambda match:match.group(1).replace('- [ ]','- [x]'),text,flags=re.M|re.S)
    assert count==1
headings=list(re.finditer(r'^#### US-\d+ —',text,re.M))
done=0
for index,heading in enumerate(headings):
    block=text[heading.start():headings[index+1].start() if index+1<len(headings) else len(text)]
    flags=re.findall(r'^- \[([ x])\]',block,re.M)
    done+=bool(flags) and all(f=='x' for f in flags)
assert len(headings)==158 and done==26,(len(headings),done)
backlog.write_text(text,encoding='utf-8',newline='\n')

progress=ROOT/'docs/PROTOTYPE-PROGRESS.md'
text=progress.read_text(encoding='utf-8').replace('raisonnement moyen. Les validations et revues sont en cours ; le backlog reste\nà 23 US terminées sur 158 jusqu’à leur clôture.','raisonnement moyen, puis relues et validées. Le backlog compte désormais\n**26 US terminées sur 158**.')
text += f'\nLes [21 contrôles finaux](../_bmad-output/implementation-artifacts/prototype-foundations-validation.json)\npassent : {passed} tests workspace, {release} tests release, builds Windows/WASM/musl,\ncompilation des tests musl, Clippy hôte/WASM, format, SPDX, garde du lock,\nreproductibilité, intégrité des ressources, tests Node et suivi de taille.\nLes 32 paquets et toutes leurs dépendances sont locaux. Les [trois revues](../_bmad-output/implementation-artifacts/prototype-foundations-review.md)\nne laissent aucun constat différé. Les sources et le lock ne changent pas\npendant les contrôles ; la CI publiée exécute ensuite les tests sur Linux musl.\n'
progress.write_text(text,encoding='utf-8',newline='\n')

development=ROOT/'docs/DEVELOPMENT.md'
text=development.read_text(encoding='utf-8').replace('le client web reste un emplacement documenté.','le client web dispose du chargeur ABI WASM, tandis que son interface reste à développer.')
text=text.replace('## Validation\n','## Validation\n\n'+f'''### Preuve US-010, US-051 et US-090 — 2026-10-10

Les [tables Unicode](us-010-ucd-tables.md), les [contours TrueType](us-051-truetype-outlines.md) et l’[ABI WASM manuelle](us-090-wasm-abi.md) ont été développés en parallèle avec GPT-6 Luna moyen, relus par trois agents indépendants et validés. Le [suivi du démonstrateur](PROTOTYPE-PROGRESS.md) rassemble les résultats et les dépendances restantes. Le backlog compte **26 US terminées sur 158**.

**{passed} tests workspace et {release} tests release passent** dans les [21 contrôles locaux](../_bmad-output/implementation-artifacts/prototype-foundations-validation.json). Les 32 paquets restent locaux ; les nouveaux liens sont Unicode→générateur en build et font→geom. Les sources et le lock restent identiques pendant les contrôles. Le job WASM exerce les vrais exports dans Node, le panic/reset et la taille optimisée ; le job Linux exécute les tests musl, compilés seulement sur l’hôte Windows local.

L’[oracle Unicode indépendant](../_bmad-output/implementation-artifacts/prototype-ucd-oracle.json) compare 1 112 064 scalaires, quinze propriétés et dix mappings par scalaire. Un test Rust exhaustif compare les empreintes officielles en CI. FontTools valide [43 576 glyphes](../_bmad-output/implementation-artifacts/prototype-font-oracle.json), et [300 rendus](../_bmad-output/implementation-artifacts/prototype-outline-raster.json) concordent pixel par pixel. Les 20 000 mutations de fontes passent sans panic ; neuf erreurs CLI Unicode laissent les sorties intactes. La [revue triée](../_bmad-output/implementation-artifacts/prototype-foundations-review.md) ne laisse aucun constat différé.

### Validation distante du lot images et polices — 2026-10-10

Le commit `20a6a05bd69d22f4f004b7a907d2c9696b24c4ad` a réussi les trois jobs de la [CI GitHub](https://github.com/simkamil/mon-bureau/actions/runs/38076202790). Son [relevé archivé](../_bmad-output/implementation-artifacts/github-ci-images-fonts.json) conserve la preuve du lot précédent.

''',1)
development.write_text(text,encoding='utf-8',newline='\n')
print(json.dumps({'done_stories':done,'total_stories':len(headings),'workspace_tests':passed,'release_tests':release,'checks':21}))
