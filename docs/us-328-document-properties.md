<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-328 — Propriétés du document

Partie bibliothèque : lecture, modification et écriture de `docProps/core.xml` et
`docProps/app.xml`, mise à jour à l'enregistrement, correspondance vers les métadonnées PDF. Le code
est dans `crates/fmt-ooxml/src/properties.rs` ; `lib.rs` n'a que les points de raccord (champs
`core_extra`/`app_extra`, lecture tolérante d'une partie sans relation, appel du nouvel écrivain).
Le dialogue Propriétés et son branchement restent à l'intégrateur.

## Inventaire

**Existait (US-300)** : `CoreProperties` (14 champs texte : title, subject, creator, keywords,
description, last_modified_by, revision, created, modified, category, content_status, identifier,
language, version) et `AppProperties` (10 : application, app_version, company, manager, pages, words,
characters, paragraphs, lines, template), `core_properties_mut`/`app_properties_mut`, lecture
tolérante, éléments inconnus gardés comme sous-arbres opaques, partie absente créée à l'écriture
avec relation (`rIdCoreProperties`, `rIdAppProperties`) et type de contenu. Limites de l'existant :
la partie était **entièrement régénérée** (inconnus rejetés en fin de fichier, préfixes `ns0:`,
attribut `xsi:type="dcterms:W3CDTF"` perdu) ; `cp:lastPrinted`, `TotalTime`, `CharactersWithSpaces`,
`DocSecurity`, `HyperlinkBase` n'étaient pas des champs ; aucune règle de dates ni de suppression.

**Ajouté** : `CoreExtra { last_printed }` et `AppExtra { total_time, characters_with_spaces,
doc_security, hyperlink_base }`, structures séparées (un littéral `CoreProperties { .. }` complet existe
dans `tests/opc_package.rs` : ajouter un champ l'aurait cassé) ; dates W3CDTF ; instantané et
changements ; mise à jour à l'enregistrement ; écrivain **par retouche des octets d'origine** ;
correspondance PDF. Les éléments structurés (`HeadingPairs`, `TitlesOfParts`) ne sont jamais
interprétés : ils restent tels quels dans le fichier.

## API (sur `Package`, types exportés par la crate)

- `properties_snapshot() -> PropertiesSnapshot { editable, read_only, notices }`. `EditableProperties`
  (chaînes, vide = absent) : title, subject, author (`dc:creator`), manager, company, category,
  keywords, comments (`dc:description`), hyperlink_base, content_status, language. `ReadOnlyProperties` :
  created / modified / last_printed (`PropertyDate::Valid(W3cDateTime)` ou `Invalid(texte)`),
  last_modified_by, revision, identifier, version, application, app_version, template, total_time,
  doc_security, `DocumentStatistics` (pages, words, characters, characters_with_spaces, lines,
  paragraphs). Ne renvoie jamais d'erreur.
- `apply_property_changes(&PropertyChanges) -> Result<ApplyReport, PropertyError>` ;
  `PropertyChanges::between(&avant, &après)` ne liste que les champs modifiés dans le dialogue.
- `update_on_save(&SaveContext) -> SaveReport`, à appeler **à l'enregistrement uniquement**.
- `pdf_metadata() -> PdfMetadata`.
- `W3cDateTime::{parse, to_w3cdtf, utc, to_utc, unix_seconds, from_unix_seconds}` : aucune horloge,
  l'appelant fournit « maintenant » (`from_unix_seconds(Date.now()/1000)` côté WASM). Convertit aussi
  les FILETIME de l'import DOC.

## Règles

- **Suppression** : chaîne vide (ou vide après nettoyage) = l'élément est retiré du fichier (avec son
  indentation) ; un champ absent n'est jamais écrit.
- **Nettoyage** : caractères interdits en XML 1.0 (contrôles sauf tab/LF/CR, U+FFFE/FFFF) supprimés ;
  champs d'une ligne : tab, CR, LF deviennent une espace ; commentaires : CR et CRLF deviennent LF.
  `ApplyReport::sanitized` dit quels champs ont été touchés.
- **Longueurs** (caractères, pas octets ; erreur `TooLong`, rien n'est appliqué) : 255 pour une ligne,
  1024 mots-clés, 4096 commentaires, 2048 base des liens, 35 langue. Les valeurs données à
  `update_on_save` (utilisateur, application) sont tronquées, jamais refusées.
- **Mots-clés** : une seule chaîne telle que saisie (séparateur `; ` ou `, ` à la charge du dialogue).
- **Éléments en double** : le **dernier** gagne en lecture (règle de l'existant) ; une modification
  touche le dernier, une suppression les retire tous ; avis `DuplicateElement`.
- **Dates** : formes `YYYY`, `YYYY-MM`, `YYYY-MM-DD`, `…Thh:mm`, `…:ss`, `…:ss.s{1,9}`, avec `Z`,
  `±hh:mm`, `±hhmm` ou sans fuseau (toléré, `offset_minutes = None`). Une date invalide reste du texte
  (`PropertyDate::Invalid`, avis `InvalidDate`), n'est jamais réécrite tant qu'on ne touche pas à
  son champ, et `update_on_save` ne la « répare » pas (`created` est présent). `modified` est écrit en
  UTC, secondes, `Z`, avec `xsi:type="dcterms:W3CDTF"`.
- **Révision** : nombre décimal → +1 (saturé) ; absente, vide ou non numérique (`abc`, `-3`, `2.5`) →
  **1** (`SaveReport::revision_restarted`). `created` et `dc:creator` sont fixés s'ils sont absents ou
  vides ; `lastModifiedBy` = utilisateur fourni (inchangé si `None`) ; `Application`/`AppVersion` =
  valeurs fournies ; statistiques : seuls les compteurs `Some` sont écrits (un `None` laisse la valeur).
- **Ouverture** : lire ne modifie rien ; une réécriture sans changement redonne les parties **octet
  pour octet** (testé sur un fichier de style Word et sur un fichier LibreOffice), même après
  `core_properties_mut()`. Changer un champ ne change que son élément ; un champ nouveau est inséré
  après le dernier élément qui le précède dans l'ordre de Word, en reprenant l'indentation. Les
  espaces de noms absents de la racine sont déclarés sur l'élément ajouté ; une racine vide `<r/>`
  est développée. Le texte est échappé (`& < >`, `\r` en `&#13;`), UTF-8.
- **Partie sans relation** : lue (avis `UnreferencedPart`) et reliée (relation ajoutée) seulement si
  on écrit des propriétés ; sinon le paquet est réécrit comme il est.
- **Repli** : si la partie d'origine n'est pas de l'UTF-8 simple (par ex. UTF-16) ou ne se balaye
  pas, l'ancien écrivain régénère la partie (perte des extras et de `xsi:type`) ; jamais de panique.
- **Robustesse** : le balayeur est borné (100 000 enfants, profondeur 1024, 1024 attributs) ; tailles
  bornées en amont par `Limits::max_xml_part_bytes`.

## Preuves

`tests/document_properties.rs` (31 tests, valeurs calculées à la main) : chaque propriété en lecture,
chaque forme de date et les dates invalides, `unix_seconds` (2000-01-01 = 946684800), suppression,
insertion et ordre, espaces de noms, racine vide, inconnus et `TitlesOfParts` préservés, aller-retour
identique, un champ = un élément, révision (8 cas), création des parties absentes avec relation et type
de contenu, partie sans relation, échappements, limites, correspondance PDF, campagne de mutations et
troncatures à graine fixe (600 tours sur les deux parties), valeurs géantes.

`tests/document_properties_libreoffice.rs` : fichier produit par LibreOffice 26.8
(`tests/docx-reference/libreoffice/properties.docx`, source `properties.fodt`).
`tools/check-docx-properties-libreoffice.py` relance ce test puis fait **relire par LibreOffice**
(`--convert-to fodt`) le docx modifié et un docx dont les parties ont été créées : titre, sujet, auteur,
mots-clés, commentaires (accents, `« »`, `—`, `œ`, `&`, `<`, retour à la ligne) sont vus exactement.
Microsoft Word n'est pas installé ; les docx du corpus (`basic.docx` sans `docProps`, `rich.docx`
écrit par LibreOffice) ne viennent pas de Word : aucun aller-retour sur un fichier Word réel.

**Défaut hors périmètre constaté** : l'écrivain de `[Content_Types].xml` et des `.rels` (US-300)
préfixe tous les éléments (`ns0:`, déclaration répétée) ; LibreOffice refuse de charger un tel paquet
(« source file could not be loaded »), Word très probablement aussi. Le script retire ces préfixes avant
de donner le fichier à LibreOffice (les parties de propriétés ne sont pas touchées). Le correctif
(écrire ces deux parties avec l'espace de noms par défaut) tient en une soixantaine de lignes dans
`lib.rs` mais change le test existant `validates_content_types_and_extension_syntax`, qui remplace le
texte littéral `</ns0:Types>` : laissé à l'intégrateur (et à l'US-307).

## À brancher par l'intégrateur

1. **Dialogue Propriétés** : `let s = pkg.properties_snapshot()` ; champs modifiables depuis
   `s.editable`, dates et statistiques en lecture seule depuis `s.read_only`, avis dans `s.notices`.
   À OK : `pkg.apply_property_changes(&PropertyChanges::between(&s.editable, &nouveau))` ; gérer
   `PropertyError::TooLong` (compteur de caractères dans les champs).
2. **Enregistrement** : juste avant `pkg.write()`, `pkg.update_on_save(&SaveContext { now, user,
   application: Some("Atelier".into()), app_version, statistics })` avec `now` =
   `W3cDateTime::from_unix_seconds(..)`, `user` = nom d'utilisateur des Options (US-329) et
   `statistics` = `Statistics` de l'US-343 converti champ à champ (`usize` en `u64`). Rien à l'ouverture.
3. **Export PDF** (US-332) :
   ```rust
   let m = pkg.pdf_metadata();
   let date = |d: PdfDateFields| PdfDate { year: d.year, month: d.month, day: d.day, hour: d.hour,
       minute: d.minute, second: d.second, utc_offset_minutes: d.utc_offset_minutes };
   let info = PdfInfo { title: m.title, author: m.author, subject: m.subject, keywords: m.keywords,
       creation_date: m.creation_date.map(date), modification_date: m.modification_date.map(date),
       creator: Some("Atelier".into()), ..PdfInfo::default() };
   ```
   Les champs de `PdfMetadata`/`PdfDateFields` sont ceux de `PdfInfo`/`PdfDate` ; un décalage de plus de
   14 h est ramené en UTC ; date invalide = pas de date.
4. **Clés de catalogue proposées** : `properties.title`, `properties.tab.summary`,
   `properties.tab.statistics`, `properties.field.title|subject|author|manager|company|category|keywords|comments|hyperlink_base`,
   `properties.created|modified|last_modified_by|last_printed|revision|application|template|total_time`,
   `properties.stat.pages|words|characters|characters_with_spaces|lines|paragraphs`,
   `properties.error.too_long` (champ, max), `properties.notice.invalid_date|non_numeric|duplicate|unreferenced`.

## Forme commune avec ODT et DOC (non branchée)

`OdtMetadata` (fmt-odf : title, subject, creator, description, keywords `Vec`, dates en texte ISO 8601)
et `DocumentProperties` (fmt-legacy : title, subject, author, keywords, comments, last_author, dates
FILETIME `u64`) se ramènent à `EditableProperties` plus des dates : mots-clés ODT joints par `"; "`,
FILETIME par `filetime_to_unix_seconds` puis `W3cDateTime::from_unix_seconds`, texte ISO par
`W3cDateTime::parse`. Un `DocumentMetadata { editable: EditableProperties, created, modified:
Option<PropertyDate>, last_modified_by, application }` suffirait aux trois formats et à
`PdfMetadata::from_snapshot`.
