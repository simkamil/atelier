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
| R0 | US-203 (partie contrat) | S | Enveloppe ATED v2 (aujourd'hui v1 seulement, `editor.rs:973`), identifiant de document u32, positions plates en octets UTF-8 avec `\n` entre paragraphes, réponse limitée aux pages modifiées, à une fenêtre de texte et à la révision. Spec et codec de référence figés. |
| R1 | US-204 | M | `text-model/src/rich/flat.rs` et `edit.rs` : index de sommes préfixes par paragraphe, édition multi-paragraphes appliquée en place avec inverse, sans clone. |
| R2 | US-216 | M | `core-wasm/src/rich_layout.rs` et `rich_paint.rs` : incrémental, `CaretLayout`, cache par page, profils TXT et document ; dessin extrait de `rich_preview.rs`. |
| R3a | US-217 | M | `core-wasm/src/rich_editor.rs` : saisie, suppression par graphème et par mot, sélection, déplacement, test de position, annuler/rétablir ; tables annexes remappées par `position_map`. |
| R3b | US-218 | M | Ouvrir, créer, enregistrer en TXT et DOCX ; images en `AnchoredObject`. |
| R4 | US-203 (partie JS) | M | `web/rich-editor-protocol.mjs` et `web/editor-page-cache.mjs`. |
| R5 | US-209 | M | `web/document-mirror.mjs` : miroir DOM accessible de la portion visible. |
| R6a | US-006 | S | Budget WASM séparé entre code et polices. |
| R7 + R8 | US-219 | M + S | Bascule (suites v1/v2, v2 par défaut, latence mesurée dans Edge) puis retrait de l'éditeur v1. |

Graphe : R0 → {R1, R2, R4, R6a} ; {R1, R2} → R3a → R3b ; {R3a, R4} → {R5, R7} ; R3b → R7 → R8 ; R7 → US-240, US-243, US-250, US-225 riche, US-322, US-327.

Lots de trois agents sur des fichiers disjoints :

1. R0 (seul), puis **lot 1** : R1, R2, R4 ; R6a par l'intégrateur.
2. **Lot 2** : R3a, R5, préparation de R7 (`tests/run-editor-*.mjs`).
3. **Lot 3** : R3b, R7 côté interface (`web/editor.mjs`, `web/app.mjs`), puis R8.

Réservés à l'intégrateur : `core-wasm/src/lib.rs`, routage de `editor.rs`, `web/app.mjs`, `web/i18n/fr-FR.txt`, `Cargo.toml`, `tools/bench/references/wasm-size.json`.

## Risques et parades

- **Régression TXT** (11 fichiers de test liés au protocole, 35 suites navigateur) : v1 gelée jusqu'à R7, suites exécutées en v1 et v2, aller-retour TXT octet pour octet bloquant.
- **Performance sur 300 pages** : application en place, incrémental, pages modifiées seulement, aucun export par frappe. Critère R7 : moins de 16 ms de bout en bout dans Edge.
- **IME** : aperçu de composition aujourd'hui estimé à 7 px par caractère (`editor.mjs:417`) ; la géométrie de composition viendra du moteur. Les essais AZERTY et IME physiques restent manuels.
- **Accessibilité du Canvas** : R5 est obligatoire avant R7.
- **ABI** : v2 versionnée, v1 gelée.
- **Intégration** : routage ajouté par l'intégrateur seul, un module par famille d'opcodes.

## Budget WASM

Les quatre polices Carlito occupent 2 734 244 octets, soit 58,8 % des 4 653 389 octets actuels ; le code et les autres données en font environ 1 919 145. Le plafond actuel (4 708 206) vaut la référence de 4 280 188 plus 10 %, une tolérance pensée pour le code mais appliquée à un fichier composé à 59 % de polices.

Estimations à remesurer à chaque tranche : R1 +5 à +15 Ko, R2 +25 à +45 Ko, R3 +30 à +60 Ko, R5 +3 à +8 Ko, R8 −25 à −50 Ko. Le total avant R8 dépasse la marge dès le lot 2. Le profil `wasm-release` est déjà au maximum (`Cargo.toml:54-62`) et `wasm-opt` est exclu par D11.

Décision (US-006) : deux budgets. Les polices forment une liste figée contrôlée par empreinte ; le code a sa propre référence avec la même tolérance de 10 %.
