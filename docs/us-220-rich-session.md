# US-220 / US-226 — session DOCX conservée et édition du texte

Ce lot du 14 octobre 2026 prépare l'édition DOCX du MVP. Il fournit une API Rust testée ; l'application continue d'afficher les DOCX en lecture seule. **US-220 et US-226 restent ouvertes.**

## Comportement

`atelier_core_wasm::rich_session::RichSession` conserve un `ImportedDocx` : modèle riche, package OPC original, styles, thème, polices et métadonnées de tableaux. `imported()` donne une référence immuable pour le futur writer, sans réimporter ni remplacer le document par du TXT.

`replace(revision, paragraphe, début, fin, texte)` remplace une plage dans un seul paragraphe. Les positions sont des octets UTF-8 situés aux frontières des graphèmes. Le texte inséré reprend les attributs directs et la référence de style du caractère précédent ; au début du paragraphe, ceux du premier caractère. Les plages de styles, sauts importés et symboles suivent la modification. Les métadonnées des caractères supprimés disparaissent.

`select`, `undo` et `redo` conservent/restaurent les sélections. Chaque modification effective ou navigation d'historique augmente la révision ; une commande obsolète est refusée. Une nouvelle modification réussie efface le rétablissement. Une opération sans changement n'ajoute pas d'entrée et ne détruit pas le rétablissement.

Le rendu utilise le même moteur que l'aperçu existant : `import_preview_docx` importe une fois et `render_imported_docx` affiche le modèle courant. Le rendu de la modification doit réussir avant sa publication. Une erreur de limites, de modèle ou de rendu préserve modèle, métadonnées, sélection, révision et historique. Le package original n'est jamais modifié par la session.

## Limites et suite

- Une seule plage dans un paragraphe ; pas de création/fusion de paragraphes ni de remplacement entre paragraphes. CR/LF et les contrôles non pris en charge sont refusés ; tabulations acceptées.
- Historique borné à 4 Mio de coût logique conservateur, configurable à une valeur inférieure pour les tests. Ce budget ne mesure pas les allocations réelles. Les instantanés incluent les métadonnées d'import ; les documents riches en métadonnées peuvent atteindre le plafond avant les documents simples.
- Limites existantes : 8 Mio de DOCX source, 128 Kio de texte avec séparateurs, 1 024 paragraphes, 256 pages et budgets de rendu existants. La pagination reste simplifiée.
- Aucun nouvel opcode WASM ni commande UI ; aucun nouvel exécutable utilisateur n'est annoncé par ce lot.
- Le raccordement au writer US-307 et les essais Word/LibreOffice restent nécessaires. Conserver le package n'est pas une preuve de sauvegarde sans perte. La branche writer infère actuellement les changements depuis le texte final : tester notamment plusieurs modifications disjointes autour d'un passage stylé, ainsi que la suppression ou le déplacement de sauts et symboles.
- Restent ensuite le curseur et la sélection dans le rendu riche, la saisie clavier/IME, le presse-papiers riche, la mise en forme et l'enregistrement intégré.

## Vérification

Les tests indépendants de `crates/core-wasm/tests/rich_session.rs` couvrent le document de référence, les styles directs et nommés, les graphèmes combinés, les commandes obsolètes, le package opaque, les sauts/symboles, l'historique, le plafond incluant les chaînes de formatage et une erreur de pagination après modification candidate. Les tests `retained_preview.rs` comparent le rendu conservé à l'ancien chemin et vérifient qu'une modification du modèle apparaît sans toucher au package.

Les commandes et résultats définitifs sont consignés dans le [reçu de validation](../_bmad-output/implementation-artifacts/docx-edit-session-validation.json). Les tests TXT et aperçu existants sont relancés avec les tests du crate. Le WASM embarqué est reconstruit, son inventaire de sources et sa taille contrôlés sans augmenter le plafond. Cela n'établit pas une validation de l'interface DOCX éditable ni un aller-retour Word.
