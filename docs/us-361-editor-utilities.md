# US-361 — À propos et licences hors ligne

Fichier → À propos affiche la version Cargo, la date UTC de compilation, l’adresse sélectionnable du dépôt et les notices complètes AGPL-3.0-or-later, Carlito/Caladea/Liberation OFL et Unicode. Aucun lien externe n’est ouvert automatiquement. SOURCE_DATE_EPOCH permet une date reproductible ; le convertisseur est testé par cargo test. Le serveur de développement annonce explicitement une date indisponible ; l’exécutable sert ses métadonnées de compilation, vérifiées par la sonde native.

US-361 est validée. La consultation fonctionne aussi pendant un aperçu DOCX sans changer le document.

Validation Windows du 2026-10-14 : 18 contrôles, 243 tests Rust des quatre paquets concernés, 1177 entrées immuables. Les suites Edge contrôlent les composants, le produit normal/contraste élevé et le vrai Worker/WASM ; Clippy hôte/WASM, fmt, licences, i18n sans nouvelle dérogation de référence, données générées, budget WASM inchangé et sonde WebView2 passent. La sonde vérifie 49 ressources et les textes de licence embarqués. Les événements clavier sont synthétiques ; aucun audit lecteur d’écran, essai matériel ou passage Linux distant n’est revendiqué.

Preuves : [validation](../_bmad-output/implementation-artifacts/editor-utilities-validation.json), [revue triée](../_bmad-output/implementation-artifacts/editor-utilities-review.md).
