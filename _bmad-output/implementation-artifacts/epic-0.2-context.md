# Epic 0.2 Context: Unicode et encodages

<!-- Compiled from the backlog and SPEC.md. Edit freely. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Fournir au moteur texte des propriétés et transformations Unicode locales, déterministes et sans bibliothèque tierce. Les tables doivent venir d’une version officielle figée de l’UCD ; les algorithmes de segmentation, de coupure, de casse et de normalisation s’appuient sur ces tables et sur les corpus de conformité de la même version.

## Stories

- Story 0.2: Générateur de tables Unicode (US-010)
- Story 0.2: Segmentation en graphèmes et en mots (US-011)
- Story 0.2: Opportunités de coupure de ligne (US-012)
- Story 0.2: Casse et normalisation (US-013)
- Story 0.2: Encodages de texte (US-014)

## Requirements & Constraints

Aucune dépendance externe : le générateur est un outil Rust du workspace, et les tables générées sont versionnées. Pour des entrées UCD identiques, la sortie doit être reproductible. La conformité se mesure sur les fichiers de tests officiels correspondant exactement à la version figée : segmentation grapheme/mot, coupure de ligne et normalisation. Les règles typographiques françaises restent distinctes des règles Unicode de coupure ; les espaces GL, dont U+00A0 et U+202F, suivent UAX #14 et ses exceptions.

## Technical Decisions

Figer Unicode 17.0.0 comme version candidate, à confirmer dans le contrat US-010. Télécharger l’archive officielle sous `https://www.unicode.org/Public/17.0.0/ucd/UCD.zip`, compléter au besoin avec les tests de `ucd/auxiliary/` et les propriétés Emoji de `ucd/emoji/emoji-data.txt`, et conserver les sources officielles dans `data/ucd/` avec manifeste de version et empreintes SHA-256. Ne jamais télécharger `UCD/latest` dans la génération.

Placer le générateur sous `tools/` et sa sortie dans `crates/unicode/src/generated.rs` (ou modules générés équivalents). Proposer une API publique stable dans `atelier-unicode` : `general_category(char)`, `line_break_class(char)`, `grapheme_break_property(char)`, `word_break_property(char)`, `is_extended_pictographic(char)`/propriétés emoji, `name(char) -> Option<&'static str>`, ainsi que conversions `to_uppercase`, `to_lowercase`, `to_titlecase`, `case_fold` et normalisations NFC/NFD qui retournent des chaînes (les expansions peuvent produire plusieurs scalaires). Garder les propriétés scalaires sous forme de tables à deux niveaux : index de page par blocs de 256 scalaires, feuilles dédupliquées, valeurs compactées dans des entiers ; stocker séquences de mappings et noms dans des pools avec offsets. L’outil lit les fichiers UCD uniquement, valide les plages/versions attendues et écrit des octets déterministes.

Le générateur de données et le moteur d’algorithmes restent séparés : UAX #29 et UAX #14 consomment les propriétés générées ; NFC/NFD consomment les décompositions canoniques, classes combinatoires, exclusions de composition et tests officiels. Les propriétés d’emoji sont des données, pas un algorithme de segmentation. L’itération avant/arrière sur graphèmes se spécifie côté US-011.

## Cross-Story Dependencies

US-010 fournit les propriétés et données nécessaires à US-011, US-012 et US-013. US-014 est déjà livré avec des tables d’encodage simples et n’a pas de dépendance fonctionnelle envers l’UCD généré. Le crate unicode existe déjà et contient le décodage/encodage ainsi que des tables privées pour Windows-1252, ISO-8859-1, CP437 et MacRoman ; le générateur ne doit pas remplacer ces tables historiques.
