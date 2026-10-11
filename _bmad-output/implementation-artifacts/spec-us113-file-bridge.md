---
title: 'US-113 — Pont natif pour texte brut du démonstrateur'
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

Permettre à la page embarquée de demander ouverture et sauvegarde atomique par dialogues STA, en ne transmettant que les octets du fichier. Ce pont rend le démonstrateur utilisable sans exposer de chemin JS.

## Boundaries & Constraints

Possession desktop-win/src/webview.rs, webview-resources.rs, win32.rs, nouveau file-bridge.rs, tests natifs et docs/us-113-file-bridge.md. lib.rs seulement pour enregistrer le module si nécessaire. Coordinateur possède resources.rs, main.rs/window.rs et CLI de validation; annoncer les hooks utiles à ce dernier. Aucun web/Rust moteur/lock/CI/Graphify. Conserver relais CSP à plafond 768 distinct du plafond bridge. Vérifier bindings uniquement à partir du SDK local déjà épinglé; pas de nouvelle dépendance externe.

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

files.rs = FileBroker grant_read/write/read/write_atomic/revoke et erreurs message_key. file_dialog.rs = StaFileDialogs, DialogOwner unsafe HWND vivant. webview-resources.rs = WebMessage source + TryGetAsString et relais CSP; callback ne doit pas ouvrir de dialogue. webview.rs Shared/WEBVIEW_MESSAGE = notification existante, ajouter file bounded. win32.rs Context/window_proc = UI STA, close/drain existants à préserver. SDK data/webview2/include/WebView2.h = oracle signatures et indices vtables (PostWebMessageAsString).

## Tasks & Acceptance

**Execution:**
- [x] `crates/desktop-win/src/file-bridge.rs` — Parser strict, base64 canonique borné, ID/queue/replay, réponses traduisibles et traitement réutilisable avec backend.
- [x] `crates/desktop-win/src/webview-resources.rs` — Vérifier source exacte, copier/enfiler commande fichier, conserver diagnostics CSP et refus réseau.
- [x] `crates/desktop-win/src/webview.rs` — Queue bornée et PostWebMessageAsString vers document; ownership COM correct, fermeture vide queue.
- [x] `crates/desktop-win/src/win32.rs` — Dépiler STA hors callback, dialogues TXT, broker read/write/revoke, état dirty et avertissement quitter.
- [x] `crates/desktop-win/tests/**` — Matrice parsing/queue/base64/origine, succès/annulation/erreur et tickets révoqués via chemins injectés seulement par code de test de confiance.
- [x] `docs/us-113-file-bridge.md` — Contrat, limitations SharedBuffer et tests de dialogue manuel encore ouverts.

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

cargo test -p atelier-desktop-win --locked --offline; cargo clippy -p atelier-desktop-win --all-targets -- -D warnings; tests natifs globaux après intégration

Validation finale du lot : 40 contrôles, 368 tests workspace, 55 tests natifs release ; sources et lock inchangés. Voir editor-integration-validation.json et editor-integration-review.md. done concerne cette tranche ; les US-113/120 complètes restent ouvertes.
