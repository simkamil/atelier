# Références de format — ZIP et XML (US-022/023, US-030/031)

Ces notes résument les points normatifs utiles aux implémenteurs. Le manifeste des fixtures est `tests/format-reference/manifest.json`; le générateur agent-only est `_bmad-output/implementation-artifacts/generate-format-references.py`. Regénération déterministe avec Python standard : `py _bmad-output/implementation-artifacts/generate-format-references.py`. Les crates Rust et la CI consomment seulement les fichiers déjà versionnés, sans Python ni dépendance ajoutée.

## ZIP — PKWARE APPNOTE

Référence primaire : [PKWARE, .ZIP File Format Specification (APPNOTE.TXT)](https://www.pkware.com/documents/casestudies/APPNOTE.TXT), publication APPNOTE courante. Sections utiles : 4.3 (structures locales, répertoire central et EOCD), 4.4 (champs des entrées centrales), 4.3.9 (data descriptor), 4.5 (extensions et extra fields, dont ZIP64), 4.4.4 (bit 11 UTF-8), 4.4.17 (méthodes de compression). Le fichier ZIP place les entêtes locaux et les payloads avant le répertoire central puis l’EOCD; la recherche d’EOCD doit respecter sa structure et son commentaire terminal, et les offsets/tailles doivent être bornés à l’archive. La méthode 0 stocke les octets; la méthode 8 transporte un flux DEFLATE brut (RFC 1951), sans framing zlib. Le CRC des données est CRC-32/ISO-HDLC (polynôme réfléchi standard), à comparer après décompression avec tailles et CRC annoncés.

Le bit général 3 signale que CRC/tailles peuvent être fournis après le flux dans un data descriptor. La signature de ce descripteur est optionnelle; déterminer les champs par les valeurs fiables du central directory et les offsets, pas en cherchant une signature arbitraire dans le payload. ZIP64 étend les champs débordés par sentinelles `0xffffffff`/`0xffff` via extra field ID `0x0001` et EOCD64/locator; le cas « forcé » de la fixture vérifie les structures sur petite archive. Le bit 11 indique des noms/commentaires UTF-8; quand il est absent, CP437 est l’encodage historique de base retenu par la story. Les chemins sûrs et limites d’extraction sont une politique de l’application, pas une garantie du format : signaler traversal/absolus et ne jamais extraire implicitement.

Fixtures ZIP sous `tests/format-reference/zip/` : classique Stored/Deflate, entrée vide, UTF-8, CP437 octet `0x82`, ZIP64 avec petites tailles, descripteurs signés/non signés, traversal, CRC altéré, queue tronquée et taille déclarée excessive. Le manifeste associe noms/contenus/méthodes/CRC ou catégorie d’erreur attendue à chaque fichier. Les archives `valid-*` ont été relues avec `zipfile` et leurs CRC ont été vérifiés via `testzip()`.

## XML 1.0 et Namespaces 1.0

Références primaires : [W3C XML 1.0, Fifth Edition](https://www.w3.org/TR/xml/) et [W3C Namespaces in XML 1.0, Third Edition](https://www.w3.org/TR/xml-names/). Sections XML utiles : 2.2–2.4 pour caractères, fins de ligne, valeurs et références; 2.8 pour prologue/DOCTYPE; 2.9–2.11 pour encodage, déclarations et normalisation; 3.1–3.3 pour éléments/attributs; 4.1–4.6 pour entités; 6 pour la grammaire. L’encodage doit respecter BOM et déclaration; UTF-8/UTF-16 sont requis par le périmètre. Rejeter octets/surrogates mal formés et caractères hors production `Char` en mode strict. Les références numériques doivent désigner des caractères XML permis. Les cinq entités prédéfinies (`amp`, `lt`, `gt`, `apos`, `quot`) sont connues; une entité déclarée en DTD ne doit pas entraîner expansion dans cette implémentation.

Namespaces : le nom étendu est le couple URI de namespace + nom local. Les déclarations sont limitées à leur portée et une déclaration interne remplace le binding hérité. Le namespace par défaut s’applique aux éléments sans préfixe, jamais aux attributs sans préfixe; `xmlns=""` retire le namespace par défaut. Après résolution, deux attributs de même couple (URI, local) sont dupliqués même si leurs préfixes diffèrent. Les préfixes réservés `xml`/`xmlns` ont les bindings imposés par la recommandation.

Les commentaires, instructions de traitement et CDATA ont une syntaxe dédiée; CDATA fournit du texte littéral (les références n’y sont pas interprétées). Un flux d’événements peut représenter CDATA comme texte si cette équivalence est documentée. Le DOCTYPE ne justifie ni résolution externe ni expansion récursive; accepter ou ignorer une déclaration sans l’exécuter reste une décision du parseur, mais les événements et diagnostics doivent préserver le contrat de sécurité de la story.

Fixtures XML sous `tests/format-reference/xml/` couvrent UTF-8 multioctet/références, UTF-16LE/BE BOM, portée/shadowing/reset des namespaces, attribut sans préfixe, PI/commentaire/CDATA/DOCTYPE bénin, et erreurs strictes (mismatch, entité inconnue, caractère interdit, UTF-8 tronqué, préfixe non lié, attributs dupliqués par nom étendu). `doctype-no-expansion.xml` est un cas hostile : ne pas développer l’entité récursive. Les cas valides XML sont parseables avec ElementTree sauf celui qui vérifie des distinctions lexicales (PI/comment/CDATA/DOCTYPE), que l’arbre standard ne conserve pas toutes. Les sémantiques détaillées et SHA-256 sont dans le manifeste; les erreurs sont des catégories, car les enums et diagnostics Rust peuvent différer.

## Résultats de l’oracle indépendant (2026-10-10)

Pilote réexécutable depuis la racine Windows : `py _bmad-output/implementation-artifacts/check-format-oracle.py`. Il compile les crates XML et ZIP en mode offline/locked, compile le petit driver Rust de ce dossier, crée ses ZIP de test dans un répertoire temporaire puis compare la vraie crate avec `zipfile` et ElementTree. Aucun générateur de fixtures ni aucune dépendance Python n’est nécessaire au produit ou à la CI. Les compteurs et détails sont conservés dans `format-oracle.json`.

Dernière exécution sur sources figées : réussite sans écart (`failures: []`). XML : 105 lectures fragmentées et 105 round-trips concordants, 20 cas strictement rejetés. Deux cas valides vérifient prologue + DOCTYPE interne et DOCTYPE externe sans accès distant; un cas de commentaire et PI contient CRLF et CR. ZIP : 52 archives Python lues par la crate, 48 archives crate relues et validées avec `zipfile.testzip()`. Quatre entrées issues d’un flux Python non seekable ont traversé la crate avec bit 3 : descripteur signé 32 bits (16 octets), non signé 32 bits (12 octets), signé 64 bits (24 octets) et non signé 64 bits (20 octets) après `force_zip64=True`. La mutation d’un payload Stored produit `CrcMismatch` pour `bad.bin`, alors que `healthy.txt` reste lisible. Une archive dont le central directory est tronqué est récupérée depuis l’en-tête local avec `is_complete=false`, contenu intact.




Empreintes du contrôle final (les sources n’ont pas changé pendant l’exécution; le pilote vérifie ce garde-fou et inscrit les empreintes dans le JSON) :

- `crates/xml/src/lib.rs`: `bc580ded78c0f549c2c654670584c5e77f9128057aea666fb98a4f6499dbbe92`
- `crates/zip/src/lib.rs`: `8e2d404d3988a70ee4152ea3b39c7a9ae7f9257c0e8c3646461f9811d47a6d4a`
- `_bmad-output/implementation-artifacts/format-oracle.rs`: `5dd19e863526ab4a01b79e441889baeb99bec01bfc9a544d3a46547aaf44f606`
- `_bmad-output/implementation-artifacts/check-format-oracle.py`: `6b9c3cdebe814d5aedd58b02a840bf01113b6e790ffce4080acac874fc95b46e`

Pour Explorer, `explorer-zip.json` confirme les mêmes empreintes de crate ZIP et driver Rust; l’exécutable oracle a le SHA-256 `db41783a1bb1cd504d6dbed0dcf2790bc540463677c90c373b407ffed6fdd165`. Le handler COM `Shell.Application` a extrait deux membres UTF-8 générés par la crate pour ZIP classique et `force_zip64`; les noms `readme.txt`/`été.txt` et octets extraits concordent. Sous Windows PowerShell 5, le script nécessite un BOM UTF-8 pour préserver les noms accentués; il est inclus dans le fichier.

## Golden outputs du Writer pour CI Rust

`tests/format-reference/zip/writer-golden-classic.zip` et `writer-golden-zip64.zip` sont des octets immuables capturés du vrai `atelier_zip::Writer`, après `cargo build -p atelier-zip --locked --offline` puis exécution du pilote `format-oracle.rs` (mode `zip-write`). Les options sont `force_zip64=false/true`, `timestamp=None` (date DOS par défaut), niveau `Normal`, avec deux entrées dans cet ordre : `café.txt` en Store (méthode 0, contenu UTF-8 `stored golden: café\n`) et `東京/🙂.xml` en Deflate (méthode 8, contenu UTF-8 `<r>zip golden 🙂</r>`). Les détails de ces arguments et méthodes sont aussi portés par `writer_options` dans le manifeste.

Une lecture indépendante Python `zipfile` a vérifié les deux archives (`testzip() is None`), l’ordre et les noms décodés, chaque payload octet pour octet, et CRC32 par entrée. Le générateur contient les sorties prévalidées en Base64 et les régénère sans compresseur ni dépendance additionnelle. Sa vérification ciblée a régénéré deux fois les fixtures : les 23 fichiers préexistants sont restés identiques à leurs SHA-256 précédents, les deux goldens reproduisent leurs empreintes ci-dessous, et le manifeste reste identique entre les deux passes.

- `writer-golden-classic.zip`: SHA-256 `02803af7522f6644dfc7d196cf1dab2e8476ed6e693aef01e74c95f85e7ba00a`
- `writer-golden-zip64.zip`: SHA-256 `e59c2fe18777f75ef4dbfd3f8f7bea94e516bb48aaf47d9b891cf01082ce10f3`
- `atelier-zip` source au moment de capture: SHA-256 `c75c8afa099be53540f36354cba44fab9934010ec2314d438bb510f935456198`
- Pilote Rust: SHA-256 `5dd19e863526ab4a01b79e441889baeb99bec01bfc9a544d3a46547aaf44f606`; exécutable utilisé: SHA-256 `76916a94e0467d9457856fea1136bac65ded958c61ee89058953175dff264a77`


## Contrôle final après revue

Le coordinateur a confirmé que son test Rust `writer_matches_independently_validated_wire_archives` passe pour les deux goldens. Contrôle final demandé : `check-format-oracle.py` réussit (`failures: []`, XML 105/105 round-trips, ZIP 52 lectures et 48 écritures); `check-explorer-zip.ps1` réussit pour classic et forced ZIP64. Une vérification Python `zipfile` séparée confirme à nouveau les deux CRC, noms, contenus et empreintes dorées. Les sources exactes et l’exécutable du contrôle final sont enregistrés dans `format-oracle.json` et `explorer-zip.json`; les SHA des crates sont XML `bc580ded78c0f549c2c654670584c5e77f9128057aea666fb98a4f6499dbbe92` et ZIP `8e2d404d3988a70ee4152ea3b39c7a9ae7f9257c0e8c3646461f9811d47a6d4a`. La correction ultérieure d’un attendu de namespace dans le manifeste ne modifie aucun octet de fixture et n’affecte pas ces vérifications des golden ZIP.
