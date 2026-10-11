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
