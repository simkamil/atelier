# US-228 — espaces françaises automatiques

L'option **Options → Espaces françaises automatiques** est activée par défaut. Elle utilise une case à cocher native étiquetée, accessible au clavier. Le choix est conservé localement lorsque le stockage est disponible; sinon il vaut pour la session.

À la frappe ordinaire, le démonstrateur TXT insère U+00A0 avant `:` et à l'intérieur des guillemets `« »`, U+202F avant `; ! ?`. Il remplace une espace ASCII existante ou ajoute celle qui manque. La frappe de `«` ajoute immédiatement l'espace intérieure; une espace ASCII saisie juste après cette espace générée est supprimée pour éviter un doublon. Les espaces Unicode déjà présentes, les sauts de ligne, tabulations et suites de ponctuation sont conservés.

Le collage, le dépôt, l'import, les compositions IME, les remplacements de sélection et les insertions de plusieurs caractères conservent exactement leur texte. L'option désactivée laisse toute frappe inchangée. Cette fonctionnalité concerne l'éditeur TXT actuellement disponible; elle ne termine pas la saisie riche US-220.

La frappe brute et la substitution sont deux transactions successives. **Ctrl+Z immédiat annule seulement la substitution**: après `café:` transformé en `café :`, une annulation laisse `café:`. Rétablir remet l'espace. La frappe suivante forme un autre groupe. Si la substitution dépasse une limite, la frappe brute déjà validée reste disponible.

`ATED` v1 ajoute l'opcode 17 ReplaceRange: révision attendue u64, temps u64, début/fin u32 puis texte UTF-8. Tous les nombres sont little-endian et les positions des offsets UTF-8. La transaction `EditKind::Other` conserve la sélection d'avant modification pour Annuler. Révision périmée, plage non valide et limites refusent toute mutation du candidat.

Le helper pur, les transactions Rust, l'ABI réelle et le navigateur vérifient ponctuation, guillemets, multioctets, annulation/rétablissement et option. Le navigateur initialise aussi l'entrée réelle de l'application et vérifie la case à cocher, le stockage local et le passage au Worker/WASM.
