# US-230 — Menu contextuel du TXT

Clic droit dans le Canvas, touche Menu/ContextMenu ou Maj+F10 sur l’entrée texte ouvrent Annuler, Rétablir, Couper, Copier, Coller et Tout sélectionner. Les actions reflètent sélection, historique, état occupé et présence des API clipboard. La sélection reste intacte à l’ouverture ; une coupe n’efface qu’après succès de l’écriture clipboard et revalidation du snapshot. Une sélection changée pendant l’opération est refusée. Le presse-papiers du système peut refuser l’accès ; le texte est alors conservé et l’erreur affichée. Les tests de contenu clipboard utilisent un adaptateur navigateur simulé, et ne certifient pas une autorisation OS sur toutes les machines.

US-230 reste partielle pour les contextes lien, tableau, image et en-tête, encore absents de l’éditeur riche. Les formats clipboard HTML/RTF ne sont pas ajoutés.

Validation Windows du 2026-10-14 : 18 contrôles, 243 tests Rust des quatre paquets concernés, 1177 entrées immuables. Les suites Edge contrôlent les composants, le produit normal/contraste élevé et le vrai Worker/WASM ; Clippy hôte/WASM, fmt, licences, i18n sans nouvelle dérogation de référence, données générées, budget WASM inchangé et sonde WebView2 passent. La sonde vérifie 49 ressources et les textes de licence embarqués. Les événements clavier sont synthétiques ; aucun audit lecteur d’écran, essai matériel ou passage Linux distant n’est revendiqué.

Preuves : [validation](../_bmad-output/implementation-artifacts/editor-utilities-validation.json), [revue triée](../_bmad-output/implementation-artifacts/editor-utilities-review.md).
