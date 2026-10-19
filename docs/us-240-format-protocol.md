# F0 — Contrat de mise en forme de l'éditeur riche (US-240, US-243, US-250)

Tranche **F0** : le contrat du protocole ATED v2 pour la mise en forme des caractères (US-240), des paragraphes (US-243) et des styles (US-250), les points d'extension qui permettent à deux agents de coder US-240 et US-243 **en parallèle** sans toucher les mêmes fichiers (puis US-250), l'`FormatState` **implémenté de bout en bout**, et deux sections de vérification : la simulation écrite des consommateurs et l'audit du rendu. Code de référence : `crates/core-wasm/src/rich_protocol/format.rs` (Rust), `web/rich-editor-format.mjs` (JS), vecteurs gelés `tests/rich-editor-format-vectors.txt`. Le protocole de base est dans [us-203-rich-editor-protocol.md](us-203-rich-editor-protocol.md) ; le moteur de styles est US-201 ([us-201-style-engine.md](us-201-style-engine.md)) ; `run_styles` est l'attribut `CharAttrs.char_style`.

## 1. Ce que F0 livre, ce qui reste aux stories

| Élément | État après F0 | Propriétaire ensuite |
|---|---|---|
| Codec Rust et JS de 8 commandes et 4 réponses, vecteurs gelés, refus stables | **fait** | personne (figé) |
| `FormatState` (moteur, client JS, test sur le vrai WASM) | **fait** | personne |
| `ListFonts` (polices dessinées, alias Word, polices du document, polices du thème) | **fait** | personne |
| `RichDocument::apply_format` (une étape d'historique, une révision) | **fait** (`crates/text-model/src/rich/edit/format.rs`, 6 tests) | personne |
| Crochets : `after_text_edit` + `Typing`, `cancel_preview`, `deny_formatting` | **posés** (no-op documentés) | US-240 / US-250 |
| `ApplyCharFormat` | **stub** `RICH_FORMAT_NOT_IMPLEMENTED` dans `rich_editor/format_char.rs` | **US-240** |
| `ApplyParaFormat`, `FormatMarks` | **stub** dans `rich_editor/format_para.rs` | **US-243** |
| `ListStyles`, `ApplyStyle`, `PreviewStyle` | **stub** dans `rich_editor/styles.rs` | **US-250** |

Fichiers que les agents suivants touchent, et **eux seuls** : US-240 : `rich_editor/format_char.rs` et `web/format-char.mjs` ; US-243 : `rich_editor/format_para.rs` et `web/format-para.mjs` ; US-250 : `rich_editor/styles.rs` et `web/styles-pane.mjs`. Chacun ajoute aussi son fichier de tests de famille (`format_char_tests.rs`, `format_para_tests.rs`, `styles_tests.rs`, `tests/rich-editor-format-char.mjs`, `-para.mjs`, `-styles.mjs`). Aucun ne modifie `rich_protocol.rs`, `rich_protocol/format.rs`, `rich-editor-protocol.mjs`, `rich-editor-client.mjs`, `rich-editor-format.mjs` : le contrat est figé par les vecteurs ; seul US-250 touche `rich_editor.rs` (aperçu, section 5.6). Les fichiers centraux de **rendu, de modèle et de format** que les tâches de l'audit (section 9) obligent malgré tout à toucher sont listés en section 10, avec l'ordre conseillé.

## 2. Principes

- **Une commande qui change le document répond un `State`** (0x8100) : pages changées, entrées d'atlas nouvelles, sélection, indicateurs. Même mécanique que `InsertText` : `cache.apply_edit` puis `finish`.
- **Une commande de lecture répond un message sans page ni atlas** (`FormatState` 0x8130, `StyleList` 0x8131, `FontList` 0x8132, `FormatMarks` 0x8133) : elle ne consomme pas l'atlas (`atlas_first` n'avance pas), ne change ni la révision ni la sélection, et sa réponse perdue **n'impose aucune resynchronisation**.
- **Une transaction = un Ctrl+Z.** Les commandes qui changent le document appellent `RichDocument::apply_format(ops, sélection, portée, joindre_précédent)` : toutes les opérations (`SetCharAttrs`, `SetParaProps`) sont une seule étape d'historique, la révision monte de 1, tout ou rien (une opération invalide restaure le document et l'historique), la redo est vidée, l'annulation restaure la sélection d'origine. Une commande qui ne change rien (déjà appliqué) ne crée ni révision ni étape et répond le même `State`.
- **Identifiant de commande : exactement une fois.** Les commandes qui changent le document (`ApplyCharFormat`, `ApplyParaFormat`, `ApplyStyle`) portent `command_id u32`, choisi par l'appelant (non nul, un par commande ; 0 = aucun). Le moteur retient l'identifiant de la **dernière commande qu'il a acceptée** (appliquée **ou** reconnue sans effet, attributs de saisie compris ; jamais une commande refusée ni non implémentée) et le publie dans `FormatState.last_command_id`. Une commande qui porte cet identifiant est refusée `RICH_DUPLICATE_COMMAND` (63) sans rien changer : cela protège d'un **rejeu de la même trame** (transport qui la livre deux fois, répétition à l'identique par un appelant qui garde l'identifiant). `createFormatClient` prend un identifiant neuf à chaque tentative : une nouvelle tentative de l'appelant n'est donc **pas** refusée comme doublon ; la sûreté vient de la **lecture de `last_command_id`** après une réponse perdue, et l'appelant ne retente que si `outcome` vaut `"not-applied"`. Après une réponse perdue, l'appelant lit `FormatState` : `last_command_id == id` veut dire appliquée, sinon non appliquée ; **la comparaison des révisions n'est pas fiable** (une frappe intercalée ou une commande qui ne pose que des attributs de saisie ne la respectent pas) et n'est pas utilisée. L'identifiant ne protège que contre la répétition de la dernière commande : une autre commande acceptée entre-temps l'efface, ce qui suffit parce que `createFormatClient` **sérialise** les commandes de mise en forme : une seule en vol par session, résolution comprise ; la suivante attend que la précédente soit résolue (sinon elle écraserait `last_command_id` avant la lecture et ferait conclure à tort « non appliquée »).
- **Garde de révision.** Les mêmes commandes portent aussi `base_revision u64` : la révision que l'appelant croit modifier. Une autre valeur est `RICH_STALE_REVISION` et rien ne change ; `0xFFFFFFFFFFFFFFFF` (`NO_REVISION_GUARD`) désactive le contrôle. Le client JS (`createFormatClient`) lit la révision **au tour de la commande** dans la file (une frappe mise en file avant elle est prise en compte), donc ne se heurte jamais à son propre garde.
- **Ordre des contrôles** : enveloppe, id de document, opcode, corps (champs dans l'ordre : `RICH_INVALID_BODY`, `RICH_INVALID_UTF8`, `RICH_PAGE_RANGE`, `RICH_TOO_MANY_PAGES`), puis le moteur : document actif (`RICH_UNKNOWN_DOCUMENT`) → lecture seule (`RICH_READ_ONLY`) → texte brut (`RICH_FORMAT_PLAIN_TEXT`) → **répétition** (`RICH_DUPLICATE_COMMAND`) → garde (`RICH_STALE_REVISION`) → style connu (`RICH_UNKNOWN_STYLE`) → effets (`RICH_LIMIT` si une limite du modèle est atteinte). Les handlers appellent `Session::begin_format(base_revision, command_id)?` en premier (il enchaîne ces contrôles dans cet ordre) et `Session::note_command(command_id)` quand la commande est acceptée.
- **Lecture seule.** DOCX à révisions ou commentaires : `ApplyCharFormat`, `ApplyParaFormat`, `ApplyStyle`, `PreviewStyle` répondent `RICH_READ_ONLY` ; `FormatState`, `ListStyles`, `ListFonts`, `FormatMarks` répondent normalement (`FormatState.read_only` vaut vrai pour griser le ruban).
- **Texte brut.** Un document ouvert avec le profil TXT ne porte aucune mise en forme (le moteur ignore les propriétés de paragraphe en profil TXT) : `FormatState.plain` vaut vrai et les commandes qui changent répondent `RICH_FORMAT_PLAIN_TEXT`.
- **Valeurs résolues.** `FormatState` donne les valeurs **effectives** (défauts du document, styles, mise en forme directe), comme Word les affiche. Un style absent du catalogue (un import peut en référencer un) retombe sur les valeurs directes au lieu de cacher le paragraphe.
- **Positions.** Octets plats comme US-203. `FormatState` rappelle la sélection qu'il décrit (`anchor`, `focus`).

## 3. Catalogue

Commandes (corps après l'en-tête de 20 octets de l'enveloppe) :

| Opcode | Nom | Réponse |
|---:|---|---|
| `0x0150` | FormatState | `0x8130` FormatState |
| `0x0151` | ApplyCharFormat | `0x8100` State |
| `0x0152` | ApplyParaFormat | `0x8100` State |
| `0x0153` | ListStyles | `0x8131` StyleList |
| `0x0154` | ApplyStyle | `0x8100` State |
| `0x0155` | PreviewStyle | `0x8100` State |
| `0x0156` | ListFonts | `0x8132` FontList |
| `0x0157` | FormatMarks | `0x8133` FormatMarks |

`0x0158` à `0x016F` et `0x8134` à `0x813F` sont **réservés** à la suite de la famille (US-241 dialogue Police, US-242 casse, US-247 reproduire la mise en forme, US-248 effacer la mise en forme) : ces stories ajoutent leurs opcodes dans ces plages et leurs attributs dans les plages réservées ci-dessous, sans toucher aux existants.

Erreurs nouvelles (plage 60 à 69, jamais renumérotées ; 64 à 69 libres) :

| Code | Nom | Quand |
|---:|---|---|
| 60 | `RICH_FORMAT_NOT_IMPLEMENTED` | la commande est définie mais son handler est un stub (jusqu'à US-240, US-243, US-250) |
| 61 | `RICH_UNKNOWN_STYLE` | `ApplyStyle` ou `PreviewStyle` nomme un style absent du catalogue ou d'un autre genre |
| 62 | `RICH_FORMAT_PLAIN_TEXT` | commande de mise en forme sur un document texte brut |
| 63 | `RICH_DUPLICATE_COMMAND` | `command_id` égal à celui de la dernière commande acceptée : répétition après réponse perdue |

Erreurs existantes utilisées : 4 `INVALID_BODY` (valeur hors borne, octet réservé non nul, énumération inconnue, doublon, valeur absente ou en trop), 10 `INVALID_UTF8`, 14 `PAGE_RANGE`, 15 `TOO_MANY_PAGES`, 16 `STALE_REVISION`, 6 `UNKNOWN_DOCUMENT`, 21 `READ_ONLY`, 19 `LIMIT`, 18 `INTERNAL`.

**Types communs.** `text16` : `len u16` (au plus 255) puis UTF-8, sans caractère de contrôle, non vide sauf mention. `rgb` : trois octets `r g b`. Entiers petit-boutiste ; `i32` en complément à deux. Octets réservés à 0. Les noms JS ci-dessous sont les seuls acceptés.

| Énumération | Numéros (ordre du fil) = noms JS |
|---|---|
| Attribut de caractère | `font` 1, `size` 2, `bold` 3, `italic` 4, `underline` 5, `strike` 6, `verticalAlign` 7, `color` 8, `highlight` 9 (10 à 63 réservés : US-241) |
| Attribut de paragraphe | `alignment` 1, `indentLeft` 2, `indentRight` 3, `firstLine` 4, `spaceBefore` 5, `spaceAfter` 6, `lineSpacing` 7, `shading` 8, `borders` 9 (10 à 63 réservés : US-244) |
| Opération | `set` 1, `clear` 2, `toggle` 3, `stepUp` 4, `stepDown` 5, `stepUpPoint` 6, `stepDownPoint` 7 |
| Soulignement | `none` 0, `single` 1, `double` 2, `thick` 3, `dotted` 4, `dashed` 5, `wave` 6 |
| Décalage | `baseline` 0, `superscript` 1, `subscript` 2 |
| Alignement | `left` 0, `center` 1, `right` 2, `justify` 3 |
| Règle d'interligne | `auto` 0 (240es de ligne), `exact` 1 (twips), `atLeast` 2 (twips) |
| Style de bordure | `single` 0, `double` 1, `dotted` 2, `dashed` 3 |
| Côtés de bordure | `top` 0, `left` 1, `bottom` 2, `right` 3, `between` 4, `bar` 5 |
| Portée | `wordOrTyping` 0, `selectionOnly` 1 |
| Filtre de styles | `all` 0, `paragraph` 1, `character` 2 |
| Genre de style | `paragraph` 0, `character` 1 |
| Marques | `paragraphEnd` 0, `space` 1, `tab` 2, `noBreakSpace` 3, `lineBreak` 4, `pageBreak` 5, `columnBreak` 6 |

Bornes : au plus 16 modifications par commande, sans attribut en double ; taille de 2 à 3276 demi-points (1 pt à 1638 pt) ; retraits de -31680 à 31680 twips, espacements de 0 à 31680 ; interligne `auto` de 15 à 2400, sinon de 1 à 31680 twips ; bordure : épaisseur de 2 à 96 (1/8 pt), écart de 0 à 31 pt ; liste de styles au plus 4096, liste de polices au plus 512, marques au plus 16384, pages de `FormatMarks` de 1 à 16.

## 4. Les modifications d'attributs (`ApplyCharFormat`, `ApplyParaFormat`)

Chaque modification est `attr u8`, `op u8`, puis **une valeur seulement si l'opération en porte une**. Une paire (attribut, opération) non listée, une valeur absente ou en trop est `RICH_INVALID_BODY` (au décodage, avant tout contrôle du moteur, et par l'encodeur).

### 4.1 Caractères

| Opération | Attributs permis | Valeur sur le fil | Valeur JS décodée |
|---|---|---|---|
| `set` | tous | font : `text16` non vide ; size : `u16` demi-points ; bold, italic, strike : `u8` 0 ou 1 ; underline, verticalAlign : `u8` numéro ; color, highlight : `rgb` | `"Calibri"` ; `24` (nombre) ; `true` ; `"double"` ; `{r:255,g:0,b:0}` |
| `clear` | tous | aucune | (pas de `value`) : l'attribut direct disparaît, la valeur revient du style |
| `toggle` | bold, italic, strike | aucune | |
| `toggle` | underline, verticalAlign | `u8` cible, jamais 0 | `"single"` ; `"superscript"` |
| `stepUp`, `stepDown`, `stepUpPoint`, `stepDownPoint` | size | aucune | |

**Cible** (`scope`). `wordOrTyping` (règle de Word) : sélection non vide, la sélection ; sinon, si le curseur est **sur un mot**, ce mot ; sinon les **attributs de saisie** (les attributs du prochain texte tapé, `Typing`). **Mot sous le curseur** (`format_state::word_under_caret`, partagé par `FormatState` et le handler) : le segment UAX #29 qui **commence à la position ou la contient** (`FlatIndex::word_at`), à condition qu'il contienne au moins une lettre ou un chiffre. Les blancs, la ponctuation, les objets et la marque de paragraphe ne sont **pas** des mots, que `word_at` renvoie pourtant (il rend aussi le blanc ou le `\n` sous la position) ; la fin d'un mot n'en fait pas partie. Exemples sur `foo, bar  baz¶qux` : position 0 (début) et 1 : « foo » ; 3 (après « foo », sur la virgule) : saisie ; 4 (entre la virgule et le blanc) : saisie ; 5 : « bar » ; 8 (fin de « bar ») et 9 (entre deux blancs) : saisie ; 13 (fin de paragraphe) : saisie ; 14 (début du paragraphe suivant) : « qux » ; 17 (fin du document) : saisie (vecteurs : test `the_word_under_the_caret_is_a_non_blank_segment_start_included_end_excluded`). `selectionOnly` : la sélection exactement ; vide, seulement les attributs de saisie. La validation de cette règle contre Word reste à US-308.

**`toggle` booléen** (règle de Word) : si **tous** les caractères visés ont la valeur résolue vraie, on retire ; sinon on met à vrai pour tous. « Retirer » écrit `Some(false)` seulement si le style résout à vrai, sinon l'attribut direct est supprimé (le document reste propre). `toggle` énuméré : vers la valeur donnée si au moins un caractère diffère, sinon retour à `none` ou `baseline` (même règle d'écriture explicite).

**Taille non définie.** `FormatState.chars.size` n'est `unset` que si le moteur ne résout aucune taille (cas non rencontré : la cascade en fournit toujours une) ; `stepUp` et `stepDown` partent alors de la taille par défaut du document (11 pt, 22 demi-points) pour chaque caractère concerné. **Police de thème.** `font.requested` est le **nom** de famille résolu (le lien vers la police de thème est résolu à l'import) ; `set font` écrit un nom, jamais un lien de thème ; les polices du thème sont proposées par `FontList.themeMajor` et `themeMinor`, comme noms.

**Pas de taille.** `stepUp` / `stepDown` (Ctrl+Maj+> et <) parcourent la liste de la boîte de taille de Word, en demi-points : 8, 9, 10, 10,5, 11, 12, 14, 16, 18, 20, 22, 24, 26, 28, 36, 48, 72 (`SIZE_STEPS` ; fonction `size_step` testée, `a_step_always_moves_or_hits_a_bound`) ; au-dessus de 72 pt, par 10 pt ; en dessous de 8 pt, par 1 pt ; bornés à 1 pt et 1638 pt. Une taille entre deux paliers va au palier suivant (précédent). Pour une sélection de tailles différentes, **chaque** caractère passe à son propre palier (Word). `stepUpPoint` / `stepDownPoint` : ± 1 pt (Ctrl+] et Ctrl+[).

**Attributs de saisie et annulation.** Le texte tapé ensuite prend les attributs de saisie dans la **même étape d'historique** que l'insertion : un Ctrl+Z retire le texte et sa mise en forme. Règle pour `after_text_edit` (US-240) : il appelle `apply_format(..., joindre_précédent)` **seulement si l'insertion a changé la révision** (`outcome.revision` différent de celui d'avant) ; sinon il n'applique rien (rien n'a été inséré) et les attributs restent en attente. L'`EditOutcome` rendu à la mise en page est **celui de l'insertion**, étendu pour couvrir le span de celui de `apply_format` (début minimal, fin maximale, paragraphes idem, `layout_scope` le plus large) : la mise en forme ne change pas les longueurs, donc la fusion est l'insertion elle-même ; la révision rendue est celle de l'insertion. Tout changement de sélection oublie les attributs de saisie (`Session::select` → `Typing::forget`).

### 4.2 Paragraphes

| Opération | Attributs permis | Valeur sur le fil | Valeur JS décodée |
|---|---|---|---|
| `set` | alignment | `u8` numéro | `"center"` |
| | indentLeft, indentRight, firstLine | `i32` twips (firstLine négatif = suspendu) | `720` |
| | spaceBefore, spaceAfter | `u32` twips | `240` |
| | lineSpacing | `rule u8`, `value u32` | `{rule:"auto", value:360}` |
| | shading | `rgb` | `{r:255,g:255,b:0}` |
| | borders | six côtés de 9 octets : `kind u8` (0 garder, 1 aucune, 2 trait), `style u8`, `eighths u16`, `space u8`, `color_mode u8` (0 automatique, 1 rgb), `rgb` ; les champs inutiles à 0 | `{top:{style:"single",eighths:4,space:1,color:null}, left:"keep", bottom:"none", right:"keep", between:"keep", bar:"keep"}` |
| `clear` | tous | aucune | |
| `toggle` | spaceBefore, spaceAfter | aucune | |
| `stepUp`, `stepDown` | indentLeft | aucune | |

**Cible** : **tous les paragraphes touchés** par la sélection ; un curseur seul touche le paragraphe qui le porte (pas de règle du mot). Une sélection qui se termine au tout début d'un paragraphe ne le touche pas ; une sélection qui commence à la fin d'un paragraphe (la marque est sélectionnée) le touche. Les paragraphes de cellules de tableau sont touchés ; les blocs opaques sont ignorés.

**`toggle` d'espacement** (« ajouter / supprimer l'espace avant ou après ») : si tous les paragraphes touchés ont une valeur résolue de 0, on met 240 twips (12 pt) ; sinon 0. **`stepUp` / `stepDown` du retrait gauche** : on passe au multiple suivant (précédent) de 720 twips (taquet par défaut de 1,27 cm ; le taquet par défaut du document dès que le modèle le porte), borné à 0 et 31680. **Bordures** : `keep` laisse le côté, `none` le supprime (aucune bordure explicite), un trait le définit ; « Aucune bordure » envoie six `none`.

**Bordures et trame : état du modèle.** `ParaProps` ne porte ni trame ni bordures (audit, section 9) : la commande est définie et décodée, le moteur US-243 doit d'abord ajouter les champs au modèle (tâche **US-243-M1**) ; `FormatState` répond `unset` pour ces deux attributs tant que le modèle ne les lit pas.

## 5. Commandes, octet par octet

Les corps ci-dessous suivent l'en-tête de 20 octets. Pour chaque commande : corps, réponse, effet sur la révision, annulation, atlas et pages, resynchronisation, lecture seule.

### 5.1 `FormatState` `0x0150`

Corps : vide. Réponse `FormatState` `0x8130` :

| Champ | Type | Sens |
|---|---|---|
| `revision` | `u64` | révision courante (celle du `State`) |
| `anchor`, `focus` | `u64`, `u64` | la sélection décrite : le client vérifie qu'elle est encore la courante |
| `last_command_id` | `u32` | `command_id` de la dernière commande de mise en forme acceptée (0 : aucune) |
| `flags` | `u8` | bit 0 `collapsed`, 1 `read_only`, 2 `partial`, 3 `typing`, 4 `plain` |
| réservé | `u8[3]` | 0 |
| 10 champs de caractères | tri-états | `font`, `size`, `bold`, `italic`, `underline`, `strike`, `verticalAlign`, `color`, `highlight`, `style` |
| 15 champs de paragraphe | tri-états | `alignment`, `indentLeft`, `indentRight`, `firstLine`, `spaceBefore`, `spaceAfter`, `lineSpacing`, `shading`, `borders` ×6 (`top` `left` `bottom` `right` `between` `bar`), `style` |

**Tri-état** : `state u8` (0 **mixte**, 1 **valeur**, 2 **non défini**) puis la valeur seulement si 1. Valeurs : font : `requested text16`, `used text16`, `substituted u8` ; size : `u16` demi-points ; bold, italic, strike : `u8` ; underline, verticalAlign, alignment : `u8` numéro ; color, highlight, shading : `rgb` ; style : `text16` ; indents : `i32` ; espacements : `u32` ; lineSpacing : `rule u8`, `value u32` ; chaque bordure : `style u8`, `eighths u16`, `space u8`, `color_mode u8`, `rgb`. « Non défini » : couleur automatique, aucun surlignage, aucun style de caractère, aucune bordure ou trame, aucune police.

JS décodé (exemple, curseur dans du texte Calibri 11 pt gras) :

```js
{ type: "formatState", revision: 7n, anchor: 3n, focus: 3n, lastCommandId: 41,
  collapsed: true, readOnly: false, partial: false, typing: false, plain: false,
  chars: {
    font: { state: "value", value: { requested: "Calibri", used: "Carlito", substituted: true } },
    size: { state: "value", value: 22 },            // demi-points : 11 pt
    bold: { state: "value", value: true }, italic: { state: "value", value: false },
    underline: { state: "value", value: "none" }, strike: { state: "value", value: false },
    verticalAlign: { state: "value", value: "baseline" },
    color: { state: "unset" }, highlight: { state: "unset" }, style: { state: "unset" } },
  paragraph: {
    alignment: { state: "value", value: "left" },
    indentLeft: { state: "value", value: 0 }, indentRight: { state: "value", value: 0 }, firstLine: { state: "value", value: 0 },
    spaceBefore: { state: "value", value: 0 }, spaceAfter: { state: "value", value: 160 },
    lineSpacing: { state: "value", value: { rule: "auto", value: 259 } },
    shading: { state: "unset" },
    borders: { top: { state: "unset" }, left: { state: "unset" }, bottom: { state: "unset" }, right: { state: "unset" }, between: { state: "unset" }, bar: { state: "unset" } },
    style: { state: "value", value: "atelier.normal" } } }
```

Règles. Curseur **sur un mot** (règle de 4.1) : l'état **du mot** (valeur uniforme ou « mixte » sur ses caractères), c'est-à-dire ce que `toggle` va modifier ; `collapsed` reste vrai. Curseur ailleurs : le caractère qui précède (celui qui suit en début de paragraphe), avec les attributs de saisie appliqués (`typing` vrai). Les blocs opaques (images en bloc, contenus inconnus) ne sont pas décrits ; les paragraphes de cellules de tableau le sont. Les noms de police et de style d'un document sont nettoyés avant d'être encodés : caractères de contrôle retirés, coupés à 255 octets sur une frontière de caractère UTF-8 (un nom vide après nettoyage est « non défini »). Sélection : la valeur uniforme de chaque caractère ou « mixte » ; un paragraphe vide compris dans la sélection montre sa marque. Au plus `MAX_STATE_RUNS` = 20 000 morceaux sont examinés : au-delà, `partial` vaut vrai (réponse sur les premiers). `font.used` est la famille que le moteur dessine réellement (substitut métrique ou repli générique) ; `substituted` vaut vrai si elle diffère de la demandée (info-bulle de US-240). Effets : aucune révision, aucune étape d'historique, aucun atlas ; autorisée en lecture seule et en texte brut. Répétée, elle donne la même réponse (testé).

### 5.2 `ApplyCharFormat` `0x0151`

Corps : `base_revision u64`, `command_id u32`, `scope u8`, `0 u8`, `count u16` (1 à 16), puis `count` × (`attr u8`, `op u8`, valeur éventuelle). Réponse : `State` ; pages changées et atlas comme `InsertText`.

```js
{ type: "applyCharFormat", baseRevision: 9n, commandId: 1, scope: "wordOrTyping",
  edits: [ { attr: "font", op: "set", value: "Calibri" }, { attr: "size", op: "set", value: 24 },
           { attr: "color", op: "set", value: { r: 255, g: 0, b: 0 } }, { attr: "bold", op: "toggle" },
           { attr: "underline", op: "toggle", value: "single" }, { attr: "highlight", op: "clear" } ] }
```

Erreurs : celles de la section 3 (`RICH_DUPLICATE_COMMAND` si `command_id` répète la dernière commande acceptée). Révision : +1 si au moins un caractère change, sinon aucune (réponse `State` identique). Annulation : une étape, qui restaure la sélection d'origine ; avec des attributs de saisie, jointe à l'insertion. Pages et atlas : `invalidated` et `pages` couvrent les paragraphes touchés ; de nouvelles tailles ou familles créent des entrées d'atlas, envoyées dans la même réponse. Resynchronisation : réponse perdue = `needsResync` du client (`Resync` 0x0118 avant la commande suivante), comme toute commande qui renvoie un `State`. Lecture seule : `RICH_READ_ONLY`. Handler : **stub US-240** (`apply_char_format`).

### 5.3 `ApplyParaFormat` `0x0152`

Corps : `base_revision u64`, `command_id u32`, `count u16` (1 à 16), `0 u16`, puis les modifications. Réponse : `State`.

```js
{ type: "applyParaFormat", baseRevision: 9n, commandId: 2,
  edits: [ { attr: "alignment", op: "set", value: "center" }, { attr: "indentLeft", op: "stepUp" },
           { attr: "spaceBefore", op: "toggle" }, { attr: "firstLine", op: "set", value: -360 },
           { attr: "lineSpacing", op: "set", value: { rule: "auto", value: 360 } } ] }
```

Révision, annulation, atlas, resynchronisation, lecture seule : comme 5.2 ; la mise en forme de paragraphe change les hauteurs, donc `invalidated` peut couvrir les pages suivantes (le gestionnaire de pages le calcule). Handler : **stub US-243**.

### 5.4 `ListStyles` `0x0153`

Corps : `filter u8`, `used_only u8` (0 ou 1), `0 u16`. Réponse `StyleList` `0x8131` : `revision u64`, `count u32` (au plus 4096), puis par style : `id text16`, `name text16` (vide : l'identifiant), `based_on text16` (vide : aucun), `target u8`, `flags u8` (bit 0 prédéfini, 1 utilisé dans le document, 2 courant pour la sélection, 3 par défaut de son genre, 4 dans la galerie rapide), `heading_level u8` (1 à 9, sinon 0), `0 u8`, puis l'échantillon de la tuile : `font text16` (vide : défaut), `half_points u16` (0 : défaut), `char_flags u8` (bit 0 gras, 1 italique, 2 barré), `underline u8`, `color_mode u8` + `rgb`, `alignment u8`.

```js
{ type: "styleList", revision: 5n, styles: [
  { id: "Heading1", name: "Titre 1", basedOn: "Normal", target: "paragraph", builtIn: true, used: true, current: false, isDefault: false, quick: true, headingLevel: 1,
    sample: { font: "Calibri Light", halfPoints: 32, bold: false, italic: false, strike: false, underline: "none", color: { r: 47, g: 84, b: 150 }, alignment: "left" } } ] }
```

Effets : lecture seule, aucune révision. Autorisée partout. Handler : **stub US-250**. Le catalogue du modèle ne porte pas encore `name`, `quick` ni `default` (tâche **US-250-M1**) : en attendant, `name` est vide, `quick` vaut vrai pour Normal et les titres, `isDefault` pour le style des paragraphes sans style.

### 5.5 `ApplyStyle` `0x0154`

Corps : `base_revision u64`, `command_id u32`, `target u8`, `0 u8[3]`, `id text16` (obligatoire pour un style de paragraphe ; **vide pour un style de caractère = retirer le style de caractère**). Réponse : `State`.

```js
{ type: "applyStyle", baseRevision: 12n, commandId: 3, target: "paragraph", id: "Heading1" }
{ type: "applyStyle", baseRevision: 12n, commandId: 4, target: "character", id: "" }
```

Style inconnu ou d'un autre genre : `RICH_UNKNOWN_STYLE`. **Style de paragraphe** : appliqué à tous les paragraphes touchés ; **conserve la mise en forme directe de caractère** (critère de US-250) **et de paragraphe** (comme Word) ; la règle des 50 % de Word n'est pas reprise (écart à valider par US-308) ; portée `LayoutScope::All` (numérotation par style). **Style de caractère** : même cible que `ApplyCharFormat` avec `wordOrTyping`. Révision, annulation, atlas : comme 5.2. Un style déjà appliqué à tout ne change rien. Handler : **stub US-250**.

### 5.6 `PreviewStyle` `0x0155` (aperçu au survol)

Corps : `target u8`, `action u8` (0 montrer, 1 annuler), `0 u16`, `id text16` (vide **si et seulement si** annuler). Réponse : `State`.

```js
{ type: "previewStyle", target: "paragraph", show: true,  id: "Heading1" }
{ type: "previewStyle", target: "paragraph", show: false, id: "" }
```

**Décision** : un aperçu est une **commande à état dans la session**, qui ne touche ni le modèle, ni l'historique, ni la révision, ni `modified` ; le moteur remet en page les paragraphes touchés au travers d'un `LayoutContext` qui les remplace, et `cancel_preview` les remet en page depuis le vrai document. **Rejeté** : appliquer le style puis annuler avec Undo, qui viderait la pile Redo, ferait monter la révision, salirait `modified` et laisserait une étape fantôme en cas de réponse perdue. Règles : (1) `show` remplace l'aperçu en cours ; (2) `cancel` sans aperçu en cours ne fait rien et répond le `State` courant (**idempotente**) : le panneau l'envoie à chaque fin de survol et après toute erreur ; (3) toute commande qui change le document ou la sélection **annule d'abord** l'aperçu (`styles::cancels_preview`) : les pages restaurées sont reportées par `Session::pending` dans le `State` de cette commande ; les lectures et la pompe de mise en page (`Pages`, `Layout`, `TextWindow`, `Resync`, `Save`...) le gardent, leurs pages montrent l'aperçu ; (4) l'enregistrement ne voit jamais l'aperçu ; (5) `ApplyStyle` du même style annule l'aperçu puis applique ; (6) `Open` et `Close` le détruisent avec la session ; (7) `RICH_READ_ONLY` en lecture seule, `RICH_FORMAT_PLAIN_TEXT` en texte brut, `RICH_UNKNOWN_STYLE` pour un id inconnu. La révision du `State` est inchangée ; `can_undo` n'est pas modifié. Handler : **stub US-250** (`Preview`, `cancel_preview`, `cancels_preview` posés). **US-250 touchera `rich_editor.rs`** (et `rich_editor/sel.rs`, `find.rs`, `io.rs`) : tous les appels de mise en page passent `&self.doc` comme `LayoutContext` (`Session::finish` ligne 813, `reach` 375, `rebuild_cache` 338, `state`, `hit_test`, `move_selection`, les appels `cache.*` de `sel.rs` et `find.rs`, environ vingt-et-un sites) ; un aperçu doit leur donner le contexte substitué. Méthode : introduire dans `Session` un champ `overlay: Option<PreviewDocument>` (un `LayoutContext` qui délègue au document et remplace les paragraphes touchés, défini dans `styles.rs`) et une macro locale `layout_doc!(self)` qui rend `&self.doc` ou le recouvrement, en emprunt de champ (pas de méthode `&self`, qui entrerait en conflit avec `&mut self.cache`) ; remplacer chaque `&self.doc` passé à `self.cache.*` par `layout_doc!(self)`. Ce remplacement est mécanique mais touche des fichiers centraux : il se fait dans la branche de US-250 seule, après la fusion de US-240 et de US-243 (qui ne touchent pas `rich_editor.rs`).

### 5.7 `ListFonts` `0x0156`

Corps : vide. Réponse `FontList` `0x8132` : `revision u64`, `theme_major text16`, `theme_minor text16` (vides sans thème), `count u32` (au plus 512), puis par famille : `name text16`, `flags u8` (bit 0 face grasse, 1 face italique, 2 utilisée par le document, 3 dessinée avec une autre famille), `substitute text16` (non vide **si et seulement si** le bit 3 est levé).

```js
{ type: "fontList", revision: 3n, themeMajor: "Calibri Light", themeMinor: "Calibri", families: [
  { name: "Calibri", bold: true, italic: true, used: true, substituted: true, substitute: "Carlito" },
  { name: "Carlito", bold: true, italic: true, used: false, substituted: false, substitute: "" } ] }
```

Contenu (calculé en interrogeant le gestionnaire de polices de la mise en page, donc fidèle à ce que le moteur dessine) : les familles que l'éditeur enregistre (aujourd'hui Carlito seulement, voir l'audit), les alias Word (Calibri, Cambria, Arial, Times New Roman, Courier New : dessinés avec leur famille métrique ou, à défaut, le repli générique), et les polices que le document nomme. Les polices récentes sont une préférence du client (stockage local), pas du moteur. Lecture seule, aucune révision. La réponse ne dépend que du document : elle est **mémorisée par révision** (`Session.fonts_memo`, testé : `the_font_list_is_memoised_by_revision`), de sorte qu'une boîte de police qui se rouvre ne relit pas le document ; le premier appel lit au plus 200 000 paragraphes (`FONT_SCAN_PARAGRAPHS`, borne) pour trouver les polices nommées. Les noms sont nettoyés comme ceux de `FormatState`. **Implémenté par F0** (`format_state.rs`), parce que US-240 (liste de polices) et US-241 (dialogue Police) en dépendent.

### 5.8 `FormatMarks` `0x0157`

Corps : `first u32`, `count u32` (1 à 16 pages ; `RICH_TOO_MANY_PAGES` au-delà, `RICH_PAGE_RANGE` si `first` atteint `MAX_PAGES`). Réponse `FormatMarks` `0x8133` : `revision u64`, `count u32` (au plus 16384), puis par marque : `page u32`, `kind u8`, `0 u8[3]`, `x f64`, `y f64`, `width f64` (origine du caractère marqué sur la ligne de base et chasse, en points de page ; `width` ≥ 0).

```js
{ type: "formatMarks", revision: 4n, marks: [ { page: 0, kind: "paragraphEnd", x: 120.5, y: 96.25, width: 4.5 }, { page: 1, kind: "tab", x: 72, y: 110, width: 36 } ] }
```

**Page non mise en page** : comme `Pages`, la commande répond `RICH_NOT_READY` si une page demandée n'est pas encore calculée (le client attend un `Layout`, comme `ensurePages`, borné), `RICH_PAGE_RANGE` si `first` atteint `page_count`, et borne `count` à `page_count` ; la réponse ne contient donc jamais une liste partielle. **Cache du client** : les marques sont mises en cache par page, avec la révision ; une page dont l'index figure dans `State.invalidated` (ou remplacée dans `State.pages`) perd ses marques ; un changement de révision sans page touchée (attributs de saisie) les garde ; `Resync` et `Open` les vident toutes.

**Décision** : les marques de mise en forme (¶, espaces, tabulations, espaces insécables, sauts) sont une **surcouche dessinée par le client** à partir de cette liste, et non des commandes ajoutées aux listes d'affichage : activer ou désactiver l'option n'invalide aucune page, ne touche pas à l'atlas, et l'option (« Avancé : marques affichées ») reste une préférence du client. Le client redemande les marques des pages visibles quand `State.pages` ou `invalidated` les touchent, et les met en cache par page et révision. Lecture seule, aucune révision. Handler : **stub US-243** (`format_marks`), autorisé en lecture seule et en texte brut.

## 6. Annulation, révision, atlas, resynchronisation, réouverture

| Situation | Comportement du contrat |
|---|---|
| Ctrl+Z après une commande de mise en forme | une étape annulée (sélection d'origine restaurée) ; `LayoutScope::All` conservé pour les styles (testé : `a_whole_document_scope_survives_undo_and_redo`) |
| Réponse **perdue** d'une commande qui change | `guarded()` du client : `needsResync` et `dropPages` ; `createFormatClient` lit alors `FormatState` (le client envoie `Resync` d'abord) et compare `last_command_id` à l'identifiant de la commande : l'erreur porte `outcome = "applied"` ou `"not-applied"` (et `commandId`) ; **aucune répétition automatique**, l'appelant décide ; un rejeu de la même trame est refusé `RICH_DUPLICATE_COMMAND` quel que soit son garde (testé sur moteur simulé avec frappe intercalée et attributs de saisie seuls ; le test du vrai WASM vérifie qu'une commande non acceptée n'est pas enregistrée, et le test Rust `the_command_identifier_is_recorded_on_acceptance_and_a_repeat_is_refused` le refus d'une répétition : le vrai WASM ne peut pas accepter de commande tant que les handlers sont des stubs, US-240 ajoute ce cas à `tests/rich-editor-format-char.mjs`) |
| Réponse perdue d'une commande de lecture | aucun effet sur l'atlas ni sur le cache : pas de `Resync` |
| **Annulation** (`AbortError`) | même résolution du résultat que la réponse perdue |
| **Resync** (0x0118) | les attributs de saisie et l'aperçu sont de l'état de session : ils survivent ; le client redemande `FormatState` |
| **Réouverture** (`Open`) | nouvelle session : attributs de saisie et aperçu détruits ; la révision repart de 0 : le panneau compte une **génération** incrémentée à chaque `open` et ignore une réponse (`FormatState`, `StyleList`) demandée à une génération précédente, car une révision seule ne distingue pas deux documents |
| Commande sans changement de révision (attributs de saisie, rien à modifier) | le panneau **redemande `FormatState` après toute réponse d'application réussie**, révision changée ou non (`typing` ou l'état du mot ont pu changer) |
| Sélection changée entre la demande et la réponse | `FormatState.anchor`/`focus` sont comparés à `cache.selection` ; sinon la réponse est jetée et redemandée |
| Lecture seule, texte brut | refus 21 et 62 ; boutons grisés d'après `FormatState.read_only` et `plain` |

## 7. Points d'extension (qui touche quoi)

| Fichier | Contenu posé par F0 | Écrit par |
|---|---|---|
| `crates/core-wasm/src/rich_protocol/format.rs` | codec Rust de toute la famille, tables, bornes, `size_step` | F0, figé |
| `crates/core-wasm/src/rich_protocol.rs` | opcodes, erreurs 60 à 63, variantes `Command` et `Response`, aiguillage | F0, figé |
| `crates/core-wasm/src/rich_editor.rs` | arms de `run()`, champs `typing`, `preview`, `last_command_id` et `fonts_memo` de `Session`, `Typing::forget` dans `select`, `after_text_edit` dans `edit`, `cancel_preview` au début de `run` | F0 ; **US-250 seule le modifiera ensuite** (contexte de mise en page de l'aperçu, voir 5.6) |
| `rich_editor/format_state.rs` | `FormatState`, `ListFonts`, `word_under_caret`, `deny_formatting`, `begin_format`, `note_command` | F0 ; US-243-M1 y ajoute trame et bordures (section 10) |
| `rich_editor/format_char.rs` | `Typing`, `after_text_edit`, stub `apply_char_format` | **US-240** |
| `rich_editor/format_para.rs` | stubs `apply_para_format`, `format_marks` | **US-243** |
| `rich_editor/styles.rs` | `Preview`, `cancels_preview`, `cancel_preview`, stubs `list_styles`, `apply_style`, `preview_style` | **US-250** |
| `crates/text-model/src/rich/edit/format.rs` | `RichDocument::apply_format` ; `edit.rs` : champ `all` des étapes, `mod format` | F0, figé |
| `rich_layout/cache.rs` | `PageCache::used_face` (police réellement utilisée) | F0, figé |
| `web/rich-editor-format.mjs` | codec JS, noms, `createFormatClient` (garde au tour, résultat inconnu) | F0, figé |
| `web/rich-editor-protocol.mjs`, `web/rich-editor-client.mjs` | tables d'opcodes, aiguillage ; `command()` accepte une fonction construite au tour de la commande | F0, figés |
| `web/format-char.mjs`, `web/format-para.mjs`, `web/styles-pane.mjs` | à créer | US-240, US-243, US-250 |
| `rich_editor/format_char_tests.rs`, `format_para_tests.rs`, `styles_tests.rs` ; `tests/rich-editor-format-char.mjs`, `-para.mjs`, `-styles.mjs` | un test de stub par famille | la story de la famille, seule |
| `rich_editor/format_tests.rs` (`Harness` partagé), `tests/rich-editor-format.mjs`, `tests/rich-editor-format-support.mjs` | tests de F0 | F0, figés |

Recette d'un handler (US-240, US-243, US-250), à suivre telle quelle : `self.begin_format(format.base_revision, format.command_id)?` (lecture seule, texte brut, répétition, garde) ; construire les `RichOp` par paragraphe ; `self.doc.apply_format(ops, (anchor, focus), portée, joindre)?` ; `let delta = self.cache.apply_edit(&self.doc, &outcome)` si la révision a changé ; `self.note_command(format.command_id)` ; `self.finish(delta, DEFAULT_LAYOUT_BUDGET, true)`. Une commande qui ne change rien est tout de même acceptée (`note_command`, réponse `State` identique). Le crochet `after_text_edit` ne peut pas échouer (l'insertion est faite).

## 8. Simulation écrite des consommateurs

Méthode : pour chaque consommateur, l'implémentation simulée méthode par méthode, avec la donnée du contrat utilisée et les chemins d'erreur. **Les manques trouvés ont été corrigés dans le contrat** ; la liste en fin de section dit lesquels.

### 8.1 US-240 : `web/format-char.mjs` + `rich_editor/format_char.rs`

| Méthode simulée | Donnée du contrat utilisée | Chemins d'erreur et cas limites |
|---|---|---|
| `refresh()` (après chaque `State` qui change révision ou sélection, regroupé par image) | `formatState()` : `revision`, `anchor`, `focus` comparés à `client.cache` ; `chars.*` pour les boutons et les boîtes | **jeter** la réponse si la sélection a changé ou si la génération d'ouverture n'est plus la courante ; `RICH_UNKNOWN_DOCUMENT` (document fermé) : ruban grisé ; réponse perdue : rien à resynchroniser, le prochain `State` relance ; réouverture : génération |
| `boutonGras/Italique/Barré` (Ctrl+B, I) | `applyCharFormat` `toggle` sans valeur ; état du bouton : `chars.bold.state` (`mixed` : `aria-pressed="mixed"`) | `READ_ONLY`/`PLAIN_TEXT` : boutons déjà grisés par `readOnly`/`plain` ; résultat inconnu (`error.outcome`, établi par `last_command_id`) : le panneau relit `refresh()` et n'affiche rien de plus ; annulation (`AbortError`) : idem |
| `souligné(style)` (Ctrl+U, menu) | Ctrl+U : `toggle underline "single"` ; menu : `set underline <un des six>` ; « Aucun » : `set none` | Les six styles existent sur le fil ; leur dessin manque (audit, T240-A) |
| `exposant/indice` (Ctrl+=, Ctrl+Maj+=) | `toggle verticalAlign "superscript" | "subscript"` | dessin manquant (T240-C) |
| `police(nom)` | `set font` ; affichage : `chars.font.value.requested` dans la boîte, `used` et `substituted` dans l'**info-bulle** « police de remplacement » (critère 4) | boîte vide si `mixed` ; nom saisi inconnu accepté (le moteur le remplace) ; plus de 255 octets : refusé côté client |
| `listeDesPolices()` | `listFonts()` : `families` (aperçu, indicateur « remplacée »), `themeMajor`/`themeMinor` (section « polices du thème »), polices récentes en stockage local | pas de liste de polices dans la v1 du contrat : **manque trouvé** (corrigé : `ListFonts`) ; l'aperçu de chaque entrée utilise la police du navigateur du même nom (le moteur ne dessine qu'avec ses faces) |
| `taille(valeur)` (boîte, saisie libre) | `set size` en demi-points : « 10,5 » → 21 ; vide si `size.state == "mixed"` | hors 1 à 1638 pt : refusé côté client avant l'envoi |
| `agrandir/réduire` (Ctrl+Maj+> et <) | `stepUp` / `stepDown` ; Ctrl+] et Ctrl+[ : `stepUpPoint` / `stepDownPoint` | le moteur calcule le palier de chaque caractère ; taille déjà au bord : rien ne change, `State` identique |
| `couleurPolice(rgb|null)`, `surlignage(rgb|null)` | `set color` / `set highlight` ; « Automatique » et « Aucun » : `clear` | `color.state == "unset"` : pastille « automatique » ; `mixed` : pas de pastille |
| Curseur sans sélection | `scope: "wordOrTyping"` ; `chars.typing` indique des attributs de saisie en attente | règle du mot : début inclus, fin exclue ; hors mot : attributs de saisie, **sans révision** (aucun texte ne change) : la commande répond le `State` courant et `FormatState` montre `typing` vrai |
| Frappe avec attributs de saisie | crochet `after_text_edit` + `apply_format(joindre_précédent)` | une seule étape d'historique : Ctrl+Z retire texte et mise en forme ; tout `select` oublie les attributs (`Typing::forget`) |
| `annuler` (Ctrl+Z) | `undo` puis `refresh()` | la sélection d'origine est restaurée par l'étape |
| Réponse perdue d'une application | `createFormatClient.applyCharFormat` : `error.outcome` | `applied` : ne pas répéter ; `not-applied` : répéter avec le garde frais ; `null` : informer l'utilisateur |

### 8.2 US-243 : `web/format-para.mjs` + `rich_editor/format_para.rs`

| Méthode simulée | Donnée du contrat utilisée | Chemins d'erreur et cas limites |
|---|---|---|
| `alignement(nom)` (Ctrl+L, E, R, J) | `set alignment` ; le bouton actif est `paragraph.alignment` ; un second clic sur l'alignement actif renvoie à `left` (Word) : décision prise **à partir d'un `FormatState` dont `anchor`/`focus` sont ceux de la sélection courante**, sinon redemander avant d'envoyer | `mixed` : aucun bouton enfoncé ; lecture seule/texte brut : grisés |
| `retrait(+/-)` | `stepUp` / `stepDown` sur `indentLeft` ; valeur affichée : `indentLeft` | le moteur arrondit à 720 twips, borne à 0 ; rien ne change à 0 : `State` identique |
| `interligne(valeur)` | `set lineSpacing` : 1,0 = `auto` 240, 1,15 = 276, 1,5 = 360, 2,0 = 480 ; la coche du menu : `paragraph.lineSpacing` (règle et valeur) | `exact` / `atLeast` pour les « Options » (US-244) ; valeur hors borne : refusée côté client |
| `espaceAvant/Après (ajouter/supprimer)` | `toggle spaceBefore/spaceAfter` ; libellé du menu d'après `paragraph.spaceBefore` (0 : « Ajouter ») | `mixed` : « Ajouter » (Word) |
| `trame(rgb|null)` | `set shading` / `clear` | **manque trouvé** : le modèle n'a pas ce champ (US-243-M1) ; `FormatState.shading` reste `unset` ; avant US-243-M1 la commande renvoie `RICH_FORMAT_NOT_IMPLEMENTED` |
| `bordures(préréglage)` | `set borders` avec `keep`/`none`/trait par côté ; état par côté dans `paragraph.borders.*` | idem ; « Aucune bordure » : six `none` |
| `marquesDeMiseEnForme(on)` | option du client ; `formatMarks({first,count})` pour les pages visibles, relancé sur `State.pages`/`invalidated` | **manque trouvé** : aucune commande pour les marques (corrigé : `FormatMarks`, surcouche client) ; réponse perdue : sans conséquence (lecture) ; réouverture : cache des marques vidé par génération |
| Sélection multi-paragraphes | tous les paragraphes touchés, y compris cellules de tableau ; blocs opaques ignorés | sélection finissant au début d'un paragraphe : celui-ci n'est pas touché (même règle que `FormatState`) |
| Annuler / réponse perdue / annulation / resync / réouverture | comme 8.1 (même `createFormatClient`) | |

### 8.3 US-250 : `web/styles-pane.mjs` + `rich_editor/styles.rs`

| Méthode simulée | Donnée du contrat utilisée | Chemins d'erreur et cas limites |
|---|---|---|
| `chargerLesStyles(filtre, utilisésSeulement)` | `listStyles` : `styles[]` (tuile dessinée à partir de `sample`), `name`, `quick` (galerie rapide), `used` (« utilisés / tous »), `current` (indication du style courant) | **manque trouvé** : le modèle ne porte ni nom d'affichage, ni drapeau rapide, ni style par défaut (US-250-M1) ; réponse perdue : sans conséquence ; réouverture : liste rechargée (génération) |
| `raccourciTitre(n)` (Titre 1 à 3) | style de genre `paragraph` avec `headingLevel === n` | aucun trouvé : raccourci sans effet |
| `styleParDéfaut()` | entrée avec `isDefault` | |
| `appliquer(id, genre)` | `applyStyle` ; garde automatique | `RICH_UNKNOWN_STYLE` (liste périmée) : recharger la liste ; résultat inconnu : `outcome` ; le paragraphe garde sa mise en forme directe de caractère |
| `survol(id)` (temporisé, 150 ms) | `previewStyle` `show` | l'aperçu remplace le précédent ; lecture seule/texte brut : galerie grisée |
| `finDeSurvol()` | `previewStyle` `cancel` (idempotente) | **à envoyer aussi après toute erreur de transport** : une réponse d'aperçu perdue laisse l'aperçu actif dans le moteur alors que la souris est partie |
| `clic` pendant l'aperçu | `applyStyle` : le moteur annule d'abord l'aperçu (`cancels_preview`) puis applique ; les pages restaurées arrivent dans le même `State` | |
| Frappe, Undo, déplacement pendant l'aperçu | le moteur annule l'aperçu avant la commande | le panneau n'a rien à faire |
| Réouverture, resync | l'aperçu est détruit avec la session ; `Resync` le garde (état de session) | un `cancel` après réouverture est sans effet |

### 8.4 Manques que la simulation a trouvés et que le contrat corrige

1. **Polices disponibles** : aucune commande n'existait pour la liste des polices avec aperçu, polices du thème, polices remplacées → `ListFonts` / `FontList`.
2. **Marques de mise en forme** : aucune donnée pour les dessiner → `FormatMarks`, surcouche client.
3. **Sélection décrite par `FormatState`** : une réponse de même révision mais d'une autre sélection n'était pas détectable → `anchor` / `focus` dans la réponse.
4. **Nom d'affichage et galerie rapide** des styles → `name` et le drapeau `quick` de `StyleEntry`.
5. **Texte brut** : le ruban ne pouvait pas savoir que le profil TXT ne porte pas de mise en forme → `FormatState.plain` et l'erreur 62.
6. **Répétition après réponse perdue** d'un `toggle` : sans identifiant, une répétition aurait annulé l'effet ; la comparaison des révisions pour savoir si la commande était passée était fausse (frappe intercalée, attributs de saisie sans révision) → `command_id`, `last_command_id`, `RICH_DUPLICATE_COMMAND`.
7. **Garde construit trop tôt** : une frappe mise en file avant la commande rendait le garde périmé → `client.command()` accepte une fonction évaluée au tour de la commande.
8. **Attributs de saisie** : aucun point d'entrée dans l'insertion, aucune étape jointe → `after_text_edit`, `Typing`, `apply_format(joindre_précédent)`.
9. **Aperçu et historique** : l'aperçu par application + Undo détruit la pile Redo → aperçu de session, hors modèle.
10. **Réouverture** : une révision identique (0) dans deux documents → génération côté panneau.

## 9. Audit du rendu

Question : le rendu actuel (mise en page `text-layout`, peinture `rich_paint.rs`, `rich_layout/cache.rs`) dessine-t-il réellement chaque attribut ? Chemin de production : `PageCache::paint_page` → `paint_line_fast` (`rich_paint.rs`) ; chemin de référence testé : `paint_line`. Référence des mesures : sources lues le 2026-10-19.

| Attribut | Dessiné ? | Preuve | Tâche |
|---|---|---|---|
| Police (famille) | **partiellement** | la famille demandée part dans `text-layout/src/lib.rs::request_for_attrs` (repli `SansSerif`), mais l'éditeur n'enregistre que les 4 faces de Carlito (`rich_preview.rs::build_fonts`, appelée avec `&[]` par `rich_layout/cache.rs:943`) ; Caladea et les trois Liberation sont dans `atelier_font::embedded` mais **non enregistrées** : Arial, Times New Roman, Courier New, Cambria sont dessinées en Carlito | **T240-F** : enregistrer les faces embarquées dans l'éditeur (`build_fonts` ; coût de taille WASM à mesurer contre la limite) |
| Taille | oui | `text-layout/src/lib.rs` : `size_half_points × 32` (lignes 762 et 821) ; hauteur de ligne dérivée de la police | — |
| Gras, italique | oui | `rich_paint.rs::paint_line` : poids 700 et italique demandés ; 4 faces de Carlito | — |
| Soulignés (6) | **un seul** | `rich_paint.rs:228` (`paint_line`) et `PageWriter::underline` (`paint_line_fast`, ligne 1035) : un trait simple pour tout `!= None` ; double, épais, pointillé, tirets, ondulé sont dessinés comme simple | **T240-A** : un dessin par style (`rich_paint.rs`) |
| Barré | **non** | aucune occurrence de `strike` dans `rich_paint.rs` ni `prepare.rs` | **T240-B** |
| Exposant, indice | **non** | `text-layout/src/lib.rs:692` les déclare « unsupported » : ni décalage de ligne de base ni réduction de taille | **T240-C** (`text-layout` et `rich_paint.rs`) |
| Couleur | oui | `rich_paint.rs::paint_line` : couleur de la plage (`StyleTable`) | — |
| Surlignage | **non** | `highlight` n'est lu ni par `rich_paint.rs` ni par `cache.rs` ; aucun rectangle sous le texte | **T240-D** (rectangle avant les glyphes) |
| Alignements | oui | `text-layout/src/lib.rs:631` à 635 (centré, droite) et 584 (justifié) | — |
| Retraits (gauche, droit, 1re ligne, suspendu) | oui | `text-layout/src/lib.rs:385` à 412 ; `cache.rs:334` à 343 pour les images | — |
| Interligne | oui | `text-layout/src/lib.rs::line_height` (`auto`, `exact`, `atLeast`, lignes 1285 à 1311) appelée en 648 | — |
| Espacements avant/après | oui | `text-layout/src/lib.rs:393` et 685 ; fusion avant/après dans `pagination.rs:815` ; règles construites en `cache.rs:229` | — |
| Trame | **non** | pas dans le modèle (`ParaProps`), ni lue à l'import (`fmt-ooxml/src/docx.rs::parse_para_props` ignore `w:shd`), ni peinte | **US-243-M1** (modèle, import, export) puis **T243-A** (rectangle de fond, `cache.rs` pour la peinture de page) |
| Bordures | **non** | idem pour `w:pBdr` ; `text-layout/src/borders.rs` calcule la géométrie des bordures mais **n'est appelé par aucun code du moteur** (`mod borders` seul dans `lib.rs`) | **US-243-M1** puis **T243-B** (brancher `borders.rs` sur la peinture de page) |
| Marques de mise en forme | **non** | aucune marque (¶, espace, tabulation) n'est dessinée ni exposée | **T243-C** : `FormatMarks` côté moteur (positions depuis les lignes mises en page) et surcouche côté client |

Constats complémentaires : (1) en profil TXT le moteur ignore toute propriété de paragraphe et de style (`cache.rs:382`), d'où `FormatState.plain` ; (2) `docx.rs::parse_char_attrs` rabat `w:dstrike` sur le barré simple : le double barré (US-241) exige un attribut de plus au modèle ; (3) `w:shd`, `w:pBdr`, `w:caps`, `w:spacing`... d'un DOCX importé ne sont pas portés par le modèle : leur conservation à l'enregistrement est à vérifier par US-243-M1 avant de promettre le comportement de Word.

## 10. Fichiers centraux et plan de fichiers par story

Aucun handler ne touche d'autre fichier central que les siens, **sauf US-250** (`rich_editor.rs`, section 5.6). Les tâches de rendu et de modèle partagent des fichiers : voici qui touche quoi, lot par lot.

**Lot parallèle (deux agents en même temps, fichiers disjoints)**

| Story | Fichiers |
|---|---|
| **US-240** | `rich_editor/format_char.rs`, `rich_editor/format_char_tests.rs`, `web/format-char.mjs`, `tests/rich-editor-format-char.mjs` ; rendu : `rich_layout/rich_paint.rs` (T240-A, B, D), `crates/text-layout/src/lib.rs` (T240-C exposant et indice) ; polices : `rich_preview.rs::build_fonts`, `rich_layout/cache.rs:943` (T240-F), `data/fonts` |
| **US-243 sans trame ni bordures** | `rich_editor/format_para.rs`, `rich_editor/format_para_tests.rs`, `web/format-para.mjs`, `tests/rich-editor-format-para.mjs` ; marques (T243-C) : **une méthode neuve** dans `rich_layout/cache.rs` (par exemple `PageCache::format_marks`), qui lit la mise en page existante (`ParagraphLayout`, `LayoutLine`, glyphes) sans modifier `text-layout` |

**Fichiers de test de F0 que US-240 peut modifier** : `rich_editor/format_tests.rs` et `tests/rich-editor-format-support.mjs`, pour les attentes de `ListFonts` seulement (T240-F : les faces enregistrées changent la liste) ; US-243 ne touche pas ces fichiers.

**Collisions à surveiller dans ce lot** : (1) `rich_layout/cache.rs` est touché par US-240 (ligne 943, T240-F) et par US-243 (méthode neuve T243-C) : régions distinctes, fusion textuelle attendue sans conflit, mais à relire ; (2) `crates/text-layout/src/lib.rs` : T240-C le modifie ; T243-C ne doit **pas** en dépendre (si la liste des marques exige une donnée de plus de `text-layout`, la tâche est repoussée après T240-C et séquencée, pas parallélisée) ; (3) `data/engine/atelier_core_wasm.wasm` et `manifest.json` sont régénérés par chaque branche : **ils entrent en conflit à chaque fusion**, l'intégrateur les régénère (`python -I tools/package-engine.py`) après la dernière fusion de chaque lot au lieu de les résoudre ; (4) `tests/rich-editor-format-support.mjs`, `rich_editor/format_tests.rs`, `rich_protocol/format.rs` sont figés : un besoin nouveau passe par l'intégrateur.

**Ensuite, séquencé**

1. **US-243-M1** (trame et bordures dans `ParaProps`) : `crates/text-model/src/rich/attrs.rs`, `fmt-ooxml/src/docx.rs`, `fmt-ooxml/src/docx/write/props.rs`, `rich_editor/format_state.rs` (lecture de la trame et des bordures), et environ 19 littéraux `ParaProps { ... }` des tests et du code qui n'utilisent pas `..Default::default()`. Aucune autre story ne tourne en même temps sur le modèle.
2. **T243-A / T243-B** (peinture de la trame et des bordures) : `rich_layout/cache.rs` (peinture de page), `text-layout/src/borders.rs` ; après que US-240 a rendu `rich_paint.rs` si la peinture y passe.
3. **US-250-M1** (nom d'affichage, drapeaux rapide et par défaut du style) : `crates/text-model/src/rich/styles.rs`, `fmt-ooxml/src/docx.rs` : après US-243-M1 (même import DOCX).
4. **US-250** : `rich_editor/styles.rs`, `rich_editor/styles_tests.rs`, `web/styles-pane.mjs`, `tests/rich-editor-format-styles.mjs` et `rich_editor.rs` (aperçu).

Tableau des tâches de l'audit :

| Fichier | Tâches | Remarque |
|---|---|---|
| `rich_layout/rich_paint.rs` | T240-A, B, D ; T243-A si la trame passe par ici | US-240 seul tant que T243-A n'a pas commencé |
| `crates/text-layout/src/lib.rs` | T240-C | US-240 seul |
| `rich_preview.rs::build_fonts`, `data/fonts`, taille WASM | T240-F | budget de taille : `node tests/check-wasm-size.mjs` (code 2 098 697 sur 2 286 262 après F0) |
| `crates/text-model/src/rich/attrs.rs`, `fmt-ooxml/src/docx.rs`, `docx/write/props.rs` | US-243-M1, US-250-M1 | séquencés |
| `rich_layout/cache.rs` | T240-F (ligne 943), T243-C (méthode neuve), T243-A/B (peinture) | trois régions différentes du même fichier |

## 11. Vecteurs, tests, commandes

Vecteurs gelés : `tests/rich-editor-format-vectors.txt` (15 requêtes, 8 réponses, 58 refus de requête, 18 refus de réponse), générés par `rich_protocol/format_tests.rs` (`ATELIER_REGEN_VECTORS=1`, seulement pour un changement délibéré), vérifiés par la même suite Rust et par `tests/rich-editor-format-vectors.mjs` (décodage, ré-encodage au même octet, troncatures, limites des encodeurs).

| Suite | Contenu | Commande |
|---|---|---|
| `rich_protocol/format_tests.rs` + `format.rs` | vecteurs, aller-retour, refus stables, troncatures, `size_step` | `cargo test -p atelier-core-wasm --locked --offline` |
| `rich_editor/format_tests.rs` | `FormatState` (sélections, mot sous le curseur, paragraphes, cellules, blocs opaques, sélection inversée ou vide, polices, noms tronqués, DOCX, lecture seule, texte brut, scan partiel), identifiant de commande, `ListFonts` et sa mémoire | idem |
| `rich_editor/format_char_tests.rs`, `format_para_tests.rs`, `styles_tests.rs` | un test de stub par famille (propre à la story) | idem |
| `crates/text-model/tests/format_edit.rs` | `apply_format` : une étape, révision, redo, tout ou rien, jointure, portée | `cargo test -p atelier-text-model --locked --offline` |
| `tests/rich-editor-format.mjs` | vecteurs JS ; client sur moteur simulé (garde au tour, réponse perdue, frappe intercalée, attributs de saisie seuls, annulation, refus, aperçu) ; **vrai WASM** : `FormatState`, `ListFonts`, texte brut | `node tests/rich-editor-format.mjs [engine.wasm]` |
| `tests/rich-editor-format-char.mjs`, `-para.mjs`, `-styles.mjs` | un scénario de stub par famille, sur le vrai WASM (support : `tests/rich-editor-format-support.mjs`) | `node tests/rich-editor-format-char.mjs [engine.wasm]`, idem `-para`, `-styles` |

**À ajouter à la CI** (job `wasm`, avec les autres suites `rich-editor-*`, après `tests/rich-editor-abi.mjs`) : `node tests/rich-editor-format.mjs`, `node tests/rich-editor-format-char.mjs`, `node tests/rich-editor-format-para.mjs`, `node tests/rich-editor-format-styles.mjs`. Les tests Rust sont déjà couverts par `cargo test -p atelier-core-wasm -p atelier-text-model ...`.

## 12. Limites et décisions ouvertes

- Le dessin de plusieurs attributs et la famille de polices manquent (section 9) : le contrat est complet, le rendu ne l'est pas ; tant que T240-A à D ne sont pas faits, les critères de rendu de US-240 ne peuvent pas être cochés.
- Palier de taille, règle du mot sous le curseur, retrait de 720 twips et 12 pt d'espacement sont des **décisions de F0**, à valider contre Word par la campagne US-308.
- `ListFonts` n'énumère pas les polices système du navigateur : l'aperçu d'une entrée utilise la police du navigateur de même nom, approximation à signaler dans l'interface.
