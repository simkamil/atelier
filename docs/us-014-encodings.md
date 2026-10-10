# US-014 — Encodages de texte

`atelier-unicode` fournit une conversion de texte portable, sans dépendance externe ni code unsafe.

## API

- `Encoding` distingue UTF-8, UTF-16 LE, UTF-16 BE, Windows-1252, ISO-8859-1, CP437 et MacRoman.
- `detect_encoding(&[u8])` choisit d'abord selon un BOM UTF-8/UTF-16, puis UTF-8 si l'entrée entière est valide, puis Windows-1252.
- `decode(&[u8], Encoding)` retourne une `String`. Les séquences invalides sont remplacées par U+FFFD. Un BOM initial est retiré seulement s'il correspond à l'encodage fourni explicitement; un BOM interne est conservé. UTF-16 sans BOM se décode quand même si l'appelant a choisi son ordre d'octets.
- `encode(&str, Encoding)` n'ajoute pas de BOM. Les encodages mono-octet retournent `EncodeError` à la première valeur non représentable; l'erreur expose le caractère, l'encodage et son offset en octets UTF-8.

ISO-8859-1 mappe directement chaque octet vers U+0000–U+00FF. Windows-1252 mappe les octets indéfinis 81, 8D, 8F, 90 et 9D vers U+FFFD au décodage; ils ne sont pas encodables. Pour CP437, les contrôles C0 et DEL sont conservés comme caractères de contrôle, conformément à l'usage des noms ZIP historiques.

## Provenance des mappings

Les tables de 256 entrées de `src/tables.rs` sont dérivées des tables de référence suivantes :

- [Unicode, Windows-1252 (table 2.01)](https://www.unicode.org/Public/MAPPINGS/VENDORS/MICSFT/WINDOWS/CP1252.TXT)
- [Unicode, CP437 (table 2.00)](https://www.unicode.org/Public/MAPPINGS/VENDORS/MICSFT/PC/CP437.TXT)
- [Apple, Mac OS Roman](https://www.unicode.org/Public/MAPPINGS/VENDORS/APPLE/ROMAN.TXT)

Les valeurs non listées dans ROMAN.TXT pour les contrôles C0 et DEL restent identiques à l'octet. La table ISO-8859-1 suit la définition directe de cet encodage. Le décodeur utilise les mêmes tables que l'encodeur, avec recherche du premier octet correspondant.

Les tests protègent aussi l'ordre exact des 256 entrées avec une empreinte FNV-1a 64 bits. Le calcul part de `0xCBF29CE484222325`, traite chaque scalaire comme quatre octets little-endian, et utilise le multiplicateur `0x00000100000001B3`. Les empreintes proviennent des tables de référence ci-dessus (les valeurs Windows-1252 non définies sont `0xFFFFFFFF`; les contrôles omis par ROMAN.TXT gardent leur valeur d'octet) : Windows-1252 `9AE14D098B17B49E`, CP437 `877036DB04EE6E19`, MacRoman `019A3AD4A72D2BAD`, ISO-8859-1 `8084B7F6C938AF25`. Les tests restent entièrement hors ligne.

## Vérification reproductible

Depuis la racine du workspace :

```powershell
cargo test -p atelier-unicode --locked --offline
cargo clippy -p atelier-unicode --all-targets --locked --offline -- -D warnings
cargo fmt -p atelier-unicode
```

Les tests couvrent la priorité du BOM, les deux ordres UTF-16 et leurs erreurs de séquence, la détection UTF-8/fallback, les BOM internes, les 256 octets des quatre encodages mono-octet, les aller-retours des valeurs définies et les erreurs d'encodage avec offset. Ils n'effectuent aucun téléchargement; aucune validation distante n'est revendiquée.
