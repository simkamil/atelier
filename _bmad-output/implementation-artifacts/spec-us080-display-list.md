---
title: 'US-080 — Liste d’affichage par page'
type: 'feature'
created: '2026-10-10'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
context: []
baseline_commit: 'b9297cd3a94293a913b2b79ba66c06e3acf4024c'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Le rendu doit représenter une page une seule fois pour permettre à plusieurs backends de produire un résultat cohérent à l’écran, en SVG, en raster et en PDF. Il manque un contrat compact, portable et validable pour ces commandes.

**Approach:** Ajouter dans `atelier-render` une API de liste d’affichage ordonnée par page et une sérialisation binaire versionnée maison. Les ressources image et police/glyphe restent des identifiants opaques fournis par le document; cette story ne charge ni images ni polices.

## Boundaries & Constraints

**Always:** Réutiliser `atelier-geom` via une dépendance de chemin locale; ne pas modifier `geom` ni éditer `Cargo.lock` (le coordinateur gère sa résolution). Utiliser Rust `std`/`core` uniquement, sans dépendance externe, avec `#![forbid(unsafe_code)]`, en-tête SPDX AGPL-3.0-or-later, types portables et aucune API de plateforme. Les coordonnées sont des `f64` finis, en unités de page et dans la plage `MAX_COORDINATE` de geom. Les couleurs sont RGBA non prémultipliées, composantes finies dans `[0,1]`; l’alpha est conservé pour les backends. L’encodeur n’émet que des flux valides; le décodeur est strict, borné et ne panique pas sur des octets arbitraires.

**Never:** Implémenter un backend, une rasterisation, l’interprétation de police, un décodeur d’image, le balisage PDF ou une intégration canvas/SVG/PDF. Ne pas dépendre de serde, d’un crate de sérialisation, ni de données natives à une architecture.

Propriété exclusive : `crates/render/**`, `docs/us-080-display-list.md` et ce spec. Le coordinateur gère les fichiers partagés, Graphify/BMAD, backlog, Cargo.lock et Git ; aucun agent supplémentaire. Dépendance geom préparée par le coordinateur avant implémentation. Toutes les commandes Cargo passent `--locked --offline`, formatage limité à cette crate. Choix de portée : piles indépendantes de clips et de transformations ; chaque pop restaure uniquement son état, un clip est fixé en coordonnées de page au moment du push. Les limites de chemins/glyphes sont cumulées sur toute la page, pas seulement par commande. Prévoir un test de format connu, des versions/opcodes inconnus, toutes les troncatures d'une liste complexe et des octets aléatoires bornés. Largeur/hauteur de page sont finies positives ; pas de troncature implicite d'entiers usize vers u32.

## I/O & Edge-Case Matrix

| Scénario | Entrée / état | Résultat attendu | Erreur |
|----------|---------------|------------------|--------|
| Sérialisation valide | Page contenant commandes géométriques et ressources opaques | Octets versionnés déterministes; décodage égal à la liste source | Aucune |
| Flux tronqué ou version inconnue | En-tête, longueur ou opérande incomplet / version non supportée | Aucun résultat partiel accepté | Erreur typée de format |
| État mal équilibré | Pop sans push, ou pile non vide à la fin de page | Rejet du flux et de la construction invalide | Erreur d’état |
| Valeur ou budget invalide | NaN/infini, coordonnée hors plage, compteur ou allocation excessifs | Rejet avant dépassement de capacité | Erreur de géométrie ou limite |

</frozen-after-approval>

## Code Map

- `crates/render/Cargo.toml` -- ajouter la dépendance locale `atelier-geom` sans dépendance externe.
- `crates/render/src/lib.rs` -- API publique, commandes, erreurs, encodeur/décodeur et tests unitaires.
- `crates/geom/src/lib.rs` -- primitives réutilisées (`Point`, `Rect`, `Path`, `AffineTransform`, `GeometryError`); référence seulement, aucune modification.
- `docs/us-080-display-list.md` -- format binaire, contrats d’état et usage futur.
- `SPEC.md` §6.6, `US-V0-V1.md` US-080/081/082/101/331 -- exigences aval et traçabilité.

## Tasks & Acceptance

**Execution:**
- [x] `crates/render/Cargo.toml` -- déclarer `atelier-geom` comme chemin local -- partager les types géométriques sans duplication.
- [x] `crates/render/src/lib.rs` -- définir page, liste, commandes, erreurs, validation et codec versionné strict -- fournir le socle commun aux backends.
- [x] `crates/render/src/lib.rs` -- couvrir les commandes, aller-retour codec et erreurs/limites par tests unitaires -- prouver la sûreté sur entrées malformées.
- [x] `docs/us-080-display-list.md` -- documenter le contrat, les budgets, les unités et les conventions des backends futurs.
- [x] `SPEC.md` / `US-V0-V1.md` -- ne pas modifier ces sources de référence dans cette story.

**Acceptance Criteria:**
- Étant donné une liste valide, quand elle est encodée puis décodée, alors l’ordre et toutes les valeurs des commandes sont préservés.
- Étant donné une page, quand l’appelant construit sa liste, alors il peut émettre remplissage et contour de chemin, séquences de glyphes positionnés (référence de police, identifiant de glyphe, position et avance), image référencée, push/pop de clip, transformation affine, zone de lien et marqueur structurel/accessibilité avec rôle et identifiant de nœud opaque.
- Étant donné les clips et transformations, quand un backend rejoue les commandes, alors les push/pop sont des portées imbriquées et équilibrées; un clip est intersecté avec le clip courant, la transformation courante s’applique aux commandes de dessin et les états reviennent à leur valeur précédente au pop. L’ordre des commandes est l’ordre de peinture; chaque page commence avec clip infini et transformation identité.
- Étant donné une forme, couleur ou transformation invalide, quand elle entre dans la liste ou le codec, alors elle est rejetée selon les validations de geom et les règles de couleur finie `[0,1]`.
- Étant donné des octets tronqués, inconnus, surnuméraires ou dépassant les budgets, quand le décodeur les lit, alors il retourne une erreur typée sans panique, allocation disproportionnée ni acceptation partielle.
- Étant donné un flux valide, quand l’encodeur l’écrit, alors le format porte une signature, une version explicite et des longueurs/comptes contrôlables; toute version inconnue est refusée.
- Étant donné les crates render et geom, quand la compilation cible une plateforme supportée, alors aucune dépendance externe ni code unsafe n’est requis et les API sont indépendantes du backend.

## Implementation Notes

API suggérée: `DisplayList { page: PageInfo, commands: Vec<Command> }`; `Command` couvre `FillPath`, `StrokePath`, `DrawGlyphRun`, `DrawImage`, `PushClip`, `PopClip`, `PushTransform`, `PopTransform`, `Link` et `Structure`. `PageInfo` contient un rectangle de page. Le codec utilise une signature stable, version majeure/mineure et opcodes explicitement numérotés en little-endian; ne pas sérialiser directement la représentation mémoire des structs.

Le décodeur expose des limites configurables ou constantes documentées sur les octets totaux, commandes, segments de chemin, glyphes et profondeur des piles. Vérifier les longueurs et les sommes avant allocation; refuser opcode/champ réservé inconnu, dépassement d’entier, octets de fin inattendus et piles de clip/transformation non équilibrées à la fin de page. Le remplissage d’un chemin peut utiliser la règle non-zéro ou pair-impair; le contour porte largeur positive finie et paramètres de trait définis. Les marqueurs de structure sont des balises de flux et ne changent pas l’état graphique. Les rectangles de lien sont exprimés en coordonnées de page transformées au moment de la commande; la cible est une référence/chaîne bornée, sans navigation exécutée par cette crate.

## Verification

**Commands:**
- `cargo test -p atelier-render --locked --offline` -- tests de l’API, codec et erreurs passent.
- `cargo clippy -p atelier-render --all-targets --locked --offline -- -D warnings` -- aucune alerte.
- `cargo fmt -p atelier-render -- --check` -- format conforme.

**Manual checks (if no CLI):**
- Vérifier qu’aucun backend n’est créé et que le diff ne touche ni `crates/geom/**`, ni `Cargo.lock`.

## Spec Change Log

## Review Triage Log

| ID | Source | Constat | Évaluation | Résolution / preuve |
| --- | --- | --- | --- | --- |
| B6 | Aveugle | L'encodeur ATDL contrôle 64 Mio après une commande et peut allouer au-delà du budget. | `medium`, confirmé ; `patch`. | Corrigé : pré-calcul exact avec arithmétique vérifiée avant validation/allocation/écriture ; tests des dix opcodes et refus avec budget réduit. |
| B7 | Aveugle | Composition de transformations validée en fin de décodage avec InvalidValue. | `false`, rejeté. | Les compositions sont bien validées avant tout retour ; une valeur numérique composée invalide relève de InvalidValue. Aucun code InvalidState n'est promis pour ce cas. |
| B8 | Aveugle | Rejet de version mineure inconnue prétendument non documenté. | `false`, rejeté. | Le format fixe 1.0 et la documentation précise que les versions inconnues sont refusées. Aucun contrat de compatibilité future n'est promis. |

Revue croisée du lot : [journal complet](next-batch-review.md). Aucun constat différé.

Validation finale du lot : 91 tests du workspace, builds MSVC/WASM/musl, Clippy hôte/WASM sans avertissement, rustfmt, SPDX et garde du lock réussis. Les tests musl sont compilés localement ; leur exécution Linux relève de la CI distante du commit publié. Preuves : [validation](next-batch-validation.json), [contrôles complémentaires et banc](next-batch-extra-checks.json), [oracle indépendant](compression-oracle.json). Aucun constat de revue différé.
