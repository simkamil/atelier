
- source_spec: `_bmad-output/implementation-artifacts/spec-us103-input.md`
  summary: Vérifier l’ordre natif WebView2 entre compositionend et insertText si plusieurs tâches sont impliquées (gravité medium non vérifiée).
  evidence: Le dédoublonnage expire au prochain timer afin de préserver un texte identique saisi plus tard ; une trace IME native est nécessaire pour établir si le commit final peut réellement arriver après ce timer. Les tests synthétiques ne constituent pas cette preuve.
