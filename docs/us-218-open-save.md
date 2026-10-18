<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-218 — Ouvrir, créer et enregistrer dans l'éditeur riche (tranche R3b)

Moteur et protocole seulement ; bandeau et boutons sont la tranche R7.

## Opcodes (ATED v2, ajouts groupés)

| Opcode | Message | Corps |
|---|---|---|
| `0x0110` | `Save` | `format u8` (1 TXT, 2 DOCX, 3 DOCM, 4 DOTX), 3 octets nuls. Réponse `SaveData` ; l'état ne change pas. |
| `0x0111` | `InsertImage` | `alt_len u16`, `alt` UTF-8 (≤ 512 octets, sans contrôle), puis l'image jusqu'à la fin du corps (1 octet à 8 Mio). Réponse `State`. |
| `0x0112` | `MakeEditableCopy` | vide. Réponse `SaveData`. |
| `0x8106` | `SaveData` | `revision u64`, `len u32`, octets, `notice_count u16` (≤ 64), puis par avis `code u16`, `len u16`, UTF-8 (≤ 255). |

Codes d'erreur : 21 `RICH_READ_ONLY`, 22 `RICH_SAVE_FAILED`, 23 `RICH_IMAGE_INVALID`, 24 `RICH_REVISIONS_UNSUPPORTED` ; `RICH_LIMIT` (19) pour une image de plus de 8 Mio, 512 images ou 32 Mio d'images par document, ou un fichier de plus de `MAX_SAVE_BYTES` (32 Mio de trame moins l'en-tête, les champs fixes et 64 avis au maximum). Le client envoie `MarkSaved` après un `Save` réussi (inchangé).

`State` : bit 6 (`0x40`) `read_only`, bit 7 (`0x80`) `has_revisions` ; le masque des drapeaux passe de `0x3f` à `0xff`.

Codes d'avis : `0x0100 + n` écrivain DOCX (rang de `WriteNotice` : 0 macros retirées, 1 paragraphe reconstruit, 2 enveloppe perdue, 3 contenu alternatif aplati, 4 blocs déplacés, 5-6 surlignage approché, 7 caractère non pris en charge, 8 fusion modifiée, 9 thème non écrit, 10 en-têtes et pieds non écrits, 11 liste sans définition, 12 avis supplémentaires) ; `0x0200 + n` perte TXT (rang de `LossCategory`, texte `clé=nombre`) ; `0x0210` caractères remplacés par `?`, `0x0212` fins de ligne mêlées ramenées à la dominante, `0x0213` séquences invalides remplacées à l'ouverture ; `0x0300` révisions acceptées, `0x0301` commentaires retirés, `0x0302` lignes supprimées retirées (copie éditable) ; `0x03ff` avis supprimés.

## Ouverture et origine

La session garde l'origine : TXT (encodage détecté, BOM, style de fin de ligne majoritaire), DOCX (l'`ImportedDocx` : paquet, squelette, référence de l'écrivain), ou nouveau document.

- `Save` TXT : `rich_to_text` puis `save_text` ; fichier d'origine rendu tel quel (octet pour octet quand l'ouverture est exacte : fins de ligne non mêlées, aucune séquence remplacée) ; nouveau document ou DOCX : UTF-8 sans BOM, CRLF. Caractères hors encodage : `?` et avis. Rapport de perte US-316 en avis.
- `Save` DOCX/DOCM/DOTX : DOCX ouvert, `write_docx_with_pictures` sur une copie du modèle de la session (l'historique reste), `regenerate_all` si le document est modifié (revient à l'écriture simple si elle échoue) ; nouveau document ou TXT : `write_new_docx_with_pictures` (profil US-202). Pas de `SaveContext` : le moteur n'a pas d'horloge, les propriétés ne sont pas mises à jour.
- DOCX avec révisions (`w:ins`, `w:del`, `w:moveFrom`, `w:moveTo`, changements de propriétés) ou commentaires : détection par `scan_revisions` (fmt-ooxml, `docx/revisions.rs`, sur tous les fichiers Word du paquet). La session est en lecture seule : toute commande d'édition (`InsertText`, `ReplaceRange`, `Delete`, `Undo`, `Redo`, `InsertImage`) répond `RICH_READ_ONLY` sans rien changer ; déplacements, sélection, `Save` restent permis. Le texte affiché est « toutes modifications acceptées » (l'import le donne déjà ; les objets-repères du texte supprimé, des déplacements et des commentaires sont retirés de la copie affichée). Un `Save` de la session lue seule rend le paquet tel qu'ouvert.
- `MakeEditableCopy` : sur un document en lecture seule, un DOCX où chaque révision est acceptée et les commentaires retirés (`accept_all_revisions` : copie des octets sources moins les plages retirées, sans réécriture XML ; parties de commentaires et leurs relations supprimées), avec avis. La session ne change pas : le client ouvre ces octets (`Open`), éditable. Refus `RICH_REVISIONS_UNSUPPORTED` pour ce qui ne s'accepte pas fidèlement : marque de paragraphe supprimée (fusion), cellule supprimée ou fusionnée, partie non UTF-8. Les lignes de tableau supprimées sont retirées. Sur un document éditable, la commande équivaut à `Save` DOCX.

## Images

`InsertImage` : `images::analyze` (US-270 : PNG, JPEG, GIF, BMP ; 8 Mio ; 16 M pixels ; EXIF), objet ancré de genre `atelier:image` inséré au caret par `RichDocument::insert_object` (nouveau, annulable, 3 octets plats, alt conservé), la sélection étant d'abord supprimée. À l'enregistrement DOCX, l'objet devient un `w:drawing` et l'image un fichier `word/media/imageN.<ext>` sous un nom libre du paquet.

Migration des images de l'éditeur TXT v1 : la v1 les garde dans un magasin JS indexé par un U+FFFC privé ; R7 les réinsérera par `InsertImage` (position, octets, texte alternatif) à la bascule. La conversion n'est pas exercée de bout en bout ici : la sous-partie du critère reste décochée.

## Tests

`cargo test -p atelier-core-wasm` (`rich_editor/tests.rs` : corpus TXT 30 fichiers, DOCX, révisions, images ; `rich_protocol/io_tests.rs`), `cargo test -p atelier-fmt-ooxml` (`docx_revisions.rs`, `docx_editor_corpus.rs` qui exporte le corpus vers `target/us218-corpus/`), `cargo test -p atelier-text-model`, `node tests/rich-editor-save.mjs` (vrai WASM : mêmes scénarios, vecteurs `tests/rich-editor-save-vectors.txt`). Fixtures : `tests/docx-reference/revisions.docx` et `comments.docx` (`tools/generate-us218-fixtures.py`). Le corpus TXT est synthétique (aucun fichier TXT n'existe dans le dépôt) : UTF-8 avec et sans BOM, UTF-16 LE/BE, Windows-1252, LF/CRLF/CR, avec et sans saut final.

Non couvert : bornes de 32 Mio vérifiées côté codec seulement (un essai à 32 Mio par le WASM dure plusieurs minutes) ; fichiers TXT mêlant les fins de ligne ne sont pas octet pour octet (avis).
