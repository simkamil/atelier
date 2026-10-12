# SPDX-License-Identifier: AGPL-3.0-or-later
"""Publish story status only after the recorded immutable-source validation passes."""
from pathlib import Path
import json

root = Path(__file__).resolve().parents[2]
art = Path(__file__).resolve().parent
validation = json.loads((art/'docx-rich-layout-validation.json').read_text(encoding='utf-8'))
assert validation['passed'] and validation['source_bytes_unchanged'] and validation['lock_bytes_unchanged']
count = next(c['passed'] for c in validation['checks'] if c['name'] == 'windows_tests')
checks = len(validation['checks'])

def write(path, contents):
    (root/path).write_text(contents, encoding='utf-8', newline='\n')

def replace_once(path, old, new):
    contents = (root/path).read_text(encoding='utf-8')
    if new in contents:
        return
    assert contents.count(old) == 1, (path, old)
    write(path, contents.replace(old, new, 1))

review = (art/'docx-rich-layout-review.md').read_text(encoding='utf-8')
table = '\n'.join(line for line in review.splitlines() if line.startswith('|'))
for name, status in [('spec-us301-docx-text-styles.md', 'done'), ('spec-us210-rich-line-layout.md', 'in-progress')]:
    path = art/name
    text = path.read_text(encoding='utf-8').replace("status: 'in-review'", f"status: '{status}'", 1)
    marker = '## Review Triage Log\n'
    start = text.index(marker) + len(marker)
    end = text.index('\n## ', start)
    text = text[:start] + '\nTrois revues indépendantes Luna moyen puis revue ciblée des correctifs. Tous les constats sont conservés séparément; décisions et preuves détaillées dans [le reçu](docx-rich-layout-review.md).\n\n' + table + '\n' + text[end:]
    text += f'\n### Validation finale — 2026-10-12\n\n{checks} contrôles réussis, {count} tests workspace; Windows/WASM/musl, Clippy hôte/WASM, format, licences, garde et fixture indépendante. Sources et lock inchangés pendant la passe. Reçu : `docx-rich-layout-validation.json`. US-301 est validée au périmètre API; US-210 reste partielle, seuil Word ≥95 % ouvert. Les resets couleur/surlignage non représentables sont préservés et signalés, sans fidélité visuelle revendiquée.\n'
    path.write_text(text, encoding='utf-8', newline='\n')

backlog = root/'US-V0-V1.md'
text = backlog.read_text(encoding='utf-8')
for story, next_story in [('210', '211'), ('301', '302')]:
    begin = text.index(f'#### US-{story} ')
    end = text.index(f'#### US-{next_story} ', begin)
    section = text[begin:end]
    if story == '210':
        for criterion in ['Coupure gloutonne', 'Alignements gauche', 'Retraits gauche', 'Interlignes simple', 'Mélange de tailles']:
            section = section.replace('- [ ] ' + criterion, '- [x] ' + criterion)
        note = f'Validation API : [US-210](docs/us-210-rich-line-layout.md), {count} tests workspace et {checks} contrôles dans le [reçu](\u005fbmad-output/implementation-artifacts/docx-rich-layout-validation.json). **US partielle** : Microsoft Word absent de cet hôte, aucun taux de comparaison établi. Le collecteur est préparatoire; raccordement à l’éditeur riche séparé.\n\n'
    else:
        section = section.replace('- [ ]', '- [x]')
        note = 'Validation API : [contrat et limites US-301](docs/us-301-docx-import.md), fixture indépendante Python, désobfuscation exacte de Carlito et import vers le moteur de lignes. Les parties inconnues restent conservées; les propriétés non représentées sont signalées. [Revue et correctifs](\u005fbmad-output/implementation-artifacts/docx-rich-layout-review.md), [validation](\u005fbmad-output/implementation-artifacts/docx-rich-layout-validation.json). Le raccordement à l’interface et l’export sémantique sont des travaux ultérieurs.\n\n'
    section = section.replace('\nRéf :', '\n' + note + 'Réf :', 1)
    text = text[:begin] + section + text[end:]
write('US-V0-V1.md', text)

section = f'''## Import DOCX et lignes riches — 2026-10-12

Deux agents GPT-6 Luna, raisonnement moyen, ont développé les API en parallèle,
puis trois agents indépendants ont conduit la revue. **US-301 est validée**;
le backlog atteint **43 US terminées sur 158**. **US-210 reste partielle** :
les cinq critères fonctionnels sont testés, mais le seuil de 95 % de coupures
identiques à Word reste ouvert, Word étant absent de cet hôte.

- [Import DOCX](us-301-docx-import.md) : texte, propriétés, styles/thème,
  paramètres, styles latents, fontTable et polices ODTTF réellement utilisables.
  Les valeurs non représentées par le modèle sont préservées et signalées.
- [Lignes riches](us-210-rich-line-layout.md) : coupures UAX14, quatre alignements,
  retraits, interlignes, tailles/polices mixtes, clusters et géométrie bornée.
- [Fixture indépendante](../tests/docx-reference/README.md) : parties relocalisées,
  vraie police Carlito, vecteur ODTTF et paquet opaque; import vers layout testé.

La [validation finale](../_bmad-output/implementation-artifacts/docx-rich-layout-validation.json)
réussit **{checks} contrôles et {count} tests workspace** : builds Windows/WASM/musl,
Clippy hôte/WASM, format, licences, garde, métadonnées et reproduction de fixture.
Sources et lock sont inchangés pendant les contrôles; les 32 paquets et toutes
leurs dépendances restent locaux. Les tests musl sont compilés localement;
leur exécution appartient à la CI Linux du commit publié. Le moteur embarqué
reste à 1 179 080 octets, avec le même plafond; sa provenance inclut le nouveau lock.

La [revue](../_bmad-output/implementation-artifacts/docx-rich-layout-review.md)
conserve 19 constats séparés, leurs réfutations ou corrections, et une relecture
ciblée des correctifs. La [première passe](../_bmad-output/implementation-artifacts/docx-rich-layout-first-attempt.json)
conserve l’arrêt Clippy sur une assertion booléenne corrigée; les échecs de
développement restent dans `docx-rich-layout-exploration.json`.
Les resets de couleur/surlignage nécessitent encore une représentation dans
le modèle partagé; ils produisent une notice explicite en attendant.

Le [collecteur Word](../tests/text-layout-reference/README.md) est préparatoire,
sans référence collectée ni résultat ≥95 % revendiqué. La [sonde COM](../_bmad-output/implementation-artifacts/us-210-word-com-check.json)
confirme l’absence de Word; le démarrage PowerShell bloqué est tracé séparément.
Ces API ne sont pas encore raccordées à l’éditeur riche : le parcours utilisateur
actuel reste TXT. La [carte Graphify](../_bmad-output/implementation-artifacts/docx-rich-layout-graph.json)
contrôle les dépendances et les statuts du lot.

'''
for path in ['docs/DEVELOPMENT.md', 'docs/PROTOTYPE-PROGRESS.md']:
    contents = (root/path).read_text(encoding='utf-8')
    anchor = '## Styles, OPC, JPEG et raster logiciel — 2026-10-12\n'
    if '## Import DOCX et lignes riches —' not in contents:
        contents = contents.replace(anchor, section+anchor, 1)
    if path.endswith('PROTOTYPE-PROGRESS.md'):
        contents = contents.replace('Le backlog compte désormais **42 US terminées sur 158**. Le dernier lot ajoute\nles styles, OPC, JPEG et le rasteriseur logiciel, développés en parallèle avec\nGPT-6 Luna, raisonnement moyen, puis relus et validés.',
            'Le backlog compte désormais **43 US terminées sur 158**. Le dernier lot livre\nl’import DOCX US-301 et le code de mise en page US-210, développé en parallèle\navec GPT-6 Luna, raisonnement moyen. US-210 reste partielle pour la preuve Word.')
    else:
        contents = contents.replace('Le raccordement de ces API à l\'éditeur riche et l\'import sémantique DOCX restent à développer.',
            'L’[import DOCX](us-301-docx-import.md) et les [lignes riches](us-210-rich-line-layout.md) sont disponibles en API; US-210 attend la comparaison Word. Le raccordement à l’éditeur riche reste à développer.')
    contents = contents.replace('Ces modules exposent des API de bibliothèque. L’import WordprocessingML\n(US-301), le backend raster de liste d’affichage (US-082) et leur raccordement\nà l’éditeur restent à développer.',
        'À la date de ce lot, l’import WordprocessingML restait à développer; il est\nmaintenant livré par US-301 ci-dessus. Le backend raster de liste d’affichage\n(US-082) et le raccordement à l’éditeur riche restent séparés.')
    write(path, contents)

replace_once('README.md', 'L’import DOCX, le backend de liste d’affichage raster et leur raccordement à l’éditeur restent des US distinctes.',
    'L’[import DOCX](docs/us-301-docx-import.md) fournit désormais texte, styles, thème, paramètres et polices incorporées. Le [moteur de lignes riches](docs/us-210-rich-line-layout.md) est développé et testé; US-210 attend la comparaison avec Word. Le backend de liste d’affichage raster et le raccordement à l’éditeur riche restent à développer. Le [suivi](docs/PROTOTYPE-PROGRESS.md) compte 43/158 US complètes.')

contents = (root/'tests/README.md').read_text(encoding='utf-8')
contents += '''
## DOCX et lignes riches

Le [corpus DOCX indépendant](docx-reference/README.md) est généré et relu par
Python ZIP/XML, avec un vrai Carlito ODTTF, des parties relocalisées et une partie
opaque. `py tools/generate-docx-reference.py --check` contrôle ses octets exacts;
les jobs Cargo consomment les fixtures versionnées sans dépendance Python.
`cargo test -p atelier-fmt-ooxml -p atelier-text-layout --locked --offline`
exerce import, contrôles malformés, styles, polices et géométrie riche intégrée.

Le [collecteur de lignes Word](text-layout-reference/README.md) nécessite Word
desktop. Sa collecte reste non exécutée ici; aucun golden Word ni taux ≥95 %
n’est simulé. US-210 demeure partielle. Les tests numériques et les références
HarfBuzz existantes ne remplacent pas cet oracle indépendant de mise en page.
'''
write('tests/README.md', contents)

with (art/'deferred-work.md').open('a', encoding='utf-8', newline='\n') as f:
    f.write('''
- source_spec: `_bmad-output/implementation-artifacts/spec-us301-docx-text-styles.md`
  summary: Représenter la réinitialisation explicite de couleur et surlignage dans le modèle riche et sa cascade.
  evidence: R6 de docx-rich-layout-review.md; direct highlight none conserve actuellement le surlignage hérité, avec marqueur opaque et notice UnsupportedProperty. Un changement coordonné du modèle/cascade/codec et un test d’intégration de rendu sont nécessaires.
''')
print(json.dumps({'validation_checks': checks, 'workspace_tests': count,
                  'US-301': 'done', 'US-210': 'partial: Word criterion open', 'done': 43, 'total': 158}))
