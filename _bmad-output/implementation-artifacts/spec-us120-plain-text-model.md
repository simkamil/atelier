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
