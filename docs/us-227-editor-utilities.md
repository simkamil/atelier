# US-227 — Caractères spéciaux dans le TXT

Insertion → Symbole ouvre une grille de 2 107 scalaires nommés issus de la cmap Carlito et de l’UCD 17.0.0, répartis en 36 blocs. Recherche par nom Unicode anglais ou code préfixé U+/0x, pages de 256 cellules maximum, onze caractères spéciaux et seize récents en mémoire de session. Ctrl+Maj+Espace insère une espace insécable. L’insertion remplace la sélection UTF-8 en une transaction annulable. U+2011 reste exact dans le texte et utilise un contour de tiret disponible si la police ne le couvre pas.

US-227 reste partielle : le choix parmi plusieurs polices et la mise en forme/édition riche ne sont pas encore raccordés. La police du TXT est Carlito. L’aperçu DOCX reste en lecture seule.

Validation Windows du 2026-10-14 : 18 contrôles, 243 tests Rust des quatre paquets concernés, 1177 entrées immuables. Les suites Edge contrôlent les composants, le produit normal/contraste élevé et le vrai Worker/WASM ; Clippy hôte/WASM, fmt, licences, i18n sans nouvelle dérogation de référence, données générées, budget WASM inchangé et sonde WebView2 passent. La sonde vérifie 49 ressources et les textes de licence embarqués. Les événements clavier sont synthétiques ; aucun audit lecteur d’écran, essai matériel ou passage Linux distant n’est revendiqué.

Preuves : [validation](../_bmad-output/implementation-artifacts/editor-utilities-validation.json), [revue triée](../_bmad-output/implementation-artifacts/editor-utilities-review.md).
