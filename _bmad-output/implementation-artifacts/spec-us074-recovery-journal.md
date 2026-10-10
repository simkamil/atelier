---
title: 'US-074 — Journal de récupération'
type: 'feature'
created: '2026-10-10'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
baseline_commit: '77622b0c410ff9f5686325e81bca1e71cf383642'
context:
  - '{project-root}/SPEC.md'
  - '{project-root}/US-V0-V1.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.8-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-us072-undo-history.md'
  - '{project-root}/docs/us-071-transactions.md'
  - '{project-root}/crates/model/src/lib.rs'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** US-071 fournit des transactions atomiques, mais aucun format durable ne permet de rejouer les transactions validées à partir d’un état de base autonome. US-074 établit le contrat de données générique dont US-325 se servira pour l’autosauvegarde.

**Approach:** Ajouter à atelier-model un codec de génération de récupération et un journal incrémental borné. Le payload d’instantané reste opaque et autonome selon le codec du document; les entrées journal contiennent des transactions ATOP entièrement validées, numérotées et protégées par CRC32.

## Boundaries & Constraints

**Always:** L’enveloppe d’instantané versionnée porte un identifiant de document, un identifiant de génération, et un payload autonome incluant toutes les ressources requises par le contrat du codec appelant. Elle permet les documents sans fichier source. Le journal versionné porte les mêmes identifiants, une séquence de départ et des entrées contiguës. Chaque entrée protège par CRC32 ses champs de séquence, longueur et octets ATOP. Vérifier longueurs, comptes, additions et plafonds avant allocation ou lecture. Décoder entièrement une entrée avec Transaction::decode avant application; utiliser Transaction::apply pour que chaque transaction soit atomique. Le rejeu commence exclusivement depuis le snapshot fourni pour cette génération. Il s’arrête au premier enregistrement tronqué, malformé, de somme incorrecte, de séquence inattendue ou dont application échoue, en gardant l’état précédant cette transaction. Les identifiants ou versions incompatibles produisent une erreur typée; aucune donnée d’une autre génération n’est rejouée.

**Never:** Déduire la base du fichier source courant, appliquer une transaction partielle, reprendre après une entrée invalide, confondre ce journal avec l’historique undo/redo, ou promettre flush/fsync, planification, publication atomique ou suppression de génération. Ces responsabilités appartiennent à US-325 et au stockage appelant.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Snapshot replay | Snapshot autonome et journal cohérent, y compris document vierge ou ressources externes embarquées | Restaurer la base puis appliquer les transactions complètes dans l’ordre; retourner le dernier état valide et la dernière séquence | Codec de snapshot signale les données invalides sans exposer d’état partiellement restauré |
| Incomplete/corrupt tail | Troncature au milieu d’un en-tête, CRC incorrect, ATOP invalide, ou application refusée | Arrêt à la transaction précédente; aucune entrée après la première anomalie n’est consultée | Résultat de reprise indique la séquence arrêtée et sa cause; base préservée |
| Generation mismatch | IDs de document/génération distincts ou version inconnue | Refuser le rejeu avant application | Erreur explicite d’incompatibilité |
| Hostile lengths/counts | Taille, compte, séquence ou addition dépasse une limite | Rejeter sans réservation/allocation guidée par la valeur annoncée | Erreur de limite ou de débordement |

</frozen-after-approval>

## Code Map

- crates/model/src/journal.rs -- enveloppe versionnée, format d’entrée, types d’identifiants/limites, codec et rejeu génériques.
- crates/model/src/lib.rs -- contrats réutilisés: Transaction<O>, Operation<S>, TransactionalState, Transaction::decode/apply; exports laissés au coordinateur.
- crates/compress/src/lib.rs -- API crc32(&[u8]) -> u32; l’ajout du lien de dépendance est coordonné hors de cette propriété.
- docs/us-074-recovery-journal.md -- format binaire, bornes, garanties de rejeu et frontière US-325.

## Tasks & Acceptance

**Execution:**
- [x] crates/model/src/journal.rs -- définir des codecs snapshot génériques et un journal append/replay avec limites configurables et erreurs typées; garder le payload snapshot opaque et restaurer atomiquement via contrat de codec.
- [x] crates/model/src/journal.rs -- tester création/rejeu, document vierge, ressources via codec, séquences, troncatures à chaque frontière, CRC, transaction invalide/échouée, mismatch et plafonds sans mutation partielle.
- [x] docs/us-074-recovery-journal.md -- documenter l’enveloppe, framing, calcul CRC, arrêt au premier défaut, appels API et limites de durabilité.

**Acceptance Criteria:**
- Given un snapshot d’une génération et ses transactions validées, when le journal est rejoué, then l’état résultat est exactement celui obtenu par application ordonnée depuis ce snapshot.
- Given une entrée incomplète, corrompue, invalide ou refusée par l’opération, when le rejeu l’atteint, then l’état demeure au point durable précédent et aucune entrée ultérieure ne s’applique.
- Given des IDs ou versions incompatibles, when le snapshot et le journal sont associés, then le rejeu échoue proprement avant toute application.
- Given des longueurs, comptes ou séquences hostiles, when les octets sont analysés, then les bornes sont contrôlées avant allocation et aucun panic ni réservation arbitraire n’a lieu.

## Implementation Notes

Ce document est le handoff BMAD du coordinateur ; ne pas réinvoquer de skill ni créer d’agent. Graphe déjà rafraîchi : lecture seulement. Le coordinateur expose `pub mod journal` et ajoute atelier-compress. Décisions avant dev : IDs opaques 16 octets chacun ; base sequence u64, entrées contiguës à partir de base+1, overflow refusé ; limites par défaut snapshot 64 Mio, journal 64 Mio, 100 000 entrées (paramétrables), ATOP existant inchangé. Une reprise valide retourne état reconstruit, dernière séquence et cause d’arrêt éventuelle ; un en-tête incompatible est une erreur globale avant replay. Préférer SnapshotCodec::decode retournant un nouvel état S, pour éviter toute restauration partielle visible. Le builder accepte un snapshot déjà sérialisé, borne ses octets avant copie ; les allocations internes du codec caller restent sa responsabilité.

Propriété exclusive: crates/model/src/journal.rs, docs/us-074-recovery-journal.md, ce spec. Le coordinateur gère lib.rs, manifestes, Cargo.lock, CI, docs partagés, Graphify et Git. Implémenter uniquement cette propriété.

Format/API recommandés: enveloppe snapshot à magie/version, UUID-like IDs opaques de document et génération, séquence de base, longueur puis payload borné et CRC32 couvrant l’enveloppe et le payload. Journal à magie/version, IDs identiques, séquence de base, puis enregistrements longueur délimitée contenant séquence, taille ATOP, bytes ATOP et CRC32 little-endian calculé sur les champs et bytes précédents de l’enregistrement. Le snapshot et le journal doivent partager la même séquence de base, contrôlée avant restauration. Une API de builder/append ajoute seulement un Transaction dont encode réussit; replay vérifie l’enveloppe et un enregistrement complet avant decode puis apply. Un trait SnapshotCodec<S> peut fournir encode(&S) et restore(payload) -> Result<S, Error>, afin de construire/valider hors état visible. Le moteur ne peut pas vérifier la sémantique d’autonomie des ressources: c’est une obligation du codec.

Choisir des plafonds constants sûrs pour snapshot total, journal total, et nombre d’entrées, en plus des limites ATOP existantes; aucun champ sur disque ne peut les augmenter. Un append qui dépasse le plafond échoue sans modifier le journal. CRC détecte les corruptions accidentelles, sans authentifier les données. Le journal ne compresse pas les transactions; atelier-compress n’est requis que pour son CRC32. Si le crate n’est pas déjà dépendance autorisée, ajouter le lien local relève du coordinateur et ne doit pas introduire de source externe.

La couche fournit des octets et le rejeu; elle ne gère ni fichiers, ni synchronisation disque. « Dernier état valide » désigne le dernier état validé par le rejeu des octets fournis, sans garantie qu’ils aient été fsyncés. La publication cohérente d’une nouvelle génération, l’ordonnanceur deux minutes/veille, la détection de récupérations et l’UI sont US-325.

| R5 | Noms Windows legacy 3/4/5 traités comme UTF-16 | medium | La spécification name impose les pages 936/950/949 pour ces trois encodages ; le décodeur UTF-16 peut perdre ou refuser leurs octets. Utiliser la représentation octet-vers-caractère déjà documentée pour ces encodages non interprétés. patch |

| R6 | Autonomie des ressources vérifiée seulement hors CI | medium | Les sept tests initiaux du journal utilisent un codec d’arbre sans ressource ; l’oracle externe vérifie un payload avec ressource. Ajouter un test Rust qui conserve ressources et état de base malgré remplacement de l’état source. patch |

## Verification

**Commands:**
- cargo test -p atelier-model --locked --offline -- expected: tests de codec, CRC, limites et atomicité passent.
- cargo fmt --check -p atelier-model -- expected: format conforme.
## Review Triage Log

Chaque ligne constitue un verdict individuel avant regroupement.

| ID | Constat | Verdict | Preuve et route |
|---|---|---|---|
| B1 | Nom UTF-16 de longueur impaire tronqué | medium | `chunks_exact(2)` perd le dernier octet. Correction locale : refuser la longueur impaire, conserver U+FFFD pour les unités invalides. patch |
| B2 | Records de langues `name` version 1 non validés | medium | `parse_names` ne lit que les records ordinaires ; une version 1 sans langTagCount passe. Valider les plages et le stockage des tags. patch |
| B3 | Sélecteurs format 14 non triés, dupliqués ou invalides | medium | `validate14` ne contrôle que le plafond Unicode. Les sélecteurs ordinaires ou doublons rendent la résolution ambiguë. Gardes locales. patch |
| B4 | Fin de plage UVS hors Unicode | medium | Seul le début est contrôlé ; `additionalCount` peut prolonger U+10FFFF. Contrôler fin et ordre des plages. patch |
| B5 | Offset UVS pointant dans les records | medium | Une valeur non nulle peut référencer l'en-tête tant que la lecture du compte réussit. Contrôler l'origine des données et leurs bornes. patch |
| B6 | Mapping non-default sans cmap de base | false | La spécification OpenType autorise explicitement l'absence du caractère de base dans cmap pour un non-default UVS. Rejeter ce cas casserait des polices conformes. Aucune correction. |
| B7 | Formats cmap non implémentés ignorés | low | L'API n'expose que 4/6/12/14 et ne lit pas les autres formats. Aucun accès non borné ; préciser la documentation trop générale plutôt que décoder des formats supplémentaires. patch documentaire |
| B8 | Format 14 accepté sous une plateforme incorrecte | medium | `parse_cmap` affecte variation avant le classement plateforme. Exiger Unicode plateforme 0, encodage 5 pour le format 14. patch |
| B9 | Tableau d'offsets TTC incomplet accepté | medium | Seul l'offset de la face demandée est lu ; le compte complet peut déborder du buffer. Valider le tableau complet avant sélection. patch |
| B10 | Archive CI datée prétendument périmée | false | `github-ci-editing.json` conserve le SHA exact 77622b0 et les conclusions de son exécution. C'est une preuve historique immuable du lot précédent, pas le statut courant. La documentation identifie les commits. |
| E1 | Caractère ordinaire accepté comme sélecteur | medium | Même défaut que B3, vérifié à `validate14` : comparaison au seul plafond Unicode. patch |
| E2 | Octet UTF-16 final perdu | medium | Même défaut que B1, vérifié à `parse_names` : `chunks_exact(2)`. patch |
| G1 | Absence de test public de format 14 en CI | medium | Aucun des cinq tests initiaux n'appelle `variation_glyph`. Le comparateur externe confirme huit rejets de polices conformes. Ajouter des fixtures synthétiques et un test public exécuté par Cargo. patch |
| R1 | Longueur format 14 lue au mauvais offset | high | Le champ uint32 commence à +2, le code le lit à +4 ; les huit faces produites par FontTools sont rejetées. Correction locale du champ, test de non-régression. patch |
| R2 | Temps de validation UVS amplifiable par réutilisation | high | Un grand nombre de sélecteurs peut référencer le même grand tableau ; la limite de chaque compte ne borne pas la somme des itérations. Décompter un budget global et éviter de revalider un même sous-tableau cmap. patch |
| R3 | Tables maxp/post tronquées acceptées | medium | maxp 1.0 à six octets et post à quatre octets passent. Vérifier minima par version et les longueurs des variantes post exposées comme tables valides. patch |
| R4 | Cmap secondaires utilisés après absence dans le préféré | medium | `glyph_index` parcourt les autres formats si le préféré ne contient pas le caractère. OpenType demande de sélectionner un seul cmap de base. Garder le choix 12 puis 4 puis 6 et retourner zéro sans mélange. patch |
| R5 | Noms Windows legacy 3/4/5 traités comme UTF-16 | medium | La spécification name impose les pages 936/950/949 pour ces trois encodages ; le décodeur UTF-16 peut perdre ou refuser leurs octets. Utiliser la représentation octet-vers-caractère déjà documentée pour ces encodages non interprétés. patch |
| R6 | Autonomie des ressources vérifiée seulement hors CI | medium | Les sept tests initiaux du journal utilisent un codec d’arbre sans ressource ; l’oracle externe vérifie un payload avec ressource. Ajouter un test Rust qui conserve ressources et état de base malgré remplacement de l’état source. patch |
| G2 | Mutations externes limitées aux sfnt | low | La revue complémentaire des outils confirme que le glob excluait TTC et que le pilote appelait seulement parse. Correction directe : inclure two-faces.ttc et exercer parse_collection aux indices 0 et 1 pour chaque entrée. patch appliqué |

## Verification

**Commands:**
- cargo test -p atelier-model --locked --offline -- expected: tests de codec, CRC, limites et atomicité passent.
- cargo fmt --check -p atelier-model -- expected: format conforme.

## Validation finale — 2026-10-10

Les corrections de revue sont appliquées ; B6 et B10 sont réfutés et aucun constat n’est différé. Les quatorze contrôles locaux réussissent : 187 tests workspace, 51 tests modèle/polices release, builds Windows/WASM/musl, compilation des tests musl, Clippy hôte/WASM, format, SPDX, garde et métadonnées des 31 paquets locaux. Lock et sources identiques avant/après. Le comparateur indépendant valide 8 faces, 25 272 mutations/troncatures de polices (TTC compris), 12 001 sélections et 267 cas de journal. Voir model-font-validation.json, model-font-oracle.json et model-font-review.md. Les tests musl sont exécutés par le job Linux distant, pas sur cet hôte Windows.
