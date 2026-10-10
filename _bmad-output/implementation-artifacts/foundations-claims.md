# us014-encodings.md

---
title: 'US-014 — Encodages de texte'
type: 'feature'
created: '2026-10-10'
status: 'in-review'
route: 'dispatch'
baseline_commit: 'NO_VCS'
review_loop_iteration: 0
context: ['C:/mon-bureau/Cargo.toml']
---

<frozen-after-approval reason="Intention autorisée par la demande de développer les US">

## Intent

Implémenter US-014 : décoder et encoder UTF-8, UTF-16 LE/BE, Windows-1252, ISO-8859-1, CP437 et MacRoman. Détecter d'abord le BOM, puis UTF-8 valide, sinon Windows-1252. Remplacer les séquences invalides au décodage par U+FFFD, sans panique.

## Boundaries & Constraints

Toujours : Rust stable, std/core uniquement, zéro dépendance externe, SPDX AGPL-3.0-or-later, forbid(unsafe_code), API portable Windows/WASM/Linux. Le coordinateur orchestre déjà BMAD et Graphify : implémenter directement, sans lancer d'autre workflow ni agent. Propriété exclusive : `crates/unicode/**`, `docs/us-014-encodings.md`, Implementation Notes de ce fichier. Ne pas modifier Cargo.toml racine, Cargo.lock, backlog, README ou documentation partagée ; le coordinateur les synchronise après validation.

API publique : `Encoding`, `detect_encoding(&[u8]) -> Encoding`, `decode(&[u8], Encoding) -> String`, `encode(&str, Encoding) -> Result<Vec<u8>, EncodeError>`. L'encodeur émet sans BOM. Le décodeur enlève un BOM initial correspondant à l'encodage explicite et préserve les BOM internes. UTF-16 sans BOM exige un encodage explicite. Pour un caractère non représentable dans un encodage mono-octet, erreur structurée avec caractère et position UTF-8 ; aucun remplacement silencieux.

Hors périmètre : génération UCD US-010, heuristique UTF-16 sans BOM, streaming, normalisation Unicode, autres encodages.

## I/O & Edge-Case Matrix

| Entrée | Résultat attendu |
|---|---|
| Vide, ASCII, UTF-8 accentué/emoji | UTF-8 détecté, aller-retour exact |
| BOM UTF-8 / UTF-16 LE / BE | Priorité BOM, retrait initial, UTF-16 avec paires de surrogates |
| UTF-8 malformé, surrogate isolé, octet UTF-16 final isolé | U+FFFD aux segments invalides, texte voisin conservé |
| UTF-8 invalide sans BOM | Détection Windows-1252 |
| Tous les 256 octets des encodages historiques | Mapping officiel ; octets Windows-1252 non définis → U+FFFD |
| Caractère non représentable | EncodeError sans panique, index UTF-8 correct |
| BOM interne ou encodage explicite mono-octet | Pas de retrait indu |

</frozen-after-approval>

## Code Map

- `crates/unicode/src/lib.rs` : squelette vide, point d'entrée public à compléter ; modules internes libres.
- `crates/unicode/Cargo.toml` : héritage workspace, aucune dépendance nécessaire.
- `SPEC.md` §5.1 et `US-V0-V1.md` US-014 : exigences condensées ci-dessus, autres US hors périmètre.
- Sources primaires vérifiées : unicode.org/Public/MAPPINGS/VENDORS/MICSFT/WINDOWS/CP1252.TXT (2.01), MICSFT/PC/CP437.TXT (2.00), APPLE/ROMAN.TXT. Documenter sources et choix de mapping dans la doc dédiée. Vérifier les 256 valeurs contre ces sources, pas seulement quelques exemples.

## Tasks & Acceptance

- [x] `crates/unicode/**` — implémenter API, mappings, erreurs documentées.
- [x] Tests — couvrir chaque ligne de matrice, fixtures de référence et aller-retour pour chaque valeur définie.
- [x] `docs/us-014-encodings.md` — documenter API, BOM, erreurs et provenance des mappings, commandes reproductibles ; ne pas annoncer de validation distante.

Critères : les sept encodages (UTF-16 LE et BE distincts) sont utilisables publiquement ; tests de corruption ne paniquent pas ; aucune dépendance externe ni unsafe.

## Implementation Notes

- Implémentation réalisée dans `crates/unicode/src/lib.rs` et tables exhaustives dans `crates/unicode/src/tables.rs`.
- Les tables Windows-1252, CP437 et MacRoman ont été générées à partir des fichiers Unicode/Apple cités dans le Code Map ; les contrôles non listés dans la table MacRoman sont conservés à leur valeur ASCII, et les octets Windows-1252 officiellement indéfinis sont représentés par U+FFFD au décodage.
- Les tests parcourent les 256 octets de chacun des quatre encodages mono-octet et vérifient l'aller-retour de chaque valeur décodable.
- Documentation d'API, BOM, erreurs, provenance et validation : `docs/us-014-encodings.md`.

## Spec Change Log

## Review Triage Log

## Verification

Après bootstrap du garde par le coordinateur : `cargo test -p atelier-unicode --locked --offline`, `cargo clippy -p atelier-unicode --all-targets --locked --offline -- -D warnings`, `cargo fmt -p atelier-unicode`. Le coordinateur effectue la validation globale et les cibles croisées.


# us060-geometry.md

---
title: 'US-060 — Géométrie 2D'
type: 'feature'
created: '2026-10-10'
status: 'in-review'
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

## Verification

`cargo test -p atelier-geom --locked --offline`, `cargo clippy -p atelier-geom --all-targets --locked --offline -- -D warnings`, `cargo fmt -p atelier-geom`. Le coordinateur vérifie le workspace et les cibles croisées après livraison.

Exécuté localement : `cargo fmt -p atelier-geom`, `cargo test -p atelier-geom --locked --offline` (6 tests réussis) et `cargo clippy -p atelier-geom --all-targets --locked --offline -- -D warnings` (réussi). `py -m graphify .` puis `py -m graphify cluster-only .` ont actualisé la carte. Validation complète du workspace et des cibles croisées laissée au coordinateur.


# us070-indexed-tree.md

---
title: 'US-070 — Arbre de nœuds indexé'
type: 'feature'
created: '2026-10-10'
status: 'in-review'
route: 'dispatch'
baseline_commit: 'NO_VCS'
review_loop_iteration: 0
context: ['C:/mon-bureau/Cargo.toml']
---

<frozen-after-approval reason="Intention autorisée par la demande de développer les US">

## Intent

Implémenter US-070 : conteneur générique de blocs dans un arbre équilibré, indexé par position et identifiant stable. Insertion, suppression, accès par position et identifiant doivent rester O(log n) ; mesurer 10 000 insertions aléatoires dans 100 000 blocs, objectif <50 ms.

## Boundaries & Constraints

Toujours : Rust stable, std/core uniquement, zéro dépendance externe, SPDX AGPL-3.0-or-later, forbid(unsafe_code), portabilité Windows/WASM/Linux. Le coordinateur orchestre BMAD/Graphify : implémenter directement sans workflow/agent supplémentaire. Propriété exclusive : `crates/model/**`, `docs/us-070-indexed-tree.md`, Implementation Notes de ce fichier. Ne pas modifier les fichiers partagés, Cargo.lock ou backlog.

API générique publique `IndexedTree<T>`, `NodeId` opaque, new/len/is_empty, insertion par position retournant l'identifiant, suppression par position, get(position), get_by_id(id), position_of(id). Les opérations invalides sont signalées sans panique ni mutation partielle. Identifiants jamais recyclés pendant la vie du conteneur, y compris après suppression de tous les éléments ; débordement signalé. Une insertion/suppression déplace les positions sans changer les identités des survivants. Arbre équilibré garantissant O(log n), par exemple AVL avec tailles de sous-arbres et index d'identifiants BTreeMap ; un Vec déplacé ou treap simplement aléatoire ne satisfait pas la garantie. Aucun parcours linéaire caché dans les opérations demandées. Réutiliser les emplacements libres pour borner la mémoire par le pic d'éléments vivants.

Banc autonome en exemple Cargo : fixture de 100 000 blocs hors chronométrage, 10 000 positions pseudo-aléatoires déterministes calculées hors mesure et valides à chaque insertion, travail observable via black_box. Chauffe puis médiane d'au moins cinq mesures release ; afficher taille, nombre d'opérations, médiane et seuil réussi/échoué. Décrire matériel/OS et protocole, sans promettre un seuil universel. Le coordinateur mesure indépendamment sur cet hôte.

Hors périmètre : transactions US-071, undo, sérialisation, IDs persistants entre documents, UI et types de blocs métier.

## I/O & Edge-Case Matrix

| Situation | Résultat attendu |
|---|---|
| Vide, insertion début/milieu/fin | Ordre et positions exacts, identifiants uniques |
| Suppression feuille/racine/deux enfants | Identités survivantes conservées, index cohérent |
| Position hors limites, ID absent ou supprimé | Erreur/None documenté, pas de mutation |
| Supprimer tout puis insérer | Aucun ID réutilisé ; mémoire d'arène réutilisée |
| Longue séquence mixte déterministe | Résultats égaux à un Vec oracle ; tailles, parents, hauteur et balance valides |
| Insertion monotone et rotations | Hauteur logarithmique, accès position/ID correct |
| Compteur d'ID épuisé | Erreur explicite et état conservé (test interne) |

</frozen-after-approval>

## Code Map

- `crates/model/src/lib.rs` : squelette vide, conteneur et API à créer.
- `crates/model/examples/bench_indexed_tree.rs` : nouveau banc std-only, aucune dépendance atelier-bench nécessaire.
- `crates/model/Cargo.toml` : pas de changement nécessaire.
- `SPEC.md` §6.3 : IDs stables et arbre équilibré positionnel, condensés ici ; aucun consommateur existant.

## Tasks & Acceptance

- [x] `crates/model/**` — conteneur équilibré, index d'identifiants, erreurs et invariants.
- [x] Tests — matrice complète avec oracle indépendant, invariants après mutations et cas de suppression à deux enfants.
- [x] Exemple de banc — protocole exact et mesure reproductible.
- [x] `docs/us-070-indexed-tree.md` — API, complexité argumentée, mémoire, protocole et limites ; ne pas annoncer de validation distante.

Critères : O(log n) démontré structurellement et par invariants ; stabilité d'identité prouvée ; banc exécuté en release et résultat documenté honnêtement. Si >=50 ms, signaler le critère incomplet au coordinateur pour optimisation avant clôture.

## Implementation Notes

## Spec Change Log

## Review Triage Log

## Verification

`cargo test -p atelier-model --locked --offline`, `cargo clippy -p atelier-model --all-targets --locked --offline -- -D warnings`, `cargo fmt -p atelier-model`, `cargo run --release -p atelier-model --example bench_indexed_tree --locked --offline`. Validation globale par coordinateur après livraison.
