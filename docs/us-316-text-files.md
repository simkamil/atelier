<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-316 — Fichiers texte

Module `atelier_text_model::plain_io` (`crates/text-model/src/plain_io/`) : ouverture et enregistrement
de texte brut, sur des octets en mémoire. Partie bibliothèque seulement ; les dialogues et le
branchement dans l'éditeur relèvent de l'intégrateur. Aucune dépendance ajoutée, aucune API système.
Le chemin actuel de l'éditeur (`PlainTextDocument::from_utf8` / `export_utf8`) n'est pas modifié.

## API

```rust,ignore
use atelier_text_model::plain_io::*;

let opened = open_text(&bytes, &OpenOptions::default())?;        // ou OpenOptions { encoding: Some(..), .. }
if opened.suggest_manual_choice() { /* proposer MANUAL_ENCODINGS, puis rouvrir avec l'encodage choisi */ }
let plain = opened.to_plain_document(PlainTextLimits::default())?;   // PlainTextDocument
let rich  = opened.to_rich_document(1_000_000)?;                     // un paragraphe par ligne

let loss = formatting_loss(&rich);                                   // avant d'enregistrer un document riche
let saved = save_rich_document(&rich, &SaveOptions::as_opened(opened.format()))?;   // saved.bytes
```

- `detect(&[u8]) -> Detection { encoding, confidence }`, `open_text`, `OpenedText` (texte en `\n`, `encoding`,
  `bom`, `line_endings` avec les comptes LF/CRLF/CR et `style()`/`dominant()`, `invalid_sequences`,
  `control_chars`, `confidence`, `format()`, `round_trip_is_exact()`).
- `save_text(&str, &SaveOptions)`, `save_plain_document`, `save_rich_document` → `SavedText { bytes, encoding, bom, eol, replaced }`.
  `SaveOptions { encoding: SaveEncoding, line_ending: LineEndingChoice, unmappable: Unmappable, origin, max_bytes }` ;
  défaut : UTF-8 sans BOM, LF, erreur si un caractère n'est pas représentable.
- `SaveEncoding` : `AsOpened`, `Utf8`, `Utf8Bom`, `Utf16Le`, `Utf16Be` (avec BOM), `Windows1252` ; liste `SAVE_ENCODINGS`.
  `LineEndingChoice` : `Lf`, `CrLf`, `AsOpened`. `MANUAL_ENCODINGS` : les sept encodages d'`atelier-unicode` ; `encoding_label`.
- Limites vérifiées avant toute allocation : `OpenOptions::max_bytes` (octets lus) et `SaveOptions::max_bytes`
  (octets de texte), 16 Mio par défaut ; `to_rich_document(max_paragraphs)`. Le texte décodé peut atteindre 3 fois
  les octets lus (`€` en 1252 = 3 octets UTF-8). Jamais de panique sur des octets arbitraires (testé) ; coût linéaire.

## Détection et fiabilité

Ordre : BOM → motif UTF-16 sans BOM → `atelier_unicode::detect_encoding` (UTF-8 si valide, sinon Windows-1252).

| Cas | Encodage | Confiance |
|---|---|---|
| BOM UTF-8 / UTF-16 | celui du BOM | `Certain` |
| ≥ 2 unités UTF-16 dont ≥ 25 % ont l'octet de poids fort nul, et 16 fois moins de nuls de l'autre côté (64 Kio examinés) | UTF-16 LE ou BE | `Likely` |
| UTF-8 valide avec caractères non ASCII | UTF-8 | `Likely` |
| ASCII pur (ou vide) | UTF-8 | `Indifferent` (tous les encodages lisent la même chose) |
| autre | Windows-1252 | `Uncertain` |
| encodage imposé | celui de l'appelant | `Chosen` |

**Justesse mesurée** (`tests/plain_io_detection.rs`) : sur 60 textes français accentués (phrases, paires, triplets
avec LF et CRLF) × 7 variantes (UTF-8, UTF-8 BOM, Windows-1252, UTF-16 LE/BE avec et sans BOM) : **420/420**
décodés comme l'original. C'est un corpus favorable (texte latin, longueur de plusieurs mots), pas une garantie.
Cas structurellement indécidables ou manqués (testés) :
- un fichier Windows-1252 dont les octets forment par hasard de l'UTF-8 valide (`cafÃ©` écrit en 1252) est lu en UTF-8 ;
- UTF-16 sans BOM non latin (chinois...) : aucun octet nul, lu comme Windows-1252 ; un seul caractère UTF-16 sans BOM : trop peu d'indices ;
- un texte UTF-8 sans BOM commençant par U+FEFF est relu comme un BOM (octets identiques).

`suggest_manual_choice()` est vrai si la confiance est `Uncertain` ou si des séquences invalides ont été remplacées.

## Lignes et caractères de contrôle

Le texte ouvert a tous ses sauts (`\r\n`, `\r`, `\n`) remplacés par `\n` ; `\n\r` est un LF puis un CR. Un paragraphe par
ligne : `split('\n')`. Un fichier qui se termine par un saut donne une dernière ligne vide (`"a\n"` → `["a", ""]`), un fichier
sans saut final non (`"a"` → `["a"]`), ce qui permet l'aller-retour exact ; fichier vide → un paragraphe vide. U+2028, U+0085,
VT et FF ne sont pas des sauts. Tous les caractères de contrôle (NUL, FF, TAB, DEL...) sont conservés tels quels ; `control_chars`
compte les C0 hors TAB/LF/CR et DEL (un nombre élevé évoque un fichier binaire). À l'enregistrement, tout saut du texte
(y compris un `\r` isolé d'un `PlainTextDocument`) devient le saut choisi.

## Aller-retour

Ouvrir puis enregistrer « comme à l'ouverture » (`SaveOptions::as_opened(opened.format())`) redonne les octets d'origine
pour tout fichier valide dans son encodage à fins de ligne homogènes (testé : 3000 cas aléatoires à graine fixe, trois chemins
dont le document riche). Impossible, et signalé par `round_trip_is_exact() == false` :
- fins de ligne mélangées : tout devient le style dominant (égalité : CRLF, puis LF, puis CR) ;
- séquences invalides (`invalid_sequences > 0`) : remplacées par U+FFFD ; vers un encodage mono-octet, U+FFFD n'est pas
  représentable (erreur, ou `?` en mode `Replace`).
Un texte qui commence par U+FEFF enregistré en UTF-8 sans BOM se relit comme un fichier avec BOM. UTF-16 sans BOM détecté est
réenregistré sans BOM.

## Enregistrement et perte

Caractère non représentable en Windows-1252 (ou autre mono-octet) : `Unmappable::Error` → `TextIoError::Unrepresentable
{ encoding, characters, total }` listant jusqu'à 1000 caractères avec `line` et `column` (base 1, en caractères) ;
`Unmappable::Replace` → `?` et `SavedText::replaced`. Jamais de perte silencieuse.

### Document riche vers texte (`rich_to_text`, `formatting_loss`)

- Chaque paragraphe des corps de section est une ligne ; un saut dans un paragraphe en fait deux.
- Tableau (règle de Word, texte seul) : une ligne par rangée, cellules séparées par une tabulation ; une cellule de `n` colonnes est
  suivie de `n - 1` tabulations ; plusieurs paragraphes (ou un tableau imbriqué) dans une cellule sont joints par une espace.
- Champs : résultat affiché conservé. Liens : texte conservé. Listes : numéros et puces non exportés (générés, non stockés).
- Non exportés : objets ancrés, blocs opaques, signets, en-têtes et pieds.
- `formatting_loss` renvoie `FormattingLoss` (comptes : `character_runs`, `formatted_paragraphs`, `styled_paragraphs`, `tables`, `objects`,
  `fields`, `links`, `bookmarks`, `headers_footers` (non vides seulement), `list_items`, `section_breaks`) ; `categories()` donne les
  catégories non nulles ; `is_empty()` : rien à perdre, pas d'avertissement. Un document ouvert depuis un fichier texte a un rapport vide.

## Pour l'intégrateur

- Ouverture : `open_text` ; si `suggest_manual_choice()`, dialogue listant `MANUAL_ENCODINGS` (`encoding_label`) avec aperçu, puis
  réouverture avec `OpenOptions.encoding`. Afficher `invalid_sequences` et `control_chars` s'ils sont non nuls ; proposer de normaliser
  si `line_endings.style() == Mixed`.
- Enregistrement : dialogue encodage (`SAVE_ENCODINGS` + « comme à l'ouverture »), fins de ligne (LF, CRLF, comme à l'ouverture), et en cas
  d'erreur `Unrepresentable` un message avec la liste, proposant « remplacer par ? » (`Unmappable::Replace`).
- Document riche : appeler `formatting_loss` ; si `!is_empty()`, confirmer avant d'enregistrer.
- Clés de catalogue proposées (`web/i18n/fr-FR.txt` non modifié) : `text-encoding-title`, `text-encoding-uncertain`, `text-encoding-invalid`,
  `text-save-encoding`, `text-save-line-endings`, `text-save-unrepresentable`, `text-save-replace`, `text-loss-title` (« L'enregistrement en texte
  supprimera : »), et par catégorie `LossCategory::catalog_key()` : `text-loss-character-formatting`, `text-loss-paragraph-formatting`,
  `text-loss-styles`, `text-loss-tables`, `text-loss-objects`, `text-loss-fields`, `text-loss-links`, `text-loss-bookmarks`,
  `text-loss-headers-footers`, `text-loss-lists`, `text-loss-section-breaks`, avec le compte en paramètre.
- Le chemin actuel `core-wasm` (opcode de chargement via `PlainTextDocument::from_utf8`, texte UTF-8 uniquement) pourra appeler
  `open_text(..).to_plain_document(..)` puis `save_plain_document` ; non fait ici.
- Fuzz : cible possible `open_text(data, &OpenOptions{encoding: <octet de sélection>, ..})` puis `save_text` « comme à l'ouverture » (aucun résultat à vérifier).

## Vérification

`cargo test -p atelier-text-model --locked --offline` (tests `plain_io*.rs` : valeurs calculées à la main, propriété à graine fixe, mesure de
détection). Contrôle hors tests avec Python (`utf-8`, `utf-8-sig`, `utf-16-le/be`, `cp1252`) : octets écrits relus à l'identique, et fichiers
écrits par Python relus par le module ; Python n'est pas utilisé par les tests. Python rejette en `cp1252` strict les cinq octets non définis
(81, 8D, 8F, 90, 9D), comme `atelier-unicode`.

## Besoin dans `atelier-unicode`

`encode` pour les encodages mono-octet cherche chaque caractère par balayage de la table (jusqu'à 256 comparaisons) et ne signale que la première
erreur : une table inverse et une variante qui collecte toutes les erreurs éviteraient les contournements de ce module (table dérivée par `decode`).
