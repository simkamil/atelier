---
title: 'Compression DEFLATE et zlib (US-020/021)'
type: 'feature'
created: '2026-10-10'
status: 'in-review'
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

## Design Notes

Les interfaces séparent le framing et le cœur: `Format::Deflate` termine au dernier bloc; `Format::Zlib` valide CMF/FLG, rejette les options non prises en charge et valide Adler-32 sur les octets émis. `Progress` évite de confondre suspension normale et fin de flux.

## Verification

**Commands:**
- `cargo test -p atelier-compress` -- succès sur les vecteurs, cas limites et aller-retours.
- `cargo check -p atelier-compress --no-default-features` -- succès sans dépendance optionnelle.
- `cargo tree -p atelier-compress` -- aucune dépendance externe.


---
title: 'US-071 — Opérations et transactions atomiques'
type: 'feature'
created: '2026-10-10'
status: 'in-review'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/SPEC.md'
  - '{project-root}/US-V0-V1.md'
  - '{project-root}/docs/us-070-indexed-tree.md'
baseline_commit: 'b9297cd3a94293a913b2b79ba66c06e3acf4024c'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Le modèle possède l'arbre indexé US-070, mais aucune frontière commune ne garantit qu'une modification est annulable ou qu'une commande composée reste indivisible après erreur. US-072 et US-074 dépendent d'un contrat d'opérations exploitable par le futur modèle de texte.

**Approach:** Ajouter une abstraction générique d'état transactionnel et d'opération inversible, puis une transaction ordonnée dont l'application valide ou restaure l'état exact. Le contrat reste indépendant de `IndexedTree<T>` et des futurs types de nœuds ; un adaptateur concret sur l'arbre doit démontrer son utilité dès cette story.

## Boundaries & Constraints

**Always:** Rust stable, `core`/`std` uniquement, `#![forbid(unsafe_code)]`, SPDX AGPL-3.0-or-later. Après échec atomique, restaurer le checkpoint exact, compteur compris : les identités provisoires ne sont jamais publiées. Après une transaction réussie puis son inverse, restaurer le contenu logique (ordre, valeurs et IDs préexistants), tout en conservant le compteur monotone pour ne jamais réattribuer une identité publiée. Les opérations ne produisent aucun effet externe au modèle. Toute erreur est un résultat explicite. Sérialisation déterministe, versionnée, bornée en taille et validée avant application.

Propriété exclusive : `crates/model/**`, `docs/us-071-transactions.md` et ce spec. Le coordinateur gère les fichiers partagés, le backlog, Graphify/BMAD, Cargo.lock et Git ; pas d'agent supplémentaire ni de modification hors périmètre. Décision interne : checkpoint par clone exact pour l'adaptateur `IndexedTree<T: Clone>` seulement ; l'API générique laisse chaque état définir son checkpoint. Documenter le coût proportionnel à l'arène allouée et aux valeurs copiées ; les mutations AVL restent O(log n). Toutes les commandes Cargo utilisent `--locked --offline`, formatage limité à la crate.

**Never:** Coupler l'API au seul `IndexedTree`, exiger que ses valeurs implémentent `Clone` pour toutes les opérations, exposer les champs internes de `NodeId`, ajouter une dépendance, ou promettre journal de récupération durable, UI undo/redo ou coédition (US-072/074 et suivantes).

## I/O & Edge-Case Matrix

| Scénario | Entrée / état | Résultat attendu | Erreur |
|----------|--------------|-----------------|--------|
| Transaction valide | opérations applicables sur état initial | Toutes appliquées dans l'ordre ; inverses composées permettent le retour exact à l'état initial | Aucune |
| Échec d'application | une opération échoue après des opérations réussies | Transaction restaurée exactement à son checkpoint ; aucune mutation partielle visible | Erreur de l'opération |
| Échec de préparation / inversion | inverse indisponible ou invalide avant mutation | État inchangé ; transaction refusée | Erreur typée |
| État de départ absent | opération référant un ID ou nœud absent | Aucun état fabriqué implicitement ; résultat inchangé | Erreur typée de précondition |
| Données sérialisées invalides | version inconnue, longueur excessive, données tronquées/incohérentes | Rejet avant application, consommation bornée | Erreur de format/version/limite |

</frozen-after-approval>

## Code Map

- `crates/model/src/lib.rs` — `IndexedTree<T>`, `NodeId` opaque, erreurs et point d'intégration de l'API générique.
- `docs/us-070-indexed-tree.md` — contrats d'identité, complexités AVL/radix et exigences de non-régression.
- `SPEC.md` §6.3 — chaque changement passe par une opération inversible et une commande est une transaction.
- `US-V0-V1.md` US-071 à US-074 — critères de story, dépendances undo/redo et journal.

## Tasks & Acceptance

**Execution:**
- [ ] `crates/model/src/lib.rs` — définir `Operation<S>` avec application, inversion et sérialisation versionnée, plus erreur typée ; définir une transaction générique réutilisable et un checkpoint/restore exact, sans supposer que chaque `S` ou valeur est clonable.
- [ ] `crates/model/src/lib.rs` — démontrer une opération réelle sur `IndexedTree` (insertion/suppression par identité ou rang), en restaurant l'association ID/valeur et le rang ; garder les erreurs d'inversion et de précondition sans mutation.
- [ ] `crates/model/src/lib.rs` — ajouter codec borné avec limites explicites, rejet version inconnue/troncature/données invalides avant mutation ; aucun `unwrap`/panique sur entrée.
- [ ] `crates/model/src/lib.rs` — tests déterministes et pseudo-aléatoires : apply puis inverse égale état initial exact ; échec au milieu, échec de préparation/inversion et décodage invalide conservent l'état.
- [ ] `docs/us-071-transactions.md` — décrire le contrat public, les garanties, limites de codec et exemples concrets.

**Acceptance Criteria:**
- Given une transaction de plusieurs opérations valides, when elle est appliquée puis inversée, then le contenu logique et les IDs préexistants sont restaurés ; une identité nouvellement publiée n'est jamais attribuée à un autre nœud. Un échec restaure aussi le compteur du checkpoint, dont les IDs provisoires n'ont pas été publiés.
- Given une opération qui échoue après une mutation précédente de la transaction, when la transaction est exécutée, then le checkpoint restaure l'état exact et retourne l'erreur sans panique.
- Given une insertion ou suppression réelle dans `IndexedTree`, when l'opération est inversée, then ordre, valeurs et IDs d'origine sont restaurés, y compris après suppression d'un nœud à deux enfants.
- Given des octets dépassant une limite, tronqués ou d'une version inconnue, when ils sont décodés, then le décodeur retourne une erreur bornée sans modifier l'état.
- Given les plateformes Windows, WASM et Linux, when la crate modèle est compilée, then l'API n'exige aucune dépendance externe ni code `unsafe`.

## Implementation Notes


## Spec Change Log


## Review Triage Log


## Design Notes

Le rollback s'appuie sur un checkpoint restaurable à coût/stratégie définis par l'état, car appliquer une suite d'inverses fallibles ne garantit pas une restauration exacte. L'abstraction peut utiliser des inverses possédant les données retirées, mais le contrôleur de transaction restaure depuis le checkpoint en cas d'erreur. Un adaptateur d'arbre peut employer une copie `T: Clone` au départ, tant que cette contrainte reste locale ; si elle est inacceptable, fournir un snapshot interne exact sans rendre les opérations de test la seule implémentation utilisable. Pour `IndexedTree`, les mutations structurelles restent O(log n) ; checkpoint complet par clone coûte O(n), explicitement distinct du coût de l'opération. Évaluer un journal interne ultérieurement si ce coût est bloquant.

## Verification

**Manual checks:**
- Inspecter les signatures pour confirmer que `Operation<S>`/transaction acceptent des états définis par le modèle, que le codec impose des limites avant allocation, et qu'aucun chemin d'erreur ne panique.
- Vérifier que les tests d'intégration couvrent une mutation concrète de `IndexedTree` et comparent aussi le prochain ID après rollback.


---
title: 'US-080 — Liste d’affichage par page'
type: 'feature'
created: '2026-10-10'
status: 'in-review'
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
- [ ] `SPEC.md` / `US-V0-V1.md` -- ne pas modifier ces sources de référence dans cette story.

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
