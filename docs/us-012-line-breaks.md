# Opportunités de coupure de ligne

`atelier_unicode::line_break::line_breaks(&str)` calcule les opportunités de coupure par défaut selon Unicode 17.0.0, UAX #14 révision 55. Elle renvoie une `Vec<LineBreak>` ; chaque élément contient un offset `usize` en octets UTF-8 et un `BreakKind` (`Allowed` ou `Mandatory`). Les offsets sont des frontières de scalaires, sont strictement croissants, et ne contiennent pas le début du texte non vide. La fin est toujours une coupure `Mandatory`; pour la chaîne vide, le résultat est le seul offset 0 obligatoire.

Les caractères BK, CR, LF et NL déclenchent une coupure obligatoire après eux, avec CR × LF indivisible. Les autres résultats sont des opportunités permises : l'appelant décide ensuite où couper en fonction de la largeur disponible. L'API ne réalise ni choix de mise en page, ni césure linguistique, dictionnaire thaï, shaping ou bidi.

La résolution LB1 utilise AI/SG/XX → AL, SA avec General_Category Mn/Mc → CM, les autres SA → AL et CJ → NS. Les règles par défaut incluent les contextes Unicode 17 (espaces, marques combinantes, nombres, guillemets East Asian, indicateurs régionaux, syllabes Hangul et syllabes orthographiques Brahmiques). U+00A0 et U+202F conservent leur classe GL et les exceptions de LB12a. Aucune adaptation française n'est appliquée.

L'algorithme parcourt les scalaires et préconstruit les contextes utilisés par les règles ; temps et mémoire sont O(n), n étant le nombre de scalaires. Les données proviennent des propriétés Unicode figées du crate, sans dépendance externe. L'API retourne les opportunités légales, jamais les positions interdites.

Référence normative : [UAX #14 révision 55](https://www.unicode.org/reports/tr14/tr14-55.html). Les 19 338 cas officiels LineBreakTest sont consommés sans filtrage. Un second contrôle applique LB9 aux mêmes cas en insérant plusieurs marques de largeurs différentes, pour vérifier les contextes complets. Des suites longues ont des opportunités attendues exactes ; la borne linéaire découle aussi des passes et des contextes préconstruits du code. Les tables East_Asian_Width sont comparées au fichier officiel sur les 1 112 064 scalaires.

L'API alloue ses contextes et sa sortie en proportion de l'entrée, sans plafond intégré. L'appelant fixe donc la taille du texte transmis lorsqu'il applique ses propres budgets ; le façonnage dispose séparément de son plafond LayoutLimits.

Validation du lot : [26 contrôles](../_bmad-output/implementation-artifacts/text-layout-validation.json), 291 tests workspace et 101 tests release sur sources inchangées ; [revue indépendante triée](../_bmad-output/implementation-artifacts/text-layout-review.md), aucun constat différé.
