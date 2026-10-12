---
title: 'US-228 — espaces françaises automatiques et annulables'
type: feature
created: '2026-10-12'
status: done
route: dispatch
baseline_commit: 76b1ab0771ea32c8edf72a39e0a2cbb26c3c8997
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

Dans le démonstrateur TXT, assurer les espaces insécables françaises pendant la frappe ordinaire, avec une option visible activée par défaut. La frappe brute est enregistrée d'abord; la substitution typographique est ensuite une transaction distincte. Ctrl+Z immédiat annule uniquement cette substitution, laissant ponctuation et mot saisis.

## Boundaries & Constraints

**Always:** U+00A0 avant deux-points et à l'intérieur de « »; U+202F avant ; ! ?. Insérer l'espace manquante après un caractère non blanc, convertir une espace ASCII existante, préserver une espace déjà correcte. À la frappe de «, ajouter U+00A0 après; si l'utilisateur tape ensuite une espace ASCII redondante immédiatement après « U+00A0, retirer uniquement cette espace redondante. Pour » et ponctuation, ne rien insérer au début du document ou après un saut de ligne/une tabulation. Ne normaliser ni une autre espace Unicode ni une séquence de ponctuation (exemple ?!). Ne franchir aucun séparateur de paragraphe. Offsets UTF-8; transactions gardées par révision. Choix produit explicite: la frappe des guillemets crée les espaces intérieures, sans nécessiter une espace manuelle.

**Never:** transformer collage/dépôt/composition/remplacement/multi-caractères, sélection non vide avant frappe, texte importé, ou frappe lorsque l'option est désactivée. Ne grouper la substitution ni avec frappe précédente ni suivante; ne prétendre la saisie riche US-220 complète.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| punctuation | mot puis : ; ! ? sans espace ou avec ASCII | NBSP avant :; NNBSP avant ;!? | Aucune transformation sans préfixe pertinent |
| quotes | « puis texte puis », espace manuelle après « NBSP | Espaces intérieures NBSP, aucune duplication | Aucun |
| unicode | Préfixe accentué/emoji, NBSP/NNBSP existantes | Offsets UTF-8 exacts, espaces existantes conservées | Invalid boundary refusée |
| boundaries | Début, CR/LF/CRLF/U+2028/U+2029/tabulation, ponctuation répétée | Aucun franchissement ni espace parasite | Aucun |
| disabled | Option décochée | Frappe exacte sans transformation | Aucun |
| exempt | Collage, composition, remplacement, sélection, multi-scalaire | Texte exact sans transformation | Aucun |
| undo | Mot + frappe + substitution puis Undo/Redo | Première annulation restaure la frappe brute seulement; Redo remet la substitution | Historique isolé |
| failure | Révision périmée, longueur finale/historique trop grands | Substitution rejetée sans altérer l'état de la frappe déjà validée | Erreur stricte et état conservé |

</frozen-after-approval>

## Code Map

- `web/text-input.mjs`: onCommand insertText avec inputType, collage/composition identifiables.
- `web/editor.mjs`: parent enregistre frappe brute, puis appelle le helper seulement si sélection initiale vide et option active.
- `crates/text-model/src/lib.rs`: EditKind::Other isole la substitution.
- `web/app.mjs`, `web/index.html`, `web/i18n/fr-FR.txt`: option accessible, préférence locale best effort.

## Tasks & Acceptance

**Execution:**
- [x] `web/french-spacing.mjs`, `tests/french-spacing.mjs` — agent french_spacing: helper pur et tests uniquement. API frenchSpacingEdit({text,anchor,focus}, insertedText, inputType='insertText', enabled=true) appelée APRÈS la frappe brute. Retour null ou {start,end,text}; la plage inclut la ponctuation nouvellement saisie pour laisser le caret à sa fin via remplacement isolé. Vérifier caret effondré et offset UTF-8 valide, un seul scalaire saisi, correspondance de suffixe. Parent garantit la sélection initiale vide. Ne pas modifier éditeur/protocole/docs.
- [x] `web/editor.mjs`, `web/editor-protocol.mjs`, `crates/core-wasm/src/editor.rs`, `crates/text-model/src/lib.rs` — parent: ReplaceRange=17, révision attendue, transaction isolée maintenant sélection antérieure au Undo.
- [x] `web/app.mjs`, `web/index.html`, `web/app.css`, `web/i18n/fr-FR.txt`, `crates/desktop-win/src/resources.rs` — parent: option étiquetée, navigable au clavier, préférence conservée si stockage disponible.
- [x] `tests/editor-browser.mjs`, `crates/core-wasm/tests/editor_engine.rs`, `docs/PROTOTYPE-PROGRESS.md`, `US-V0-V1.md` — parent: tests moteur/navigateur réels et suivi honnête.

**Acceptance Criteria:**
- Given la frappe ordinaire, when une ponctuation française est saisie, then les caractères exportés et les annulations suivent la matrice.
- Given l'option désactivée, when texte français est saisi ou collé, then aucun caractère n'est réécrit.
- Given une composition active, when elle est validée, then son texte est conservé exactement et aucune transaction typographique n'est ajoutée.

## Implementation Notes

## Spec Change Log

## Review Triage Log

Every independent finding and its individual verdict is recorded in [the shared triage](editor-drag-spec.md#review-triage-log) and editor-drag-spacing-perf-review.json.

## Verification

- `node --test tests/french-spacing.mjs` — cas purs et offsets.
- Tests transactionnels Rust, WASM réel et navigateur conduits par le parent.

Final receipt: editor-drag-spacing-perf-validation.json; 23 gates, 749 Windows tests, 78 layout release tests, 82 browser assertions. Frozen matrices are traced by editor-drag-spacing-perf-matrix.json. Native external Windows drag and the 16 ms UI criterion remain open.
