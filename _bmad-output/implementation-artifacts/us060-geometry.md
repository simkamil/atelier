---
title: 'US-060 — Géométrie 2D'
type: 'feature'
created: '2026-10-10'
status: 'done'
route: 'dispatch'
baseline_commit: 'NO_VCS'
review_loop_iteration: 0
context: ['C:/mon-bureau/Cargo.toml']
---

<frozen-after-approval reason="Intention autorisée par la demande de développer les US">

## Intent

Implémenter US-060 dans atelier-geom : points, rectangles, matrices affines, chemins avec lignes, courbes quadratiques/cubiques et fermeture, boîtes englobantes et aplatissement avec tolérance. Ces primitives serviront au rendu sans dépendre des autres nouvelles US.

## Boundaries & Constraints

Toujours : Rust stable, std/core uniquement, zéro dépendance externe, SPDX AGPL-3.0-or-later, forbid(unsafe_code), portabilité Windows/WASM/Linux. Le coordinateur orchestre BMAD/Graphify : implémenter directement sans autre workflow ni agent. Propriété exclusive : `crates/geom/**`, `docs/us-060-geometry.md`, Implementation Notes de ce fichier. Ne pas modifier fichiers partagés, Cargo.lock ou backlog.

Coordonnées f64, convention de composition affine explicitement documentée et testée avec transformations non commutatives. API claire pour identité, translation, échelle, rotation, application et inverse (matrice singulière signalée). Rectangles normalisés et union/intersection explicites. Chemins à sous-chemins multiples ; fermeture relie au point initial. Boîtes de courbes serrées tenant compte des extrema réels, pas seulement des points de contrôle. Aplatissement conserve les extrémités et la séparation des sous-chemins, garantit une distance au segment <= tolérance pour les points de la courbe. Subdivision bornée ; tolérance non positive/non finie, coordonnées non finies ou budget dépassé retournent une erreur explicite plutôt qu'une approximation prétendument conforme ou une boucle sans fin. Documenter limites numériques finies raisonnables et refuser débordements non finis.

Hors périmètre : rasterisation, clipping polygonal, opérations booléennes, strokes, SVG et UI.

## I/O & Edge-Case Matrix

| Situation | Résultat attendu |
|---|---|
| Points/rectangles inversés, chevauchants, disjoints | Normalisation, union/intersection/containment cohérents |
| Translation puis échelle vs ordre inverse | Convention affine observable ; inverse restaure un point |
| Matrice singulière / valeurs non finies | Erreur ou Option documentée, pas de NaN silencieux |
| Chemin vide, plusieurs sous-chemins, close | Vide représenté, sous-chemins séparés, fermeture correcte |
| Courbes avec extrema internes | Bounds contiennent extrema exacts, cas quadratique et cubique |
| Courbe colinéaire qui dépasse/revient ou extrémités confondues | Aplatissement conserve l'excursion, distance au segment contrôlée |
| Tolérance positive / invalide / trop exigeante | Erreur géométrique testée ; invalides et épuisement borné signalés |

</frozen-after-approval>

## Code Map

- `crates/geom/src/lib.rs` : squelette vide, nouvelles API publiques ici ou modules internes.
- `crates/geom/Cargo.toml` : héritage workspace, pas de dépendance nécessaire.
- `SPEC.md` §6.2 geom et `US-V0-V1.md` US-060 : exigences condensées ici.
- Aucun consommateur existant ; choisir des noms simples et documenter les conventions.

## Tasks & Acceptance

- [x] `crates/geom/**` — primitives, matrices, chemins, bounds et aplatissement.
- [x] Tests — couvrir toute la matrice ; calculs analytiques et échantillonnage indépendant de courbes pour vérifier tolérance/bounds, dont excursions colinéaires.
- [x] `docs/us-060-geometry.md` — exemples d'API, conventions, gestion des erreurs et commandes, sans prétention d'exécution distante.

Critères : tous les types de segments US-060 sont disponibles ; bounds et tolérance sont démontrés par tests non triviaux ; entrées pathologiques sont bornées sans unsafe ni dépendance.

## Implementation Notes

- `atelier-geom` now exposes validated finite `Point`, normalized `Rect`, column-vector `AffineTransform`, and multi-subpath `Path` APIs from `crates/geom/src/lib.rs`.
- Affine composition is documented and tested as first-this-then-next. Inverse reports singular matrices explicitly.
- Path bounds include analytically evaluated quadratic and cubic extrema. Flattening uses bounded De Casteljau subdivision and a finite-segment control-point test, including collinear excursions.
- Geometry rejects non-finite inputs and coordinate magnitudes above `1e150`; flattening reports invalid tolerance and subdivision exhaustion.
- Added crate unit coverage and `docs/us-060-geometry.md` with API examples and local verification commands.

## Spec Change Log

## Review Triage Log

- B1 — low, patch : Dimensions de rectangle au-delà de MAX_COORDINATE : fait réel, documentation trop générale. La limite de coordonnées et les dimensions jusqu’à 2×MAX sont maintenant distinguées ; test de coins opposés passé.
- B2 — false, rejected : Les comparaisons exactes distinguent les polynômes dégénérés et la racine double ; elles ne prouvent pas une perte d’extremum. La formule q normalisée calcule les deux racines. Les références analytiques à deux extrema, ligne et racine double ainsi que 200 200 échantillons indépendants passent. Aucun contre-exemple chiffré de l’issue alléguée n’est fourni ; ajouter un epsilon arbitraire pourrait supprimer une racine valide.
- B3 — false, rejected : rotation accepte les angles f64 finis et utilise sin_cos standard. Aucune sortie invalide ni erreur démontrée dans la plage publique ; limiter arbitrairement les angles changerait le contrat sans cas défectueux établi.
- B4 — low, rejected : Un grand nombre de move_to entraîne une sortie proportionnelle à l’entrée déjà allouée. Le contrat borne les segments de subdivision, pas le nombre de commandes d’entrée. Le cas de millions de sous-chemins vides n’est pas un usage quotidien ; un quota supplémentaire ajouterait état et branches sans amplification liée aux courbes.
- B5 — false, rejected : FlattenedSubpath documente explicitement closed et l’absence de duplication du premier point. Les consommateurs doivent honorer ce champ ; aucun consommateur existant n’omet la fermeture. Les tests vérifient le drapeau, les extrémités et la reprise après close.
- E1 — low, patch : Même incohérence documentaire réelle que B1, conservée comme constat distinct : width/height restent infallibles, dimensions jusqu’à 2×MAX documentées et testées.
- V1 — false, rejected : Le test flatten_keeps_subpaths_and_catches_collinear_excursion échantillonne déjà les bounds de la cubique 0,10,-10,1, dont la dérivée a deux racines internes ; retirer une racine fait échouer les échantillons. Une référence analytique supplémentaire 0,3,-3,0 vérifie désormais explicitement ±sqrt(3)/2.
- R2 — medium, patch : Reprise après close repartait du dernier point et déplaçait la fermeture. Le prochain segment ouvre maintenant un sous-chemin au point initial ; tests ligne/quadratique/cubique et close répété passent.
- R3 — medium, patch : Matrice initiale insuffisamment exercée pour chemin vide et courbes 2D. Ajout des cas vides/move-only, de Bernstein indépendant sur quadratique/cubique et extrémités confondues ; neuf tests de crate passent.

## Verification

`cargo test -p atelier-geom --locked --offline`, `cargo clippy -p atelier-geom --all-targets --locked --offline -- -D warnings`, `cargo fmt -p atelier-geom`. Le coordinateur vérifie le workspace et les cibles croisées après livraison.

Exécuté localement : `cargo fmt -p atelier-geom`, `cargo test -p atelier-geom --locked --offline` (6 tests réussis) et `cargo clippy -p atelier-geom --all-targets --locked --offline -- -D warnings` (réussi). `py -m graphify .` puis `py -m graphify cluster-only .` ont actualisé la carte. Validation complète du workspace et des cibles croisées laissée au coordinateur.

Validation indépendante finale : 67 tests du workspace passent ; builds MSVC/WASM/musl, Clippy hôte/WASM, fmt et SPDX réussis. Tests musl compilés seulement ; CI distante non exécutée. Preuves : foundations-validation.json et foundations-review.md.
