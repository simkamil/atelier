<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Revue — tabulations, pagination et curseur riche

Baseline : `13c605b19926bd08b439fc711d132d5a638f8e5e`. Trois relecteurs GPT-6 Luna, raisonnement moyen, sans historique, lancés avant réception des constats. Toutes leurs réponses ont été reçues avant triage et correction. Snapshot initial : [métadonnées](rich-page-geometry-review-snapshot-initial.json), diff source de 176187 octets, SHA-256 `c8b72f3caa016716f73f4c7dec89ba5c8869b62e9ff74f5ff16d453ac1504b38`. Le diff complet contient aussi les exports Graphify; leur intégrité est contrôlée séparément.

La [première validation](rich-page-geometry-validation-initial.json) passe 20 contrôles et 700 tests workspace sur une source immuable. Les correctifs ci-dessous nécessitent une nouvelle validation. Les constats de documentation sont traités individuellement, sans les confondre avec des défauts exécutables.

## Relecteur aveugle — dix constats

| ID | Constat | Triage et preuve |
|---|---|---|
| B1 | Les barres seraient réservées aux lignes contenant une tabulation. | **False positive.** La spec gelée exige une décoration fixe qui ne consomme pas U+0009; la boucle de `lib.rs` s'applique à chaque ligne éligible. Le test des décorations utilise aussi un paragraphe sans tabulation. Documentation précisée pour éviter cette interprétation. |
| B2 | Une barre de largeur zéro n'indique pas sa hauteur de dessin. | **Low, corrigé.** Contrat de rendu précisé dans `tabs.rs` et `us-211-tabs.md` : trait vertical, top/hauteur issus du `LayoutLine` référencé. Il ne s'agit pas d'un rectangle rempli de largeur nulle. |
| B3 | Reprise de la grille automatique ambiguë avec des taquets non triés ou hors marge. | **False positive pour le tri.** `validate_tab_settings` rejette positions non positives, doublons et ordre décroissant avant calcul. La règle hors marge est déterministe et reste non comparée à Word. La documentation précise « dernier taquet non-barre ». |
| B4 | Aucun réglage global sans répéter des paramètres à chaque paragraphe. | **Low, corrigé dans la documentation.** L'API prend déjà `&TabSettings`; l'appelant conserve et réutilise une seule référence pour tous les paragraphes. Aucun état global implicite requis par la spec. |
| B5 | Ordre de relaxation de contraintes concurrentes non documenté. | **Medium, corrigé.** Priorité aux sauts explicites, puis déplacement des groupes qui tiennent, puis minima veuves/orphelines; contraintes impossibles signalées. Régression des solidarités opposées aux sauts explicites. |
| B6 | Validité des indices de sauts manuels peu explicite. | **Low, corrigé.** Documentation : indices strictement croissants, uniques et intérieurs (0 et nombre de lignes exclus); tests d'entrées invalides complétés. |
| B7 | Tolérance d'arrondi pour la hauteur non documentée. | **Low, corrigé.** Égalité entière exacte en unités 1/64 pt; une unité de différence est rejetée, déjà testé par `malformed_inputs_and_limits_return_typed_errors`. Aucune tolérance flottante. |
| B8 | Des placements page-locaux identiques pourraient produire des lignes chevauchantes dans le curseur. | **False positive pour des pages différentes.** Le curseur distingue les pages par index; `real_tabbed_layout_paginates_and_maps_carets_between_pages` vérifie des tops identiques sur deux pages. Sur une page, le paginateur accumule les hauteurs positives et l'adaptateur rejette une géométrie forgée. Un dépassement de page diagnostiqué n'est pas une perte de topologie. |
| B9 | Géométrie des espaces initiaux/intérieurs omise du contrat. | **Low, corrigé.** Documentation : les espaces visibles initiaux/intérieurs doivent être couverts par les glyphes; seuls les espaces terminaux rognés donnent des arrêts inférés à avance nulle. Validation `required_geometry_end` et tests de plages incomplètes. |
| B10 | Absence de drapeau par arrêt indiquant une ligature approximative. | **False positive dans ce périmètre.** La spec gelée autorise explicitement l'interpolation uniforme sans GDEF. Le contrat public indique que tous les arrêts internes multigraphèmes sont approximatifs; aucun indicateur par arrêt n'est promis. Le test d'interpolation vérifie les positions numériques et le rejet d'un offset interne au graphème combinant. |

## Relecteurs cas limites et lacunes

| ID | Constat | Triage et preuve |
|---|---|---|
| E1 | Un avant explicite dépassant la page provoquerait un overflow de soustraction. | **False positive pour le mécanisme allégué.** Le snippet annoncé `saturating_sub` ne correspond pas au snapshot (`checked_sub`). Avec hauteur positive et y non négatif représentable, `page_height - y` est représentable même négatif. Le cas déclenche cependant le défaut indépendant R2, corrigé et testé. Les véritables additions hors i64 restent des erreurs typées. |
| G1 | `break_after` n'est exercé par aucun test. | **Medium, corrigé.** Test de deux paragraphes tenant ensemble sans règle, séparés avec `break_after`, avec paragraphes vides intermédiaires et absence de page vide terminale. Il détecte la suppression de cette branche. |

## Contrôles complémentaires de l'intégrateur

| ID | Défaut confirmé | Correction attendue et validation |
|---|---|---|
| R1 | Ligne composée uniquement de contrôles tabulation : extents nuls, hauteur ramenée à 1. | **Medium, corrigé.** Extents de repli sur ligne vide/contrôles uniquement; glyphes texte conservant leurs métriques mixtes. Test des tailles par défaut et résolues. |
| R2 | Avant explicite sur page vide et plusieurs lignes : la règle orpheline relance la même page jusqu'au budget. | **Medium, corrigé.** Présence réelle de placements distinguée d'un y non nul dû à l'espacement. Relaxation diagnostiquée si l'espacement rend le minimum impossible; conservation et progression testées aux limites de hauteur. |
| R3 | Sommes des lignes restantes et lookahead `break_after` répétés sans charge du budget. | **Medium, corrigé.** Réutilisation des préfixes validés, calcul préalable des frontières/paragraphes suivants et suppression des recherches répétées dans les diagnostics. Régression avec nombreuses petites pages et budget borné. |
| R4 | Une chaîne keep-next traverse un saut explicite et déplace du contenu inutilement. | **Medium, corrigé.** Chaînes tronquées aux frontières explicites, conflit signalé; régression numérique. |
| R5 | Déplacement vertical aval seul : remonter vers la fin partagée d'une ligne peut rester sur la même ligne au mouvement suivant. | **Medium, corrigé.** Variante avec affinité d'entrée et de sortie; API existante conservée avec convention aval documentée. Régression de remontées successives sur coupures souples. |
| R6 | Matrice US-211 promet des retraits mais ne les exerce pas avec tabs par défaut. | **Medium, corrigé.** Attentes numériques pour retrait initial et suspendu, prochain multiple de 2268 après le pen courant. |

Avant le snapshot, l'intégrateur avait également fait corriger le scan de toutes les frontières à chaque mise en forme d'une plage, l'agrégation de glyphes simplement adjacents comme un même cluster, et des scans imbriqués dans la construction du curseur. Les régressions correspondantes font partie de la validation finale. Les trois leaders sont vérifiés numériquement, avec limite de décorations effectivement atteignable.

L'audit final des matrices complète les tests des hauteurs nulles/négatives, des limites de pages, des sélections inversées entre paragraphes, des déplacements avec affinité dans les deux directions et des barres sur lignes sans tabulation. Ces assertions couvrent des entrées citées explicitement par les specs; elles n'ajoutent pas de comportement produit. Le [reçu intermédiaire après revue](rich-page-geometry-validation-post-review.json) est conservé; les tests complétés sont ensuite revérifiés sur une nouvelle source immuable. Un échec Clippy de préparation (`field_reassign_with_default` dans le nouveau test de grande police) a été corrigé par une initialisation directe, sans suppression de lint.

## Vérification finale

La clôture est conditionnée au [reçu final immuable](rich-page-geometry-validation.json), à l'[audit des matrices](rich-page-geometry-matrix.json) et au [contrôle Graphify](rich-page-geometry-graph.json). Les trois développeurs corrigent leurs propres modules en parallèle; l'intégrateur exécute ensuite les contrôles complets. La fidélité Word d'US-211/212 reste ouverte; ce lot ne raccorde pas les API à l'interface riche.

Résultat final : 707 tests workspace, 57 tests text-layout release et 20 contrôles passés; 23 lignes de matrices reliées aux tests réellement exécutés ou à une preuve de processus explicite. Graphify : 6014 nœuds, 15436 liens, 158 US dont 47 terminées, 33 paquets et 42 dépendances Cargo sans cycle. Aucun endpoint manquant. Les 560 groupes de relations à extrémités communes restent dans l'extraction brute; le graphe simple peut les fusionner.

Le premier refresh canonique a refusé de retirer `crates_raster_tests_reference_image`, une référence sémantique à un PNG versionné restaurée par le cache incrémental. Le refresh conserve maintenant les trois PNG de `crates/raster/tests/reference`, comme les fixtures JPEG déjà conservées, avec leurs vrais chemins et relations. Le test `reference_images_are_unchanged` vérifie leurs octets. Aucun symbole source ni fichier produit n'a été supprimé pour contourner ce contrôle. La relance réussit; CLI final en mode code-only et génération locale du rapport : zéro token LLM. Les coûts sémantiques cumulés historiques restent distincts.
