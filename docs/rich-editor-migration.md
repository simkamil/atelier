# Bascule de l'éditeur principal sur le modèle riche

Plan validé par le propriétaire du produit le 2026-10-18. Il remplace l'éditeur texte brut d'US-120 par un éditeur unique fondé sur le modèle riche ; un fichier TXT devient un document riche au « profil TXT ». Les stories correspondantes sont dans [le backlog](../US-V0-V1.md), épique E1.3a.

## Diagnostic

Réutilisable :

- Modèle riche : opérations inversibles `RichOp` (`crates/text-model/src/rich/ops.rs:27`), dont scission et fusion de paragraphes, et table de correspondance des positions `position_map` (`ops.rs:308`).
- Recherche, statistiques et TXT ont déjà leur version riche : `ParagraphSource for RichDocument` (`search/source.rs:92`), `statistics/rich.rs`, `rich_to_text` avec rapport de perte (`plain_io/rich.rs:107`).
- Import et écrivain DOCX déjà liés au WASM (`core-wasm/src/staged.rs:26`, `docx_export.rs:7`) ; document par défaut `default_rich_document`.
- Mise en page incrémentale (`text-layout/src/incremental.rs:106`) et géométrie du curseur (`caret.rs:161`) validées, mais absentes du WASM.
- `CanvasRenderer` déjà partagé avec l'aperçu (`web/rich-preview.mjs:2`).

Manquant :

- `RichSession` (`core-wasm/src/rich_session.rs`) n'est branché à aucun opcode, se limite à un paragraphe et clone document, historique et tables annexes à chaque frappe (`rich_session.rs:236-239`, `:262`).
- Aucun index de positions plates ; `IndexedTree` indexe par nombre de nœuds (`crates/model/src/lib.rs:82`).
- Tables annexes hors du modèle (`breaks`, `symbols`, `run_styles`, `crates/fmt-ooxml/src/docx.rs:234-236`).
- Aucun miroir DOM accessible du document (SPEC §6.14) ; seul existe un `textContent` global (`web/editor.mjs:201`).

L'éditeur TXT actuel n'atteint pas non plus les objectifs : plafond de 128 Kio (`core-wasm/src/editor.rs:11`), clone de la session à chaque commande (`editor.rs:58`), remise en page complète (`editor.rs:670-690`) et état complet renvoyé à chaque réponse (`editor.rs:720-770`).

## Décision

Un seul éditeur. Deux modes doubleraient les opcodes, les suites, l'accessibilité et chaque critère US-22x ; corriger les performances du TXT revient de toute façon à le réécrire ; supprimer l'ancien chemin libère du budget WASM. L'éditeur v1 reste derrière un drapeau jusqu'au contrôle de parité de la bascule (US-219), puis il est retiré.

Profil TXT : Carlito 11 pt, interligne simple, aucun espacement (`editor-layout.rs:14-19`) ; enregistrement par `rich_to_text` avec le rapport de perte d'US-316.

## Découpage

| Tranche | Story | Taille | Contenu |
|---|---|---|---|
| R0 | US-203 (partie contrat) | S | Enveloppe ATED v2 (aujourd'hui v1 seulement, `editor.rs:973`), identifiant de document u32, positions plates en octets UTF-8 avec `\n` entre paragraphes, réponse limitée aux pages modifiées, à une fenêtre de texte et à la révision. Spec et codec de référence figés. Les positions plates restent propres au protocole : modèle et journal de récupération gardent identifiant de nœud + décalage (§6.3). |
| R1 | US-204 | M | `text-model/src/rich/flat.rs` et `edit.rs` : index de sommes préfixes par paragraphe, édition multi-paragraphes appliquée en place avec inverse, sans clone. Tables annexes intégrées au modèle : `breaks` et `symbols` en éléments en ligne (3 octets chacun), `run_styles` en attribut de run `CharAttrs.char_style` (0 octet), validé par le propriétaire du produit le 2026-10-18. |
| R2 | US-216 | M | `core-wasm/src/rich_layout.rs` et `rich_paint.rs` : incrémental, `CaretLayout`, cache par page, profils TXT et document ; dessin extrait de `rich_preview.rs`. Prévalidation globale de la pagination supprimée : moins de 4 ms par frappe sur 300 pages (aujourd'hui environ 72 ms). |
| R3a | US-217 | M | `core-wasm/src/rich_editor.rs` : saisie, suppression par graphème et par mot, sélection, déplacement, test de position, annuler/rétablir, y compris sur les éléments en ligne de R1. |
| R3b | US-218 | M | Ouvrir, créer, enregistrer en TXT et DOCX ; images en `AnchoredObject` ; DOCX à révisions ou commentaires en lecture seule. Dépend de l'écrivain DOCX livré, pas de sa certification Word. |
| R4 | US-203 (partie JS) | M | `web/rich-editor-protocol.mjs` est figé par R0 ; R4 crée `web/editor-page-cache.mjs` et l'intégration. |
| R5 | US-209 | M | `web/document-mirror.mjs` : miroir DOM accessible de la portion visible. |
| R6a | US-006 | S | Budget WASM séparé entre code et polices. Terminée le 2026-10-18 ; US-205 sortira ensuite les polices du WASM. |
| R7 + R8 | US-219 | M + S | Bascule (suites v1/v2, v2 par défaut, latence mesurée dans Edge) puis retrait de l'éditeur v1. |

Graphe : R0 → {R1, R2, R4, R6a} ; {R1, R2} → R3a → R3b ; {R3a, R4} → {R5, R7} ; R3b → R7 → R8 ; R7 → US-240, US-243, US-250, US-225 riche, US-322, US-327.

Lots de trois agents sur des fichiers disjoints :

1. R0 (seul), puis **lot 1** : R1, R2, R4 (R6a déjà livrée).
2. **Lot 2** : R3a, R5, préparation de R7 (`tests/run-editor-*.mjs`).
3. **Lot 3** : R3b, R7 côté interface (`web/editor.mjs`, `web/app.mjs`), puis R8.

Réservés à l'intégrateur : `core-wasm/src/lib.rs`, routage de `editor.rs`, `web/app.mjs`, `web/i18n/fr-FR.txt`, `Cargo.toml`, `tools/bench/references/wasm-size.json`.

## Périmètre de la première mise en page

Le lot 1 et R2 n'implémentent pas la mise en page des tableaux ni des en-têtes et pieds de page. Les paragraphes de cellule sont mis en page comme des paragraphes de corps pleine largeur (mode dégradé documenté) et les en-têtes et pieds ne sont pas dessinés. Les accesseurs de tableaux, de cellules et d'en-têtes et pieds seront ajoutés par la story qui les traitera, pas par US-203.

### Prérequis ajouté le 2026-10-18 : liste d'affichage compacte

La relecture de R3a a mesuré environ 500 octets par glyphe dans les pages ATDL de l'éditeur riche : une page de texte dense dépasse 4 Mio, et chaque frappe transporterait plusieurs mégaoctets du Worker au fil principal. La cible de 16 ms par frappe (R7) est hors d'atteinte sans une liste d'affichage qui référence les glyphes par (police, glyphe) et n'envoie chaque contour qu'une fois, mis en cache côté Canvas en `Path2D` (critère d'US-101). Cette tranche (R2b, liée à US-080 et US-101) doit précéder R7 ; R3a borne seulement les réponses en attendant.

Avancement du 2026-10-18 (R2b) : livré sur la branche `r2b-glyph-atlas`. Les pages de l'éditeur sont peintes en `DrawGlyphRun` (police 0, indice d'atlas) et un atlas par session de document est transmis une seule fois, dans une section des réponses State et Pages ([format et bornes](us-203-rich-editor-protocol.md), « Glyph atlas »). Page dense de lignes de 100 colonnes (`tests/rich-editor-abi.mjs`, vrai WASM) : 1 561 octets par glyphe avant, 36 en régime établi, première réponse 120 648 octets atlas compris (4 215 269 octets de page avant). Rendu identique au pixel près à l'ancien dessin dans Edge (`tests/run-rich-atlas-render-browser.mjs`, 24 comparaisons, tolérance zéro). Réserve : un atlas désynchronisé par une réponse abandonnée côté client ne se rétablit qu'à l'`Open` suivant tant qu'aucune requête ne permet au client de relancer l'époque (voir la spec) ; à trancher avant R7.

## Risques et parades

- **Régression TXT** (11 fichiers de test liés au protocole, 35 suites navigateur) : v1 gelée jusqu'à R7, suites exécutées en v1 et v2, aller-retour TXT octet pour octet bloquant.
- **Performance sur 300 pages** : application en place, incrémental, pages modifiées seulement, aucun export par frappe. Critère R7 : latence de frappe inférieure à 16 ms, de l'événement clavier à la fin du dessin Canvas, sur 300 pages (Edge headless, rendu logiciel, événements synthétiques) ; mesuré en R7c (p95 6,5 ms, max 7,7 ms) ; avec l'attente de l'image suivante p95 17,7 ms, max 20,3 ms, à cause du rafraîchissement à 60 Hz.
- **IME** : aperçu de composition aujourd'hui estimé à 7 px par caractère (`editor.mjs:417`) ; la géométrie de composition viendra du moteur. Les essais AZERTY et IME physiques restent manuels.
- **Accessibilité du Canvas** : R5 est obligatoire avant R7.
- **ABI** : v2 versionnée, v1 gelée.
- **Intégration** : routage ajouté par l'intégrateur seul, un module par famille d'opcodes.

## Budget WASM

Les quatre polices Carlito occupent 2 734 244 octets, soit 58,8 % des 4 653 389 octets actuels ; le code et les autres données en font environ 1 919 145. Le plafond actuel (4 708 206) vaut la référence de 4 280 188 plus 10 %, une tolérance pensée pour le code mais appliquée à un fichier composé à 59 % de polices.

Estimations à remesurer à chaque tranche : R1 +5 à +15 Ko, R2 +25 à +45 Ko, R3 +30 à +60 Ko, R5 +3 à +8 Ko, R8 −25 à −50 Ko. Le total avant R8 dépasse la marge dès le lot 2. Le profil `wasm-release` est déjà au maximum (`Cargo.toml:54-62`) et `wasm-opt` est exclu par D11.

Décision (US-006) : deux budgets. Les polices forment une liste figée contrôlée par empreinte ; le code a sa propre référence avec la même tolérance de 10 %.

Mise à jour du 2026-10-18 : US-006 est livrée. US-205 retirera ensuite les polices du WASM pour les charger via `Host.fonts` ; le budget polices portera alors sur les ressources de l'hôte.

Décision du 2026-10-18 (propriétaire du produit) : au branchement de R3a, la référence de code est réétalonnée sur la taille mesurée, comme croissance voulue avec sa raison écrite dans `tools/bench/references/wasm-size.json`, en gardant la tolérance de 10 %. Objectif : revenir sous l'ancienne limite de code (2 111 059 octets) après le retrait de l'éditeur v1 (R8).
