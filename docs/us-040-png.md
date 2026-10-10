# US-040 — Décodeur PNG

`atelier-image::decode_png` décode les PNG statiques en pixels RGBA8 orientés de haut en bas, avec alpha droite. Les valeurs 1/2/4/8/16 bits sont étendues par mise à l’échelle entière arrondie au plus proche; les échantillons 16 bits utilisent leur précision source pour `tRNS` avant conversion. Les profils ICC, chunks gamma et métadonnées de rendu ne modifient pas les pixels.

Le décodeur couvre les types couleur PNG 0, 2, 3, 4 et 6 avec leurs profondeurs permises, les cinq filtres de ligne et l’entrelacement Adam7. Les indices de palette et les chunks de transparence sont validés. Chaque chunk, y compris les chunks auxiliaires ignorés, doit avoir un CRC valide. Les chunks critiques inconnus, les chunks mal ordonnés, les méthodes inconnues, les données suivant IEND et les octets supplémentaires après le flux zlib sont rejetés.

Les plafonds configurables par `DecodeLimits` s’appliquent à la taille totale d’entrée et au nombre de pixels (défauts : 64 MiB et 16 777 216 pixels). Les longueurs de lignes, passes, buffers et coordonnées sont calculées avec contrôles d’overflow. Le flux zlib doit produire exactement la taille des scanlines calculée, vérifier son Adler-32 et consommer la totalité des données IDAT. Les erreurs reviennent sous `ImageError`; aucun résultat partiel n’est exposé.

## Vérification

Les tests Rust construisent leurs PNG en mémoire et couvrent les combinaisons de modes/profondeurs, `tRNS`, filtres, Adam7, CRC, ordre des chunks, limites et données malformées.

```powershell
cargo test -p atelier-image --locked --offline
cargo check -p atelier-image --target wasm32-unknown-unknown --locked --offline
```

Le [corpus versionné](../tests/image-reference/README.md) est vérifié par le test Cargo en CI : les 162 PNG valides de PngSuite correspondent exactement aux pixels de référence PyPNG et les 14 invalides sont rejetés. Les licences accompagnent le corpus; les oracles Python demeurent des outils d’agent, sans dépendance produit.
