---
title: 'US-112 — Ressources embarquées et origine locale'
type: 'feature'
created: '2026-10-11'
status: 'in-progress'
baseline_commit: 'e901bde9cf4c3307fcf16f3da34a8ac9cd56a547'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.12-context.md'
---

<frozen-after-approval reason="persistent user authorization to continue parallel US development">

## Intent

Remplacer about:blank par une page locale réellement servie depuis les octets de l'exécutable. Elle présente honnêtement le socle Atelier et permet de vérifier modules, Worker et WASM ; ce lot ne prétend pas être un éditeur. US-111 reste ouverte sur les handles, mais son hébergement réel permet de travailler sur ce module dépendant sans la déclarer terminée.

## Boundaries & Constraints

Rust std et API COM système manuelles seulement ; JS ES modules sans dépendance. Ressources incorporées par include_bytes, origine exacte https://app.atelier/, table explicite HTML/JS/CSS/WASM/Carlito et licences. Le coordinateur fournit data/engine/atelier_core_wasm.wasm, artefact optimisé du moteur actuel avec preuve de reproductibilité ; ne pas créer un faux WASM ni lancer Cargo dans build.rs. Un checkout neuf doit compiler sans artefact target préexistant. Les autres polices déjà embarquées dans atelier-font restent disponibles par son API ; documenter la table web exacte.

Bloquer et journaliser localement toutes les demandes hors origine, navigations externes et nouvelles fenêtres. Aucun préfixe d'URL permissif ; rejeter credentials/ports/encodages ambigus/path traversal, chemins inconnus 404. Intercepter * et tous contextes ; CSP restrictive sans inline, compatible modules/Worker/WASM locaux. Tester et documenter les limites de WebSocket/service workers plutôt que promettre un firewall au-delà des preuves. Le journal borné de tentatives ne doit contenir ni query ni fragment sensible, et reste dans la racine US-116 si persisté. Ne pas ouvrir un navigateur sur un lien externe, fonction US-113 future.

Possession exclusive : nouveaux resources.rs, webview-resources.rs si utile, webview.rs, win32.rs pour couture/self-check, web/index.html, web/app.mjs, web/app.css, web/i18n/fr-FR.txt et tests natifs de ressources, docs/us-112-embedded-resources.md. Le coordinateur possède lib.rs/main.rs/window.rs, CI, docs globales et preuves. Fournir les signatures à y enregistrer ; ne pas les modifier. Aucun autre agent, Graphify, commit ou push. Lire GRAPH_REPORT en premier et vérifier les indices ABI dans WebView2.h officiel. Chaque unsafe justifie pointeurs et durée de vie. Les handlers ne gardent aucun contexte HWND détruit, se désenregistrent avant Close et libèrent les références sur STA ; préserver les contrôles US-111.

## I/O & Edge-Case Matrix

| État | Comportement | Erreur |
|---|---|---|
| URL connue locale GET/HEAD | octets embarqués, MIME, CSP et nosniff | HRESULT remonté |
| Route locale inconnue/méthode non admise | 404/405 borné | aucune sortie réseau |
| Origine externe, préfixe trompeur, credentials/port/traversée | refus et trace bornée | aucun contenu local exposé |
| Navigation externe ou window.open | annulée/handled et trace | aucun navigateur ouvert |
| Worker, fetch WASM, compilation et Ping | vrai moteur et origine locale | erreur visible traduisible |
| Fermeture pendant événements | callbacks drainés | aucune fuite de callback mesurable |

</frozen-after-approval>

## Code Map

- `crates/desktop-win/src/webview.rs` : RAII STA, Shared, callbacks existants et getters réels ; étendre sans les remplacer par des mocks.
- `crates/desktop-win/src/win32.rs` : couture Ready et self-check caché existant ; ajouter un contrôle de ressources distinct sans modifier le sens du test handles.
- `data/webview2/include/WebView2.h` : oracle ABI ; ICoreWebView2 Navigate 5, add navigation 7, add new-window 44, add resource 55, filter 57 à recompter.
- `web/engine-worker.mjs`, `web/engine-client.mjs`, `web/wasm-protocol.mjs` : Worker réel, écho ABI actuel ; page de socle peut vérifier Ping mais doit indiquer que l'édition manque.
- `target/next-batch-notes.md` : repérage précédent, non normatif ; SDK fait foi.

## Tasks & Acceptance

**Execution:**
- [x] `resources.rs` — table compilée, MIME, politique URL/méthodes/journal borné et tests de matrice.
- [x] `webview.rs` / module COM dédié — IStream réel et WebResourceRequested/navigation/new-window, journal, RAII et libération.
- [x] `web/index.html`, `web/app.mjs`, `web/app.css`, `web/i18n/fr-FR.txt` — page accessible du socle, Worker/WASM réels, textes catalogue.
- [x] `win32.rs` — self-check runtime caché avec résultats structurés des ressources, CSP, refus et callbacks, transmissible au coordinateur.
- [x] `docs/us-112-embedded-resources.md` — contrat, reproduction, couverture et critères encore ouverts explicitement.

**Acceptance Criteria:**
- Given le binaire copié sans dossier web/data, when la page démarre, then HTML/modules/CSS/Carlito/WASM sont servis par les octets incorporés et le Ping Worker réel réussit.
- Given une page locale, when les tentatives externes de la matrice surviennent, then elles échouent et les traces observables sont bornées ; les scénarios non couverts restent des critères ouverts.
- Given fermeture/erreur d'événement, when les handlers sont retirés, then le compte de callbacks retrouve son niveau initial et les anciens self-checks gardent leur sémantique.

## Implementation Notes

Implémentation parallèle par GPT-6 Luna, raisonnement moyen. Contrôles indépendants globaux en cours dans `embedded-foundations-validation.json`. Les cases du backlog complet ne sont cochées qu’après validation de leurs critères.

## Spec Change Log

## Review Triage Log

| ID | Verdict | Surface | Preuve et décision | Route |
|---|---|---|---|---|
| B1 | low | refresh-graph.py | Le tuple est bien une régression de cette modification, dans la branche de normalisation SDK. Correction directe du Path unique. | patch |
| B2 | medium | files.rs read | Une substitution régulière entre identité par chemin et ouverture passe les anciens contrôles. Comparaison ajoutée sur le handle ouvert avant lecture, test de substitution. | patch |
| B3 | low | files.rs parents | La course avec remplacement hostile de parent existe. Le broker natif de confiance ne constitue pas une sandbox contre un processus local de même privilège ; cette limite est explicitement documentée. Le correctif demanderait une nouvelle politique de parcours/handles, au-delà d’une correction directe, pour un scénario absent de l’usage courant. | rejet low complexe; limite conservée |
| B4 | low | files.rs ReplaceFileW | Une substitution extérieure dans la dernière fenêtre check→ReplaceFileW est possible. Le chemin reste celui explicitement choisi ; aucune garantie CAS n’est annoncée. Fermer cette fenêtre nécessiterait une nouvelle coordination inter-processus. Scénario rare dans cette fenêtre précise, correction complexe rejetée ; limite documentée. | rejet low complexe; limite conservée |
| B5 | medium | files.rs opened handle | Même défaut vérifié que B2 : OPEN_REPARSE_POINT seul ne détecte pas la substitution par un autre fichier régulier. Corrigé avec B2. | patch |
| B6 | low | web/index.html | Le texte statique affirme une vérification sans exécution de contrôle JS. Le libellé devient neutre Origine embarquée. | patch |
| B7 | low | web/i18n/fr-FR.txt | Opérationnel dépasse la preuve de transport par écho. Les libellés explicitent désormais écho/transport. | patch |
| B8 | false | README.md / docs/us-113-file-access.md | Le README renvoie au contrat qui décrit grant_read/grant_write/revoke et les types opaques. L’intégration et le cycle JS sont explicitement exclus de l’intention de cette tranche ; aucune API JS fichiers n’est annoncée. | rejet |
| B9 | false | docs/us-120-plain-text-model.md | L’intention porte sur le propriétaire isolé et exclut explicitement l’intégration UI/core-wasm. Les raccordements restants sont déjà nommés ; leur absence ne constitue pas une régression de cette API. | rejet |
| B10 | medium | win32.rs self-check | La matrice pure classifiait HEAD/404/405, mais aucun test du binding réel ne vérifiait ces réponses. Ajout de sondes runtime avec body et Content-Length HEAD vérifiés. | patch |
| G1 | medium | file_dialog.rs Show | Constat de vérification reçu : le helper d’annulation testé n’était pas appelé par Show. Interprétation HRESULT commune utilisée en production, testée pour annulation/succès/erreur ; dialogue manuel reste ouvert. | patch |
| E1 | medium | webview-resources.rs navigation | Le getter retournait un HRESULT sans appeler put_Cancel. Le refus par défaut doit précéder une erreur de lecture URI ; test de vtable en échec. Le setter de refus peut lui-même échouer : erreur remontée, aucune garantie absolue inventée. | patch |
| E2 | medium | webview-resources.rs new window | Le getter pouvait retourner avant put_Handled, laissant le refus non établi. Même politique de refus initial et test vtable. | patch |
| E3 | medium | files.rs opened handle | Substitution régulière vérifiée dans le chemin de lecture ; identique B2/B5, comparaison de handle appliquée. | patch |
| E4 | low | files.rs ReplaceFileW | Même fenêtre extérieure très courte que B4, déjà documentée comme contrôle au mieux et non CAS. Nouvelle politique de coordination complexe pour un scénario rare ; pas de promesse d’isolation contre un autre processus local. | rejet low complexe; limite conservée |
| E5 | low | refresh-graph.py | Même branche tuple erronée que B1, corrigée directement. | patch |
| E6 | medium | webview-resources.rs navigation claim | La trace et le refus manquaient en cas d’échec de getter ; même défaut E1, test et trace non classifiée ajoutés. | patch |
| E7 | medium | files.rs read claim | L’identité initiale seule ne prouvait pas celle du handle lu ; même défaut B2/E3, corrigé avant lecture. | patch |


Preuves et regroupement : `embedded-foundations-review.md`.

## Verification

- Tests resources purs, cargo test/clippy desktop-win, fmt ; auto-contrôle réel WebView2 caché à intégrer par le coordinateur. Ne pas modifier les docs globales ni cocher l'US sur sa seule implémentation.

Validation après corrections : 37 contrôles, 351 tests workspace, 48 tests natifs release ; sources et lock inchangés. Voir embedded-foundations-validation.json et embedded-foundations-review.md. Le statut done des tranches fichiers/modèle ne termine pas les US-113/120 complètes. US-112 reste in-progress pour les scénarios explicitement ouverts.


---
title: 'US-113 — Accès fichiers autorisé et sauvegarde atomique'
type: 'feature'
created: '2026-10-11'
status: 'done'
baseline_commit: 'e901bde9cf4c3307fcf16f3da34a8ac9cd56a547'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.12-context.md'
---

<frozen-after-approval reason="persistent user authorization to continue parallel US development">

## Intent

Implémenter le backend fichiers nécessaire au pont natif de US-113 : dialogues Windows d'ouverture/enregistrement, lecture bornée et sauvegarde atomique. L'accès depuis JS et SharedBuffer seront raccordés ensuite. Cette tranche ne valide ni les polices système ni le transfert 100 Mo/300 ms ni l'US complète.

## Boundaries & Constraints

Possession exclusive de nouveaux crates/desktop-win/src/files.rs, file_dialog.rs et tests dédiés, docs/us-113-file-access.md. Le coordinateur possède lib.rs/main.rs, webview.rs/win32.rs sont possédés par l'agent US-112 : ne pas y écrire. Fournir le module et sa signature à enregistrer au coordinateur. Rust std, FFI Win32/COM écrite à la main, aucune dépendance nouvelle. Aucun agent, Graphify, commit/push, catalogue/global docs. Lire GRAPH_REPORT en premier et vérifier les IID/slots de IFileOpenDialog/IFileSaveDialog/IShellItem dans les headers Windows SDK locaux (shobjidl_core.h) ou documentation Microsoft primaire ; ne pas transposer les slots WebView2. Ne pas afficher de dialogue pendant les tests automatisés.

API propriétaire STA pour dialogue natif parent HWND optionnel explicite (pas de pointeur raw accepté dans une API safe sans contrat solide). Filtres par format validés et buffers UTF-16 gardés vivants pendant COM ; options filesystem, open fichier existant, save confirmation écrasement. Annulation distincte de l'erreur. COM initialisation équilibrée, chaînes CoTaskMem et interfaces libérées avec RAII. Types non Send/Sync pour possessions COM. Aucun panic ne traverse l'ABI.

Backend transport-indépendant : autoriser un chemin choisi par dialogue (ou API trusted native explicite) et retourner un ticket opaque ; read/write par ticket, pas par chemin arbitraire reçu de JS. Capabilities lecture/écriture distinctes, bornées et révocables, IDs non réutilisés au cours du broker. Les tickets ne sont pas des secrets contre du code local, mais empêchent de confondre un chemin web et un choix natif. Lecture bornée avec vérification réelle après metadata (fichier grandissant), limites checked. Erreurs structurées code/message_key + code Windows optionnel, sans chemin privé dans le message visible par défaut.

Écriture atomique : temporaire create_new dans le même dossier, write_all, sync_all, ReplaceFileW si destination existante ; destination nouvelle via renommage sans remplacement pour éviter écraser une création concurrente. Ne jamais effacer l'original avant le remplacement ; nettoyer uniquement son propre temporaire sur échec. Définir comportement face à liens/reparse points/destination disparue ou remplacée par répertoire et refus de ticket lecture pour écrire. Les sauvegardes modifient uniquement la destination explicitement autorisée. Aucun test sur documents utilisateur ; répertoires uniques sous target ou temp vérifiés.

## I/O & Edge-Case Matrix

| État | Comportement | Erreur |
|---|---|---|
| Dialogue validé/annulé | ticket natif ou None | HRESULT typé et traduisible |
| Ticket lecture et fichier UTF-8/binaire | octets exacts sous limite | taille/lecture refusées |
| Ticket inconnu/révoqué/mauvais droit | refus avant IO | code stable |
| Sauvegarde existante/nouvelle | bytes finaux exacts, original remplacé atomiquement | course création = original concurrent préservé |
| Échec write/flush/replace/rename | ancien fichier préservé, propre temp nettoyé | code et étape conservés |
| Filtres ou chemins invalides | refus avant appel système | aucune panique |

</frozen-after-approval>

## Code Map

- `crates/desktop-win/src/data.rs`, `window.rs` : conventions IO, chemins et erreurs du shell ; ne pas les modifier.
- `crates/desktop-win/src/webview.rs` : future origine fiable et pont ; aucun raccord postMessage dans cette tranche.
- `SPEC.md` §6.8 et `US-V0-V1.md` US-113 : COM fichiers et ReplaceFileW ; SharedBuffer/polices/URL restent à intégrer.
- Windows SDK `shobjidl_core.h` et API shell32/ole32/kernel32 : oracle primaire pour interfaces et fonctions utilisées.

## Tasks & Acceptance

**Execution:**
- [x] `files.rs` — broker autorisations, lecture bornée, erreurs et écriture atomique testable avec injection des échecs IO.
- [x] `file_dialog.rs` — COM STA RAII, filtres, ouverture/enregistrement et annulation ; types publics utilisables par le coordinateur.
- [x] Tests dédiés — matrice, original inchangé à chaque panne injectée, droits/tickets/limites et bindings fake-vtable si utile sans ouvrir UI.
- [x] `docs/us-113-file-access.md` — API, ownership, garanties exactes Windows et critères restant manuels/non intégrés.

**Acceptance Criteria:**
- Given un ticket autorisé, when la lecture ou sauvegarde réussit, then les bytes correspondent au fichier choisi et aucun autre chemin n'est accessible par le contrat.
- Given une panne avant remplacement, when la sauvegarde échoue, then l'ancien fichier est inchangé et le temporaire possédé est nettoyé.
- Given un HRESULT annulation ou erreur, when le dialogue retourne, then annulation et erreur sont distinctes et toutes les possessions COM sont libérées.

## Implementation Notes

Implémentation parallèle par GPT-6 Luna, raisonnement moyen. Contrôles indépendants globaux en cours dans `embedded-foundations-validation.json`. Les cases du backlog complet ne sont cochées qu’après validation de leurs critères.

## Spec Change Log

## Review Triage Log

| ID | Verdict | Surface | Preuve et décision | Route |
|---|---|---|---|---|
| B1 | low | refresh-graph.py | Le tuple est bien une régression de cette modification, dans la branche de normalisation SDK. Correction directe du Path unique. | patch |
| B2 | medium | files.rs read | Une substitution régulière entre identité par chemin et ouverture passe les anciens contrôles. Comparaison ajoutée sur le handle ouvert avant lecture, test de substitution. | patch |
| B3 | low | files.rs parents | La course avec remplacement hostile de parent existe. Le broker natif de confiance ne constitue pas une sandbox contre un processus local de même privilège ; cette limite est explicitement documentée. Le correctif demanderait une nouvelle politique de parcours/handles, au-delà d’une correction directe, pour un scénario absent de l’usage courant. | rejet low complexe; limite conservée |
| B4 | low | files.rs ReplaceFileW | Une substitution extérieure dans la dernière fenêtre check→ReplaceFileW est possible. Le chemin reste celui explicitement choisi ; aucune garantie CAS n’est annoncée. Fermer cette fenêtre nécessiterait une nouvelle coordination inter-processus. Scénario rare dans cette fenêtre précise, correction complexe rejetée ; limite documentée. | rejet low complexe; limite conservée |
| B5 | medium | files.rs opened handle | Même défaut vérifié que B2 : OPEN_REPARSE_POINT seul ne détecte pas la substitution par un autre fichier régulier. Corrigé avec B2. | patch |
| B6 | low | web/index.html | Le texte statique affirme une vérification sans exécution de contrôle JS. Le libellé devient neutre Origine embarquée. | patch |
| B7 | low | web/i18n/fr-FR.txt | Opérationnel dépasse la preuve de transport par écho. Les libellés explicitent désormais écho/transport. | patch |
| B8 | false | README.md / docs/us-113-file-access.md | Le README renvoie au contrat qui décrit grant_read/grant_write/revoke et les types opaques. L’intégration et le cycle JS sont explicitement exclus de l’intention de cette tranche ; aucune API JS fichiers n’est annoncée. | rejet |
| B9 | false | docs/us-120-plain-text-model.md | L’intention porte sur le propriétaire isolé et exclut explicitement l’intégration UI/core-wasm. Les raccordements restants sont déjà nommés ; leur absence ne constitue pas une régression de cette API. | rejet |
| B10 | medium | win32.rs self-check | La matrice pure classifiait HEAD/404/405, mais aucun test du binding réel ne vérifiait ces réponses. Ajout de sondes runtime avec body et Content-Length HEAD vérifiés. | patch |
| G1 | medium | file_dialog.rs Show | Constat de vérification reçu : le helper d’annulation testé n’était pas appelé par Show. Interprétation HRESULT commune utilisée en production, testée pour annulation/succès/erreur ; dialogue manuel reste ouvert. | patch |
| E1 | medium | webview-resources.rs navigation | Le getter retournait un HRESULT sans appeler put_Cancel. Le refus par défaut doit précéder une erreur de lecture URI ; test de vtable en échec. Le setter de refus peut lui-même échouer : erreur remontée, aucune garantie absolue inventée. | patch |
| E2 | medium | webview-resources.rs new window | Le getter pouvait retourner avant put_Handled, laissant le refus non établi. Même politique de refus initial et test vtable. | patch |
| E3 | medium | files.rs opened handle | Substitution régulière vérifiée dans le chemin de lecture ; identique B2/B5, comparaison de handle appliquée. | patch |
| E4 | low | files.rs ReplaceFileW | Même fenêtre extérieure très courte que B4, déjà documentée comme contrôle au mieux et non CAS. Nouvelle politique de coordination complexe pour un scénario rare ; pas de promesse d’isolation contre un autre processus local. | rejet low complexe; limite conservée |
| E5 | low | refresh-graph.py | Même branche tuple erronée que B1, corrigée directement. | patch |
| E6 | medium | webview-resources.rs navigation claim | La trace et le refus manquaient en cas d’échec de getter ; même défaut E1, test et trace non classifiée ajoutés. | patch |
| E7 | medium | files.rs read claim | L’identité initiale seule ne prouvait pas celle du handle lu ; même défaut B2/E3, corrigé avant lecture. | patch |


Preuves et regroupement : `embedded-foundations-review.md`.

## Verification

- Tests ciblés desktop-win et clippy -D warnings après enregistrement des modules par le coordinateur ; tests IO Windows réels et pannes injectées. Pas de tests interactifs ni de claims SharedBuffer/performance.

Validation après corrections : 37 contrôles, 351 tests workspace, 48 tests natifs release ; sources et lock inchangés. Voir embedded-foundations-validation.json et embedded-foundations-review.md. Le statut done des tranches fichiers/modèle ne termine pas les US-113/120 complètes. US-112 reste in-progress pour les scénarios explicitement ouverts.


---
title: 'US-120 — Modèle concret de texte brut annulable'
type: 'feature'
created: '2026-10-11'
status: 'done'
baseline_commit: 'e901bde9cf4c3307fcf16f3da34a8ac9cd56a547'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.11-context.md'
---

<frozen-after-approval reason="persistent user authorization to continue parallel US development">

## Intent

Construire le propriétaire concret du texte brut qui manque à US-120, avec sélection, saisie, suppression, annulation et rétablissement. Son API doit pouvoir être appelée ensuite depuis core-wasm. Ce lot est une tranche du démonstrateur, pas le modèle riche complet US-200 ni l'intégration UI US-120 terminée.

## Boundaries & Constraints

Possession exclusive de crates/text-model/src/**, crates/text-model/tests/**, crates/text-model/Cargo.toml et docs/us-120-plain-text-model.md. Réutiliser atelier-model History/Transaction/Operation et atelier-unicode pour les frontières de graphèmes, sans modifier leurs sources. Deux dépendances locales par chemin autorisées ; le coordinateur régénère Cargo.lock. Aucun core-wasm, protocole partagé, text-layout, fichier web, doc globale, Graphify, commit, push ou sous-agent. Lire GRAPH_REPORT avant d'analyser et communiquer tôt les signatures publiques retenues au coordinateur. Vérifier uniquement le crate ; ne pas formatter tout le workspace pendant les travaux concurrents.

Rust sûr, erreurs typées, limites explicites texte/historique/opérations, aucune panique sur entrée externe. Première tranche : texte UTF-8 possédé et sélection dirigée en offsets UTF-8 globaux, API exportable. Cette représentation de texte brut est distincte des futures positions par NodeId du modèle riche ; le documenter. Ne pas inventer des IDs stables non nécessaires à cette tranche. Document vide permis. Préserver octets LF/CRLF existants et BOM UTF-8 au round-trip si conservée comme métadonnée ; entrée UTF-8 invalide refusée sans remplacement silencieux. Ne pas interpréter UTF-16/ANSI comme UTF-8. Insertions exactes, saut de paragraphe avec convention de fin de ligne déterministe documentée.

API publique visée : PlainTextDocument::from_utf8(bytes, limits), text()/export_utf8(), selection()/set_selection(), insert_text(text, now_ms), delete_backward(now_ms), delete_forward(now_ms), undo()/redo(), can_undo()/can_redo(). Des noms équivalents cohérents sont acceptables s'ils sont annoncés. Les sélections sont des bytes UTF-8 valides ; suppression d'un caret retire un graphème Unicode entier, CRLF atomique. Sélection non vide remplacée/supprimée en une transaction ; ancres inversées acceptées. Undo/redo restaurent sélection et texte exacts ; mouvements ferment le groupe de frappe, budget conservateur couvre aussi les inverses. Pas d'opération invalide qui altère texte, sélection ou historique. Les limites sont validées avant allocation proportionnelle et mutation.

## I/O & Edge-Case Matrix

| État | Comportement | Erreur |
|---|---|---|
| UTF-8 accents/emoji, LF/CRLF, vide, BOM | import/export exact | mauvais UTF-8 refusé |
| Sélection normale/inversée valide | remplacement transactionnel et caret après insertion | offset interne à un caractère refusé |
| Caret accent décomposé/emoji/CRLF | suppression d'un graphème entier | bord document = aucun edit |
| Undo/redo et frappe rapprochée | contenu/sélection restaurés et groupe selon History | budget insuffisant = état inchangé |
| Texte/offset/taille hors limites | refus typé avant mutation | historique intact |
| Nouvelle édition après undo | branche redo supprimée après succès | échec préserve redo |

</frozen-after-approval>

## Code Map

- `crates/text-model/src/lib.rs` : squelette à remplacer par le module concret et ses exports.
- `crates/model/src/lib.rs` : TransactionalState checkpoint/restore, Operation inversible et codec borné ; réutiliser les garanties existantes.
- `crates/model/src/history.rs` : History::apply/move_cursor/undo/redo, EditKind, budget logique conservateur.
- `crates/unicode/src/segmentation.rs` et exports du crate : graphèmes UAX29 vérifiés ; ne pas refaire un découpage char.
- `US-V0-V1.md` US-120 et US-200, `SPEC.md` §6.3/6.7 : frontière texte brut versus document riche.

## Tasks & Acceptance

**Execution:**
- [x] `crates/text-model/src/**` — propriétaire, limites, erreurs, opérations et History ; annoncer le contrat dès qu'il est fixé.
- [x] `crates/text-model/Cargo.toml` — ajouter uniquement atelier-model et atelier-unicode locaux.
- [x] `crates/text-model/tests/**` — tests de matrice et séquences déterministes d'éditions/inverses contre un oracle indépendant.
- [x] `docs/us-120-plain-text-model.md` — API, représentation, fins de ligne, budgets, limites et intégration encore absente.

**Acceptance Criteria:**
- Given UTF-8 valide, when une séquence insertion/suppression/undo/redo est exécutée, then texte et sélection concordent avec les états attendus et l'export reste exact.
- Given entrée invalide ou budget dépassé, when une opération est tentée, then texte/sélection/historique sont identiques à leur état précédent.
- Given graphèmes composés et CRLF, when backspace/delete est appliqué au caret, then aucune séquence valide n'est coupée au milieu d'un graphème.

## Implementation Notes

Implémentation parallèle par GPT-6 Luna, raisonnement moyen. Contrôles indépendants globaux en cours dans `embedded-foundations-validation.json`. Les cases du backlog complet ne sont cochées qu’après validation de leurs critères.

## Spec Change Log

## Review Triage Log

| ID | Verdict | Surface | Preuve et décision | Route |
|---|---|---|---|---|
| B1 | low | refresh-graph.py | Le tuple est bien une régression de cette modification, dans la branche de normalisation SDK. Correction directe du Path unique. | patch |
| B2 | medium | files.rs read | Une substitution régulière entre identité par chemin et ouverture passe les anciens contrôles. Comparaison ajoutée sur le handle ouvert avant lecture, test de substitution. | patch |
| B3 | low | files.rs parents | La course avec remplacement hostile de parent existe. Le broker natif de confiance ne constitue pas une sandbox contre un processus local de même privilège ; cette limite est explicitement documentée. Le correctif demanderait une nouvelle politique de parcours/handles, au-delà d’une correction directe, pour un scénario absent de l’usage courant. | rejet low complexe; limite conservée |
| B4 | low | files.rs ReplaceFileW | Une substitution extérieure dans la dernière fenêtre check→ReplaceFileW est possible. Le chemin reste celui explicitement choisi ; aucune garantie CAS n’est annoncée. Fermer cette fenêtre nécessiterait une nouvelle coordination inter-processus. Scénario rare dans cette fenêtre précise, correction complexe rejetée ; limite documentée. | rejet low complexe; limite conservée |
| B5 | medium | files.rs opened handle | Même défaut vérifié que B2 : OPEN_REPARSE_POINT seul ne détecte pas la substitution par un autre fichier régulier. Corrigé avec B2. | patch |
| B6 | low | web/index.html | Le texte statique affirme une vérification sans exécution de contrôle JS. Le libellé devient neutre Origine embarquée. | patch |
| B7 | low | web/i18n/fr-FR.txt | Opérationnel dépasse la preuve de transport par écho. Les libellés explicitent désormais écho/transport. | patch |
| B8 | false | README.md / docs/us-113-file-access.md | Le README renvoie au contrat qui décrit grant_read/grant_write/revoke et les types opaques. L’intégration et le cycle JS sont explicitement exclus de l’intention de cette tranche ; aucune API JS fichiers n’est annoncée. | rejet |
| B9 | false | docs/us-120-plain-text-model.md | L’intention porte sur le propriétaire isolé et exclut explicitement l’intégration UI/core-wasm. Les raccordements restants sont déjà nommés ; leur absence ne constitue pas une régression de cette API. | rejet |
| B10 | medium | win32.rs self-check | La matrice pure classifiait HEAD/404/405, mais aucun test du binding réel ne vérifiait ces réponses. Ajout de sondes runtime avec body et Content-Length HEAD vérifiés. | patch |
| G1 | medium | file_dialog.rs Show | Constat de vérification reçu : le helper d’annulation testé n’était pas appelé par Show. Interprétation HRESULT commune utilisée en production, testée pour annulation/succès/erreur ; dialogue manuel reste ouvert. | patch |
| E1 | medium | webview-resources.rs navigation | Le getter retournait un HRESULT sans appeler put_Cancel. Le refus par défaut doit précéder une erreur de lecture URI ; test de vtable en échec. Le setter de refus peut lui-même échouer : erreur remontée, aucune garantie absolue inventée. | patch |
| E2 | medium | webview-resources.rs new window | Le getter pouvait retourner avant put_Handled, laissant le refus non établi. Même politique de refus initial et test vtable. | patch |
| E3 | medium | files.rs opened handle | Substitution régulière vérifiée dans le chemin de lecture ; identique B2/B5, comparaison de handle appliquée. | patch |
| E4 | low | files.rs ReplaceFileW | Même fenêtre extérieure très courte que B4, déjà documentée comme contrôle au mieux et non CAS. Nouvelle politique de coordination complexe pour un scénario rare ; pas de promesse d’isolation contre un autre processus local. | rejet low complexe; limite conservée |
| E5 | low | refresh-graph.py | Même branche tuple erronée que B1, corrigée directement. | patch |
| E6 | medium | webview-resources.rs navigation claim | La trace et le refus manquaient en cas d’échec de getter ; même défaut E1, test et trace non classifiée ajoutés. | patch |
| E7 | medium | files.rs read claim | L’identité initiale seule ne prouvait pas celle du handle lu ; même défaut B2/E3, corrigé avant lecture. | patch |


Preuves et regroupement : `embedded-foundations-review.md`.

## Verification

- cargo test -p atelier-text-model --offline (lock sera actualisé par le coordinateur), clippy du crate avec -D warnings et fmt limité aux fichiers possédés ; pas d'affirmation de latence 300 pages sans mesure réelle.

Validation après corrections : 37 contrôles, 351 tests workspace, 48 tests natifs release ; sources et lock inchangés. Voir embedded-foundations-validation.json et embedded-foundations-review.md. Le statut done des tranches fichiers/modèle ne termine pas les US-113/120 complètes. US-112 reste in-progress pour les scénarios explicitement ouverts.
