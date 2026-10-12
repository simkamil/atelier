---
title: Aperçu DOCX en lecture seule dans le MVP Windows
status: done
baseline: 3fc5d860e673fc683f2fb92bcc73f1d621556286
---

## Résultat attendu

Depuis le MVP TXT, « Ouvrir DOCX » ouvre un dialogue Windows distinct puis affiche un aperçu A4 paginé du document importé. Les styles déjà pris en charge par le moteur sont visibles. « Retour au texte » et Échap retrouvent exactement le document TXT, sa sélection, son historique et son état de sauvegarde. L'aperçu est explicitement en lecture seule. Ce lot connecte les briques des US-301/210 ; il ne termine pas l'édition riche ni la fidélité complète à Word.

## Contrats communs et limites

- Entrée DOCX : 8 MiB maximum ; aucun chemin ni accès réseau exposé au JavaScript. Les limites TXT restent identiques.
- Aperçu : A4 portrait, marges 25 mm, texte latin LTR. Limites cumulées : texte UTF-8 128 KiB, 1 024 paragraphes, 256 pages, 65 536 glyphes, 26 MiB de listes ATDL. Refuser les dépassements avec une erreur contrôlée. Limiter le ZIP décompressé à 32 MiB et les polices embarquées à un budget explicite compatible avec cette enveloppe.
- Moteur pur : `rich_preview::preview_docx(&[u8]) -> Result<RichPreview, PreviewError>`, `RichPreview { pages: Vec<Vec<u8>>, text: String, notice_count: u32 }`. Chaque page est une ATDL autonome. Ne modifier aucun état TXT. Utiliser uniquement les quatre faces Carlito nécessaires comme polices de repli, pas toute la collection embarquée ; enregistrer les polices DOCX dans les limites.
- Pont moteur : commande ATED 18, corps = octets DOCX ; opcodes 0–17 conservés. Réponse ATRP : magic 4 octets, version u16=1, flags u16=0, nombre de pages u32, longueur texte u32, nombre de notices u32, texte UTF-8 puis pour chaque page longueur u32 et ATDL. Maximum réponse 32 MiB. Décodage strict, aucun suffixe accepté.
- Vue : `createRichPreview({canvas, scrollport, spacer, textRepresentation, onClose, onError})`, méthodes `setDocument({pages,text,noticeCount})`, `open({returnFocusTo})`, `close()`, `dispose()`. Pages Uint8Array ou ATDL parsées par CanvasRenderer. Validation atomique avant affichage ; miroir texte via textContent ; Échap ferme, focus rendu au déclencheur. Aucun accès au modèle TXT.
- Pont natif : opération `openDocx`, réponse base64 identique au protocole existant ; filtre DOCX, capacité FileBroker limitée à 8 MiB, révoquée après lecture. Annulation et erreurs restent explicites. Files et messages restent bornés.
- Le parent intègre les modules, gère les générations asynchrones et ignore les réponses obsolètes après fermeture/dispose. Les commandes TXT sont gelées durant dialogue et aperçu. Les limites de rendu et les éléments importés non affichés doivent être annoncés clairement sans promettre une fidélité Word complète.

## Répartition des écritures

Trois sous-agents GPT-6 Luna, raisonnement moyen, travaillent sur des ensembles disjoints. Ils lisent ce contrat intégralement, ne lancent pas d'autres agents, ne font ni Git ni rafraîchissement Graphify global et remontent fichiers, tests et limites.

1. **engine** : nouveaux `crates/core-wasm/src/rich_preview.rs` et `crates/core-wasm/tests/rich_preview.rs` uniquement. Import, résolution, composition, pagination, tracés/styles, tests avec fixtures réelles. Le parent ajoute modules et dépendances. Signaler immédiatement tout changement nécessaire au contrat.
2. **ui** : nouveaux `web/rich-preview.mjs` et `tests/rich-preview.mjs` uniquement. CanvasRenderer existant, cycle de vie, clavier/focus, validation atomique et tests de vue isolés.
3. **native** : `crates/desktop-win/src/file-bridge.rs`, `crates/desktop-win/src/win32.rs` et tests natifs associés dans ces fichiers uniquement. Opération, filtre, lecture bornée et tests de routage/broker sans dialogue interactif.

Le parent possède tous les autres fichiers : Cargo, lib/editor Rust, protocole JS, editor/app/native-host, HTML/CSS/catalogues, ressources natives, tests intégrés, documentation, packaging et validation finale. Les changements de contrat sont coordonnés avant modification. Les tâches dépendantes restent chez le parent.

Précision d'intégration : la vue accepte uniquement les pages `Uint8Array`. Les objets ATDL pré-parsés sont refusés pour empêcher de contourner la validation binaire et son budget agrégé. Le moteur garde les structures intermédiaires en 1/64 pt et convertit les coordonnées en points à l'émission ATDL ; tous les contours d'un glyphe partagent une seule opération de remplissage.

## Matrice de validation figée

| Cas | Résultat vérifiable |
| --- | --- |
| DOCX de référence indépendant | Aperçu non vide, texte attendu, titre coloré et taille/style distincts, rendu ATDL valide |
| Document multi-page | Pages A4 distinctes, texte dans l'ordre, pagination déterministe sans perte |
| DOCX invalide et dépassements | Erreur contrôlée ; aucune mutation TXT, aucun aperçu partiel |
| Annulation du dialogue / erreur de lecture | Retour au TXT inchangé ; contrôles de nouveau disponibles |
| Lecture native > limite TXT mais <=8 MiB | DOCX accepté ; plafonds TXT existants toujours testés |
| Protocole malformed / bornes / suffixes | Rejet strict ; requêtes et réponses en file restent bornées |
| Aller-retour TXT → aperçu → TXT | Texte, sélection, historique undo/redo et dirty identiques ; aucune commande de modification dans l'aperçu |
| Clavier/focus et miroir accessible | Échap ferme, focus restauré, texte injecté comme texte, défilement clavier utilisable |
| Réponse tardive après fermeture/dispose | Ignorée sans réouverture ni publication d'état TXT |
| Application intégrée Worker/WASM | Import via pont simulé, rendu réel et retour TXT testés dans le navigateur |

## Achèvement

Relecture indépendante des frontières moteur, UI et native ; corrections puis fmt, clippy, tests Rust/JS/navigateur, compilation Windows/WASM/Linux applicable et package reproductible. Mesurer l'augmentation WASM liée aux nouvelles briques/polices et réviser toute référence de taille uniquement avec justification explicite. Mettre à jour le suivi des US sans cocher des critères non réalisés. Rafraîchir Graphify, enregistrer les preuves, commit et push autorisés par la session. Rapporter séparément toute impossibilité de CI distante.

## Review Triage Log

Trois relecteurs indépendants GPT-6 Luna moyen, conformément au modèle demandé par l'utilisateur. Tous les résultats ont été reçus avant triage. Le relecteur de cas limites n'a signalé aucun défaut.

| ID | Verdict et preuve | Route / résolution |
| --- | --- | --- |
| Blind-1 | medium : le Canvas à 100% n'ajoute aucune largeur de contenu quand l'A4 dépasse le viewport. La droite ne peut pas être parcourue. | patch : largeur de contenu A4 et Canvas sticky de largeur viewport ; tests étroits et défilement. |
| Blind-2 | low : les pages sont bien ancrées à gauche ; il s'agit d'un choix esthétique, sans contenu perdu sur un viewport large. | rejet : centrer demanderait de modifier la géométrie de rendu sans réparer de défaut fonctionnel. |
| Blind-3 | false : `layout_paragraph` lit les propriétés résolues, convertit indent_left/right/first_line et applique Alignment, y compris Justify, dans text-layout/src/lib.rs. | rejet : la couche de pagination ne doit pas réappliquer ces propriétés. |
| Blind-4 | low : les contrôles Word de saut de page ne sont pas tous représentés ; le message générique était peu précis. | patch : limitation explicite dans la légende, catalogue et documentation. |
| Blind-5 | medium : aucun en-tête ni pied de page n'est rendu, alors que « en-têtes complexes » était trop restrictif. | patch : annoncer tous les en-têtes/pieds de page omis. |
| Blind-6 | medium : les marqueurs de listes ne sont pas générés ; la légende ne le précisait pas. | patch : indiquer puces et numéros de liste omis. |
| Blind-7 | low : Canvas ne fournit pas de liens activables ; le texte peut rester visible. | patch : indiquer les liens inactifs. |
| Blind-8 | low : seuls des nombres d'avertissements traversent ATRP ; les omissions sont connues mais non détaillées par document. | rejet : une nouvelle représentation de diagnostics ajouterait une surface publique ; la légende énumère désormais les limites de chaque aperçu. |
| Blind-9 | medium : le badge dirty/clean restait attaché au TXT sous une vue DOCX. | patch : badge « Lecture seule », puis restauration du badge TXT exact. |
| Blind-10 | low : titre principal/fenêtre/pied de page identifiaient la vue comme texte brut. | patch : identité DOCX pendant l'aperçu, identité TXT restaurée au retour. |
| Gap-1 | medium : le test natif vérifiait le genre OpenDocx mais pas le filtre réellement passé au picker ; une substitution TXT restait invisible. | patch : helper privé utilisé dans process_file_request, test des options/filtre reçus pour DOCX et TXT sans UI interactive. |

Corrections d'intégration antérieures à la revue : unités ATDL, contre-formes des glyphes, FontId réellement façonné, budgets cumulés avant accumulation des commandes, spacer recalculé après affichage, libération des listeners et inhibition des raccourcis natifs pendant l'aperçu. Aucun changement de périmètre requis.


## Validation traçable

| Ligne de la matrice | Preuve exécutée |
| --- | --- |
| Référence DOCX, styles et pagination | core-wasm/tests/rich_preview.rs : reference_docx_renders_real_title_color_and_valid_page, inline_bold_italic_underline_and_size_reach_the_display_list, generated_long_docx_paginates_without_losing_text_order |
| Erreurs et bornes | invalid_and_over_limit_docx_return_controlled_errors, paragraph_and_text_budgets_reject_oversized_models, glyph_budget_is_enforced_while_layout_is_built ; tests/rich-preview-protocol.mjs : ATRP et limites 8 MiB/TXT |
| Annulation, lecture native et limites séparées | file_bridge::tests::selected_paths_cover_success_cancellation_errors_and_revocation, open_docx_has_a_distinct_bounded_operation ; win32::tests::docx_route_selects_its_own_dialog_and_accepts_files_over_txt_limit |
| Protocole malformé et file bornée | tests/rich-preview-protocol.mjs, tests/editor-protocol.mjs, tests/editor-abi.mjs |
| Retour TXT intact, sélection, historique, dirty | core-wasm/tests/rich_preview_session.rs::preview_success_and_failure_preserve_txt_selection_history_dirty_and_revision ; tests/editor-abi.mjs ; tests/editor-browser.mjs |
| Clavier, focus et miroir | tests/rich-preview.mjs ; navigateur réel : Échap, restauration du déclencheur, inhibition Ctrl+S, miroir et pixels bleus |
| Réponses tardives / dispose | tests/editor-browser.mjs : late native response after close, disposing app during DOCX dialogue |
| Produit assemblé Worker/WASM | tests/run-editor-browser.mjs charge web/index.html/app.mjs et la fixture indépendante ; import natif simulé, vrais WASM et pixels Canvas |

Les tests de régression ajoutés après revue couvrent les paragraphes vides (réserve minimale de travail dans l'enveloppe 8 M), les tabulations (avance conservée sans contour .notdef), le scroll horizontal réel, l'identité DOCX/TXT et le filtre effectivement fourni au dialogue natif. La validation générale est consignée séparément dans rich-docx-preview-validation.json, avec l'inventaire immuable des sources et du package testé.

Validation finale : 24 contrôles réussis ; 760 tests Rust Windows, 111 assertions navigateur, inventaire immuable de 856 entrées. Package WASM 3883549 octets, SHA-256 9137af551246ea89b340ce5acc7f3e063b76522e5cc986caf6dca0ee63189e33. Aucun critère Word ou édition DOCX déclaré terminé.
