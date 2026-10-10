# Epic 0.4 Context: XML

<!-- Compiled from planning artifacts. Edit freely. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Fournir les briques XML sans dépendance externe nécessaires aux formats bureautiques : une lecture événementielle stricte, une écriture contrôlée et la conservation fidèle des portions non interprétées. Les entrées de documents restent non fiables; le traitement doit être borné et ne jamais résoudre de ressource externe.

## Stories

- Story 0.4: Analyseur XML en flux (US-030)
- Story 0.4: Écrivain XML (US-031)
- Story 0.4: Sous-arbres XML opaques (US-032)

## Requirements & Constraints

Le parseur traite XML en flux, avec éléments, attributs, références prédéfinies et numériques, CDATA, commentaires, instructions de traitement, namespaces, UTF-8 et UTF-16. Il rapporte les erreurs avec position et applique des limites aux profondeurs, événements, noms, attributs, texte et mémoire. Le mode tolérant ne répare que la fermeture finale manquante. Les DTD ne déclenchent ni interprétation des entités déclarées ni accès externe.

Le writer échappe les caractères, vérifie les noms et l’équilibre de la structure, produit du UTF-8 déterministe et une sortie lisible par le parseur. La fidélité aux contenus inconnus est nécessaire pour qu’une ouverture puis sauvegarde ne détruise pas ces informations. Elle est sémantique XML, non une conservation garantie des octets de la source.

## Technical Decisions

Le crate XML est Rust sûr, sans dépendance externe ni `unsafe`; il est séparé des consommateurs OOXML/ODF. Reader et Writer restent les interfaces de base. Les captures opaques doivent respecter les plafonds existants avant allocations et transporter les informations namespace nécessaires à la réémission dans un autre contexte. Ne pas supposer que renommer les préfixes est sûr : des valeurs telles que `xsi:type="p:Widget"` portent une convention QName que le parseur XML générique ne peut pas reconnaître.

Les tests de format valident l’interopérabilité et les erreurs strictes; les analyseurs doivent tolérer des fichiers malformés sans panique et sans expansion DTD.

## Cross-Story Dependencies

US-032 dépend des événements et résolutions de namespaces du Reader (US-030), et de l’écriture contrôlée du Writer (US-031). Le contrat opaque doit permettre aux formats consommateurs d’attacher puis de réémettre un sous-arbre sans modifier les contrats d’événements ordinaires.
