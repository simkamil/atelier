# US-343 — Statistiques du document

Module `atelier_text_model::statistics` (`crates/text-model/src/statistics/`) : mots, caractères avec et sans espaces, paragraphes d'un document riche, d'un document texte brut ou d'une sélection, selon des règles alignées sur Microsoft Word. Lignes et pages viennent de la mise en page : l'appelant les fournit, le module les restitue.

**Périmètre livré : bibliothèque et interface TXT.** Le compteur de mots document/sélection et le dialogue sont raccordés au moteur Rust ; pages et lignes proviennent de la mise en page réelle. Voir [la livraison UI](us-340-343-navigation-ui.md). Les notes/zones de texte et le raccordement riche restent ouverts.

**État de la comparaison à Word : non faite.** Microsoft Word n'est pas installé ici et aucun document enregistré par Word n'a pu être comparé (voir « Comparaison à un oracle externe »). Le critère « identique à Word, tolérance ≤ 0,5 % » n'est donc **pas** démontré ; seule une comparaison à LibreOffice (14 documents) l'est, avec des écarts expliqués.

## API

```rust
use atelier_text_model::statistics::*;

let options = StatisticsOptions {
    include_headers_footers: false,        // défaut : corps seul
    include_notes_and_text_boxes: false,   // prévu, sans effet aujourd'hui
    lines: Some(1234), pages: Some(300),   // fournis par la mise en page, restitués tels quels
    ..StatisticsOptions::default()
};
let s: Statistics = document(&rich_doc, &options);                  // document riche
let s = selection(&rich_doc, &model_selection, &options)?;          // atelier_model::selection::Selection
let s = plain_text(plain_doc.text(), &options);                     // texte brut
let s = plain_selection(&plain_doc, &options);                      // sélection du PlainTextDocument
let s = plain_text_range(text, 3..17, &options)?;                   // plage d'octets quelconque
```

- `Statistics { words, characters, characters_with_spaces, paragraphs, lines: Option<usize>, pages: Option<usize> }`. `characters` est le nombre de caractères **sans** espaces. `lines` et `pages` sont la recopie de `StatisticsOptions::lines/pages` : **aucun calcul**, `None` si l'appelant ne les connaît pas.
- `Counts { words, characters, characters_with_spaces, paragraphs }` : la partie additive. Implémente `+`, `-` (saturante), `+=`, `-=`, `Sum`, `checked_sub`, `replace(old, new)` et `with_layout(lines, pages)`.
- `count_paragraph(&str, &CountingRules) -> Counts` : le seul algorithme de comptage. Le document riche, le texte brut, les sélections et les caches l'appellent tous ; rien n'est dupliqué.
- `CountingRules { char_unit: CharUnit, count_empty_paragraphs: bool }` (défauts : `Scalar`, `false`).
- Erreurs (`StatisticsError`) : nœud inconnu ou non paragraphe, décalage hors du texte ou au milieu d'un caractère UTF-8, plage dont les extrémités sont dans deux récits différents (corps / en-tête), paragraphe absent du cache.

## Règles de comptage

Niveaux : **certain** (documenté par Microsoft ou observé), **probable**, **hypothèse** (non vérifiée contre Word ; à contrôler à l'intégration). Toutes les règles de classification sont dans `statistics/rules.rs` : `classify`, `is_ideograph`, `is_cjk_punctuation`, `CountingRules`.

| # | Règle | Niveau |
| --- | --- | --- |
| 1 | Un mot est une suite maximale de caractères qui ne sont pas des séparateurs. La ponctuation collée appartient au mot : `Bonjour,` = 1, `(a)` = 1, `and/or` = 1, `word—word` = 1. | certain |
| 2 | Séparateurs : espace, tabulation, saut de ligne (`\n`, U+000B), U+000C, U+0085, U+1680, U+2000–2006, U+2008–200A, U+2028, U+2029, U+205F, U+3000. | probable (liste exacte : hypothèse pour les espaces Unicode rares) |
| 3 | `l'école` et `l’école` (apostrophe droite ou typographique) = 1 mot. | certain |
| 4 | Un tiret isolé entouré d'espaces (`a - b`, `a — b`) compte pour un mot. | certain |
| 5 | Une espace insécable (U+00A0, U+202F, U+2007) **ne sépare pas** : `10 km` avec insécable = 1 mot. Une suite d'insécables seule n'est pas un mot. | probable |
| 6 | Nombres : `1,234.56` = 1 mot ; `3 000 000` = 3 mots ; `3 000` avec insécable = 1. | certain / probable (insécable) |
| 7 | Caractères sans espaces : tous sauf les séparateurs et les insécables. Avec espaces : tous (espaces, tabulations, sauts de ligne compris). La marque de paragraphe n'existe pas dans le texte et ne compte jamais. | caractères avec espaces : certain ; tabulation / saut de ligne comptés avec espaces mais pas sans : probable ; insécable exclue du « sans espaces » : **hypothèse** |
| 8 | Unité de caractère (`CharUnit`) : `Scalar` par défaut (un point de code), `Grapheme` (un cluster UAX #29 : `e` + accent combinant = 1, une famille d'émoji = 1), `Utf16` (une unité UTF-16 : un émoji hors BMP = 2). Aucun octet. | **hypothèse** : Word compte probablement en unités UTF-16 ; le choix par défaut est une option, pas une certitude |
| 9 | Écritures sans espaces : chaque caractère han, hiragana, katakana (jamais le hangul) compte pour un mot ; la ponctuation idéographique (`、。「」`…) n'est jamais un mot à elle seule ; un sélecteur de variation reste collé à son idéogramme. `abc日本def` = 4 mots. Le hangul compte un mot par suite séparée par des espaces. | idéogrammes = un mot chacun : certain (énoncé de la story) ; ponctuation idéographique, kana, hangul : **hypothèse** |
| 10 | Un paragraphe vide (zéro caractère) n'est pas compté dans les paragraphes ; un paragraphe d'espaces seules l'est. Option `count_empty_paragraphs`. | règle de la story ; non vérifiée contre Word : **hypothèse** |
| 11 | Caractères invisibles (U+200B, U+00AD, U+200D…) : comptés comme caractères, ne séparent pas. | **hypothèse** |
| 12 | Champs : le résultat affiché est dans le texte du paragraphe, donc compté ; l'instruction (`PAGE`, `DATE`…) n'y est pas, donc non comptée. | certain pour le modèle ; Word : probable |
| 13 | Objets ancrés (images, formes) : ne portent pas de texte dans le modèle, comptent pour rien. | probable |
| 14 | Numérotation et puces générées : non comptées (ce n'est pas du texte du document). LibreOffice les compte (voir plus bas). | probable |

## Portée

- Corps de toutes les sections ; cellules de tableaux comprises, tableaux imbriqués compris, dans l'ordre du document. Blocs opaques (contrôles de contenu non interprétés) : ignorés.
- `include_headers_footers` (défaut `false`) : tous les en-têtes et pieds de toutes les sections et variantes. Word ne les compte pas dans la barre d'état (probable) ; LibreOffice les compte.
- `include_notes_and_text_boxes` (défaut `false`) : **sans effet aujourd'hui**, le modèle ne porte ni notes ni zones de texte (les objets ancrés sont opaques). L'option existe pour câbler la case du dialogue ; quand le modèle portera ces récits, ils seront ajoutés à `scoped_paragraphs` (`statistics/rich.rs`).
- Une sélection compte ce qu'elle recouvre, y compris dans un en-tête, quelles que soient les options de portée.

## Sélection

`selection(doc, &Selection, &options)` parcourt chaque plage du modèle (ancre et tête dans n'importe quel ordre, plusieurs paragraphes, à travers les cellules de tableau en ordre du document). Un mot partiellement sélectionné compte pour un mot ; une sélection vide (caret) donne des zéros ; les plages qui se recouvrent sont fusionnées (aucun texte compté deux fois). Un fragment de paragraphe compte comme paragraphe s'il contient du texte ; un paragraphe entièrement recouvert suit la règle 10. Une plage qui commence dans le corps et finit dans un en-tête est refusée (`RangeAcrossStories`).

**Mot coupé.** Deux sélections adjacentes `[a, b)` et `[b, c)` dont la borne `b` tombe dans un mot comptent ce mot dans chacune : `mots(a,b) + mots(b,c) = mots(a,c) + 1` (0 sinon) ; de même pour le paragraphe coupé. Les caractères s'additionnent exactement (unité `Scalar`). Testé par propriété.

Le coût de `selection` est linéaire en nombre de paragraphes du document (parcours des identifiants, puis comptage des seuls paragraphes touchés) : environ 11 ms pour 6 000 paragraphes, sélection complète.

## Mise à jour incrémentale (barre d'état)

- `Counts` est additif : le total d'un document est la somme des `Counts` de ses paragraphes ; la différence avant/après d'une frappe est `total.replace(ancien, nouveau)`.
- `StatisticsCache` (document riche) garde un `Counts` par paragraphe (`NodeId`).
  - `update_paragraph(&doc, id)` : frappe ou suppression de texte dans un paragraphe déjà en cache, coût = comptage de ce paragraphe (3 µs mesurés).
  - `sync(&doc, &changed)` : après un changement de structure (scission, fusion, insertion, suppression, tableaux) ; parcourt les identifiants du document (sans lire de texte), retire les paragraphes disparus et ne recompte que les paragraphes nouveaux ou listés dans `changed` (1,9 ms pour 6 000 paragraphes).
  - `statistics()`, `counts()`, `paragraph_counts(id)`, `set_layout(lines, pages)`.
- `PlainStatisticsCache` (texte brut) : `update(&nouveau_texte, début, ancienne_fin, nouvelle_longueur)` pour le remplacement des octets `début..ancienne_fin` ; ne recompte que les lignes touchées (et leurs voisines, pour les paires CR/LF), les autres sont décalées (22 µs pour 3 Mo). Arguments incohérents : recomptage complet (jamais de résultat faux).
- Les caches ne s'abonnent à rien : l'intégrateur leur signale les paragraphes modifiés (les `RichOp` portent déjà `para`). Si les options de portée ou `CountingRules` changent, reconstruire le cache.

## Comparaison à un oracle externe

**Word réel : aucune comparaison.** Word n'est pas installé. Les corpus du dépôt n'en contiennent aucun comptage : `tests/docx-reference/basic.docx` est synthétique (pas de `docProps/app.xml`), `tests/doc-reference/*.doc` et `tests/odt-reference/*.odt` sont produits par LibreOffice. Hors du dépôt, des documents enregistrés par Word (`Application` = « Microsoft Office Word ») existent sur la machine, avec leurs compteurs `Words`, `Characters`, `CharactersWithSpaces`, `Paragraphs` dans `app.xml`, mais je n'ai pas extrait leur texte (documents personnels de l'utilisateur) : **contrôle local à faire à l'intégration** :

```
python -I tools/extract-statistics-oracle.py <dossier_sortie> <fichiers .docx enregistrés par Word>
ATELIER_STATS_ORACLE_DIR=<dossier_sortie> cargo test -p atelier-text-model --test statistics_oracle local_oracle -- --nocapture
```

Le test affiche, par document et par compteur, notre valeur, celle de Word et l'écart, et échoue au-delà de 0,5 %. L'extracteur ne lit que le corps (zones de texte et dessins exclus), ce qui correspond au comptage par défaut de Word. Attention : les compteurs d'`app.xml` sont ceux du dernier enregistrement. Le contrôle par import (`atelier-fmt-ooxml` / `atelier-fmt-legacy`) n'a pas été écrit : ces crates dépendent de `atelier-text-model`, une dépendance de test créerait un cycle, et la consigne interdit d'y toucher ; à faire dans le crate d'import à l'intégration.

**LibreOffice : 14 documents ODT** (`tests/odt-reference`, LibreOffice 26.8, compteurs `meta:document-statistic`), via `tools/extract-statistics-oracle.py` et `crates/text-model/tests/statistics-data/`. Ce n'est pas Word : ses règles diffèrent. Avec les règles de Word, **7 documents sur 14 sont identiques sur les quatre compteurs** ; les 7 autres diffèrent, chaque écart est expliqué et figé par un test (`word_profile_differences_with_libreoffice_are_the_documented_ones`) :

| Document | Mots (nous − LO) | Caractères | Paragraphes | Explication |
| --- | --- | --- | --- | --- |
| `multi-page` | +300 (+7,0 %) | 0 | 0 | 300 tirets cadratins isolés : LO ne les compte pas, Word oui (règle 4) |
| `para-format` | +8 (+6,0 %) | 0 | 0 | 8 tirets cadratins isolés |
| `text-accents` | +1 (+4,3 %) | 0 | 0 | 1 tiret cadratin isolé |
| `header-footer` | +105 (+6,5 %) | −26 | −2 | LO compte l'en-tête et le pied (7 mots, 26 caractères, 2 paragraphes) et ignore 112 tirets cadratins |
| `fields-notes` | −4 | −16 | −1 | LO compte le corps de la note (4 mots) et le numéro d'appel de note |
| `lists` | −10 | −16 | 0 | LO compte les 10 puces et numéros générés (règle 14) |

Contre-épreuve : `libreoffice_profile_reproduces_libreoffice_counts` applique **à nos compteurs** le périmètre de LibreOffice (corps + en-têtes + pieds + notes) et sa seule règle de mots divergente (tirets cadratins seuls non comptés) : les 14 documents sont alors identiques, sauf `lists` (étiquettes générées, que le texte extrait ne contient pas). Cela prouve que nos compteurs de caractères, de paragraphes et nos mots reproduisent LibreOffice partout ailleurs, et que les écarts ci-dessus n'ont pas d'autre origine. Les règles de Word n'ont **pas** été changées pour coller à LibreOffice. Mesure : un `-` ou un `.` isolé n'est pas couvert (seul le tiret cadratin apparaît dans le corpus).

Conclusion honnête : l'écart « mots » contre LibreOffice dépasse 0,5 % sur 4 documents, pour une raison identifiée et voulue ; l'écart contre **Word** est inconnu.

## Limites

- Lignes et pages : non calculées.
- Notes, zones de texte, tout objet ancré : non comptés (le modèle n'a pas leur texte).
- Numérotation de listes : pas de texte généré.
- Règles marquées « hypothèse » : à valider sur des documents Word.
- Sélection : cohérente pour une seule histoire (corps ou un en-tête) ; pas de sélection à cheval sur deux récits.
- Coupure de mot par les caches incrémentaux : sans objet, ils comptent par paragraphe.
- Mesure indicative (release, hôte x86-64) : document riche de 6 000 paragraphes, 540 000 mots, 2,8 millions de caractères (environ 300 pages de 20 paragraphes de 80 mots) : comptage complet **11 ms** ; construction du cache 11,6 ms ; mise à jour d'un paragraphe 3 µs ; `sync` 1,9 ms ; sélection du tout 11,4 ms ; texte brut de 3,2 Mo : 10,3 ms, mise à jour incrémentale 22 µs. Reproductible : `cargo test -p atelier-text-model --release --test statistics_perf -- --ignored --nocapture`.

## À brancher par l'intégrateur

1. Déclaration `pub mod statistics;` dans `crates/text-model/src/lib.rs` (commit séparé).
2. Barre d'état (US-344) : un `StatisticsCache` par document ; `update_paragraph` à chaque frappe, `sync` après les opérations structurelles ; mots du document = `cache.counts().words` ; mots de la sélection = `selection(...)` à chaque changement de sélection (anti-rebond conseillé sur les gros documents).
3. Dialogue : `document(...)` ou `cache.statistics()` avec `lines` et `pages` fournis par la mise en page (US-342), cases « en-têtes et pieds » et « notes et zones de texte » reliées à `StatisticsOptions` ; `StatisticsCache::build` à refaire quand une option change.
4. Contrôle contre Word réel (commande ci-dessus) avant de cocher le critère « comptage identique à Word ».
5. Lignes à ajouter dans les fichiers communs (non modifiés ici) : `docs/DEVELOPMENT.md` / `docs/*PROGRESS.md` : « US-343 bibliothèque : `atelier_text_model::statistics`, voir docs/us-343-statistics.md » ; `US-V0-V1.md` : case « Comptage identique à Word » à ne cocher qu'après le contrôle 4. Le `Cargo.lock` et la CI ne changent pas (aucune dépendance).

## Tests

`crates/text-model/tests/` : `statistics.rs` (29 cas calculés à la main), `statistics_property.rs` (propriétés à graine fixe : somme des paragraphes = global, cache incrémental = recomptage, sélections adjacentes cohérentes avec leur union, cache texte brut), `statistics_oracle.rs` (LibreOffice + contrôle local), `statistics_perf.rs` (mesure, ignoré par défaut). Données : `statistics-data/*.stats.txt` générées par `tools/extract-statistics-oracle.py`.

Intégration (2026-10-13) : le correctif de revue `c4e0539` est fusionné. Plusieurs plages disjointes du même paragraphe comptent ce paragraphe une seule fois; la régression est vérifiée dans `statistics.rs` et la passe complète après fusion. Le dialogue et la comparaison à Word restent ouverts.

## Éditeur riche (R7b-2, 2026-10-18)

La commande `Statistics` (`0x0131`) de l'éditeur v2 rend les mêmes six compteurs pour le document et pour la sélection que le TXT de v1 (mots, caractères, caractères avec espaces, paragraphes, lignes, pages), calculés sur le modèle riche (`statistics::document` et les règles de `statistics::selection`) et sur la mise en page (`PageCache::line_count`, `lines_in_range`) ; contrat : [us-340-search.md](us-340-search.md). Lignes et pages restent provisoires tant que la mise en page n'est pas terminée (`layout_complete`), l'interface les affiche alors « — ». En-têtes, pieds, notes et zones de texte ne sont pas comptés, comme dans l'option par défaut.
