# Epic 0.8 Context: Modèle commun

<!-- Compiled from planning artifacts. Edit freely. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Fournir le socle commun des documents : une représentation à identités stables, des modifications exprimées par opérations inversibles et des transactions atomiques. Ce socle doit servir le modèle de texte et les fonctions ultérieures d’annulation, de récupération et de collaboration sans coupler les abstractions à un type de document unique.

## Stories

- US-070 : Arbre de nœuds indexé
- US-071 : Opérations et transactions
- US-072 : Pile annuler / rétablir
- US-073 : Modèle de sélection
- US-074 : Journal de transactions

## Requirements & Constraints

- Chaque nœud possède une identité stable ; les opérations expriment les modifications et peuvent être inversées. Une commande composée est une transaction atomique : une erreur restaure l’état exact et fournit une erreur explicite.
- Les positions de sélection s’appuient sur des identités de nœud et des décalages. Les abstractions génériques de modèle ne doivent pas dépendre d’une implémentation de sélection qui n’est pas encore disponible.
- L’annulation/rétablissement conserve l’historique après enregistrement et est limité par un budget mémoire déclaré. La récupération après crash utilise son propre journal durable et un instantané autonome ; elle ne doit pas être confondue avec l’historique interactif.
- Cible Rust stable, API sûre et sans dépendance externe ; préserver les contrats existants de `atelier-model` et le format versionné `ATOP` déjà livré.

## Technical Decisions

- Garder les contrats d’opération et de transaction génériques. Un état choisit son propre checkpoint et sa restauration ; ne pas imposer `Clone` à tous les états ou à toutes les valeurs. Un adaptateur concret peut porter ses propres contraintes.
- Une transaction prépare l’inverse depuis l’état précédant chaque opération, applique dans l’ordre et restaure un checkpoint exact à tout échec. L’inverse s’exécute dans l’ordre inverse. Les transactions n’ajoutent pas de journal durable ni de sémantique de coédition.
- L’historique interactif garde assez d’information pour appliquer l’opération inverse, rétablir l’opération et restaurer les snapshots de sélection. Ses coûts en mémoire génériques doivent être explicitement estimés ; la taille sérialisée `ATOP` ne représente pas à elle seule la mémoire conservée.
- Garder les modules isolés dans `crates/model` et n’exporter depuis la racine que l’API publique nécessaire. Les tests de modèle doivent vérifier le contenu et les identités, pas uniquement les résultats d’appel.

## Cross-Story Dependencies

US-071 s’appuie sur les identités de US-070 et fournit les primitives de transaction à US-072 et US-074. US-072 peut stocker et rendre des snapshots de sélection opaques ; l’implémentation du modèle et des transformations de sélection reste à US-073. Le journal durable de US-074 est un consommateur distinct des transactions, sans dépendance envers la pile undo/redo.
