# US-120 plain-text model

`atelier-text-model` owns UTF-8 plain text and an undo/redo history. Its offsets are global UTF-8 byte offsets, separate from the future rich model's stable `NodeId` positions. The document owns no UI or bridge integration; core-wasm can call this API in a later slice.

`PlainTextDocument::from_utf8(bytes, limits)` rejects malformed UTF-8 and input beyond `max_text_bytes`. A leading UTF-8 BOM is retained as metadata, excluded from `text()` and its offsets, and prepended by `export_utf8()`. All other bytes, including existing LF and CRLF sequences, remain exact. UTF-16 and legacy encodings are not decoded here.

`Selection { anchor, focus }` is directed; reverse selections are supported. Both offsets must be in range and at UTF-8 scalar boundaries. `insert_text` inserts the exact string and replaces a nonempty selection. `delete_backward` and `delete_forward` remove a selection or one Unicode extended grapheme cluster, so combining sequences, emoji sequences, and CRLF are atomic. Paragraph breaks are supplied as literal LF text; existing line endings are preserved and are not normalized.

Undo and redo use `atelier-model::History` transactions. Adjacent typing joins the same history group when timestamps are at most one second apart; Unicode whitespace closes the group after insertion. Cursor moves and non-typing edits close a group. A successful edit after undo clears redo. An empty `insert_text` at a caret is a no-op: it preserves redo, selection, and history, and succeeds even when the history budget is zero. Failed UTF-8, offset, text-size, and history-budget checks leave the logical text, selection, and history unchanged. The history limit is a conservative logical estimate, not allocator accounting; each edit declares twice the removed plus inserted byte lengths, plus `size_of::<ReplaceText>()`, twice the selection size, 16 machine words, and 256 bytes of margin for history/deque/transaction storage and allocator metadata.

The current implementation clones the complete `String` for each transaction checkpoint and computes grapheme boundaries across the full text for deletion. No benchmark currently demonstrates the target of under 16 ms for a 300-page document; that performance target remains unverified.

## Public API

- `PlainTextDocument::from_utf8`, `text`, `export_utf8`, `selection`, `set_selection`
- `insert_text`, `delete_backward`, `delete_forward`, `undo`, `redo`, `can_undo`, `can_redo`
- `PlainTextLimits { max_text_bytes, max_history_bytes }`
- `Selection { anchor, focus }`

The demonstrator still needs the later core-wasm command bridge, renderer, and UI/input integration. This crate does not provide rich document structure or stable node positions.

Le lot ressources, fichiers et texte brut a été développé en parallèle avec trois
agents GPT-6 Luna moyen, puis corrigé après trois revues indépendantes.
La [validation finale](../_bmad-output/implementation-artifacts/embedded-foundations-validation.json)
réussit **37 contrôles, 351 tests workspace et 48 tests natifs release**,
sur sources et lock inchangés. Les diagnostics debug/release chargent les onze
ressources, vérifient le vrai Worker/WASM par écho Ping, HEAD/404/405 et six
catégories de refus CSP avec traces natives. La [copie isolée de l’exécutable](../_bmad-output/implementation-artifacts/embedded-foundations-portable.json)
fonctionne sans dossier web/data. Les [18 constats de revue](../_bmad-output/implementation-artifacts/embedded-foundations-review.md)
sont triés ; les corrections retenues sont appliquées, aucun nouveau constat
n’est différé. Les [échecs exploratoires](../_bmad-output/implementation-artifacts/embedded-foundations-exploration.json)
restent conservés.

US-112 reste ouverte pour la journalisation des violations CSP internes aux
Workers et les scénarios service-worker. US-113 reste ouverte pour le pont
fichiers JS, SharedBuffer, les polices/URL et les essais de dialogues natifs.
US-120 reste ouverte pour la mise en page et la connexion saisie/modèle/rendu,
ainsi que ses mesures de latence et démarrage. US-111 conserve l’échec strict
de handles ; ses contrôles fonctionnels de 100 cycles et callbacks passent.
Le backlog reste à **35 US entièrement validées sur 158**. Les tests musl sont
compilés localement ; leur exécution appartient à la CI Linux du commit publié.

## Validation du premier éditeur intégré — 2026-10-11

Trois agents GPT-6 Luna moyen ont développé en parallèle le moteur, l’interface
et le pont fichiers, puis corrigé le lot après trois revues indépendantes.
La [validation finale](../_bmad-output/implementation-artifacts/editor-integration-validation.json)
réussit **40 contrôles, 368 tests workspace et 55 tests natifs release**,
avec sources et lock inchangés. Le navigateur utilise le vrai Worker/WASM/Canvas
et un Host factice : saisie/sélection, pagination, composition, annulations,
sauvegarde obsolète, raccourcis de la vraie page et miroir textuel accessible.
L’ABI réelle vérifie aussi le rollback d’un candidat lorsque le registre de
128 Mio ne peut réserver sa réponse.

Les diagnostics WebView2 debug/release chargent les **17 ressources** et vérifient
saisie accentuée, pixels de glyphes, undo/redo, HTTP et six catégories CSP.
La [copie isolée](../_bmad-output/implementation-artifacts/editor-integration-portable.json)
fonctionne sans dossier web/data. Les tests de routage natif injectent des résultats
de dialogue contrôlés et passent par le broker réel ; ils n’ouvrent aucun dialogue.
La [revue](../_bmad-output/implementation-artifacts/editor-integration-review.md)
conserve les 12 constats séparément : cinq corrections retenues et trois fonctions
V1 reportées (presse-papiers, navigation par mot, colonne préférée), avec les rejets
motivés. Les limites de wrapping et de Tab sont également décrites dans le contrat UI.

Le premier parcours TXT est testable avec `cargo run -p atelier-desktop-win --release --locked --offline`. La tranche reste limitée à
128 Kio, latin gauche à droite et mise en page complète à chaque modification.
Les dialogues manuels, AZERTY/IME physique et mesures de 300 pages/démarrage restent
ouverts pour US-120. US-113 conserve SharedBuffer, polices/URL ; US-111 conserve
l’échec strict de handles. Les **35/158 US complètes** sont inchangées. Les tests
musl sont compilés localement ; leur exécution appartient à la CI Linux publiée.
