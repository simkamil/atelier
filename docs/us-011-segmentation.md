# US-011 — Segmentation Unicode

Le module `atelier_unicode::segmentation` expose `grapheme_boundaries(&str)` et `word_boundaries(&str)`. Les deux fonctions suivent la segmentation étendue par défaut de UAX #29, révision 47, avec les données Unicode 17.0.0 embarquées par US-010. Il n'y a ni dictionnaire ni tailoring linguistique.

Chaque résultat est un `BoundaryIndex` qui emprunte le texte et stocke les frontières comme offsets d'octets UTF-8. `boundaries()` inclut 0 et la longueur du texte; pour une chaîne vide, il ne contient que 0. `segments()` fournit des plages demi-ouvertes couvrant exactement le texte, dans les deux sens.

`is_boundary`, `floor_boundary`, `ceil_boundary`, `previous_boundary`, `next_boundary`, `forward_from` et `backward_from` rejettent les offsets supérieurs à la longueur et les offsets au milieu d'un scalaire UTF-8 avec `OffsetError`. Floor et ceil incluent l'offset lorsqu'il est une frontière; previous et next sont stricts. Les deux itérateurs directionnels renvoient des segments entiers: si l'offset est dans un segment, celui-ci est le premier résultat dans chaque direction.

La construction prend O(n) temps et O(n) mémoire pour n scalaires; chaque étape d'itération est O(1), et la navigation par recherche binaire est O(log k) pour k frontières. Les contextes de GB9c, GB11 et des indicateurs régionaux sont tenus à jour pendant le parcours.
