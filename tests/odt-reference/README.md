# Référence ODT de test

Ces fichiers sont de **vrais documents LibreOffice** : `tools/generate-odt-reference.py` écrit de petites sources ODT « à plat » (`.fodt`, XML écrit à la main, lisible dans le script), demande à LibreOffice (`soffice --headless --convert-to odt:writer8`) de les enregistrer, puis lui fait rendre chaque paquet obtenu en PDF pour en relever le **nombre de pages**. Le producteur et sa version sont consignés dans `manifest.json` (LibreOffice 26.8 sous Windows pour le lot actuel). Les octets produits par LibreOffice embarquent des dates et un identifiant de génération : ils ne sont pas reproductibles, ce sont donc les fichiers versionnés, épinglés par SHA-256, qui font référence.

| Fichier | Ce qu'il couvre |
| --- | --- |
| `text-accents.odt` | texte accentué et non latin, espaces répétées, tabulation, saut de ligne, métadonnées (titre, auteur, mots-clés, sujet) |
| `char-format.odt` | gras, italique, soulignement, barré, exposant/indice, couleur, surlignage, police et taille, petites capitales, interlettrage |
| `para-format.odt` | alignement (dont `end`), retraits, espacements, interligne 150 %, tabulations, enchaînements, saut de page |
| `headings-styles.odt` | titre, sous-titre, titres de niveau 1 à 3, style personnalisé hérité de « Corps de texte », style de caractère |
| `lists.odt` | liste à puces sur trois niveaux, liste numérotée avec sous-niveau et valeur de départ |
| `table-merge.odt` / `table-merge.fodt` | tableau avec en-tête fusionné, fusion verticale, lignes d'en-tête, colonnes répétées ; `.fodt` est la version « à plat » écrite par LibreOffice lui-même |
| `nested-table.odt` | tableau dans une cellule de tableau |
| `image.odt` | image dans le texte (texte alternatif) et image ancrée au paragraphe |
| `header-footer.odt` | en-tête et pied de page avec numéro de page et nombre de pages, trois pages |
| `links-bookmarks.odt` | lien externe, lien interne, signet ponctuel et signet étendu |
| `fields-notes.odt` | champs date et heure, note de bas de page, autre champ |
| `sections-pages.odt` | changements de page maîtresse (portrait, paysage, portrait) et section de texte |
| `multi-page.odt` | long document de six pages |

`expected.json` donne, pour chaque fichier, les valeurs relevées **indépendamment** du code Rust (lecteur Python de la bibliothèque standard, règles de blancs ODF réécrites) : texte de chaque paragraphe dans l'ordre du document, titres, nombres de tableaux, de cellules de liste, d'images, liens, signets, pages maîtresses, métadonnées, et `pageCount` (nombre de pages rendu par LibreOffice, contrôle que l'import actuel ne peut pas encore vérifier : voir `docs/us-310-odt-import.md`).

```powershell
py tools/generate-odt-reference.py          # régénère le lot (LibreOffice requis)
py tools/generate-odt-reference.py --check  # SHA-256 + relecture indépendante, sans LibreOffice
cargo test -p atelier-fmt-odf --locked --offline --test odt_reference
```

`--check` compare les empreintes et la taille de `manifest.json` puis relit chaque paquet avec `zipfile` et `xml.etree` pour retrouver les valeurs de `expected.json`. Le test Rust `odt_reference` importe les mêmes fichiers avec `atelier_fmt_odf::odt::import_odt` et compare texte, titres, tableaux, images, liens, signets, sections et métadonnées aux attendus.

Le corpus reste volontairement petit (environ 250 Kio). Les images sont des dégradés PNG synthétiques fabriqués par le script ; aucun contenu tiers n'est embarqué.
