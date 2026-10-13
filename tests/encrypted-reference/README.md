# Référence de documents chiffrés (US-315)

Huit petits fichiers produits par **LibreOffice** (pas par Microsoft Word, non installé) à partir du même contenu : deux lignes de texte pour Word, trois cellules par deux lignes pour Excel. Les quatre fichiers `encrypted.*` sont enregistrés avec le mot de passe de test **`test`** ; les quatre `plain.*` sont les mêmes documents sans mot de passe.

| Fichier | Filtre LibreOffice | Contenu |
|---|---|---|
| `encrypted.docx`, `encrypted.xlsx` | `MS Word 2007 XML`, `Calc MS Excel 2007 XML` | conteneur CFB `EncryptionInfo` + `EncryptedPackage` (chiffrement standard) |
| `encrypted.doc` | `MS Word 97` | FIB avec `fEncrypted` |
| `encrypted.xls` | `MS Excel 97` | flux `Workbook` avec enregistrement `FILEPASS` |
| `plain.docx`, `plain.xlsx` | idem | paquet ZIP ordinaire |
| `plain.doc`, `plain.xls` | idem | conteneur CFB ordinaire |

Production : une instance `soffice --headless` pilotée par le Python fourni avec LibreOffice (UNO), `storeToURL` avec le filtre ci-dessus et la propriété de support `Password` = `test`. (Le paramètre de filtre JSON de `--convert-to` n'a pas chiffré les fichiers.) Les octets ne sont pas reproductibles (horodatages, sels aléatoires) : les sommes de contrôle de `SHA256SUMS` figent les fichiers stockés.

Utilisés par `crates/fmt-legacy/tests/detect.rs`. Détails : [docs/us-315-encrypted-detection.md](../../docs/us-315-encrypted-detection.md).
