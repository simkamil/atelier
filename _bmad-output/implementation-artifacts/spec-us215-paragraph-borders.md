---
title: 'US-215 — Bordures et trames des paragraphes paginés'
type: 'feature'
created: '2026-10-12'
status: 'done'
route: 'dispatch'
review_loop_iteration: 1
baseline_commit: '2ae5aecf73f7ff9b7849bebde7facbc54330d5b2'
context: []
---

<frozen-after-approval reason="Poursuite autonome et parallèle autorisée par l’utilisateur">

## Intent

**Problem:** Les paragraphes paginés n’ont aucune géométrie ni commande de rendu pour leurs bordures et trames. ParaProps ne possède pas de champs typés correspondants.

**Approach:** Ajouter des policies séparées et un plan géométrique dans text-layout, puis un adaptateur vers les commandes existantes dans render. Une bordure équivalente encadre plusieurs paragraphes consécutifs; la trame reste propre à chaque paragraphe. Rendre ces primitives dans les backends existants sans coupler le modèle ou l’éditeur TXT à cette fonctionnalité.

## Boundaries & Constraints

**Always:** L’agent possède text-layout/src/borders.rs, text-layout/tests/borders.rs, render/src/paragraph_borders.rs, render/tests/paragraph_borders.rs et docs/us-215-paragraph-borders.md. Le parent possède lib.rs, Cargo et backlog. Policies avec bords haut/bas/gauche/droite/entre, style Solid/Double/Dashed/Dotted, épaisseur positive, RGB, distance au texte non négative, trame RGB optionnelle. Dimensions de layout en 1/64 pt, conversion vers points seulement dans l’adaptateur render. Entrées explicites des bornes de colonne x/largeur par paragraphe: ne pas assimiler la largeur du texte à celle de la colonne. Consommer ParagraphLayout + Pagination; valider toutes les plages et placements utilisés, ordre source, unicité, conservation des lignes, dimensions positives et arithmétique. Regrouper seulement les paragraphes source adjacents sur une page, avec mêmes bords, espacements et bornes horizontales; la couleur de trame NE rompt PAS un cadre identique. Un paragraphe sans bord interrompt le groupe. Les paragraphes à zéro ligne n’ont pas de cadre et interrompent le regroupement. Chaque groupe donne haut/bas une fois et côtés continus; between seulement entre paragraphes distincts du groupe, jamais entre lignes du même paragraphe. Un groupe multipage est coupé en cadres fermés indépendants par page, règle Atelier documentée sans comparaison Word authentique. Trames sous les bordures; elles couvrent le rectangle de contenu de chaque fragment de paragraphe, sans le vide d’espacement entre paragraphes. Trait Double: deux lignes dont l’enveloppe totale correspond à l’épaisseur demandée; styles tirets/points découpés géométriquement, car StrokeStyle n’a pas de dash. Les points sont ronds. Adapter vers Command/DisplayPath/FillPath ou StrokePath existants et vérifier une sortie SVG et son encodage DisplayList. Budgets explicites de fragments/primitives/travail et vérifications avant allocation; aucun résultat partiel exposé sur erreur.

**Never:** Ajouter des champs à ParaProps, utiliser des opaques inventés, modifier les autres modules ou créer un cycle Cargo. Aucune dépendance externe ni unsafe, aucun commit ni Graphify par l’agent. Ne pas prétendre que l’éditeur riche, l’import OOXML ou la fidélité Word sont validés. Le parent ajoute render → text-layout, sans dépendance inverse, puis régénère Cargo.lock hors réseau et le manifeste WASM.

## I/O & Edge-Case Matrix

| Scénario | Entrée / état | Résultat attendu | Erreur |
|---|---|---|---|
| Cadre isolé | quatre côtés, largeur de colonne supérieure au texte | quatre côtés aux bornes explicites, distances et épaisseurs respectées | aucune |
| Cadre partagé | trois paragraphes adjacents de mêmes bords | un haut/bas, côtés continus, deux between | aucune |
| Trames distinctes | mêmes bordures, deux couleurs de trame | cadre partagé et deux fonds indépendants sous les traits | aucune |
| Ruptures | bord/couleur/distance/colonne différente ou paragraphe sans bord/zéro ligne | cadres distincts, aucun regroupement au-delà du séparateur | aucune |
| Multipage | paragraphe ou groupe coupé sur deux pages | cadres fermés locaux à chaque page, aucun between intra-paragraphe | aucune |
| Styles | solid/double/dashed/dotted avec couleurs et tailles variées | géométrie déterministe, points ronds, double d’enveloppe bornée | aucune |
| Rendu | plan valide → DisplayList → binaire/SVG | validation et roundtrip identiques, commandes dans le bon ordre | aucune |
| Malformé | largeurs nulles, épaisseur nulle, coordonnées extrêmes, indices/plages doublés ou manquants | aucun plan ni commandes partiels | erreur typée |
| Limites | très longue bordure en pointillés et petit budget | progression bornée avant allocation excessive | erreur de limite |

</frozen-after-approval>

## Code Map

- `crates/text-layout/src/{lib,pagination}.rs` — géométrie 1/64pt et placements; width est la largeur effective du texte.
- `crates/render/src/lib.rs` — Command, DisplayPath, StrokeStyle, Color et budgets; pas de tirets natifs.
- `crates/render/src/svg.rs` — backend existant sans changement de format.
- `crates/geom/src/lib.rs` — points/rectangles validés utilisés par l’adaptateur.

## Tasks & Acceptance

- [x] `crates/text-layout/src/borders.rs` — policies et plan validé borné, regroupement et trames.
- [x] `crates/render/src/paragraph_borders.rs` — adaptateur déterministe vers commandes existantes.
- [x] `crates/{text-layout,render}/tests/*borders.rs` — matrice, géométrie et rendu SVG/binaire.
- [x] `docs/us-215-paragraph-borders.md` — unités, API, styles, fermeture multipage et limites.

Le pipeline layout → pagination → bordures → DisplayList fonctionne par API et produit des commandes réellement consommables par les backends. Les résultats numériques ne sont pas présentés comme une comparaison visuelle avec Word; l’état utilisateur reste partiel tant que le rendu riche n’est pas raccordé.

## Implementation Notes

Shell cmd.exe/login:false, cwd C:/mon-bureau. Lire le rapport Graphify avant les sources. Tests ciblés seulement; informer le parent des exports/dépendances nécessaires pour compiler. Si une surface diffère, adapter au contrat des types existants et signaler une impossibilité réelle.

## Spec Change Log

## Review Triage Log

## Verification

- `cargo test -p atelier-text-layout --test borders --locked --offline`
- `cargo test -p atelier-render --test paragraph_borders --locked --offline`

Tranche API validée le 2026-10-12 : 739 tests workspace Windows, 78 text-layout release, 21 contrôles. L’US utilisateur reste partielle selon docs/backlog. Revue et matrice : `rich-incremental-borders-review.md`, `rich-incremental-borders-matrix.json`.
