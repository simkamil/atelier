# US-013 — Casse et normalisation

Deux modules de `atelier-unicode` s'appuient sur les tables Unicode 17.0.0 de US-010 (`properties`). Aucune table n'a été ajoutée : `generated.rs` et `tools/ucd-gen` sont inchangés, donc la taille WASM ne varie que du code des deux modules.

## API

`atelier_unicode::case` (toutes les fonctions prennent un `&str` valide et renvoient un `String`) :

- `to_uppercase`, `to_lowercase`, `to_titlecase`, `to_case_folded` : mappings complets (ß → SS, ŉ → ʼN, İ → i̇), règle conditionnelle `Final_Sigma` de `SpecialCasing.txt` pour la minuscule et la casse de titre.
- `to_titlecase` : par mot UAX #29 (US-011), le premier scalaire casé prend son mapping de titre (ǆ → ǅ, ß → Ss), les suivants passent en minuscule.
- `to_case_folded` : repli complet par défaut (statuts C et F de `CaseFolding.txt`).
- `caseless_eq(a, b)` : comparaison insensible à la casse canonique, `NFD(fold(NFD(x)))` (D145).

`atelier_unicode::normalization` : `nfd`, `nfc`, `is_nfd`, `is_nfc`. Décomposition canonique récursive, ordonnancement canonique par classe de combinaison, composition avec exclusions (`Full_Composition_Exclusion` déjà filtrée par le générateur) et Hangul algorithmique. `is_nfd` est exact et sans allocation ; `is_nfc` répond sans allocation pour les chaînes sous U+0300 et compare sinon avec `nfc`.

## Limites

- Règles dépendantes de la langue (`tr`, `az`, `lt` de `SpecialCasing.txt`) : hors périmètre, non appliquées. Les lignes restent lisibles via `properties::conditional_casing_rules()` et `case_fold_turkic`.
- Pas de NFKC/NFKD (les décompositions de compatibilité ne sont pas utilisées).
- La casse de titre suit la segmentation en mots par défaut, sans dictionnaire ni exceptions linguistiques.
- Chaque fonction est linéaire en la taille de l'entrée : le réordonnancement des suites de marques utilise un tri par comptage au-delà de 32 marques ; le contexte `Final_Sigma` ne parcourt chaque suite de caractères ignorables de casse qu'un nombre borné de fois. Aucune panique possible sur une chaîne UTF-8 valide.

## Preuves

- `crates/unicode/tests/normalization.rs` : 100 % de `NormalizationTest.txt` (20 034 lignes, 200 340 contrôles NFC/NFD), invariance de tous les scalaires absents de la partie 1, exactitude de `is_nfc`/`is_nfd` sur chaque scalaire, lois d'idempotence sur des chaînes pseudo-aléatoires, test de 600 000 marques combinantes sans comportement quadratique.
- `crates/unicode/tests/case.rs` : majuscule, minuscule, titre et repli de chaque scalaire comparés à `UnicodeData.txt`, `SpecialCasing.txt` (lignes inconditionnelles) et `CaseFolding.txt` ; cas ciblés (ß, ŉ, ǅ, sigma final, İ) ; entrées longues.

## Validation d’intégration — 2026-10-12

La [passe finale](../_bmad-output/implementation-artifacts/styles-formats-raster-validation.json)
réussit 41 contrôles, 507 tests workspace et 55 tests natifs release,
avec sources et lock inchangés. Les 57 assertions de l’éditeur utilisent le vrai
Worker/WASM. Clippy Windows/WASM, format, licences et builds Windows/WASM/musl
passent ; les tests musl sont compilés localement et exécutés en CI Linux.
La [copie portable](../_bmad-output/implementation-artifacts/styles-formats-raster-portable.json)
fonctionne avec le seul atelier.exe et ses 18 ressources embarquées.
Les limites des US parentes et l’échec strict de handles US-111 restent ouverts.
