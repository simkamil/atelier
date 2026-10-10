---
title: 'Compression DEFLATE et zlib (US-020/021)'
type: 'feature'
created: '2026-10-10'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/SPEC.md'
  - '{project-root}/US-V0-V1.md'
  - '{project-root}/crates/compress/src/lib.rs'
  - '{project-root}/docs/us-020-021-compression.md'
baseline_commit: 'b9297cd3a94293a913b2b79ba66c06e3acf4024c'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Le workspace ne sait pas décoder ni encoder DEFLATE, nécessaire aux paquets ZIP, aux PNG et aux flux Flate des PDF. Les entrées sont non fiables et peuvent provoquer une expansion excessive.

**Approach:** Compléter `atelier-compress` en Rust autonome avec un décodeur incrémental DEFLATE brut/zlib à sortie plafonnée, ainsi qu’un encodeur LZ77/Huffman en niveaux rapide et normal. Garder les buffers de sortie contrôlés par l’appelant et les formats explicitement choisis.

## Boundaries & Constraints

**Always:** Zéro dépendance externe; `std`/`core` seulement; `#![forbid(unsafe_code)]`; en-tête SPDX `AGPL-3.0-or-later`. RFC 1951: blocs stockés, Huffman fixe/dynamique, distance jusqu’à 32 KiB, bits de champs et codes Huffman selon leurs ordres distincts. RFC 1950: en-tête zlib, DEFLATE, Adler-32 final; vérifier l’en-tête et le checksum. CRC-32 est une primitive publique utile aux consommateurs ZIP, indépendante du framing DEFLATE/zlib. Les erreurs de données invalides/tronquées ou de plafond doivent être des valeurs `Result`, sans panique.

**Never:** Pas de gzip, dictionnaire prédéfini zlib, format ZIP, buffering sans borne, dépendance vers `zip`/`image`, ni changement de `Cargo.lock`. Ne pas promettre que CRC-32 appartient au format zlib.

Propriété exclusive : `crates/compress/**`, `docs/us-020-021-compression.md` et ce spec. Le coordinateur gère les fichiers partagés, Graphify/BMAD, backlog et Git ; aucun agent supplémentaire. Les commandes Cargo passent `--locked --offline`, le formatage est limité à cette crate. Le framing fait partie de l'API de compression (`format` explicite), comme de décompression. Les niveaux Fast et Normal utilisent tous deux LZ77 par chaînes de hachage et Huffman dynamique, avec des budgets de recherche distincts ; ne pas remplacer par de simples blocs stockés ou Huffman fixe. La mémoire du décodeur reste bornée indépendamment du volume total d'entrée/sortie, même si `push` reçoit un grand morceau. Les corpus indépendants couvrent les blocs dynamiques, distances chevauchantes, fenêtre 32 KiB, arbres dégénérés autorisés, troncatures et checksums. Une vérification externe par zlib de la bibliothèque standard Python est un outil d'agent, pas une dépendance du produit ou de la CI.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Flux incomplet | Données découpées à chaque frontière possible; buffer de sortie court | Reprendre avec état conservé; même octet final que le flux monolithique | Signaler besoin d’entrée/sortie; `finish` sur données incomplètes retourne `UnexpectedEof` |
| Expansion au plafond | Flux valide dépassant `max_output` | N’émettre aucun octet au-delà du plafond | `OutputLimit` dès que le prochain octet excède la limite |
| Données malformées | Huffman invalide, distance hors fenêtre, checksum zlib erroné | Aucune lecture hors limites ni corruption mémoire | Erreur typée; état terminal après erreur |
| Entrée vide | Flux DEFLATE/zlib valide représentant zéro octet | Sortie vide et fin confirmée | Rejeter en-tête ou checksum absent en zlib |

</frozen-after-approval>

## Code Map

- `crates/compress/src/lib.rs` -- façade publique, types d’erreur, états incrémentaux, encodeur et fonctions CRC32/Adler32.
- `crates/compress/Cargo.toml` -- conserver les métadonnées du workspace, sans dépendance ajoutée.
- `docs/us-020-021-compression.md` -- contrats d’API, RFC couvertes, méthode et résultats mesurés du corpus.
- `crates/compress/tests/` -- vecteurs fixes et tests de flux/aller-retour; données de référence versionnées sous `crates/compress/tests/corpus/`.

## Tasks & Acceptance

**Execution:**
- [x] `crates/compress/src/lib.rs` -- définir `Format::{Deflate,Zlib}`, `InflateState::new(format, max_output)` et `push(input, output, finish) -> Result<Progress, InflateError>`; `Progress` indique octets consommés/produits et statut `NeedInput`, `NeedOutput`, `Done` -- permettre le pilotage borné sans allocation proportionnelle à la sortie.
- [x] `crates/compress/src/lib.rs` -- implémenter blocs stockés/fixes/dynamiques, fenêtre de 32 KiB, contrôles de longueurs/distances, Adler-32 et CRC-32; exposer `compress(input, level, output)` avec `Level::{Fast,Normal}` et erreur de capacité -- couvrir l’interopérabilité RFC.
- [x] `crates/compress/tests/` -- ajouter vecteurs indépendants RFC/zlib pour les trois types de blocs, les frontières, checksums, corruption, troncature et plafond; tests pseudo-aléatoires déterministes aller-retour dans les deux niveaux -- prévenir régressions et dépendance aux sources de hasard système.
- [x] `crates/compress/tests/corpus/` et `docs/us-020-021-compression.md` -- constituer des entrées texte répétitif, texte mixte UTF-8, binaire répétitif et incompressible, enregistrer tailles compressées et ratio par niveau -- rendre le compromis documenté et reproductible.

**Acceptance Criteria:**
- Given un flux DEFLATE ou zlib RFC valide avec n’importe quel type de bloc, when il est fourni par morceaux et parfois avec un buffer de sortie d’un octet, then le décodage reprend sans perte et vérifie Adler-32 pour zlib.
- Given une limite de sortie de N octets, when une sortie de N+1 octets est demandée, then au plus N octets sont produits et une erreur `OutputLimit` est rendue.
- Given des flux malformés, tronqués, une distance impossible ou un checksum erroné, when le décodeur les traite, then il retourne une erreur stable sans panic.
- Given chaque entrée du corpus, when les niveaux rapide et normal compressent dans un buffer suffisant, then US-020 décompresse exactement l’entrée; les tailles et ratios sont rapportés par niveau.
- Given entrée vide, faible capacité et données pseudo-aléatoires de longueurs couvrant les frontières de blocs, when compressées puis décompressées, then la sortie correspond octet pour octet, ou l’encodeur retourne une erreur de capacité explicite.

## Implementation Notes

RFC de référence : [RFC 1950](https://www.rfc-editor.org/rfc/rfc1950.html) définit l’enveloppe zlib et Adler-32; [RFC 1951](https://www.rfc-editor.org/rfc/rfc1951.html) définit les blocs DEFLATE et le codage LZ77/Huffman. DEFLATE seul n’inclut pas de checksum. Corpus absent au baseline: construire des fixtures transparentes et garder les générateurs déterministes. Le générateur dynamique doit limiter les longueurs de codes conformément au format. L’API proposée est synchronisée et pilotée par l’appelant; ne pas exposer un `Read`/`Write` qui impose buffering ou bloque en WASM.

## Spec Change Log

## Review Triage Log

| ID | Source | Constat | Évaluation | Résolution / preuve |
| --- | --- | --- | --- | --- |
| B1 | Aveugle | Documentation compression affirmant que Cargo.lock ne change pas malgré l'ajout render→geom du lot. | `low`, formulation ambiguë ; `patch`. | Corrigé : compression sans dépendance ajoutée, mise à jour locale render→geom explicitement distinguée. |
| B2 | Aveugle | Incohérence présumée entre quatre corpus et seize flux annoncés. | `false`, rejeté. | Quatre corpus × deux niveaux × deux framings = seize ; les vecteurs de décodage forment un ensemble distinct. |
| B3 | Aveugle | « écrit dans directement dans » dans la documentation. | `low`, coquille ; `patch`. | Corrigé. |
| B4 | Aveugle | Mémoire O(n) de l'encodeur non explicitée. | `low`, contrat documentaire incomplet ; `patch`. | Corrigé : tokens et chaînes de positions proportionnels à l'entrée ; mémoire bornée indépendamment du volume seulement pour inflate. |
| B5 | Aveugle | Tri répété des nœuds Huffman supposé quadratique sur une grande entrée. | `false`, rejeté. | Les alphabets sont bornés par le format : 286 littéraux/longueurs, 30 distances et 19 longueurs de codes. Leur taille ne croît pas avec l'entrée. |
| B10 | Aveugle | Commandes documentées insuffisantes pour reproduire interopérabilité et ratios. | `low`, confirmé ; `patch`. | Commande de l'oracle indépendant documentée ; son JSON contient les tailles et empreintes, régénération des références explicite et conditionnée au succès. |
| V1 | Vérification | Les allers-retours internes seuls peuvent laisser passer une erreur commune aux deux codecs en CI. | `medium`, confirmé ; `patch`. | Ajout de seize sorties connues validées par zlib indépendant et comparées exactement par un test Rust dans la CI normale. Python reste un outil d'agent selon D11. |

Revue croisée du lot : [journal complet](next-batch-review.md). Aucun constat différé.

## Design Notes

Les interfaces séparent le framing et le cœur: `Format::Deflate` termine au dernier bloc; `Format::Zlib` valide CMF/FLG, rejette les options non prises en charge et valide Adler-32 sur les octets émis. `Progress` évite de confondre suspension normale et fin de flux.

## Verification

**Commands:**
- `cargo test -p atelier-compress --locked --offline` -- succès sur les vecteurs, cas limites et aller-retours.
- `cargo check -p atelier-compress --no-default-features --locked --offline` -- succès sans dépendance optionnelle.
- `cargo tree -p atelier-compress --locked --offline` -- aucune dépendance externe.

Validation finale du lot : 91 tests du workspace, builds MSVC/WASM/musl, Clippy hôte/WASM sans avertissement, rustfmt, SPDX et garde du lock réussis. Les tests musl sont compilés localement ; leur exécution Linux relève de la CI distante du commit publié. Preuves : [validation](next-batch-validation.json), [contrôles complémentaires et banc](next-batch-extra-checks.json), [oracle indépendant](compression-oracle.json). Aucun constat de revue différé.
