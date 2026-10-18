# Travail avec les sous-agents

Règles du dépôt (mises à jour le 2026-10-18) :

- Trois agents en parallèle au plus, relecteurs compris.
- Le code (développement et corrections) est confié à Sonnet, effort moyen ; l'exécution des tests et des validations à Haiku.
- Une branche et un worktree par story, créés depuis le `main` local ; chaque agent est propriétaire d'un ensemble de fichiers distinct.
- Les fichiers communs (`Cargo.toml`, `Cargo.lock`, CI, README, backlog, ABI, interface, modèle) reviennent à un intégrateur unique, qui tranche les dépendances, fusionne et exécute les contrôles du produit assemblé.
- Chaque branche est relue par un agent indépendant au contexte neuf avant fusion.
- Les preuves d'un lot suivent la définition de « terminé » du [backlog](../US-V0-V1.md) : tests et suites verts, une ligne de validation datée dans la story, sans reçu JSON ni script ad hoc.

La livraison d'un module ne vaut pas livraison de l'US entière : les critères restants sont conservés dans le backlog, et les critères de campagne (Word, accessibilité physique, PC de référence) sont vérifiés par US-308, US-354 et US-362.
