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
