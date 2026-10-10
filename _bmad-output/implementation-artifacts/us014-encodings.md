---
title: 'US-014 — Encodages de texte'
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

- B9 — false : la priorité BOM/UTF-8/repli est celle de l’US ; la doc exige un choix explicite pour UTF-16 sans BOM et nomme le repli Windows-1252. Aucun autre encodage ne peut être déduit fiablement de ces octets.
- B10 — false : les empreintes fixes viennent des références officielles, les aller-retours vérifient chaque octet et l’oracle indépendant vérifie 1 024 octets de mapping plus 4 000 séquences UTF-8. Modifier simultanément code et assertions pourrait aussi contourner des vecteurs complets ; ce n’est pas un défaut observé.
- Vérification initiale corrigée : empreintes des mappings, offset après é, BOM UTF-16 avec emoji et surrogate bas isolé ajoutés ; les six tests passent.

## Verification

Après bootstrap du garde par le coordinateur : `cargo test -p atelier-unicode --locked --offline`, `cargo clippy -p atelier-unicode --all-targets --locked --offline -- -D warnings`, `cargo fmt -p atelier-unicode`. Le coordinateur effectue la validation globale et les cibles croisées.

Validation indépendante : six tests unitaires, 5 028 cas contre un oracle indépendant, builds MSVC/WASM/musl, Clippy hôte/WASM, fmt et SPDX réussis. Tests musl compilés seulement ; CI distante non exécutée. Preuves : encoding-oracle.json et foundations-validation.json.
