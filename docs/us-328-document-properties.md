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
  `±hh:mm`, `±hhmm` (décalage borné à ±14:00 comme le `dateTime` de XML Schema) ou sans fuseau
  (toléré, `offset_minutes = None`). Une date invalide reste du texte
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
  pas, elle est régénérée proprement à partir des valeurs (préfixes `cp`, `dc`, `dcterms`, `xsi`, espace
  de noms par défaut pour `app.xml`) ; les éléments inconnus sont alors perdus, ce que l'instantané
  signale d'avance par l'avis `PartRegenerated`. L'ancien écrivain (préfixes `nsN:`) n'existe plus.
- **Opérations qui se touchent** : à une même position, une insertion passe avant la suppression ou le
  remplacement qui commence là (ex. supprimer `title` et ajouter `keywords` en même temps).
- **`]]>` dans une valeur** : écrit `]]&gt;` ; l'analyseur XML du projet (`atelier-xml`) a été corrigé
  pour ne refuser que la séquence littérale `]]>` dans le texte, pas celle obtenue en décodant `&gt;`.
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

**Paquet OPC** : `[Content_Types].xml` et les `.rels` étaient écrits avec des préfixes générés (`ns0:`,
déclaration répétée sur chaque élément) et LibreOffice refusait de charger tout paquet écrit par le
projet. Ces deux types de parties sont maintenant écrits avec l'espace de noms par défaut déclaré une
seule fois sur la racine (la lecture accepte toujours les deux formes). `tools/check-docx-properties-libreoffice.py`
donne à LibreOffice le fichier **tel qu'écrit**, sans retouche, et il l'ouvre. Le test existant
`validates_content_types_and_extension_syntax` cherchait le littéral `</ns0:Types>` : il cherche
désormais `</Types>`. Tests : `tests/document_properties_files.rs` (absence de préfixe généré et
déclaration unique dans les quatre types de parties, lecture de l'ancienne forme, propriété à graine fixe
sur 400 jeux aléatoires de `core.xml`/`app.xml`).

## Branchement dans l'application (2026-10-17)

Fait, sur l'aperçu DOCX (le seul chemin d'ouverture DOCX de l'application) :

1. **Moteur** (`crates/core-wasm/src/properties.rs`, sans état : le DOCX est dans la commande, comme l'opcode 18).
   Opcode 26 `PropertiesRead` (octets DOCX) répond `ATPP` : onze champs texte, leurs limites en caractères,
   trois dates (absente, valide en W3CDTF, texte illisible), dernier auteur, révision, application, version,
   modèle, durée, six compteurs, nombre d'avis. Opcode 27 `PropertiesApply` (`u32` taille, DOCX, onze champs
   `u8 présent, u32 taille, UTF-8`) répond `ATPA` : statut 0 puis nombre de champs modifiés et le nouveau DOCX
   (l'entrée inchangée octet pour octet quand rien ne change), ou statut 1 `TooLong` (indice du champ, maximum,
   longueur), sans rien appliquer. Seul le paquet OPC est réécrit : le reste du fichier est conservé.
   Format exact en tête du module ; ces opcodes suivent les existants (0 à 25).
2. **Enregistrement** : `update_on_save` est appelé par l'export étagé de type 3 (`ExportDocxSave`, nouveau, les
   types 1 et 2 sont inchangés), dont le préfixe d'entrée porte l'heure Unix, l'utilisateur et la version
   (`encodeSaveStamp` côté JS). Statistiques écrites : mots, caractères, caractères avec espaces et paragraphes,
   comptés par `atelier_text_model::statistics::document` (US-343) sur le texte du corps. **Pages et lignes ne
   sont pas écrites** : l'export ne fait pas de mise en page, `app.xml` garde ce qu'il contenait. L'application
   n'a aucun chemin d'enregistrement DOCX (le bouton Enregistrer écrit du TXT) : l'appel est vérifié par des tests
   du moteur, pas par une action utilisateur.
3. **Interface** : `web/properties-dialog.mjs` + `.css`, protocole dans `web/properties-protocol.mjs`. Commande
   « Propriétés… » de l'onglet Fichier, active quand un aperçu DOCX est prêt. Six champs modifiables (titre, sujet,
   auteur, mots-clés, commentaires, catégorie) avec compteur `n / max caractères` ; au-delà de la limite le champ est
   `aria-invalid`, un message l'explique et OK est désactivé ; une réponse `TooLong` du moteur est affichée de la
   même façon. Dates (création, modification, impression) et statistiques en lecture seule, formatées par
   `formatDateTime`/`formatNumber`, « Non renseigné » si absentes. OK/Annuler/Échap (Échap ne ferme pas aussi
   l'aperçu), focus piégé et rendu à la commande, étiquettes et compteurs reliés par `aria-describedby`. Les
   modifications validées remplacent les octets DOCX gardés en mémoire pour l'aperçu courant : elles sont perdues à
   la fermeture de l'aperçu tant qu'aucun enregistrement DOCX n'existe.
4. **PDF** : non branché. Il n'y a pas d'export PDF dans l'application (US-332 non faite) et `atelier-core-wasm`
   ne dépend pas de `atelier-fmt-pdf` ; relier les deux demande une dépendance dans un `Cargo.toml`. Vérifié de
   chaque côté : `PdfMetadata` reprend champ par champ titre, auteur, sujet, mots-clés et dates
   (`tests/document_properties.rs`) et l'écrivain PDF écrit ces six champs dans le dictionnaire Info
   (`crates/fmt-pdf/tests/document.rs`, `info_carries_title_author_subject_keywords_and_both_dates`). La
   conversion à écrire à l'export est celle du point 3 de la section suivante. Le critère « reprises dans les
   métadonnées du PDF exporté » reste donc ouvert.

Tests : `cargo test -p atelier-core-wasm --test properties` (lecture, aller-retour DOCX après modification,
limites, entrées malformées, `update_on_save`), `node tests/properties-abi.mjs` (codecs JS contre le WASM réel,
y compris l'enregistrement tamponné), `node tests/run-ui-browser.mjs` (dialogue isolé : dates, `TooLong`, échec du
moteur, attente), `node tests/run-editor-browser.mjs` (application réelle : commande, saisie, limite, Annuler et
Échap sans effet, OK puis relecture, focus). Non vérifié : lecteur d'écran réel, fichier écrit par Word.

## Branchement restant (export PDF, enregistrement DOCX)

1. **Enregistrement** (à relier à une commande utilisateur) : juste avant `pkg.write()`, `pkg.update_on_save(&SaveContext { now, user,
   application: Some("Atelier".into()), app_version, statistics })` avec `now` =
   `W3cDateTime::from_unix_seconds(..)`, `user` = nom d'utilisateur des Options (US-329) et
   `statistics` = `Statistics` de l'US-343 converti champ à champ (`usize` en `u64`). Rien à l'ouverture.
2. **Export PDF** (US-332) : à écrire quand l'export existera et que le crate pourra dépendre de `atelier-fmt-pdf` :
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
3. **Clés de catalogue** : le dialogue utilise des clés `properties-*` (voir `web/i18n/fr-FR.txt`) ; ci-dessous la proposition initiale : `properties.title`, `properties.tab.summary`,
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
