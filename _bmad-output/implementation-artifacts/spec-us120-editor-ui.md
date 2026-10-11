---
title: 'US-120 — Interface de texte brut Canvas'
type: 'feature'
created: '2026-10-11'
status: 'done'
route: 'dispatch'
baseline_commit: '3cb335442bc3b199b9e25ab177bcce396cf2cc55'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/editor-contract.md'
---

<frozen-after-approval reason="persistent user authorization to continue parallel US development">

## Intent

Permettre de saisir, sélectionner, annuler et rétablir un document texte paginé, puis ouvrir/enregistrer via le host natif. Remplacer la page de contrôle statique par une interface de démonstration réellement utilisable.

## Boundaries & Constraints

Possession de web/** sauf wasm-loader.mjs (préserver sauf nécessité signalée), tests JS/HTML associés et docs/us-120-editor-ui.md. Ne pas modifier Rust/natif/resources.rs/lock/provenance, CI ou docs globales. Préserver les sondes Ping et dataset.engineEcho=ok pour le diagnostic natif existant. Les ID DOM historiques status-title/status-detail/status-indicator/engine-state peuvent être conservés discrètement pour diagnostics. Étendre la sémantique seulement pour Editor. Pas de textarea visible : Canvas et saisie cachée. Tous les libellés passent par le catalogue français.

Lire et appliquer le contrat commun dans context. Il fixe les formats, limites, comportements et propriétés des fichiers. Pas de commit/push ni de sous-agent. Lire GRAPH_REPORT avant analyse. Communiquer tôt toute contradiction du contrat avant de modifier. L'utilisateur autorise plusieurs tranches cohérentes en parallèle avec GPT-6 Luna moyen. Les US complètes restent ouvertes quand leurs critères physiques, performances ou intégrations supplémentaires ne sont pas prouvés.

## I/O & Edge-Case Matrix

| Scénario | Attendu | Échec |
|---|---|---|
| UTF-8 valide, BOM/CRLF, vide | document exact et opération attendue selon contrat | aucun remplacement silencieux |
| Commande malformée, origine/longueur incorrecte | refus borné | document et système de fichiers inchangés |
| Saisie/sélection/undo/redo | état cohérent et réponse corrélée | état précédent préservé sur refus |
| Annulation ou erreur de fichier | document encore accessible, dirty conservé | erreur traduisible sans chemin |
| Sauvegarde réussie ou révision devenue obsolète | fichier exact; dirty effacé seulement pour version sauvegardée | pas de confirmation mensongère |

</frozen-after-approval>

## Code Map

web/text-input.mjs = capture beforeinput/composition et callbacks; ne pas gérer touches mortes par mapping JS. Les callbacks normaux sans composition utilisent la sélection actuelle au moment où la commande FIFO est exécutée pour éviter les snapshots obsolètes sur frappe rapide; les commandes de composition peuvent conserver leur sélection de début si aucune mutation concurrente. web/canvas-renderer.mjs = replay/viewport/overlay; pages ATDL FillPath ne nécessitent aucun resolver police. Les overlays caret/selection peuvent être DOM superposés ou extensions compatibles renderer. web/app.mjs = conserver relais CSP borné. Les erreurs Failed sont des codes à traduire, aucune stack technique dans UI.

## Tasks & Acceptance

**Execution:**
- [x] `web/wasm-protocol.mjs et web/editor-protocol.mjs` — Registre Editor et codec applicatif strict symétrique Rust, validation ATES/ATEX et bornes.
- [x] `web/engine-worker.mjs et web/engine-client.mjs` — Accepter EditorResult/Failed corrélés et des réponses plus grandes, conserver sondes écho et annulation FIFO.
- [x] `web/editor.mjs` — Orchestration FIFO, rendu ATDL, caret/selection/composition, commandes clavier/clics et état sauvegardé.
- [x] `web/native-host.mjs` — Host borné et corrélé, annulation/erreurs sans chemins; injectable pour tests navigateur.
- [x] `web/app.mjs, index.html, app.css et i18n/fr-FR.txt` — Interface sobre alignée sur styles existants, actions Nouveau/Ouvrir/Enregistrer sous/Undo/Redo, focus, état modifié et erreurs.
- [x] `tests/editor*.mjs et tests/editor*.html` — Tests codec malformé et vrai WASM/Worker/Canvas, saisie, selection, undo/redo, host factice open/save/cancel/error.
- [x] `docs/us-120-editor-ui.md` — Reproduction et limites des preuves synthétiques, physiques et natives restantes.

**Acceptance Criteria:**
- Given le runtime initialisé, when la tranche est exercée avec ses autres composants raccordés, then les comportements du contrat commun sont observables et aucune donnée n'est confiée à une couche non propriétaire.
- Given une commande invalide, annulée ou hors budget, when elle est traitée, then la matrice applicable est couverte par un test exécuté et l'état reste cohérent.
- Given les sondes de transport et de cycle de vie antérieures, when les nouveaux messages sont ajoutés, then leurs garanties restent vérifiées.

## Implementation Notes

## Spec Change Log

## Review Triage Log

| ID | Verdict | Route | Preuve / décision |
|---|---|---|---|
| B1 | medium | patch | Le Canvas seul ne rend pas le texte accessible. Ajout d’une représentation textuelle DOM lisible avec textContent et vérification navigateur ; la validation assistive complète reste ouverte. |
| B2 | false | reject | Le shell natif refuse toute seconde navigation top-level dans Queue.begin_navigation, même propre : le lien ne détruit pas le document dans le produit Windows. Le wordmark devient néanmoins statique pour éviter une action sans effet et protéger les aperçus web. |
| B3 | medium | defer | Absence de copy/cut vérifiée dans text-input.mjs, déjà présent avant ce lot. Le presse-papiers relève explicitement de la story V1 US-225, distincte du démonstrateur V0 US-120 ; limitation conservée pour les prochains travaux. |
| B4 | medium | defer | Ctrl+flèche par mot est reconnu par l’adaptateur préexistant mais refusé explicitement par ce contrôleur. La story V1 US-221 porte ce comportement ; erreur visible et contenu préservé, pas de revendication que cette story est livrée. |
| B5 | false | reject | Le produit V0 est un exécutable Windows/Win32 ; il ne peut être lancé sous macOS. Les raccourcis Command du navigateur ne prouvent aucune prise en charge macOS et ce lot n’en revendique pas. |
| B6 | low | reject | Le wrapping aux graphèmes est effectif, sans altération du texte. La mise en page du démonstrateur est explicitement simplifiée ; préférer UAX14 correspond à US-210. La correction demanderait un nouvel algorithme plutôt qu’une correction directe. |
| B7 | low | defer | La colonne visuelle préférée n’est pas conservée après une ligne courte ; US-214 contient explicitement ce critère V1. La navigation simplifiée actuelle est documentée sans clôturer US-214. |
| B8 | medium | patch | Sélectionner uniquement un LF/CRLF/CR produit zéro rectangle. Ajouter un fin rectangle de fin de ligne pour un saut explicite sélectionné et un test de régression. |
| B9 | low | reject | Tab conserve la navigation de focus standard de la textarea ; les tabulations chargées sont rendues comme quatre espaces. US-211 porte les tabulations du traitement de texte. Intercepter Tab sans mode d’échappement créerait un piège clavier ; pas de changement dans ce prototype. |
| B10 | low | patch | Le heading reste Document sans titre malgré une ouverture. Le protocole ne divulgue ni nom ni chemin ; remplacer le heading par Document texte évite cette qualification incorrecte sans nouvelle métadonnée. |
| E1 | medium | patch | BigInt accepte un Number non sûr déjà arrondi. Refuser Number non entier sûr avant conversion, en conservant la capacité u64 complète pour BigInt ; test codec. |
| V1 | medium | patch | Les tests injectent une sélection après le dialogue et ne prouvent pas le routage Open/Save. Factoriser la route privée de production pour injecter des résultats de dialogue contrôlés et vérifier read/write/cancel/error via le même chemin ; les dialogues réels restent manuels. |

## Verification

node tests/editor-protocol.mjs; tests Worker avec artefact WASM frais; navigateur headless Edge réel via harness existant; conserver anciens tests front/Worker

Validation finale du lot : 40 contrôles, 368 tests workspace, 55 tests natifs release ; sources et lock inchangés. Voir editor-integration-validation.json et editor-integration-review.md. done concerne cette tranche ; les US-113/120 complètes restent ouvertes.
