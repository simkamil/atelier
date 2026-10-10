---
title: 'US-022/023 — Lecture et écriture ZIP'
type: 'feature'
created: '2026-10-10'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/SPEC.md'
  - '{project-root}/US-V0-V1.md'
  - '{project-root}/docs/DEVELOPMENT.md'
baseline_commit: 'ff7f3a3097ae23792afda188c3b5d0b739b76b5d'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Les formats OOXML et ODF reposent sur ZIP, mais `atelier-zip` est un squelette. Un ZIP malformé ou hostile ne doit ni faire paniquer le moteur ni empêcher l’ouverture des entrées valides.

**Approach:** Fournir dans `atelier-zip` un lecteur indexé par répertoire central et un écrivain déterministe réutilisables par les formats documentaires. Le périmètre commun couvre les méthodes Store et Deflate, Zip64, les deux encodages de noms requis et des limites appliquées avant allocation ou décompression.

## Boundaries & Constraints

**Always:** Rust sûr (`#![forbid(unsafe_code)]`), API fondée sur `std`/`core`, aucune dépendance externe, licence SPDX AGPL-3.0-or-later. Limites configurables (`Limits`) avec défauts prudents ; offsets, longueurs et ratio contrôlés sans débordement. Appliquer les plafonds d’entrées, noms, sortie par entrée et cumulée avant allocation/décompression. Les tailles déclarées restent vérifiées contre la sortie réelle et CRC32. Conserver les erreurs locales pour lire les entrées saines. Sans central exploitable, récupérer seulement les en-têtes locaux successifs avec tailles fiables ; signaler l'incomplétude de l'archive, ne pas la présenter comme complète. Si aucune entrée exploitable, retourner une erreur globale.

**Never:** Pas de chiffrement, ZIP multi-disque, méthodes autres que Store/Deflate, normalisation Unicode de compatibilité, extraction automatique sur disque ni recherche naïve des signatures ZIP dans les données. Les chemins absolus, préfixes de lecteur, séparateurs ambigus et composants `..` sont signalés comme noms non sûrs ; ne jamais les transformer en chemins disque implicitement. Les noms UTF-8 invalides avec drapeau UTF-8 sont en erreur ; sinon décoder CP437 avec `atelier-unicode`. Détecter séparément les noms dupliqués après normalisation des séparateurs et composants sûrs.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Archive valide | central directory, Store/Deflate, Zip64 ou classique | Entrées accessibles par index/nom, contenu et CRC validés | Erreur locale sur méthode ou données invalides |
| Nom encodé | UTF-8 signalé, ou CP437 sans drapeau | Nom Unicode décodé sans perte selon encodage | UTF-8 invalide signalé |
| Limite atteinte | tailles, ratio, nombre ou nom dépassés | Refus avant allocation excessive | `LimitExceeded` ciblé |
| Archive partiellement corrompue | enregistrement central ou flux local invalide/tronqué | Entrées saines restent consultables | état/erreur attaché à l’entrée fautive |
| Écriture déterministe | ordre fourni et date fixe | octets stables pour mêmes entrées/options | erreur de nom, compression ou taille |
| Taille Zip64 | seuil dépassé ou option forcée sur fixture minuscule | enregistrements Zip64 valides | erreur d’écriture explicite |

Pour récupérer d’une troncature, ne scanner que des frontières d’enregistrements atteintes séquentiellement depuis l’offset connu, et seulement lorsqu’une longueur/structure permet de sauter le contenu. Avec des descripteurs de données sans taille connue, ne pas deviner la frontière dans le payload : arrêter la récupération et préserver les entrées déjà indexées/lues. Toute entrée centrale est isolée et validée indépendamment ; aucune récupération ne promet de retrouver des données sans métadonnées fiables.

Propriété exclusive : `crates/zip/**`, `docs/us-022-023-zip.md`, ce spec. Le coordinateur gère manifest/dépendances déjà préparées, Cargo.lock, CI, Graphify, backlog et Git ; ne pas les modifier ni lancer Graphify/sous-agents. Cargo utilise `--locked --offline`, fmt limité à la crate. Les références indépendantes de l'agent corpus vivent dans `tests/format-reference/zip/`, en lecture seule. Le code peut être réparti en modules privés. Les métadonnées centrales tronquées mais de longueur connue ne doivent pas interdire les autres entrées indexables. Tester Zip64 forcé avec petites données et les décisions aux seuils u16/u32 sans allocation de 4 Gio.

</frozen-after-approval>

## Code Map

- `crates/zip/src/lib.rs` — API publique et façade `Archive`, `Entry`, `Writer`, erreurs et limites.
- `crates/zip/Cargo.toml` — dépendances locales vers `atelier-compress` et `atelier-unicode` (manifest géré par le coordinateur).
- `crates/compress/src/lib.rs` — `InflateState::new(Format::Deflate, max_output)` et `push` incrémental ; compresser un flux Deflate en buffer dimensionné par l’appelant.
- `crates/unicode/src/lib.rs` — `decode(..., Encoding::Cp437)` et `encode(..., Encoding::Cp437)` ; l’encodeur renvoie une erreur si le nom est inexprimable.
- `docs/us-022-023-zip.md` — contrats, formats et preuves d’interopérabilité.
- `US-V0-V1.md` — mise à jour des critères seulement après validation de l’implémentation par le coordinateur.

## Tasks & Acceptance

**Execution:**
- [x] `crates/zip/src/lib.rs` — définir `Limits`, `Archive::open(&[u8], Limits)`, `entries()`, `entry(name)`, `Entry::read_all()` et `EntryError`; conserver les erreurs locales dans l’index — API sûre et multi-usage, central directory classique/Zip64 avec extensions et descripteurs valides.
- [x] `crates/zip/src/lib.rs` — valider les en-têtes locaux, méthodes 0/8, tailles, CRC et noms; appliquer toutes les limites avant allocation — entrées hostiles bornées et erreurs isolées.
- [x] `crates/zip/src/lib.rs` — exposer `Writer::new(WriteOptions)`, `add(name, bytes, method)`, puis `finish() -> Result<Vec<u8>, ZipError>` — ordre d’appel préservé, horodatage fixe optionnel, Zip64 automatique et `force_zip64` pour petite fixture.
- [x] `crates/zip/src/lib.rs` — ajouter tests unitaires de la matrice, fixtures malformées/tronquées, Zip64 forcé et réouverture par lecteur — aucune panique et limites démontrées.
- [x] `docs/us-022-023-zip.md` — consigner format supporté, limites par défaut, erreurs et décisions de récupération — contrats disponibles aux futurs clients OOXML/ODF.

**Acceptance Criteria:**
- Given un ZIP standard avec répertoire central, Store/Deflate, noms UTF-8/CP437, Zip64 et descripteurs, when le lecteur ouvre et lit les entrées, then contenu, ordre et CRC correspondent aux données sources.
- Given des tailles, ratios, nombres ou noms supérieurs aux limites, when l’archive est indexée ou une entrée lue, then le refus intervient avant l’allocation ou la décompression excédentaire.
- Given une entrée malformée ou tronquée dans une archive indexable, when une autre entrée valide est demandée, then celle-ci reste accessible avec une erreur précise seulement pour l’entrée défectueuse.
- Given des entrées ajoutées dans un ordre et avec une date fixe, when elles sont écrites, then l’ordre est préservé, les octets sont reproductibles et Zip64 est émis si requis ou forcé.
- Given les sorties produites, when elles sont relues par `Archive`, Python `zipfile` et vérifiées dans l’Explorateur Windows, then les noms, tailles et contenus des fixtures concordent.

## Implementation Notes

État de reprise du 2026-10-10 : le lecteur indexé et l'écrivain déterministe sont implémentés dans `crates/zip/src/lib.rs`. Les métadonnées d'entrée sont privées et exposées par getters; les erreurs restent isolées par entrée. Si une archive centrale non vide ne contient aucune entrée sans erreur d'indexation, l'ouverture retourne `NoUsableEntries`; une archive centrale vide valide reste acceptée. Les contrôles couvrent aussi tailles et noms limités avant décodage/allocation, noms Windows non sûrs, descripteurs signés/non signés classiques et Zip64, offsets hostiles, multi-disque global et par entrée, central endommagé de longueur connue, CRC d'une entrée parmi des voisines saines, budget cumulatif, et seuils u16/u32 sans grande allocation. Une régression vérifie qu'un nom local de 65 535 octets est refusé avant décodage quand le central annonce un nom court.

API en mémoire pour cette tranche : les appels ZIP OOXML/ODF disposent déjà des octets d’archive. Le lecteur Deflate doit piloter `InflateState` par blocs vers un buffer limité, contrôler `Progress` et s’arrêter exactement à la fin du flux. Le format Zip64 porte les tailles/offsets étendus du central directory et l’EOCD64/locator ; le writer choisit Zip64 selon tailles, offsets et compte, sans attendre un débordement. Le bit 3 requiert validation d’un descripteur signé ou non signé après le flux, selon les valeurs centrales.

Validation finale : 140 tests du workspace, dont 21 ZIP et 28 XML; les 49 tests ZIP/XML passent aussi en release. Treize contrôles globaux réussissent, 31 paquets locaux et lock préservé. Oracles finaux : XML 105 lectures/105 réécritures/20 rejets, ZIP 52 lectures/48 écritures et quatre variantes de descripteurs; Explorer classique/Zip64 réussi. Trois revues indépendantes triées, correctifs vérifiés et aucun constat différé. Preuves dans `formats-validation.json`, `format-oracle.json`, `explorer-zip.json` et `formats-review.md`.

## Verification

**Commands:**
- `cargo test -p atelier-zip --locked --offline` — tests lecteur/écrivain, corruption, limites et interop locale passent.
- `cargo test -p atelier-zip --release --locked --offline` — mêmes contrats sans dépendre des assertions de debug.
- `python -c "import zipfile; z=zipfile.ZipFile('...'); print(z.testzip())"` sur une fixture générée — affiche `None`.

**Manual checks:** Ouvrir la fixture ZIP générée dans l’Explorateur Windows et extraire les fichiers ; comparer noms et octets à la fixture attendue.

## Design Notes

La troncature ne justifie pas un scan arbitraire des signatures : les octets `PK` peuvent apparaître dans un flux compressé. La reprise s’appuie sur les offsets centraux déjà validés et les longueurs connues ; un descripteur ambigu borne la récupération. L’API retourne les métadonnées fautives par entrée afin que les intégrateurs puissent afficher ou ignorer cette entrée sans perdre les autres.

## Spec Change Log

Plan initial précisé avant implémentation : récupération locale bornée même si EOCD manque, signalement d'incomplétude explicite et erreurs isolées ; ne pas opposer un refus global systématique au critère de document partiellement lisible.

## Review Triage Log

Trois revues indépendantes reçues et triées avant regroupement; voir [revue complète](formats-review.md). Correctifs validés; aucun constat différé.

| ID | Domaine | Verdict | Constat | Preuve / décision | Route |
|---|---|---|---|---|---|
| B1 | ZIP | low | Index central tronqué : les entrées suivantes ne sont pas récupérées. | index_central s’arrête quand la longueur/fenêtre du prochain record n’est plus fiable. Les records de longueur connue restent isolés (test damaged_central_record_with_known_length_does_not_hide_later_records). Une seconde récupération locale avec fusion des index élargirait un cas rare d’archive corrompue; ce n’est pas une correction directe. | reject — limitation de récupération documentée |
| B2 | ZIP | false | Les noms différant seulement par la casse sont acceptés. | Les noms ZIP sont comparés exactement après normalisation des séparateurs/composants; aucune extraction disque n’est effectuée. La casse Windows ne définit pas l’identité des membres ZIP. Le consommateur d’une extraction éventuelle gère les collisions du système cible. | reject |
| B3 | ZIP | low | Une première occurrence déjà rejetée masque le diagnostic de doublon. | Confirmé : mark_duplicates sautait toute entrée déjà en erreur. Deux registres séparent désormais le diagnostic des noms sûrs répétés et le rejet des doublons utilisables. Une homonyme saine reste lisible et son flag de doublon est conservé; régression rejected_entries_do_not_hide_a_healthy_name_lookup. | patch |
| B4 | ZIP | low | La récupération locale s’arrête aux descripteurs ou sentinelles Zip64. | Arrêt bit 3 nécessaire sans tailles centrales fiables : rechercher une signature dans le payload est explicitement interdit. La récupération d’un local Zip64 avec extra fiable reste une extension rare des archives privées de central; le lecteur Zip64 central normal fonctionne. Étendre ce scanner demanderait des branches et validations supplémentaires. | reject — borne explicite de récupération |
| B5 | ZIP | low | Writer garde données originales et comprimées sans plafond d’ajouts. | L’API construit volontairement une archive Vec en mémoire à partir de données fournies par l’appelant; Limits gouverne Archive. La copie originale était néanmoins inutile : Pending conserve désormais seulement sa longueur et son CRC avec les octets comprimés, réduisant la mémoire sans modifier le format. | patch — suppression de copie inutile |
| V1 | ZIP | medium | L’oracle ZIP indépendant n’est pas imposé par la CI Cargo normale. | Constat de vérification accepté. Deux sorties classiques/Zip64 figées, contenant Store/Deflate et Unicode, ont été validées indépendamment par zipfile et sont comparées octet par octet aux sorties Writer dans les tests Rust CI. Toute dérive partagée lecteur/écrivain échouera contre ces formats connus conformes. | patch — fixtures wire indépendamment validées |
