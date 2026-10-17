# US-270 — Insérer une image

L'éditeur de texte brut insère des images PNG, JPEG, GIF et BMP à la position du curseur, depuis le ruban (Insertion > Image), par collage ou par glisser-déposer de fichier. L'image est une unité du texte (un caractère privé), alignée sur la ligne de base, réduite à la largeur de la zone de texte si besoin, orientation EXIF appliquée. Ses octets d'origine sont conservés ; l'export DOCX les écrit sans les ré-encoder.

## Contrat

**Modèle** (`atelier-text-model::inline_image`). `InlineImage` : format, octets d'origine, taille stockée, orientation EXIF 1..8. Dans l'éditeur, une image occupe un scalaire de la zone privée supplémentaire B (U+100000 et suivants, 4096 au plus) ; la session garde la table scalaire -> image. Le texte, le curseur, la suppression et l'historique Annuler/Rétablir existants s'appliquent donc sans changement : l'insertion est une entrée d'historique isolée (une seule étape d'annulation, qui restitue aussi le texte sélectionné qu'elle remplaçait). Dans le modèle riche, la même image est un `AnchoredObject` de genre `atelier:image` (`to_object_data`/`from_object_data`), insérable par `RichOp::InsertInline` donc annulable.

**Mise en page** (`atelier-text-layout::inline_object`, utilisé par la mise en page de l'éditeur). Taille naturelle = pixels × 0,75 pt (96 ppp ; la résolution du fichier est ignorée), largeur et hauteur échangées pour les orientations 5 à 8. Réduction proportionnelle à la largeur de la zone de texte (487,28 pt) et à la hauteur d'une page ; jamais d'agrandissement. La ligne qui porte une image a pour ascendante la plus grande hauteur d'image et garde la descendante du texte (3 pt) ; les lignes de texte seul gardent exactement leur géométrie (14 pt). Une ligne trop haute pour le bas de page passe à la page suivante. Le curseur, les rectangles de sélection et la recherche prennent la hauteur de la ligne ; un clic dans une image choisit sa ligne.

**Rendu.** L'image est une commande `DrawImage` (id = emplacement dans la session) de la liste d'affichage. Le moteur décode l'image à l'insertion (validation complète) puis à la demande (opcode 29) : orientation appliquée, réduction par moyenne de surface à 1600 pixels au plus sur le grand côté, RGBA droit. `web/image-insert.mjs` met les bitmaps en cache, les demande avant de publier un état qui les référence, vide le cache au chargement d'un document (les identifiants sont réutilisés) et borne la mémoire à 256 Mio.

**Entrées** (`web/image-insert.mjs`, `web/text-drag-drop.mjs`, `web/editor-shell.mjs`). Bouton Image du ruban (sélecteur `<input type=file>` masqué ; `image/*` est accepté pour qu'un format refusé produise un message). Collage : une image sans texte brut dans le presse-papiers est insérée ; avec du texte brut, le texte garde la priorité. Glisser-déposer de fichiers sur la page : repère d'insertion pendant le survol, insertion à l'endroit lâché. Plusieurs fichiers sont insérés dans l'ordre ; une erreur n'arrête pas les suivants.

**Refus** (messages du catalogue, bloc « US-270 » de `web/i18n/fr-FR.txt`) : format non pris en charge (WebP, TIFF, HEIF, SVG, texte renommé, variantes que les décodeurs refusent), image corrompue ou tronquée, trop volumineuse (8 Mio ou 16 millions de pixels), nombre maximal d'images (512 images distinctes, 32 Mio au total ; deux images identiques partagent un emplacement), fichier illisible. Le document n'est pas modifié.

**Enregistrement DOCX** (`atelier_fmt_ooxml::docx_image`, opcode 30 du moteur). Pour chaque image : partie `word/media/imageN.<ext>` (format d'origine, octets identiques), relation `rIdImgN` de type image, `w:drawing`/`wp:inline` dont `wp:extent` et `a:ext` sont la taille affichée en EMU (12 700 EMU par point), réduite à la zone de texte de la page. `read_pictures` relit une image d'un document importé (octets, format, taille EMU, paragraphe, position). L'export de la session coupe le texte en paragraphes (CR, LF, CRLF), A4 avec marges de 54 pt comme l'éditeur.

**ABI.** Opcodes Editor : 28 `InsertImage` (`u64 nowMs`, octets), 29 `ImageBitmap` (`u32 id`, réponse `ATIM` : id, largeur, hauteur, longueur, RGBA), 30 `ExportDocx` (réponse `ATDX` : nombre d'avis, nombre d'images, DOCX). Les opcodes 26 et 27 sont réservés à US-328. Codes d'échec : `EDITOR_IMAGE_UNSUPPORTED`, `_CORRUPT`, `_TOO_LARGE`, `_LIMIT`, `_UNKNOWN`, `EDITOR_EXPORT_LIMIT`, `EDITOR_EXPORT_FAILED`. L'état (`ATES`) expose `imageIds` côté JavaScript.

## Limites connues

- Le texte brut n'a pas de format d'image : un TXT enregistré contient U+FFFC à la place de chaque image (l'indicateur de modification suit ce contenu). L'export DOCX existe dans le moteur et sur `editor.exportDocx()` mais aucun bouton « Enregistrer en DOCX » n'est branché ; l'ouverture d'un DOCX reste un aperçu en lecture seule qui n'affiche pas les images.
- Copier un texte contenant une image colle un caractère privé hors de l'application ; coller ce texte dans le même document duplique l'image. Un caractère privé collé depuis une autre session peut désigner une autre image.
- L'orientation EXIF n'est lue que pour JPEG. Dans le DOCX, l'étendue est celle de l'image orientée et les octets d'origine (avec leur EXIF) sont conservés : un lecteur qui ignore l'EXIF montrera l'image non pivotée.
- Résolution des fichiers (ppp), profils de couleur, GIF animés (première image) et transparence de fond non pris en charge. Statistiques : une image compte comme un caractère.
- Déplacement et redimensionnement : US-271.
- Le sélecteur est celui du navigateur ; le dialogue natif viendra avec le pont de fichiers.
- Aucun essai avec lecteur d'écran, ni WebView2 réel : seuls Edge/Chromium headless et des événements synthétiques (collage, glisser-déposer) ont été utilisés.

## Preuves

- `cargo test -p atelier-text-model --test inline_image` (7), `-p atelier-text-layout --test inline_object` (6), `-p atelier-fmt-ooxml --test docx_images` (8), `-p atelier-core-wasm --test insert_image` (21 : insertion, annulation, remplacement de sélection, réduction, pages, EXIF 1-8, bitmap orienté, refus, bornes, export DOCX, clic).
- `node tests/image-insert.mjs` (lecture bornée, clés, protocole, collage, sélecteur, cache) et `node tests/run-image-insert-browser.mjs` (produit réel : ruban, sélecteur, collage, glisser-déposer, pixels du Canvas, EXIF, annuler/rétablir, nouveau document, refus WebP/TIFF/corrompu/trop gros, export DOCX).
- Régression corrigée au passage : `atelier-image` refusait les PNG écrits par Chromium (flux zlib terminé par un bloc vide) ; fixture `crates/image/tests/fixtures/png/browser-200x100-red.png`.
- Moteur WASM : 4 635 682 octets, plafond 4 708 206 (marge 72 524 octets).
