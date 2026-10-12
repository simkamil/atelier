# Référence DOC de test

Cinq petits documents Word 97–2003 (`text-accents.doc`, `underlines.doc`, `char-formatting.doc`, `para-formatting.doc`, `styles.doc`, environ 10 Kio chacun) produits par `tools/generate-doc-reference.py`.

**Provenance : LibreOffice, pas Microsoft Word.** Word n'était pas installé au moment de la création (l'automatisation COM `Word.Application` n'est pas disponible) ; le générateur écrit pour chaque document un ODT à plat (`.fodt`) à partir de la description unique `DOCS`, puis demande à LibreOffice de l'enregistrer au format « MS Word 97 » (`soffice --headless --convert-to "doc:MS Word 97"`). Le `.doc` est ensuite reconverti en texte par LibreOffice (`txt:Text (encoded):UTF8`), ce qui donne une seconde lecture indépendante enregistrée dans `expected.json` (`libreoffice_text_lines`). LibreOffice écrit `nFib` 0x0101 et le flux `1Table`.

Contenu :

- `text-accents.doc` : français accentué, guillemets, tiret, points de suspension, euro et marque commerciale (octets 0x80–0x9F du texte compressé), japonais, grec et émoji (pièce Unicode avec paires de substitution), une tabulation ; titre, sujet, auteur, dernier auteur, mots-clés, commentaires et dates dans `\x05SummaryInformation` (page de code UTF-8).
- `char-formatting.doc` : gras, italique, souligné simple et double, barré, exposant, indice, couleurs (indice de la table `Ico`, RVB), surlignage, tailles, polices Arial et Courier New, combinaison.
- `underlines.doc` : seize types de soulignement (simple, double, épais, pointillé, tirets, tirets longs, point-tiret, point-point-tiret, ondulé, double ondulé et variantes grasses) ; les valeurs `sprmCKul` écrites sont relevées par le lecteur Python (`kul_observed` dans `expected.json`) : 1, 3, 6, 4, 7, 0x27, 9, 0x0A, 0x0B, 0x14, 0x17, 0x37, 0x19, 0x1A, 0x1B, 0x2B.
- `para-formatting.doc` : quatre alignements, retraits gauche, droite et première ligne (négatif et positif), espacements avant et après, interligne multiple, exact et au moins.
- `styles.doc` : `Heading 1` et `Heading 2` (identifiants `Heading1`, `Heading2`), style personnalisé `Mon Style` (identifiant `MonStyle`), style de caractère `Emphase forte`, style intégré `List` hérité du `Body Text` de LibreOffice. LibreOffice écrit `istdBase` = 0x0FFF pour les styles définis par l'utilisateur : l'héritage de styles est donc vérifié par `crates/fmt-legacy/tests/doc_import.rs`, pas par ce corpus (hormis `List` → `Body Text` → `Normal`).

`expected.json` sert de manifeste : taille et SHA-256 de chaque fichier, résumé du FIB (`n_fib`, `ccp_text`, nombre de pièces, flux de tables), propriétés du document lues par le lecteur Python indépendant (les dates sont décalées par le fuseau horaire de la machine génératrice), texte de chaque paragraphe et propriétés attendues (alignement, retraits et espacements en twips, interligne, style, propriétés effectives du style, propriétés des plages de caractères). Ces valeurs sortent de la description du générateur, pas du code Rust du projet. Les valeurs de `List` viennent des styles internes de LibreOffice (115 % d'interligne, 7 pt après) et sont notées comme telles dans le générateur.

Régénérer (LibreOffice requis ; la variable `SOFFICE` désigne l'exécutable) puis valider :

```powershell
py tools/generate-doc-reference.py
py tools/generate-doc-reference.py --check
```

Le mode `--check` n'a pas besoin de LibreOffice : il compare taille et SHA-256 des `.doc` stockés, relit le conteneur composé, le FIB et la table des pièces avec le petit lecteur Python du générateur, décode le texte principal et le compare à la description et à `expected.json`, relit les propriétés du document et vérifie que les attendus enregistrés correspondent à la description. Les octets d'un `.doc` ne sont pas reproductibles (horodatages, identifiants LibreOffice) : le générateur n'est donc pas relancé à l'identique par `--check`.

Le test Rust `crates/fmt-legacy/tests/reference.rs` importe les quatre fichiers et compare le texte, les propriétés du document, les styles, les propriétés de paragraphe et les plages de caractères à `expected.json` (lu par un petit analyseur JSON du test).

Références primaires : [Microsoft Open Specifications, MS-DOC](https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-doc/ccd7b486-7881-484c-a137-51170af7cc22) et [MS-OLEPS](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-oleps/bf7aeae8-c47a-4939-9f45-700158dac3bc).
