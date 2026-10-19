# US-243 — Mise en forme des paragraphes depuis le ruban

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
| Trame de fond, Bordures (US-243-M1, T243-A, T243-B) | voir « Trame et bordures » ci-dessous | Rust (modèle, cascade, DOCX, état, handler, mise en page, peinture), WASM, navigateur (pixels du Canvas, menu, palette, Ctrl+Z) |

Raccourcis : Ctrl+L, E, R, J ; Ctrl+M, Ctrl+Maj+M ; Ctrl+1, 2, 5 (par `code`, donc aussi en AZERTY) ; Ctrl+0 ; Ctrl+Maj+8. Info-touches : G N D J M O L Q, B (Bordures) et V (Trame de fond : toutes les lettres réservées à US-243 étaient prises).

## Décisions

- **Un seul réglage de page pour tout le document** : la surcouche suppose des pages de même taille ; elle prend la géométrie de `editor.getGeometry()` si l'éditeur l'offre, sinon déduit la hauteur de la zone d'espacement du défilement, sinon A4.
- Saut de colonne : signalé comme saut de page (la mise en page les traite pareil, `prepare.rs`).
- `FormatMarks` ne lit que `glyphs` et `lines` de la mise en page (pas de `text-layout/src/lib.rs`). Plus de `MAX_FORMAT_MARKS` marques sur les pages demandées : `RICH_LIMIT` (le client demande une page à la fois).
- Le ¶ d'un paragraphe se pose à la fin de sa dernière ligne ; un saut de ligne ou de page à la fin de sa ligne.

## Trame et bordures

- **Modèle** (`text-model/src/rich/attrs.rs`) : `ParaProps.shading: Option<ParaShading>` (couleur de fond ; `pattern` = le `w:val` d'un motif autre que `clear`, dessiné en `pattern_color`, mélangé au fond selon `pctNN`, `solid` = plein) et `ParaProps.borders: ParaBorders` (haut, gauche, bas, droite, entre, barre ; `ParaBorder` = style simple/double/pointillé/tireté ou `None` explicite, épaisseur en 1/8 pt, écart en pt, couleur ou automatique). La cascade des styles passe par `overlay` / `clear` : chaque côté se superpose séparément, la trame se remplace entièrement. Le style `None` (`w:val="nil"`) masque la bordure d'un style.
- **DOCX** : import de `w:shd` et `w:pBdr` (`parse_para_props`), famille d'écriture `Borders` et `Shading` dans `write/props.rs` (`GenEl` porte désormais des enfants). Une famille inchangée garde sa forme d'origine : les styles de bordure que le modèle ne dessine pas (vague, relief…), les motifs `pctNN` d'origine, `themeFill`, `w:shadow`, `w:frame` ne sont perdus que si l'utilisateur modifie les bordures ou la trame de ce paragraphe. Les couleurs de thème et les styles inconnus sont lus comme couleur automatique et trait simple.
- **ODT, DOC** : non importés (les propriétés `fo:border*`, `fo:background-color` d'un ODT restent des attributs opaques ; le DOC ne lit pas `sprmPBrc*` ni `sprmPShd`).
- **Commande** : `ApplyParaFormat` `shading` (set couleur ; clear = « Aucune couleur », qui masque aussi la trame d'un style) et `borders` (`keep`, `none`, trait par côté). Toutes les cibles, un pas d'historique, `LayoutScope::All` (le cadre d'un paragraphe dépend de ses voisins).
- **Mise en page** : `PageCache::lay_out_one` agrandit la première ligne (écart + épaisseur de la bordure haute) et la dernière (bordure basse, ou « entre » si le paragraphe suivant partage le cadre) ; la règle de somme de la pagination reste vraie. Deux paragraphes consécutifs partagent une boîte quand leurs bordures et leurs retraits gauche et droit sont identiques (règle de Word) : haut une fois, bas une fois, ligne « entre » entre eux. Le curseur et la sélection (`caret_window`) ignorent la place des bordures. Une édition relance aussi les voisins quand un cadre est en jeu (`widen_for_frames`).
- **Peinture** : `rich_paint.rs` (`FrameFragment`, `frame_draws`, `paint_frames`) dessine, avant les glyphes de la page, la trame (zone de texte retraits compris, bordures latérales incluses) puis les bordures sous forme de rectangles ; le même `Vec<Draw>` sert `paint_page` (écriture directe, atlas) et `push_frames` (chemin `Command` de référence des tests) : mêmes octets. Cadre ouvert aux sauts de page (haut seulement sur la première ligne, bas seulement sur la dernière) ; la barre (`w:bar`) est conservée et non dessinée.
- **Saut de page** : le cadre est ouvert à la coupure. Le haut n'est dessiné que sur la première ligne du paragraphe, le bas que sur sa dernière, la ligne « entre » que si le paragraphe finit sur la page ; jamais de « entre » à la coupure. Les côtés se prolongent sur chaque page. Écriture DOCX : seul le côté modifié de `w:pBdr` est réécrit, les autres côtés gardent leur XML d'origine (themeColor, shadow, frame, styles inconnus).
- **Réserves de couleur** : un `themeColor` de bordure et un `themeFill` de trame ne sont pas résolus (lus comme couleur automatique, donc noir ou rien) ; ils sont conservés à l'écriture tant que le côté ou la trame n'est pas modifié. Une trame modifiée perd son `themeFill`. « Bordures extérieures » ne touche pas la ligne entre paragraphes.
- **Limites** : `text-layout/src/borders.rs` (US-215) planifie tout le document d'un coup et n'est pas appelable page par page ; il reste inutilisé. L'aperçu DOCX de `rich_preview.rs` ne dessine ni trame ni bordures.

## Branchements demandés à l'intégrateur (fichiers hors périmètre de la tranche)

1. `web/app.mjs`, après `void formatController.attach(editor);` (ligne 391) : `paraFormat.attachEditor(editor);`, et `paraFormat.detachEditor();` au démontage. **Sans cela le bouton ¶ mémorise son état mais rien ne se dessine dans le produit** (le test navigateur l'attache lui-même).
2. `web/rich-editor.mjs`, objet `api` : `getGeometry,` (déjà défini ligne ~790) ; facultatif, la surcouche a un repli.
3. `tests/rich-editor-ui-browser.mjs` lignes 205 à 210 : l'assertion de préparation (« Police puis Paragraphe, chacun avec son bouton désactivé ») ne tient plus dès que les modules remplissent leurs groupes.
4. CI (job `wasm`) : `node tests/rich-editor-format-para.mjs` ; navigateur : `node tests/run-rich-editor-format-para-browser.mjs`.
