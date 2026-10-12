<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Revue — document initial, pagination incrémentale et bordures

Le diff de sources de 286 828 octets et le snapshot complet, binaires et fichiers
non suivis compris, sont identifiés dans `rich-incremental-borders-review-original.json`. Le snapshot final après corrections est `rich-incremental-borders-review-snapshot.json`.
Trois agents GPT-6 Luna moyen indépendants ont été lancés avant la collecte des
résultats. Le modèle demandé par l’utilisateur prime sur le conseil BMAD de
modèle identique au parent. Tous leurs résultats ont été reçus avant ce triage.
La couche edge-case ne signale aucun problème (`[]`). Le blind hunter, contraint
à dix observations, a notamment relevé des critères utilisateur déjà ouverts.

| ID | Origine et observation | Verdict | Preuve et traitement |
|---|---|---|---|
| B1 | Checkpoint sans `page_paragraph_lines`, risque veuves/orphelines | false | Recherche de toutes les occurrences : le vecteur est uniquement créé, rempli et vidé, jamais lu pour une décision. Les décisions veuves/orphelines utilisent les lignes, préfixes et placements (notamment pagination.rs 612–617). Ajouter ce vecteur au checkpoint ne change aucun résultat. Stockage inutilisé laissé inchangé. |
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

Les défauts locaux B2/B10/V1/R1/R2/R3 sont corrigés dans ce lot. B1 est réfuté
par les lectures effectives du code. Les limites B3–B9 étaient déjà des critères
ouverts; leur suivi n’est pas assimilé à une validation. Les quatre matrices
d’acceptation sont auditées contre les tests effectivement exécutés par le reçu
final; compilation musl et exécution Windows sont distinguées.

Premier contrôle global : 736 tests Windows et 75 tests release réussis; arrêt Clippy sur une branche identique dans le calcul du vide des pointillés. Le [reçu de tentative](rich-incremental-borders-validation-attempt1.json) conserve cet échec. La version corrigée passe la nouvelle validation immuable complète : 21 contrôles, 739 tests Windows et 78 tests release, 844 entrées et lock inchangés. L’audit manuel des matrices a aussi renforcé les erreurs de géométrie, les ruptures de cadre et l’atomicité après erreurs de layout/arithmétique.

Publication source : `51034b31d2871d5f3245c6ef6aaab620b685ec92`. Les 844 entrées du reçu de validation correspondent aux blobs publiés, avec uniquement les normalisations LF enregistrées dans le reçu de publication. La CI de ce commit refuse ses trois jobs avant démarrage pour paiement/plafond Actions; preuve dans `rich-incremental-borders-ci.json`. Aucun résultat Linux distant n’est déduit des compilations locales.
