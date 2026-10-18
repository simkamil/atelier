# Atelier — consignes pour Claude

Règles de travail des sous-agents : [docs/SUBAGENT-WORKFLOW.md](docs/SUBAGENT-WORKFLOW.md).

## Erreurs à ne pas reproduire

Chaque erreur commise deux fois est notée ici, avec la parade.

- **Critère coché sans effet pour l'utilisateur.** US-270, US-328 et US-307 ont été cochées alors que rien n'était persisté ou que des parties du modèle étaient recopiées ; US-216 a été cochée sur la moyenne d'une latence dont le maximum dépassait la cible. Parade : chaque brief exige qu'un critère soit vrai de bout en bout, du point de vue de l'utilisateur et au sens littéral de son libellé (un maximum reste un maximum) ; sinon la case reste décochée avec une réserve datée. L'intégrateur relit chaque case cochée avant fusion.
- **Contrat d'interface figé sans simuler ses consommateurs.** Le contrat R0 de l'éditeur riche a demandé trois tours de relecture, chacun révélant un manque pour un autre agent (accès au document, styles résolus, géométrie de page, pompe de mise en page). Parade : le brief d'un contrat partagé impose à son auteur de simuler l'implémentation de chaque consommateur méthode par méthode, en citant la donnée utilisée, avant de rendre la main ; la relecture pose la même question.
- **Changement de représentation sans prévenir ses lecteurs.** R1 a sorti les sauts et symboles du texte des paragraphes alors que l'aperçu DOCX et R2 les y lisaient encore ; R1 a ajouté une variante de `BreakKind` qui a cassé la compilation à l'intégration. Parade : avant de lancer une story qui modifie un type ou une représentation partagés, lister ses lecteurs (`grep`) et l'écrire dans le brief ; intégrer les branches dépendantes dans une branche d'intégration, compiler, puis seulement fusionner dans `main`.
