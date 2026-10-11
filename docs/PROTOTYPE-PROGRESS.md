# Préparation du démonstrateur — 2026-10-11

US-010/011/012, US-051/053/054/055 et US-090/091 ont été développées en parallèle avec GPT-6 Luna,
raisonnement moyen, puis relues et validées. Le backlog compte désormais
**32 US terminées sur 158**.

- [Unicode](us-010-ucd-tables.md) : 71 fichiers officiels Unicode 17.0.0,
  licence et SHA-256, générateur local et build reproductible. L’[oracle](../_bmad-output/implementation-artifacts/prototype-ucd-oracle.json)
  compare 1 112 064 scalaires, quinze propriétés, dix mappings par scalaire,
  159 801 noms, 5 914 décompositions et seize règles de casse conditionnelle.
- [Contours TrueType](us-051-truetype-outlines.md) : glyphes simples et composites,
  limites cumulées et erreurs typées. Les 43 576 glyphes des vingt faces
  [concordent avec FontTools](../_bmad-output/implementation-artifacts/prototype-font-oracle.json).
  Les [300 rendus comparés](../_bmad-output/implementation-artifacts/prototype-outline-raster.json)
  sont identiques pixel par pixel avec le même rasteriseur indépendant ;
  20 000 mutations/troncatures ciblées ne déclenchent [aucune panique](../_bmad-output/implementation-artifacts/prototype-outline-mutations.json).
- [ABI WASM](us-090-wasm-abi.md) : allocations suivies, échange binaire, copie et
  libération JS, erreur et nouvelle instance après un vrai panic. La référence
  optimisée mesure 52 323 octets, avec seuil de régression à 57 555 octets.
  Les tests tournent sur les vrais artefacts WASM dans Node.

Le démonstrateur US-120 reste à construire. Les coupures de ligne Unicode,
le façonnage latin, le registre de polices et l'historique sont disponibles.
Il reste le Worker (US-092), le démarrage de l'interface (US-101), le rendu Canvas
(US-103), la coque Windows et le pont natif (US-113), puis leurs interactions.
US-120 dépend de US-012/054/055/072/101/103/113 ; les quatre premières sont validées.
La connexion des fontes système au Host appartient à US-113. Il n'existe pas
encore d'interface permettant d'ouvrir et modifier un document.

Les [21 contrôles du lot précédent](../_bmad-output/implementation-artifacts/prototype-foundations-validation.json)
passent : 233 tests workspace, 43 tests release, builds Windows/WASM/musl,
compilation des tests musl, Clippy hôte/WASM, format, SPDX, garde du lock,
reproductibilité, intégrité des ressources, tests Node et suivi de taille.
Les 32 paquets et toutes leurs dépendances sont locaux. Les [trois revues](../_bmad-output/implementation-artifacts/prototype-foundations-review.md)
ne laissent aucun constat différé. Les sources et le lock ne changent pas
pendant les contrôles ; la CI publiée exécute ensuite les tests sur Linux musl.

US-011, US-053 et US-091 ont été développées en parallèle avec GPT-6 Luna moyen,
puis relues et validées :

- [Segmentation](us-011-segmentation.md) : 766 cas GraphemeBreakTest et
  1 944 cas WordBreakTest officiels, navigation à chaque position UTF-8,
  itération inverse et longues suites de contexte en temps linéaire.
- [Crénage et ligatures](us-053-kerning-ligatures.md) : GSUB liga,
  GPOS kern et repli kern historique, clusters conservés et budgets cumulés.
  Les 54 716 [références HarfBuzz](../tests/font-layout-reference/README.md)
  concordent sur vingt faces ; 10 000 mutations ne déclenchent aucune panique.
- [Protocole binaire](us-091-binary-protocol.md) : dix messages versionnés,
  IDs u64/BigInt, codecs Rust/JS croisés, golden frames, erreurs et capacité
  maximale exacte. L'ABI transporte les trames par écho ; ses handlers
  métier et le Worker restent à développer.

Les [25 contrôles du lot US-011/053/091](../_bmad-output/implementation-artifacts/text-bridge-validation.json)
passent, avec **260 tests workspace et 70 tests release**,
builds Windows/WASM/musl, Clippy hôte/WASM, format, SPDX et ressources.
La taille WASM reste de 52 323 octets. Les sources et le lock restent identiques
pendant les contrôles, et les 32 paquets gardent des dépendances locales.
La [revue complète](../_bmad-output/implementation-artifacts/text-bridge-review.md)
ne laisse aucun constat différé. Les tests musl sont compilés localement ;
leur exécution appartient à la CI Linux du commit publié.

US-012, US-054 et US-055 sont développées, relues et validées :

- [Coupures de ligne](us-012-line-breaks.md) : 19 338 cas officiels UAX14,
  les mêmes cas enrichis de marques LB9 et les largeurs EAW de tous les scalaires.
- [Façonnage latin](us-054-latin-shaping.md) : glyphes/clusters et positions
  déterministes, interlettrage signé, chasse, 54 716 références HarfBuzz sur vingt faces.
- [Gestionnaire de polices](us-055-font-manager.md) : vingt faces embarquées,
  cinq alias métriques, proximité CSS, sources/TTC, couverture et limites atomiques.
  Quatre variantes Arial réellement installées sont sélectionnées par l'API System.

Les [26 contrôles finaux](../_bmad-output/implementation-artifacts/text-layout-validation.json)
passent : **291 tests workspace et 101 tests release**, builds Windows/WASM/musl,
Clippy hôte/WASM, format, SPDX, reproductibilité et intégrité des ressources.
La taille WASM reste de 52 323 octets. Sources et lock sont identiques pendant
les contrôles ; les 32 paquets n'utilisent que des dépendances locales.
Les [trois revues Luna moyen](../_bmad-output/implementation-artifacts/text-layout-review.md)
ne laissent aucun constat différé. Le coordinateur a finalisé les corrections
après la limite d'usage des deux agents de développement ; les agents frais
de revue ont pu travailler. Les tests musl sont compilés localement et exécutés
par la CI Linux du commit publié.
