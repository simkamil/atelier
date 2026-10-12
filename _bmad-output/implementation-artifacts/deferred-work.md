
- source_spec: `_bmad-output/implementation-artifacts/spec-us103-input.md`
  summary: Vérifier l’ordre natif WebView2 entre compositionend et insertText si plusieurs tâches sont impliquées (gravité medium non vérifiée).
  evidence: Le dédoublonnage expire au prochain timer afin de préserver un texte identique saisi plus tard ; une trace IME native est nécessaire pour établir si le commit final peut réellement arriver après ce timer. Les tests synthétiques ne constituent pas cette preuve.

- source_spec: `spec-us120-editor-ui.md`
  summary: Absence de copy/cut vérifiée dans text-input.mjs, déjà présent avant ce lot. Le presse-papiers relève explicitement de la story V1 US-225, distincte du démonstrateur V0 US-120 ; limitation conservée pour les prochains travaux.
  evidence: Revue B3, traces dans text-input.mjs et editor-layout.rs ; critères V1 US-225/221/214 explicitement non clôturés.

- source_spec: `spec-us120-editor-ui.md`
  summary: Ctrl+flèche par mot est reconnu par l’adaptateur préexistant mais refusé explicitement par ce contrôleur. La story V1 US-221 porte ce comportement ; erreur visible et contenu préservé, pas de revendication que cette story est livrée.
  evidence: Revue B4, traces dans text-input.mjs et editor-layout.rs ; critères V1 US-225/221/214 explicitement non clôturés.

- source_spec: `spec-us120-editor-ui.md`
  summary: La colonne visuelle préférée n’est pas conservée après une ligne courte ; US-214 contient explicitement ce critère V1. La navigation simplifiée actuelle est documentée sans clôturer US-214.
  evidence: Revue B7, traces dans text-input.mjs et editor-layout.rs ; critères V1 US-225/221/214 explicitement non clôturés.

## Résolutions partielles du lot édition TXT — 2026-10-11

Les trois reports précédents (copy/cut, Ctrl+flèches par mot, colonne visuelle) sont résolus pour le parcours TXT. Voir `editing-ergonomics-validation.json` et `editing-ergonomics-review.md`. Les critères riches, objets, formats HTML/interne et essais physiques des US parentes restent ouverts. Les entrées historiques ci-dessus conservent la décision du lot initial.

- source_spec: `_bmad-output/implementation-artifacts/spec-us201-style-engine.md`
  summary: Comparer dans Word le style calculé des paragraphes d’une table imbriquée sans style propre.
  evidence: R6 (medium non vérifié) : le moteur hérite du style de table ancêtre; les sources consultées ne prouvent pas une divergence. Une fixture DOCX et son rendu Word de référence trancheraient ce cas.

- source_spec: `_bmad-output/implementation-artifacts/spec-us301-docx-text-styles.md`
  summary: Représenter la réinitialisation explicite de couleur et surlignage dans le modèle riche et sa cascade.
  evidence: R6 de docx-rich-layout-review.md; direct highlight none conserve actuellement le surlignage hérité, avec marqueur opaque et notice UnsupportedProperty. Un changement coordonné du modèle/cascade/codec et un test d’intégration de rendu sont nécessaires.

- source_spec: `spec-rich-incremental-borders.md`
  summary: Prévalidation globale et copies du cache limitent la latence incrémentale.
  evidence: Benchmark CPU 300 pages; prévalidation O(n) et cible de frappe de 16 ms encore ouverte.

- source_spec: `spec-rich-incremental-borders.md`
  summary: Raccorder la fabrique de document riche au flux Nouveau.
  evidence: Le chemin UI actuel crée un document TXT; US-202 reste partielle.

- source_spec: `spec-rich-incremental-borders.md`
  summary: Raccorder les commandes de bordures au rendu de l’éditeur riche.
  evidence: L’adaptateur Rust est livré; aucun consommateur UI riche actuel, US-215 reste partielle.

- source_spec: `spec-rich-incremental-borders.md`
  summary: Définir la persistance et l’import des politiques de bordure par paragraphe.
  evidence: Politiques fournies séparément à ParagraphBorderInput; le modèle ParaProps ne les conserve pas.

- source_spec: `spec-rich-incremental-borders.md`
  summary: Vérifier les bordures et regroupements sur un corpus OOXML et Word authentique.
  evidence: Les fixtures numériques et SVG actuelles ne constituent pas un oracle Word.

- source_spec: `spec-rich-incremental-borders.md`
  summary: Appliquer DefaultPresentation à une géométrie de section typée dans le flux riche.
  evidence: Section ne contient pas de géométrie; valeurs exactes disponibles dans le profil externe.

- source_spec: `spec-rich-incremental-borders.md`
  summary: Mesurer la frappe complète sur 300 pages sur le PC de référence.
  evidence: Benchmark update CPU sur i9-13900KF/64 Gio; aucun événement clavier ni frame UI, seuil 16 ms non certifié.
