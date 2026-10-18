# Suivi modèle et polices — 2026-10-10

US-050, US-073 et US-074 sont développées, revues et validées : **20 US sur 158**.
Le lot a été développé en parallèle avec GPT-6 Luna, raisonnement moyen, puis
relu par trois agents indépendants du même modèle.

- [US-050](us-050-opentype-tables.md) : tables sfnt/TTC communes aux polices
  TrueType et OpenType/CFF, cmap 4/6/12/14, métriques horizontales et noms.
  Les contours et le shaping appartiennent aux US suivantes.
- [US-073](us-073-selection.md) : plages dirigées, affinités et transformation
  atomique des positions lors des insertions, suppressions, scissions, fusions
  et retraits de nœuds. Le modèle qui possède le texte valide les frontières
  UTF-8 et publie la sélection avec la transaction documentaire.
- [US-074](us-074-recovery-journal.md) : instantané autonome versionné et journal
  borné par génération, CRC32, séquences contiguës et arrêt au premier défaut.
  Le codec documentaire inclut et valide les ressources. La persistance sur
  disque, fsync et la planification restent à intégrer dans US-325.

La revue a révélé une erreur de longueur du cmap format 14 et plusieurs
validations insuffisantes des tables de police. Les tests de référence
couvrent désormais leurs corrections dans Cargo et dans la CI, sans runtime externe.
Le [tri détaillé](../_bmad-output/implementation-artifacts/model-font-review.md)
conserve chaque constat, ses preuves et sa résolution.

Preuves du lot : [contrôles locaux](../_bmad-output/implementation-artifacts/model-font-validation.json),
[comparaison indépendante](../_bmad-output/implementation-artifacts/model-font-oracle.json),
[empreinte de revue](../_bmad-output/implementation-artifacts/model-font-review-snapshot.json).

Le lot précédent, commit `77622b0c410ff9f5686325e81bca1e71cf383642`, a réussi les
trois jobs de la [CI GitHub](https://github.com/simkamil/atelier/actions/runs/38068167771).
Son [relevé archivé](../_bmad-output/implementation-artifacts/github-ci-editing.json)
identifie ce commit ; les changements du présent lot nécessitent leur propre CI.

Les **14 contrôles locaux réussissent** : 187 tests workspace, 51 tests en
release (40 modèle, 11 polices), builds Windows/WASM/musl, compilation des tests
musl, Clippy hôte/WASM, formatage, SPDX, garde et métadonnées. Les 31 paquets
restent locaux, sans dépendance externe ; `atelier-model → atelier-compress`
fournit CRC32. Les contrôles conservent les octets du lock et des sources.

Le comparateur indépendant vérifie huit faces TTF/OTF/TTC contre FontTools
(cmap, variantes, métriques et noms Unicode), 25 272 entrées mutées ou tronquées
sans panique, 12 001 sélections contre un modèle Python et 267 cas de journal
contre `struct` et `zlib.crc32`. Il régénère aussi les sept fixtures binaires
versionnées à l’identique. Les outils Python sont réservés aux agents.

Les vingt constats des revues et contrôles sont triés individuellement ; les
corrections sont validées, deux constats sont réfutés, aucun n’est différé.
Le job WASM inclut désormais `atelier-font`. Les tests musl sont compilés sur
Windows ; leur exécution sur Linux appartient à la CI du commit publié.
