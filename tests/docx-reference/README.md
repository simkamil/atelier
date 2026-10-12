# Référence DOCX de test

Les entrées ZIP utilisent le mode stocké, avec métadonnées fixes : la reproduction binaire ne dépend pas de la version de zlib de l’hôte.

`basic.docx` est un petit paquet OOXML synthétique, construit par `tools/generate-docx-reference.py` avec la bibliothèque standard Python. Son document principal et ses styles utilisent des noms de parties relocalisés (`content/main.xml`, `content/styles.xml`) afin que les lecteurs découvrent les parties par les relations OPC plutôt qu’en supposant `word/document.xml`.

La fixture contient deux paragraphes français/anglais. Le premier utilise le style de paragraphe `Heading1` et un run `Strong` dont le gras direct est explicitement désactivé. Le second est assez long pour être coupé sur plusieurs lignes par un moteur de mise en page. Les styles définissent une taille par défaut de 22 demi-points avec `minorHAnsi`, `Normal` comme style de paragraphe par défaut, `Heading1` hérité de `Normal` en 32 demi-points et couleur `accent1`, et le style de caractère gras `Strong`. Le thème définit `Carlito` comme police latine mineure et `Liberation Serif` comme police latine majeure.

Le paquet ajoute `settings.xml` avec taquet par défaut de 720 twips, un `compatSetting`, un indicateur de compatibilité et un style latent. `fontTable.xml` référence le vrai `Carlito-Regular.ttf` du dépôt, obfusqué dans une partie `.odttf` avec la clé `{00112233-4455-6677-8899-AABBCCDDEEFF}`. Les 32 premiers octets sont XORés avec les octets de la clé GUID inversés, répétés deux fois. `expected.json` enregistre les SHA-256 de la police originale et obfusquée, le vecteur des 32 premiers octets, le texte et les propriétés de style attendus, ainsi que le résultat d’une nouvelle lecture XML/relations. Le paquet contient aussi une partie opaque qui permet de vérifier sa conservation.

Régénérer puis valider les octets exacts et le reparsing indépendant :

```powershell
py tools/generate-docx-reference.py
py tools/generate-docx-reference.py --check
```

Le mode `--check` reconstruit les octets en mémoire, les compare byte à byte avec `basic.docx` et `expected.json`, puis ouvre le DOCX comme ZIP et résout les relations à partir de leurs parties sources. Il reparcourt ensuite les XML pour vérifier les textes, identifiants de styles, propriétés directes, parties relocalisées, paramètres, clé et données de police. Il ne fait appel ni au code Rust du projet, ni à Word, ni à un moteur de rendu.

Références primaires : [ECMA-376, Part 1, Fundamentals and Markup Language Reference](https://ecma-international.org/publications-and-standards/standards/ecma-376/) (§17.8.1, Font Embedding) et [Microsoft Open Specifications, Font Part](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/ea097c57-5794-4624-b08e-017b47051b1d). La police Carlito provient de `data/fonts/carlito/` et sa licence est dans `data/fonts/licenses/Carlito-OFL.txt`.
