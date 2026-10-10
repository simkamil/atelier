# Epic 0.6 Context: Polices et mise en forme du texte

<!-- Compiled from planning artifacts. Edit freely. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Établir les primitives de lecture des polices et de mise en forme typographique qui permettent au moteur de produire une mise en page déterministe sur desktop et web. Les polices OpenType alimentent ensuite les contours, le positionnement et le gestionnaire de polices.

## Stories

- Story US-050: Tables de base OpenType
- Story US-051: Contours TrueType
- Story US-052: Contours CFF
- Story US-053: Crénage et ligatures
- Story US-054: Mise en forme du texte latin
- Story US-055: Gestionnaire de polices
- Story US-056: Polices embarquées

## Requirements & Constraints

Le moteur V1 prend en charge les polices lues depuis des fichiers, y compris celles embarquées et incorporées, et fournit une mise en forme latine avec correspondance caractère-glyphe, métriques, crénage et ligatures. Le traitement des écritures complexes et du bidi est différé ; les API doivent rester utilisables pour des séquences de glyphes génériques. La sortie doit être déterministe à police identique, quelle que soit la plateforme.

Les polices et fichiers ouverts sont des entrées non fiables : toutes les tailles, profondeurs et plages doivent être contrôlées, aucune panique ne doit être possible sur des données invalides. Le fuzzing maison couvre les parseurs de polices. Le produit ne peut ajouter aucune dépendance tierce. Les polices distribuées doivent respecter leurs licences ; les fontes embarquées prévues sont sous SIL OFL 1.1 avec textes de licence inclus.

## Technical Decisions

Le crate `atelier-font` appartient au moteur partagé et reste en Rust sûr (`forbid(unsafe_code)`). Les données de police sont interprétées en big-endian, sans copie inutile lorsque des vues bornées suffisent. Les erreurs de format sont renvoyées sous forme de types explicites. La séparation entre conteneur sfnt, tables OpenType et tracés évite de coupler le parsing aux moteurs `shape`/`render`.

## Cross-Story Dependencies

US-051, US-052, US-053, US-054 et US-055 dépendent de US-050. US-056 fournit les fichiers redistribuables utilisés par le gestionnaire et les scénarios de référence.
