---
title: 'Navigation rapide dans le texte brut'
type: 'feature'
created: '2026-10-11'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
baseline_commit: '81321351c400abc92f7e73341cd5326868fdf125'
context: ['C:/mon-bureau/_bmad-output/implementation-artifacts/editing-contract.md', 'C:/mon-bureau/graphify-out/GRAPH_REPORT.md']
---

<frozen-after-approval reason="US existantes et poursuite autonome en parallèle autorisées par l’utilisateur">

## Intent

**Problem:** Le démonstrateur refuse Ctrl+flèche et Ctrl+effacement, et perd la colonne visuelle après une ligne courte. Cela gêne la rédaction au clavier.
**Approach:** Étendre le moteur existant avec navigation par mot/paragraphe, suppression par mot et colonne préférée. La glue ajoute Ctrl+A, F8/Échap et Shift aux déplacements. Le même moteur fournit des sélections de mot/paragraphe à la souris.

## Boundaries & Constraints

**Always:** Offsets UTF-8 aux frontières de graphèmes, budgets existants, commandes atomiques et annulables, aucune dépendance externe. Les US complètes restent ouvertes tant que leurs critères riches/manuels ne sont pas tous vérifiés. Travail parallèle explicitement autorisé par l’utilisateur, sans nouveau checkpoint de permission. La racine intègre les fichiers partagés, repaquette le WASM, exécute les contrôles globaux et pousse après validation.

**Never:** Aucun modèle riche, changement de version de trame, nouvelle segmentation locale, ni promesse de navigation entre objets inexistants. Les tranches US-214/220/221/223 ne clôturent pas ces stories complètes.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Mot | Texte avec accents/espaces/ponctuation, Ctrl+flèche | Déplacement selon les segments UAX29 et espaces définis dans le contrat | Pas de frontière invalide |
| Paragraphe | CR/LF/CRLF, lignes vides | Limites de paragraphes logiques, CRLF indivisible | Début/fin bornés |
| Vertical | Longue/courte/longue | X préféré restauré, réinitialisé après mouvement non vertical/édition/clic | Aucun saut hors document |
| Effacement | Sélection ou caret, Ctrl+Backspace/Delete | Sélection entière ou plage au prochain mot, une opération undo | Échec laisse l’état inchangé |
| Unité souris | Point, unité, ancre de base | Mot/paragraphe entier, extension orientée et atomique | Champs/offsets invalides refusés |

</frozen-after-approval>

## Code Map

- `crates/core-wasm/src/editor.rs` — transaction candidate/session; garde les opcodes 0–11 inchangés.
- `crates/core-wasm/src/editor-layout.rs` — lignes, slots, caret/hit-test.
- `crates/unicode/src/segmentation.rs` — `word_boundaries`, source UCD17 déjà validée.
- `web/editor-protocol.mjs`, `web/editor.mjs` — glue possédée par la racine.

## Tasks & Acceptance

**Execution:**
- [x] `crates/core-wasm/src/editor.rs`, `editor-layout.rs` — directions 10–13 motG/motD/paragrapheH/paragrapheB; opcodes12/13 effacement mot;14 sélection au point; préférence X.
- [x] `crates/core-wasm/tests/editor_engine.rs` — limites UTF8/graphèmes/CRLF, sélection inversée, undo/redo, atomisation, colonne.
- [x] `web/editor-protocol.mjs`, `web/editor.mjs` — encoder extensions, mapper commandes clavier, Ctrl+A/F8 et maintien de la vue.
- [x] `docs/us-221-keyboard-editing.md` — comportement exact, preuve et limites des stories.

**Acceptance Criteria:**
- Given une ligne courte entre deux longues, when deux déplacements verticaux, then le curseur rejoint sa colonne initiale.
- Given une sélection inversée multioctet, when une suppression de mot puis undo, then texte et sélection d’origine reviennent.
- Given une commande invalide, when envoyée au vrai WASM, then réponse d’erreur et état précédent inchangé.

## Implementation Notes

Agent moteur: seuls crates/core-wasm/src/editor.rs, editor-layout.rs, tests/editor_engine.rs et docs/us-221-keyboard-editing.md. Racine: glue web, packaging, tests browser globaux, suivi US et validation. Ne pas modifier les fichiers des autres agents.
Commandes Graphify/commit/push et changements de statut final: racine uniquement. Ne pas lancer cargo fmt global pendant les changements parallèles. Tests ciblés autorisés.

## Spec Change Log

## Review Triage Log

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

## Verification

- Contrôles ciblés Rust/Node/navigateur, puis validation globale et CI Windows/WASM/Linux du commit publié.
- Ne pas assimiler les événements synthétiques à une validation physique AZERTY/IME.

Validation finale : 41 contrôles, 377 tests workspace, 55 tests natifs release, 57 assertions navigateur ; sources/lock inchangés. Voir editing-ergonomics-validation.json et editing-ergonomics-review.md. done concerne cette tranche TXT, pas la clôture des US parentes.


---
title: 'Sélection souris et défilement du texte brut'
type: 'feature'
created: '2026-10-11'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
baseline_commit: '81321351c400abc92f7e73341cd5326868fdf125'
context: ['C:/mon-bureau/_bmad-output/implementation-artifacts/editing-contract.md', 'C:/mon-bureau/graphify-out/GRAPH_REPORT.md']
---

<frozen-after-approval reason="US existantes et poursuite autonome en parallèle autorisées par l’utilisateur">

## Intent

**Problem:** Le Canvas sait placer le curseur mais ne permet pas de glisser une sélection. La lecture et la correction de plusieurs lignes deviennent difficiles.
**Approach:** Ajouter un contrôleur de pointeur isolé avec capture, double mot/triple paragraphe, Shift et défilement automatique borné. Le moteur est l’unique autorité des plages de texte.

## Boundaries & Constraints

**Always:** Offsets UTF-8 aux frontières de graphèmes, budgets existants, commandes atomiques et annulables, aucune dépendance externe. Les US complètes restent ouvertes tant que leurs critères riches/manuels ne sont pas tous vérifiés. Travail parallèle explicitement autorisé par l’utilisateur, sans nouveau checkpoint de permission. La racine intègre les fichiers partagés, repaquette le WASM, exécute les contrôles globaux et pousse après validation.

**Never:** Pas de segmentation JS indépendante, mutation du texte, multiplication par DPR du hit-test. Ctrl+clic phrase, marge gauche et Alt-bloc rectangulaire restent V1; US-222 complète demeure ouverte.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Glisser | Pointer primaire, coordonnées client | Capture; ancre stable, focus suit jusqu’au relâchement | Coalescer déplacements, conserver dernier point |
| Shift | Sélection inversée existante | Ancre dirigée préservée | Pas de nouvelle ancre à chaque move |
| Double/triple | Mot/paragraphe au point | Unité entière; extension par unité lors du drag | Plage moteur validée |
| Bord/page | Pointeur hors viewport, pages/gaps, zoom .5–2 | Scroll RAF borné, hit-test de la page la plus proche | Pas de boucle après cancel/blur/dispose |
| Concurrence | Down/move/up rapides, promesse pending | Ordre stable, un seul drag actif | Une erreur arrête le drag et est rapportée |

</frozen-after-approval>

## Code Map

- `web/editor.mjs` — `canvasClick` actuel sera remplacé par le contrôleur; racine possède ce fichier.
- `web/canvas-renderer.mjs` — zoom et scroll en CSS px, pages ATDL.
- `crates/core-wasm/src/editor.rs` — nouvelle SelectPoint opcode14, agent moteur.

## Tasks & Acceptance

**Execution:**
- [x] `web/mouse-selection.mjs` — attacher contrôleur isolé avec callbacks document/viewport/selection et dispose.
- [x] `tests/mouse-selection.mjs` — capture, Shift inverse, async/up/cancel, multi-pages/gaps/zoom, autoscroll borné.
- [x] `web/editor.mjs` — intégrer selectPoint dans la queue; empêcher édition concurrente IME/busy, restaurer focus.
- [x] `crates/desktop-win/src/resources.rs` — embarquer nouvelle route; mise à jour sonde comptage/README/packaging par racine.
- [x] `tests/editor-browser.mjs` — sélection réelle et unité moteur; pixels/scroll/undo existants préservés.
- [x] `docs/us-222-mouse-selection.md` — preuves et limites.

**Acceptance Criteria:**
- Given un drag sortant du Canvas, when pointerup/cancel, then aucune sélection ni frame fantôme ne continue.
- Given un document paginé zoomé, when drag au bord, then scroll et focus restent bornés et la plage correcte traverse les pages.
- Given double-clic puis drag, when le pointeur passe un mot, then la sélection s’étend par unités complètes.

## Implementation Notes

Agent souris: seuls web/mouse-selection.mjs, tests/mouse-selection.mjs et docs/us-222-mouse-selection.md. Racine: web/editor.mjs, ressources natives, tests browser globaux et suivi. Ne pas modifier les fichiers des autres agents.
Commandes Graphify/commit/push et changements de statut final: racine uniquement. Ne pas lancer cargo fmt global pendant les changements parallèles. Tests ciblés autorisés.

## Spec Change Log

## Review Triage Log

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

## Verification

- Contrôles ciblés Rust/Node/navigateur, puis validation globale et CI Windows/WASM/Linux du commit publié.
- Ne pas assimiler les événements synthétiques à une validation physique AZERTY/IME.

Validation finale : 41 contrôles, 377 tests workspace, 55 tests natifs release, 57 assertions navigateur ; sources/lock inchangés. Voir editing-ergonomics-validation.json et editing-ergonomics-review.md. done concerne cette tranche TXT, pas la clôture des US parentes.


---
title: 'Presse-papiers texte brut'
type: 'feature'
created: '2026-10-11'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
baseline_commit: '81321351c400abc92f7e73341cd5326868fdf125'
context: ['C:/mon-bureau/_bmad-output/implementation-artifacts/editing-contract.md', 'C:/mon-bureau/graphify-out/GRAPH_REPORT.md']
---

<frozen-after-approval reason="US existantes et poursuite autonome en parallèle autorisées par l’utilisateur">

## Intent

**Problem:** L’éditeur colle déjà du texte mais ne copie ni ne coupe la sélection du modèle. Le textarea invisible ne contient pas le document.
**Approach:** Gérer ClipboardEvent synchroniquement sur la surface de saisie, écrire seulement text/plain, puis couper dans la queue du modèle après succès de l’écriture. Renforcer le collage simple et Ctrl+Maj+V sans double insertion.

## Boundaries & Constraints

**Always:** Offsets UTF-8 aux frontières de graphèmes, budgets existants, commandes atomiques et annulables, aucune dépendance externe. Les US complètes restent ouvertes tant que leurs critères riches/manuels ne sont pas tous vérifiés. Travail parallèle explicitement autorisé par l’utilisateur, sans nouveau checkpoint de permission. La racine intègre les fichiers partagés, repaquette le WASM, exécute les contrôles globaux et pousse après validation.

**Never:** Pas de navigator.clipboard, permission, format riche/HTML/image, donnée native arbitraire, ni suppression si l’écriture échoue. US-225 complète demeure ouverte.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Copie | Sélection normale/inversée UTF8 | Texte exact écrit avant retour du handler, aucune mutation | Vide/busy/composition/queue occupée: ne pas toucher le presse-papiers |
| Coupe | Plage non vide, écriture réussie | Suppression de cette plage seulement, une transaction undo | Révision/plage obsolète ou setData échoue: contenu préservé |
| Collage | text/plain et HTML simultanés | Texte seul remplace sélection; Ctrl+Maj+V même parcours | Paste + beforeinput: une insertion |
| Raccourcis | Ctrl+A, F8, Échap | Commandes sémantiques pour glue racine; AltGr/IME préservés | Ne pas détourner les raccourcis de champs UI |

</frozen-after-approval>

## Code Map

- `web/text-input.mjs` — handlers DOM et dédoublonnage paste/composition existants.
- `web/editor.mjs` — queue et sélection UTF8 possédées par la racine.
- `tests/frontend-browser.mjs` — compatibilité adaptateur existante.

## Tasks & Acceptance

**Execution:**
- [x] `web/text-input.mjs` — callback optionnel getClipboardSnapshot, copy/cut synchrones, commandes deleteSelection/selectAll/extensionMode.
- [x] `tests/text-input-editing.mjs` — contrat callbacks, setData exception, vide/busy/composition, multi-octets, paste/shortcuts et nettoyage listeners.
- [x] `web/editor.mjs` — snapshot stable seulement sans pending/busy/composition, suppression avec vérification révision/plage.
- [x] `tests/editor-browser.mjs` — vrai Worker/WASM, clipboard exact et annulation unique, refus sans perte, raccourcis.
- [x] `docs/us-225-plain-clipboard.md` — limites format riche et preuves.

**Acceptance Criteria:**
- Given du texte accentué sélectionné, when copy, then le presse-papiers reçoit le texte exact sans changer revision/dirty.
- Given setData qui lève, when cut, then aucune suppression ne se produit.
- Given une coupe réussie, when Ctrl+Z, then une seule annulation restaure texte et sélection.

## Implementation Notes

Agent saisie: seuls web/text-input.mjs, tests/text-input-editing.mjs et docs/us-225-plain-clipboard.md. Racine: web/editor.mjs, tests editor-browser globaux, suivi US et validation. Ne pas modifier les fichiers des autres agents.
Commandes Graphify/commit/push et changements de statut final: racine uniquement. Ne pas lancer cargo fmt global pendant les changements parallèles. Tests ciblés autorisés.

## Spec Change Log

## Review Triage Log

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

## Verification

- Contrôles ciblés Rust/Node/navigateur, puis validation globale et CI Windows/WASM/Linux du commit publié.
- Ne pas assimiler les événements synthétiques à une validation physique AZERTY/IME.

Validation finale : 41 contrôles, 377 tests workspace, 55 tests natifs release, 57 assertions navigateur ; sources/lock inchangés. Voir editing-ergonomics-validation.json et editing-ergonomics-review.md. done concerne cette tranche TXT, pas la clôture des US parentes.
