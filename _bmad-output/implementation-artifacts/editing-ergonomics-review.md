# Revue du lot édition TXT — 2026-10-11

Trois revues indépendantes GPT-6 Luna moyen : blind hunter, edge cases et verification gaps. Tous les résultats ont été collectés avant triage. Le snapshot initial est identifié dans `editing-ergonomics-review-snapshot.json`. Les constats de racine complètent ces revues. Aucune US complète supplémentaire n’est déclarée terminée.

| ID | Verdict | Décision et preuve |
|---|---|---|
| B1 | low | patch : la documentation contredisait preventDefault anticipé. Corrigée : l’événement propriétaire est annulé pour éviter de copier le tampon invisible, sans suppression en cas d’échec. |
| B2 | false | Dans le contrôleur, le callback reçoit la sélection de la session : mouvements/clics utilisent les slots de graphèmes du layout, Ctrl+A les bornes0/len, composition restaure cette même sélection. Aucun chemin UI ne fournit une ancre au milieu d’un graphème. L’API bas niveau SetSelection accepte des frontières de caractères UTF-8, mais n’est pas exposée librement par ce contrôleur ; l’adaptateur ne doit pas inventer une segmentation JS divergente. |
| B3 | false | Ctrl+A lit state seulement après initialisation valide ; aucun chemin ne remet state à null. La queue vérifie disposed avant exécution. Aucun état absent atteignable dans le scénario cité. |
| B4 | medium | patch : Ctrl+A conservait le mode F8, donc une flèche étendait toute la sélection. Reset du mode et scénario intégré F8→Ctrl+A→flèche. |
| B5 | false | BoundaryIndex.is_boundary appelle validate(offset), qui rejette les offsets hors texte. Le test ABI réel past-end confirme erreur sans mutation. |
| B6 | false | Le Canvas courant est une surface de viewport, avec le scroll déjà soustrait au rendu. Le spacer ne crée pas d’overflow horizontal : scrollLeft reste nul dans cette intégration. Ajouter scrollLeft au hit-test suit le contrat viewport ; le déplacement du rect horizontal supposé n’est pas atteint ici. |
| B7 | false | Les pages sont empilées verticalement ; le choix par Y puis clamp X correspond à cette géométrie. Aucun mode de pages côte à côte n’est fourni par le propriétaire. |
| B8 | false | Les DOMRect et scrolls proviennent du navigateur, les pages/zoom sont validés et fournis par l’état moteur. Le scénario non fini supposé exige un callback étranger au contrat du propriétaire actuel. |
| B9 | medium | patch : isEnabled pouvait lever hors du catch au settle et laisser une capture active sans résultat. Le nouveau contrôleur expose ce callback à son propriétaire. Wrapper protégé, annulation capture/RAF et test pending avec mouvement coalescé ; aucune rejection non gérée. |
| B10 | low | patch : doc du format précise anchorEnd==anchor pour une extension caractère. Le moteur appliquait déjà cette garde. |
| B11 | false | Le scénario intégré existant émet Ctrl+Maj+V, paste avec text/plain et HTML, puis beforeinput ; il vérifie texte seul et insertion unique. Le test adaptateur seul n’est pas l’unique preuve. |
| E1 | medium | patch : même défaut que B4, conservé séparément pour traçabilité. Reset F8 sur selectAll et test intégré. |
| V1 | medium | patch : ajout du parcours réel caret→deleteWordBackward→undo ; les tests Rust de sélection ne prouvaient pas cette liaison. |
| V2 | medium | patch : ajout des événements clavier Ctrl+Gauche/Haut/Bas dans le vrai Worker/WASM ; les tests des opcodes seuls ne couvraient pas leur mapping. |
| O1 | medium | patch : le test de colonne lisait les octets32..40 plutôt que36..44. Offset corrigé et focus21 exigé sur longue/courte/longue. |
| O2 | medium | patch : onze injections SelectPoint invalides via dispatch du vrai WASM couvrent page, NaN/infini, unité, booléen, réservé, ancres UTF-8/hors texte et tailles ; état/export identiques après chaque refus. |
| C1 | medium | patch : Clippy refusait navigation_tests placé avant des items. Module déplacé en fin de fichier ; Clippy ciblé passe sans autorisation de lint. |

Tous les constats retenus sont corrigés. Aucun nouveau report. La suppression de mot depuis un caret restaure au undo la plage sélectionnée enregistrée par l’API existante ; cette limite est documentée, sans changer l’API utilisée par le développement externe US-200.

Reproduction : `cargo test -p atelier-core-wasm --locked --offline`, `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, `node tests/mouse-selection.mjs`, `node tests/editor-abi.mjs`, `node tests/run-frontend-browser.mjs input`, `node tests/run-editor-browser.mjs` et `py -X utf8 _bmad-output/implementation-artifacts/validate-editing-ergonomics.py`.
