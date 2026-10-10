# Références des décodeurs d’images

Le corpus contient les 176 PNG de PngSuite (162 valides et 14 invalides), ainsi
que 25 GIF/BMP générés pour couvrir les profondeurs, orientations, masques,
entrelacement, transparence, décalages et modes RLE. `cases.tsv` alimente le test
Cargo `independent_reference_pixels_and_invalid_pngsuite`, exécuté en CI.
`manifest.json` conserve les empreintes des entrées et sorties RGBA de référence.

PngSuite provient de Willem van Schaik :
[archive du 19 juillet 2017](http://www.schaik.com/pngsuite/PngSuite-2017jul19.tgz),
SHA-256 `0294b244c95a8342c01b00010cf34abdcabc7c6a34fd0fe1bd963917537bfdc8`.
Les fichiers `pngsuite/PngSuite.LICENSE` et `PngSuite.README` sont conservés
intégralement. Les fixtures générées par Atelier sont sous AGPL-3.0-or-later.

Les pixels PNG sont calculés par PyPNG 0.20220715.0, indépendamment du Rust.
Les métadonnées de rendu sont retirées uniquement de l’entrée temporaire de
l’oracle : le contrat produit expose les échantillons bruts, sans correction
gamma, ICC ni réduction sBIT. Les valeurs sont converties en RGBA8 par arrondi
entier au plus proche. Les GIF/BMP sont vérifiés par Pillow 12.3.0 et par des
valeurs calculées explicitement pour les offsets GIF et les modes bitfields/RLE.
Pillow perd le dernier demi-octet d’un run absolu RLE4 de cinq pixels ; ce témoin
utilise donc les indices littéraux du flux pour sa référence.

Le générateur agent
`_bmad-output/implementation-artifacts/generate-image-references.py` régénère
les fichiers à l’identique avec l’archive placée dans
`target/images-fonts-oracle/PngSuite-2017jul19.tgz`. PyPNG et Pillow servent
seulement à produire et vérifier les références ; les tests Cargo les lisent
sans Python ni réseau et le produit ne dépend d’aucun de ces outils.

Le serveur historique de PngSuite est accessible en HTTP; ses deux adresses
HTTPS ont échoué à la négociation TLS lors de cette validation. L’archive figée
et chaque fichier sont identifiés par SHA-256. Le corpus est versionné pour
que les tests et la CI n’aient aucun téléchargement à effectuer. Le contrôle
PowerShell `tests/check-assets.ps1` vérifie les 201 entrées, les 187 références,
les empreintes, les dimensions et la correspondance exacte de `cases.tsv`
avec le manifeste; le job Windows l’exécute en CI.
