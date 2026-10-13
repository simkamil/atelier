<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-315 — Détection des documents protégés par mot de passe

`atelier-fmt-legacy::detect::detect_protection(&[u8]) -> Detection` classe un fichier sans
l'importer ni le déchiffrer, pour que l'application affiche un message clair au lieu d'une erreur
technique. Partie bibliothèque seulement : le message et son branchement relèvent de l'intégrateur.
Aucune dépendance ajoutée, `#![forbid(unsafe_code)]`, aucune API système (WASM).

```rust,ignore
use atelier_fmt_legacy::detect::{detect_protection, Family, Protection};

let found = detect_protection(&bytes);
if found.is_protected() { /* afficher le message ; found.family et found.protection précisent */ }
```

## Types (tous `#[non_exhaustive]` sauf la structure `Detection { family, protection }`)

- `Family` : `EncryptedOoxmlPackage` (docx, xlsx ou pptx : indiscernable sans la clé), `Word97`,
  `Excel97`, `PowerPoint97`, `OtherCompound`, `Unknown` (ZIP, texte, vide, tronqué, CFB abîmé).
- `Protection` : `None`, `Encrypted(Encryption)`, `Obfuscated` (XOR), `RightsManaged` (DRM),
  `EncryptedUnknown`, `Undetermined` (non classé : jamais une erreur).
- `Encryption` : `Standard` (EncryptionInfo 2.2, 3.2, 4.2), `Agile` (4.4), `Extensible` (3.3, 4.3),
  `Rc4`, `Rc4CryptoApi` (Excel), `Unknown`.
- `Detection::is_protected()` : vrai pour `Encrypted`, `Obfuscated`, `RightsManaged`, `EncryptedUnknown`.

## Règles et niveau de preuve

| Cas | Règle | Preuve |
|---|---|---|
| CFB avec flux `EncryptionInfo` et `EncryptedPackage` | variante lue dans la version (4 octets) | **fichiers LibreOffice** (`encrypted.docx`, `encrypted.xlsx`, chiffrement standard) ; agile, extensible, inconnue : fixtures construites |
| `WordDocument` | drapeaux du FIB via `Fib::parse`, le code de `import_doc` | **fichier LibreOffice** `encrypted.doc` (et `import_doc` renvoie bien `PasswordProtected`) ; obscurci XOR : fixture |
| `Workbook`/`Book` | enregistrement `FILEPASS` parmi les premiers enregistrements (XOR, RC4, RC4 CryptoAPI) | **fichier LibreOffice** `encrypted.xls` (protégé ; variante fixée par le test) ; les trois variantes : fixtures |
| `PowerPoint Document` | jeton de `Current User` (0xF3D1C4DF = chiffré) | fixture seulement |
| `\x06DataSpaces` avec transformation `DRM…` ou flux `DRMContent` | `RightsManaged` | fixture seulement, noms tirés de [MS-OFFCRYPTO] de mémoire |
| reste | `Unknown`/`Undetermined` (un seul des deux flux, flux vide ou tronqué, CFB sans marque : `OtherCompound`) | fixtures |

Les noms de flux se comparent sans tenir compte de la casse. Les documents LibreOffice non chiffrés
(`plain.*`) ne sont pas signalés. Aucun fichier produit par Microsoft Word n'a été testé. Détails du
corpus : [tests/encrypted-reference/README.md](../tests/encrypted-reference/README.md) (mot de passe `test`).

## Limites

- Ouverture CFB avec limites serrées (4096 entrées, profondeur 8, 16 Kio lus au plus ; 512 octets
  par flux). Le coût dépend seulement du lecteur CFB (table d'allocation) : mesuré à environ
  0,02 ms pour un petit conteneur et 5 ms pour 96 Mio, en profil de test non optimisé.
- Un ZIP (docx/xlsx non chiffré, ODT) n'est jamais signalé : l'ODT se détecte par
  `OdtError::PasswordProtected` dans `atelier-fmt-odf`.
- Le mot de passe d'ouverture seul est détecté ; les protections « en écriture » ou de structure ne le sont pas.

## Pour l'intégrateur

- Clé de catalogue proposée : `encrypted-document` ; texte : « Les documents protégés par mot de passe seront pris en charge dans une prochaine version. » (`web/i18n/fr-FR.txt` non modifié).
- `import_docx` sur un docx chiffré renvoie `Err(DocxError::Opc(OpcError::Zip("ZIP archive contains no usable entries")))` : le message technique actuel. `DocxError` n'est pas `#[non_exhaustive]` : ajouter une variante casserait les `match` exhaustifs des appelants. Le plus simple est d'appeler `detect_protection` avant `import_docx` (et `import_xlsx` plus tard) sur les octets lus, et d'afficher le message si `is_protected()`.
- `.doc` : `import_doc` renvoie déjà `DocError::PasswordProtected` ; la détection peut servir de chemin commun.
- `.gitattributes` : `tests/encrypted-reference/.gitattributes` marque les fichiers comme binaires ; ajouter au fichier racine `*.xls binary` et `*.xlsx binary` (absents).
- Fuzz : cible possible `detect_protection(data)` (aucun résultat à vérifier, seulement l'absence de panique).
