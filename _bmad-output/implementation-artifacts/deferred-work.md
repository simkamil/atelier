
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
