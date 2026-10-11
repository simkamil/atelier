# SPDX-License-Identifier: AGPL-3.0-or-later
"""Close validated Worker and shell stories while retaining immutable evidence."""
from pathlib import Path
import hashlib
import json
import re

ROOT = Path(__file__).resolve().parents[2]
ART = Path(__file__).resolve().parent
proof = ART/'worker-shell-validation.json'
evidence = json.loads(proof.read_text(encoding='utf-8'))
assert len(evidence['checks']) == 27 and all(c['exit_code'] == 0 for c in evidence['checks'])
assert evidence['source_bytes_unchanged'] and evidence['lock_bytes_unchanged']
assert all(hashlib.sha256((ROOT/p).read_bytes()).hexdigest() == h for p,h in evidence['source_sha256'].items())
checks = {c['name']:c for c in evidence['checks']}
assert checks['metadata']['package_count'] == 32 and checks['metadata']['no_external_dependencies']
review = (ART/'worker-shell-review.md').read_text(encoding='utf-8')
assert 'Aucun constat différé' in review
passed = checks['windows_tests']['passed']
release = checks['shell_release_tests']['passed']
stories = [('110','win32-window','us-110-win32-window.md'),('116','portable-data','us-116-portable-data.md')]
for number, suffix, document in stories:
    path = ART/f'spec-us{number}-{suffix}.md'
    text = path.read_text(encoding='utf-8')
    assert "status: 'in-review'" in text
    text = text.replace("status: 'in-review'", "status: 'done'").replace('- [ ]', '- [x]')
    text += f'\nValidation finale : 27 contrôles, {passed} tests workspace et {release} tests shell release. Sources et lock identiques pendant les contrôles. Voir [preuve](worker-shell-validation.json) et [revue triée](worker-shell-review.md), aucun constat différé.\n'
    path.write_text(text, encoding='utf-8', newline='\n')
    path = ROOT/'docs'/document
    text = path.read_text(encoding='utf-8')
    text += f'\nValidation du lot : [27 contrôles](../_bmad-output/implementation-artifacts/worker-shell-validation.json), {passed} tests workspace et {release} tests shell release sur sources inchangées ; [revue indépendante](../_bmad-output/implementation-artifacts/worker-shell-review.md). Les tests musl sont compilés localement et exécutés en CI Linux.\n'
    path.write_text(text, encoding='utf-8', newline='\n')

path = ART/'spec-us092-worker.md'
text = path.read_text(encoding='utf-8')
assert "status: 'in-review'" in text
text = text.replace("status: 'in-review'", "status: 'in-progress'").replace('- [ ]', '- [x]')
text += '\nLe transport est implémenté et validé par les contrôles du lot. US-092 reste ouverte : le critère backlog de progression/annulation d’une ouverture ou d’un export métier attend les handlers documentaires. Les copies coopératives seules ne le clôturent pas.\n'
path.write_text(text,encoding='utf-8',newline='\n')
path = ROOT/'docs/us-092-worker.md'
text = path.read_text(encoding='utf-8')
text += '\nStatut US-092 : transport vérifié, US encore ouverte. Le dispatch WASM reste un écho ; aucune ouverture/export documentaire n’est validée. Les copies par blocs progressent et s’annulent réellement, mais les handlers métier doivent encore offrir des points de coopération avant de clôturer le quatrième critère. Le [banc navigateur](../tests/worker-browser.html) est exécuté par `node tests/run-worker-browser.mjs` dans Edge headless en local et en CI, sans isolation SharedArrayBuffer. Il couvre le démarrage Worker ES par défaut, les octets/IDs, FIFO, annulation et reprise. WebView2 reste à intégrer.\n'
path.write_text(text,encoding='utf-8',newline='\n')
path = ROOT/'US-V0-V1.md'
text = path.read_text(encoding='utf-8')
def partial_worker(match):
    block = match.group(1)
    block = block.replace('- [ ]', '- [x]', 3)
    return block.rstrip()+'\n\nUS-092 en cours (2027-10-11) : [transport et limites](docs/us-092-worker.md). Progression/annulation des copies vérifiées ; le critère opération métier ouverture/export reste ouvert.\n\n'
text,count = re.subn(r'(^#### US-092 —.*?)(?=^#### |\Z)',partial_worker,text,flags=re.M|re.S)
assert count == 1
for number, _, document in stories:
    def close(match):
        return match.group(1).rstrip().replace('- [ ]','- [x]')+f'\n\nValidation US-{number} (2027-10-11) : [contrat et limites](docs/{document}), [preuve du lot](_bmad-output/implementation-artifacts/worker-shell-validation.json).\n\n'
    text,count = re.subn(rf'(^#### US-{number} —.*?)(?=^#### |\Z)',close,text,flags=re.M|re.S)
    assert count == 1
headings = list(re.finditer(r'^#### US-\d+ —',text,re.M))
done = 0
for i,heading in enumerate(headings):
    block = text[heading.start():headings[i+1].start() if i+1<len(headings) else len(text)]
    flags = re.findall(r'^- \[([ x])\]',block,re.M)
    done += bool(flags) and all(f=='x' for f in flags)
assert len(headings)==158 and done==34,(len(headings),done)
path.write_text(text,encoding='utf-8',newline='\n')

path = ROOT/'docs/PROTOTYPE-PROGRESS.md'
text = path.read_text(encoding='utf-8').replace('**32 US terminées sur 158**','**34 US terminées sur 158**',1)
start = text.index('Le démonstrateur US-120 reste à construire.')
end = text.index('\nLes [21 contrôles du lot précédent]',start)
text = text[:start]+'''Le démonstrateur US-120 reste à construire. Le socle texte, le Worker,
la fenêtre Win32 et la résolution portable sont disponibles. La coque Windows
peut être lancée avec `cargo run -p atelier-desktop-win --locked --offline` ;
elle affiche une fenêtre native vide et mémorise sa géométrie.
Il reste WebView2 (US-111), ses ressources et le pont natif (US-112/113),
le Canvas (US-101), la saisie clavier/IME (US-103) et leur intégration US-120.
US-092 reste ouverte : le Worker transfère réellement les buffers avec progression et annulation
pendant les copies par blocs ; le dispatch WASM synchrone n'est pas préemptible
et conserve son écho ABI. Les handlers métier d'ouverture/export restent à intégrer.
Il n'existe pas encore d'interface pour ouvrir et modifier un document.
''' + text[end:]
text += f'''
US-110 et US-116 sont développées, relues et validées ; le transport de US-092
est implémenté et vérifié en parallèle avec GPT-6 Luna moyen :

- [Worker](us-092-worker.md) : FIFO, IDs u64, buffers transférés et détachés,
  mémoire WASM réutilisable, annulation et progression réelles des copies.
- [Fenêtre Windows](us-110-win32-window.md) : boucle native, Per-Monitor V2,
  thème système et géométrie persistante, tests de vrais HWND cachés.
- [Stockage portable](us-116-portable-data.md) : une racine commune,
  choix local explicite en cas d'échec et arrêt avant tout cache inaccessible.

Les [27 contrôles](../_bmad-output/implementation-artifacts/worker-shell-validation.json)
passent, avec **{passed} tests workspace et {release} tests shell release**.
Les sources et le lock sont restés identiques pendant la validation.
La [revue indépendante](../_bmad-output/implementation-artifacts/worker-shell-review.md)
ne laisse aucun constat différé dans le transport livré et les deux US Windows. Le multi-écran synthétique ne remplace pas un
essai sur plusieurs écrans physiques ; les tests natifs n'affichent pas de fenêtre.
'''
path.write_text(text,encoding='utf-8',newline='\n')

path = ROOT/'web/README.md'
text = path.read_text(encoding='utf-8').replace('Le client, son Worker et l’interface d’édition restent à développer.', 'Le [client et son Worker](../docs/us-092-worker.md) exécutent le transport WASM hors du fil principal. L’interface d’édition reste à développer.')
path.write_text(text,encoding='utf-8',newline='\n')
path = ROOT/'README.md'
text = path.read_text(encoding='utf-8')
text += '''
La [fenêtre Win32](docs/us-110-win32-window.md) et le [stockage portable](docs/us-116-portable-data.md)
sont disponibles : `cargo run -p atelier-desktop-win --locked --offline` démarre
la coque native vide. Le [Worker WASM](docs/us-092-worker.md) assure le transport
hors du fil principal. WebView2, le Canvas et la saisie restent nécessaires au démonstrateur.
'''
path.write_text(text,encoding='utf-8',newline='\n')
path = ROOT/'tests/README.md'
text = path.read_text(encoding='utf-8')
text += '''
Les tests [Worker](wasm-worker.mjs) utilisent un vrai Worker Node et le vrai WASM :
FIFO, octets exacts, transfert/détachement, progression/annulation et reprise.
`cargo test -p atelier-desktop-win --locked --offline` couvre stockage et géométrie ;
la CI lance aussi `atelier.exe --self-check target/shell-check/window-state.txt`
pour exercer une fenêtre Win32 cachée sans interaction. Voir les [preuves](../_bmad-output/implementation-artifacts/worker-shell-validation.json).
'''
path.write_text(text,encoding='utf-8',newline='\n')
path = ROOT/'docs/DEVELOPMENT.md'
text = path.read_text(encoding='utf-8')
entry = f'''### Preuve US-092, US-110 et US-116 — 2027-10-11

Le [Worker](us-092-worker.md), la [fenêtre Windows](us-110-win32-window.md) et le
[stockage portable](us-116-portable-data.md) constituent ce lot ; les deux US Windows portent le backlog à **34/158 US validées**. US-092 reste ouverte pour l’annulation/progression d’une opération métier réelle.
Développement et revues en parallèle avec GPT-6 Luna moyen.
Les [27 contrôles](../_bmad-output/implementation-artifacts/worker-shell-validation.json)
passent : {passed} tests workspace, {release} tests shell release, Worker réel avec vrai WASM,
HWND caché, builds Windows/WASM/musl, Clippy, format, ressources et licence.
Les sources et le lock sont inchangés pendant les contrôles ; les 32 paquets restent locaux.
La [revue triée](../_bmad-output/implementation-artifacts/worker-shell-review.md)
ne laisse aucun constat différé. Les limites de dispatch synchrone et de validation
multi-écran sont décrites dans les contrats. Les tests musl s'exécutent en CI Linux.
La [CI précédente](https://github.com/simkamil/mon-bureau/actions/runs/38120218029)
sur c091f6a52aa58af94966592ff356750471200d44 est [archivée](../_bmad-output/implementation-artifacts/github-ci-text-layout.json).

'''
text = text.replace('## Validation\n','## Validation\n\n'+entry,1)
path.write_text(text,encoding='utf-8',newline='\n')

paths = [ROOT/'README.md',ROOT/'US-V0-V1.md',ROOT/'web/README.md',ROOT/'tests/README.md',ROOT/'docs/DEVELOPMENT.md',ROOT/'docs/PROTOTYPE-PROGRESS.md',
         ROOT/'docs/us-092-worker.md', ART/'spec-us092-worker.md', *(ROOT/'docs'/doc for _,_,doc in stories),*(ART/f'spec-us{number}-{suffix}.md' for number,suffix,_ in stories),ART/'worker-shell-claims.md',ART/'worker-shell-review.md']
record = {'date':'2027-10-11','validated_stories':34,'total_stories':158,'runtime_validation_sha256':hashlib.sha256(proof.read_bytes()).hexdigest(),
          'documentation_sha256':{p.relative_to(ROOT).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}}
(ART/'worker-shell-documentation.json').write_text(json.dumps(record,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
print(json.dumps({'done':done,'checks':27,'workspace_tests':passed,'shell_release_tests':release}))
