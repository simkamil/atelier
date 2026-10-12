---
title: 'Intégration US-202, US-213 et US-215'
type: 'feature'
created: '2026-10-12'
status: 'done'
route: 'dispatch'
review_loop_iteration: 1
baseline_commit: '2ae5aecf73f7ff9b7849bebde7facbc54330d5b2'
context: []
---

<frozen-after-approval reason="Développement parallèle, validation, docs et push autorisés par l’utilisateur">

## Intent

**Problem:** Les prochains consommateurs du moteur riche ont besoin d’un document initial cohérent, d’une mise en page réutilisable et de commandes de rendu de bordures. Ces fonctionnalités partagent les types du modèle et de la pagination, mais peuvent être développées en fichiers isolés.

**Approach:** Intégrer trois implémentations GPT-6 Luna medium sous les specs US-202/213/215, vérifier les invariants intermodules, puis publier les preuves et docs. Les critères de flux utilisateur, performance machine de référence et comparaison Word restent ouverts quand non mesurés.

## Boundaries & Constraints

**Always:** Parent propriétaire des exports, Cargo.lock, jeton FontManager, provenance WASM, tests globaux, graphe et docs de suivi. Agents propriétaires des fichiers de leur spec. Ajouter render → text-layout sans cycle et sans dépendance externe. Le jeton de cache FontManager est distinct entre instances et après mutations réussies; les mutations échouées le conservent. Aucun recyclage lors d’un déplacement de manager. Rendu inchangé du moteur TXT, plafond WASM existant conservé. Revue par trois agents indépendants avant triage; corriger les problèmes démontrés avant validation finale immuable. Graphify initial et final, contrôler les arêtes Cargo et préserver les concepts sémantiques existants. Inclure tous les fichiers non suivis et binaires dans le diff de revue; ne pas supprimer les worktrees externes. Docs et backlog ne ferment que les critères démontrés. Vérifications Windows natives, WASM et compilation musl hors réseau; distinguer compilation locale et exécution Linux distante. Publier commits sans force et inspecter une fois le résultat CI, sans présenter un blocage de facturation comme un test réussi.

**Never:** Ajouter des dépendances réseau, unsafe, contournements de budgets, tests de performance fragiles ou faux corpus Word. Appeler toute l’US validée lorsque seule une API fonctionne. Modifier un fichier détenu par un autre agent pendant son travail. Déduire des mesures utilisateur de benchmarks CPU seuls. Modifier les sources durant la validation immuable.

## I/O & Edge-Case Matrix

| Scénario | Entrée / état | Résultat attendu | Erreur |
|---|---|---|---|
| Révisions | deux managers, déplacement, register/remove réussis et échoués | jetons uniques et mutation atomique, ancienne révision stable sur erreur | erreur existante |
| Pipeline | fabrique → style résolu → layout/session → pages → bordures → DisplayList | résultats déterministes et géométrie valide | erreurs typées |
| Architecture | nouveaux modules et dépendance locale | exports utilisables, zéro cycle Cargo, aucune source externe | contrôle échoue si violation |
| Livraison | source revue et inventaire immuable | tous les checks applicables passent, provenance WASM correspondant aux sources | arrêt publication si échec |

</frozen-after-approval>

## Code Map

- `crates/{text-model,text-layout,render}/src/*` — trois specs détaillées dans ce répertoire; modules publics intégrés par le parent.
- `crates/font/src/manager.rs` et `crates/font/tests/cache_revision.rs` — jeton global process/instance WASM, muté seulement après validation.
- `Cargo.lock`, `crates/render/Cargo.toml` — nouvelle arête locale, régénération hors réseau.
- `tools/package-engine.py`, `data/engine/manifest.json` — provenance conservatrice du moteur embarqué.
- `graphify-out/GRAPH_REPORT.md` — architecture initiale; refresh final avec analyse Cargo.

## Tasks & Acceptance

- [x] Intégrer les exports et le jeton de cache FontManager, avec régressions dédiées.
- [x] Intégrer et tester un pipeline représentatif entre les trois US.
- [x] Revoir le diff complet, auditer chaque ligne des matrices, corriger puis exécuter les contrôles immuables.
- [x] Actualiser docs/backlog/Graphify, enregistrer les preuves, commit et push.

Les trois APIs sont utilisables, les résultats incrémentaux égalent les calculs complets, les bordures donnent de vraies commandes de rendu. La livraison conserve explicitement les limites de l’éditeur riche et de l’environnement CI.

## Implementation Notes

Plan établi après lecture du graphe et investigations indépendantes des trois développeurs. SPEC.md §6.16 définit une machine PC2020/8Go; cet hôte ne permet pas d’en certifier les cibles. Les consignes utilisateur de parallélisme et push priment sur les suggestions séquentielles/locales du workflow BMAD. Aucun artefact planning-artifacts supplémentaire trouvé; SPEC.md et US-V0-V1.md font autorité.

## Spec Change Log

## Review Triage Log

| ID | Origine et observation | Verdict | Preuve et traitement |
|---|---|---|---|
| B1 | Checkpoint sans `page_paragraph_lines`, risque veuves/orphelines | false | Recherche de toutes les occurrences : le vecteur était uniquement créé, rempli et vidé, jamais lu pour une décision. Les décisions utilisent les lignes, préfixes et placements. Ajouter ce vecteur au checkpoint ne change aucun résultat. Stockage inutilisé laissé inchangé; décisions vérifiées sur les placements. |
| B2 | Constructeurs FontManager paniquent à épuisement des révisions | low | `with_limits` emploie `expect`, et `new` appelle aussi des inscriptions infallibles. La limite théorique est réelle; documentation `# Panics` des trois constructeurs et distinction avec les erreurs des mutations. Aucune réutilisation de jeton. |
| B3 | Prévalidation globale O(n) à chaque édition | medium | Boucle de prévalidation sur tous les layouts et préfixes. Coût non supprimé; compteurs séparés dans le benchmark, mesure CPU et écart à 16 ms publiés. Limite de performance préexistante à la livraison du flux utilisateur, US-213 reste partielle. Suivi différé. |
| B4 | Fabrique non raccordée à Nouveau | medium | L’éditeur existant crée du TXT; la nouvelle fabrique Rust ne modifie pas ce chemin. Critère utilisateur antérieur toujours ouvert, US-202 partielle; intégration suivie séparément. |
| B5 | Bordures non raccordées au rendu de l’éditeur riche | medium | L’adaptateur produit des commandes réellement encodables mais aucun appel UI riche n’existe. Critère utilisateur antérieur toujours ouvert, US-215 partielle; intégration suivie séparément. |
| B6 | Politiques de bordure sans persistance/import par paragraphe | medium | `ParagraphBorderInput` est fourni par l’appelant et `ParaProps` n’a pas ces champs. Stockage externe explicitement documenté; absence antérieure dans le modèle, suivi de persistance/import différé. |
| B7 | Pas de corpus OOXML/Word authentique de bordures | medium | Aucun fixture ne constitue un oracle Word; les tests numériques/SVG ne certifient pas sa fidélité. Critère Word antérieur maintenu ouvert et suivi différé. |
| B8 | Géométrie par défaut dans un sidecar, absente de Section | medium | `Section` ne porte aucune géométrie typée. `DefaultPresentation` fournit les valeurs exactes, à appliquer par l’intégration; API et limite documentées. Extension/intégration du modèle antérieurement absentes, suivi différé. |
| B9 | Benchmark CPU ne prouve pas une frappe à 16 ms | medium | L’exemple chronomètre `update`, sans clavier ni UI, sur un i9-13900KF/64 Gio, pas le PC de référence. US-213 conserve le critère ouvert et publie la mesure réelle; suivi interaction différé. |
| B10 | Documentation bordures en anglais | low | Corps anglais dans le diff initial; traduction française et limites de livraison explicites. Correction directe. |
| V1 | Tests `between` vérifient seulement le nombre de traits | medium | Constat pré-vérifié de la couche verification-gap accepté : assertion des coordonnées avec espacement non nul et rejet d’un intervalle insuffisant ajoutés. |
| R1 | Coins ouverts avec espacement positif | medium | Top/bottom s’arrêtaient aux bornes de colonne tandis que les côtés étaient décalés à l’extérieur. Correction des extrémités jusqu’aux côtés et régression numérique avec espacements distincts. |
| R2 | Pas des points incompatible avec l’écart documenté | low | Période 4 épaisseurs moins diamètre 1 = écart 3, documentation 2. Période corrigée à 3 épaisseurs et vérifiée numériquement. |
| R3 | Cache : mutations de police, insertions/suppressions, propriétés peu vérifiées | medium | Tests supplémentaires comparant un calcul fraîchement façonné et paginé, avec compteurs d’invalidation et conservation après mutation échouée. |


## Verification

- Reçu `rich-incremental-borders-validation.json`: workspace Windows, release text-layout, clippy/fmt/licence/metadata, WASM réel/ABI/browser, musl compilation.
- Audit de matrices, comparaison de blobs publiés, santé du graphe et état CI enregistrés séparément.

Tranche API validée le 2026-10-12 : 739 tests workspace Windows, 78 text-layout release, 21 contrôles. L’US utilisateur reste partielle selon docs/backlog. Revue et matrice : `rich-incremental-borders-review.md`, `rich-incremental-borders-matrix.json`.
