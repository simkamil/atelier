# US-243 — Mise en forme des paragraphes depuis le ruban (tranche sans trame ni bordures)

Contrat : [us-240-format-protocol.md](us-240-format-protocol.md) §4.2, §5.3, §5.8, §8.2. Code : `crates/core-wasm/src/rich_editor/format_para.rs` (`ApplyParaFormat`, `FormatMarks`), `PageCache::format_marks` (`rich_layout/cache.rs`, méthode neuve), `web/format-para.mjs` et `.css`, bloc `# US-243` de `web/i18n/fr-FR.txt`.

## Fait

| Élément | Où | Preuve |
|---|---|---|
| Alignements (gauche, centré, droite, justifié), second clic sur l'actif = gauche | `format-para.mjs` (`align`, décidé sur un `FormatState` lu juste avant) | test navigateur : lignes dessinées centrées, bouton pressé, mixte sans bouton pressé |
| Augmenter / diminuer le retrait (720 twips, borné à 0) | moteur `indent_step` | Rust + navigateur (bord gauche des lignes décalé de 72 pt) |
| Interligne : 1,0 à 3,0 (menu coché), Options d'interligne (Simple, 1,5, Double, Au moins, Exactement, Multiple) | menu + `createDialog` | pas de lignes mesuré sur le Canvas (240 → 480, exact 24 pt) |
| Espace avant / après (ajouter / supprimer) | engine `toggle` ; sélection mixte : `set 240` (« Ajouter » de Word) | Rust + navigateur |
| Tous les paragraphes touchés, une seule étape Ctrl+Z | `touched_paragraphs` + `RichDocument::apply_format` | Rust, WASM, navigateur |
| Enregistrement DOCX puis réouverture | writer `fmt-ooxml` : rien à corriger (jc, ind, spacing écrits) | `tests/rich-editor-format-para.mjs` (alignement, retraits, 1re ligne, interligne `atLeast` et `exact`, espacements) |
| Marques de mise en forme (T243-C) | `FormatMarks` côté moteur (¶, espace, tabulation, espace insécable, saut de ligne, saut de page), surcouche Canvas côté client, bouton ¶ (aria-pressed), Ctrl+Maj+8, préférence mémorisée | Rust (géométrie, sauts), WASM, navigateur (pixels de la surcouche aux positions du moteur, suit le zoom et la frappe) |
| Trame, Bordures | boutons désactivés avec raison (`aria-disabled`, `title`, `aria-describedby`) ; `ApplyParaFormat` refuse la commande entière `RICH_FORMAT_NOT_IMPLEMENTED` | Rust, WASM, navigateur |

Raccourcis : Ctrl+L, E, R, J ; Ctrl+M, Ctrl+Maj+M ; Ctrl+1, 2, 5 (par `code`, donc aussi en AZERTY) ; Ctrl+0 ; Ctrl+Maj+8. Info-touches : G N D J M O L Q ; B est gardée pour Bordures (et Trame, sans lettre) tant qu'elles sont désactivées.

## Décisions

- **Un seul réglage de page pour tout le document** : la surcouche suppose des pages de même taille ; elle prend la géométrie de `editor.getGeometry()` si l'éditeur l'offre, sinon déduit la hauteur de la zone d'espacement du défilement, sinon A4.
- Saut de colonne : signalé comme saut de page (la mise en page les traite pareil, `prepare.rs`).
- `FormatMarks` ne lit que `glyphs` et `lines` de la mise en page (pas de `text-layout/src/lib.rs`). Plus de `MAX_FORMAT_MARKS` marques sur les pages demandées : `RICH_LIMIT` (le client demande une page à la fois).
- Le ¶ d'un paragraphe se pose à la fin de sa dernière ligne ; un saut de ligne ou de page à la fin de sa ligne.

## Branchements demandés à l'intégrateur (fichiers hors périmètre de la tranche)

1. `web/app.mjs`, après `void formatController.attach(editor);` (ligne 391) : `paraFormat.attachEditor(editor);`, et `paraFormat.detachEditor();` au démontage. **Sans cela le bouton ¶ mémorise son état mais rien ne se dessine dans le produit** (le test navigateur l'attache lui-même).
2. `web/rich-editor.mjs`, objet `api` : `getGeometry,` (déjà défini ligne ~790) ; facultatif, la surcouche a un repli.
3. `tests/rich-editor-ui-browser.mjs` lignes 205 à 210 : l'assertion de préparation (« Police puis Paragraphe, chacun avec son bouton désactivé ») ne tient plus dès que les modules remplissent leurs groupes.
4. CI (job `wasm`) : `node tests/rich-editor-format-para.mjs` ; navigateur : `node tests/run-rich-editor-format-para-browser.mjs`.
