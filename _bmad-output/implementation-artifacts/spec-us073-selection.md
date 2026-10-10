---
title: 'US-073 — Modèle de sélection'
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
  - '{project-root}/crates/model/src/lib.rs'
  - '{project-root}/crates/model/src/history.rs'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** L’historique US-072 sait restaurer un snapshot de sélection opaque, mais `atelier-model` ne définit ni sélection stable par identité, ni règle commune pour transformer ses positions au fil des éditions.

**Approach:** Ajouter un type de sélection concret à plages dirigées (ancre/tête), plus un contrat explicite de correspondance de positions pour les événements structurels et textuels. Le moteur reste découplé des opérations futures du modèle texte.

## Boundaries & Constraints

**Always:** Utiliser `NodeId` et des offsets `usize`; offsets textuels exprimés en octets UTF-8 et valides uniquement aux frontières de caractères, avec validation déléguée au modèle de texte faute d’accès au contenu ici. Une plage conserve la direction ancre/tête. Une sélection contient une ou plusieurs plages et un index primaire valide; préserver l’ordre fourni. Les transformations sont déterministes, utilisent `checked_add`/vérifications de limites, et s’appliquent à une copie candidate : échec ne laisse aucun endpoint partiellement transformé. Les événements portent assez d’information concrète (nœuds source/cible, offsets, longueur) pour que `Operation<S>` demeure générique.

**Never:** Modifier `Operation<S>`, `Transaction`, le codec `ATOP`, l’API de l’historique ou prétendre que les futures opérations de `atelier-text-model` sont déjà intégrées. Aucun état texte concret ni dépendance externe.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Plusieurs plages | Plages dirigées et primaire valide | Ordre, direction et primaire conservés | Refuser liste vide, index primaire invalide ou dépassement de la limite de plages |
| Insertion | insertion `(node, at, byte_len)` | Avant `at` inchangé; après `at` décalé; égalité suit l’affinité de chaque endpoint | Débordement ou cible incohérente : erreur, état intact |
| Suppression | plage `[start,end)` dans un nœud | Positions dedans ou à `end` se replient à `start`; positions après diminuent de la longueur supprimée | Intervalle invalide, nœud incohérent ou arithmétique invalide : erreur atomique |
| Scission / fusion | nœud source et frontière; fusion avec longueur gauche | Après frontière, endpoint passe au nœud droit; fusion remappe le droit à gauche + longueur gauche | Échec de vérification sans mutation |
| Retrait de nœud | ID supprimé et position de repli explicite | Tous ses endpoints prennent le repli déterministe fourni par l’opération appelante | Repli invalide/absent : erreur atomique |

</frozen-after-approval>

## Code Map

- `crates/model/src/selection.rs` -- types, validation et événements/mapping purs à ajouter.
- `docs/us-073-selection.md` -- sémantique publique et exemples d’intégration.
- `crates/model/src/history.rs` -- référence de snapshot opaque; aucun changement demandé, vérifier sa compatibilité par un test réel.
- `crates/model/src/lib.rs` -- types `NodeId` et transactions; les exports racine sont laissés au coordinateur.

## Tasks & Acceptance

**Execution:**
- [x] `crates/model/src/selection.rs` -- définir `Selection`, `SelectionRange`, `TextPosition`, `Affinity`, erreurs et événements explicites de mapping, sans coupler les opérations génériques au texte.
- [x] `docs/us-073-selection.md` -- documenter offsets, affinité, cas limites et obligations d’intégration futures.
- [x] `crates/model/src/selection.rs` -- couvrir transformations insert/delete/split/merge/remove, multi-plages, bornes, erreurs atomiques et UTF-8.
- [x] `crates/model/src/selection.rs` -- ajouter un test utilisant `History<Op, Selection>` pour confirmer le snapshot avant/après restauré par undo/redo.

**Acceptance Criteria:**
- Given une sélection valide, when chaque événement de mapping est transformé, then tous les endpoints suivent les règles déterministes ci-dessus et aucune plage n’est perdue.
- Given insertion au point d’un endpoint, when son affinité est Before ou After, then il reste avant ou suit le texte inséré respectivement; une sélection réduite par défaut suit l’insertion.
- Given un offset ou une longueur invalide, when la transformation échoue, then la sélection d’origine demeure inchangée.
- Given un snapshot concret, when une transaction réussit puis est annulée/rétablie via `History`, then la sélection exacte avant/après est restaurée.

## Implementation Notes

Ce document est le handoff BMAD du coordinateur ; ne pas réinvoquer de skill ni créer d’agent. Graphe déjà rafraîchi : lecture seulement. Le coordinateur expose `pub mod selection`; les exemples publics peuvent utiliser ce chemin. Aucun export racine à modifier dans lib.rs.

Propriété exclusive d’implémentation : `crates/model/src/selection.rs`, `docs/us-073-selection.md`, ce spec. Le coordinateur gère `lib.rs`, Git, Cargo et Graphify. Le modèle texte n’existe pas encore : définir un petit contrat explicite `PositionMap`/événement que les opérations futures peuvent émettre et appliquer; ne pas inventer un branchement automatique dans `Operation<S>`. Prévoir un maximum documenté de plages (choix recommandé : 64) pour borner stockage et arithmétique. Affinité recommandée: endpoints de début à gauche et de fin à droite; une caret collapse suit l’insertion. Pour suppression, l’intervalle est demi-ouvert et un endpoint exactement à sa fin rejoint son début. Une scission à la frontière envoie l’égalité à gauche pour Before et à droite pour After. Retrait d’un nœud exige du caller un fallback valide; le mapping n’infère pas de voisinage dans un arbre générique. Les longueurs sont des octets, mais la validation d’une frontière UTF-8 nécessite le texte propriétaire.

## Design Notes

Éviter une obligation de trait sur chaque opération générique. Une transaction future peut exposer/collecter les événements de correspondance de position et les appliquer avec le changement de document sous le même checkpoint; cette orchestration relève de l’adaptateur concret. Valider l’ensemble d’une transformation avant publication, ou transformer une copie et remplacer seulement après succès. La compatibilité avec US-072 est réelle via son paramètre `Selection: Clone`; cela ne prouve pas encore l’intégration avec une édition textuelle.

| R5 | Noms Windows legacy 3/4/5 traités comme UTF-16 | medium | La spécification name impose les pages 936/950/949 pour ces trois encodages ; le décodeur UTF-16 peut perdre ou refuser leurs octets. Utiliser la représentation octet-vers-caractère déjà documentée pour ces encodages non interprétés. patch |

| R6 | Autonomie des ressources vérifiée seulement hors CI | medium | Les sept tests initiaux du journal utilisent un codec d’arbre sans ressource ; l’oracle externe vérifie un payload avec ressource. Ajouter un test Rust qui conserve ressources et état de base malgré remplacement de l’état source. patch |

## Verification

**Commands:**
- `cargo test -p atelier-model --locked --offline` -- nouveaux cas de mapping et snapshot History passent.
- `cargo fmt --check -p atelier-model` -- format conforme.

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
- `cargo test -p atelier-model --locked --offline` -- nouveaux cas de mapping et snapshot History passent.
- `cargo fmt --check -p atelier-model` -- format conforme.


## Validation finale — 2026-10-10

Les corrections de revue sont appliquées ; B6 et B10 sont réfutés et aucun constat n’est différé. Les quatorze contrôles locaux réussissent : 187 tests workspace, 51 tests modèle/polices release, builds Windows/WASM/musl, compilation des tests musl, Clippy hôte/WASM, format, SPDX, garde et métadonnées des 31 paquets locaux. Lock et sources identiques avant/après. Le comparateur indépendant valide 8 faces, 25 272 mutations/troncatures de polices (TTC compris), 12 001 sélections et 267 cas de journal. Voir model-font-validation.json, model-font-oracle.json et model-font-review.md. Les tests musl sont exécutés par le job Linux distant, pas sur cet hôte Windows.
