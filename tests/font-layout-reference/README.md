# Référence indépendante de crénage et ligatures

`layout.tsv` est généré avec HarfBuzz, via uharfbuzz 0.56.3, sur les vingt
fichiers TTF inchangés. FontTools sert uniquement à identifier les features
et les séquences de ligatures à inclure. Ces outils Python restent réservés
à la préparation des références ; le produit et la CI Rust ne les utilisent pas.

Chaque ligne contient le chemin de police, les scalaires hexadécimaux d'entrée,
puis les glyphes séparés par `;`. Chaque glyphe décrit
`gid,cluster_start,cluster_end,x_offset,y_offset,x_advance,y_advance`.
Les clusters sont en indices de scalaires et les positions en unités de police.

Le corpus couvre toutes les paires des 52 lettres ASCII, les ligatures accessibles
des features latines, des runs répétés et plusieurs textes français précomposés.
Le script est Latn, la langue par défaut, la direction gauche à droite.
Seules les features kern/liga sont activées ; toutes les autres features présentes
sont explicitement désactivées. Les règles de marques et les formats absents des
polices réelles sont exercés séparément par les fixtures synthétiques.

Le manifeste fixe les versions de l'oracle, les empreintes des polices et du corpus.
Le test Rust exécute la façade publique et compare chaque glyphe et position.
