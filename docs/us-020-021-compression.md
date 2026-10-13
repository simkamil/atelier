<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Compression DEFLATE et zlib (US-020/021)

`atelier-compress` fournit un cœur Rust sans dépendance externe et sans code
unsafe. Le décodeur prend en charge DEFLATE brut (RFC 1951) et zlib (RFC 1950).
Il accepte les blocs stockés, Huffman fixes et Huffman dynamiques, les distances
jusqu'à 32 Kio, les copies chevauchantes, et les arbres incomplets uniquement
dans les exceptions prévues par le format. Le mode zlib valide CMF/FLG, refuse
FDICT et vérifie Adler-32. CRC-32/ISO-HDLC est exposé comme primitive distincte
pour les consommateurs ZIP; ce n'est pas un checksum zlib.

## API et limites

- `InflateState::new(Format, max_output)` puis `push(input, output, finish)`;
  `Progress` rend les longueurs consommées/produites et `NeedInput`, `NeedOutput`
  ou `Done`. L'appelant conserve les tranches non consommées et rappelle
  `push`; un octet de sortie suffit pour progresser.
- Le décodeur conserve une fenêtre circulaire de 32 Kio, quelques centaines
  d'octets de tables et l'état du parseur. Il ne met en mémoire tampon ni le flux
  complet, ni la sortie complète. `finish=true` transforme une troncature en
  `UnexpectedEof`; une sortie au-delà de `max_output` retourne `OutputLimit`.
- Après une erreur, l'état est terminal et doit être abandonné. Les erreurs de
  format, de troncature et de plafond sont retournées comme `Result`, sans
  panique.
- `compress(input, level, output)` produit du DEFLATE brut. Pour choisir
  explicitement le framing, `compress_with_format(input, Format, level, output)`
  produit DEFLATE ou zlib; `compress_zlib` est un raccourci. Les niveaux Fast et
  Normal utilisent un LZ77 à chaînes de hachage et des arbres Huffman dynamiques;
  Fast examine au plus 32 candidats et Normal au plus 256. L'encodeur écrit
  directement dans le tampon fourni et rapporte la capacité totale requise en
  cas de manque; le préfixe qui tient dans le tampon peut alors être partiel et
  doit être ignoré.
- L'encodeur traite l'entrée complète et conserve une chaîne de positions et
  des tokens LZ77 : sa mémoire auxiliaire est O(n) en taille d'entrée. Seul le
  décodeur est incrémental avec une mémoire indépendante du volume du flux.
- Les fonctions publiques `adler32` et `crc32` permettent aux autres crates de
  calculer les sommes de contrôle correspondantes.

L'API est synchrone et pilotée par l'appelant: elle n'impose pas de `Read` ou
`Write`, de blocage, ni de buffer de sortie caché. gzip, dictionnaire zlib et
conteneur ZIP restent hors du périmètre.

## Vecteurs et méthode

Les vecteurs versionnés sous `crates/compress/tests/vectors/` ont été produits
avec le module `zlib` de la bibliothèque standard Python: bloc stocké, bloc fixe,
bloc dynamique, DEFLATE brut et zlib représentant une entrée vide, zlib avec
Adler-32 et une distance indépendante de 32768 octets après un bloc stocké. Les fixtures malformées
couvrent un arbre de longueurs de codes incomplet, un arbre littéral incomplet,
un arbre de distances incomplet et une distance demandée avant toute sortie.
Les tests les décodent en morceaux d'un octet avec une sortie d'un octet, puis
vérifient aussi les appels monolithiques. Ils couvrent troncature, checksum,
en-tête, distances, plafond et reprise avec buffers courts. Des entrées
pseudo-aléatoires reproductibles couvrent les longueurs 0, 1, 2, 3, 255–258,
32767–32769 et 65537, les deux niveaux et les deux framings.

Les fichiers du corpus sont des données transparentes et reproductibles dans
`crates/compress/tests/corpus/`. Le ratio est `taille DEFLATE brut / taille
source`; les octets de framing zlib et de checksum ne sont pas comptés.

| Entrée | Source (octets) | Fast (octets, ratio) | Normal (octets, ratio) |
|---|---:|---:|---:|
| `repetitive.txt` | 1440 | 92, 6,39 % | 92, 6,39 % |
| `mixed-utf8.txt` | 740 | 77, 10,41 % | 77, 10,41 % |
| `binary-repeat.bin` | 2048 | 330, 16,11 % | 330, 16,11 % |
| `incompressible.bin` | 4096 | 4137, 101,00 % | 4137, 101,00 % |

Ces mesures sont reproductibles en appelant `compress` sur les fixtures avec
`Level::Fast` et `Level::Normal`. Ce petit corpus donne des tailles identiques
aux deux niveaux; leur différence se trouve dans le budget de recherche LZ77,
et dépend donc du corpus.

Les 16 flux sous `tests/encoder-wire/` (quatre fichiers × deux niveaux × deux
framings) sont acceptés par les décompresseurs raw et zlib indépendants de
Python, avec restitution exacte de la source. Un test Rust compare la sortie
de l'encodeur à ces octets versionnés en CI ; il ne dépend pas du décodeur de
la crate pour détecter une régression du format.

## Vérification

```text
cargo test -p atelier-compress --locked --offline
cargo check -p atelier-compress --no-default-features --locked --offline
cargo tree -p atelier-compress --locked --offline
py -X utf8 _bmad-output/implementation-artifacts/check-compression-oracle.py
```

Le script Python est un outil de validation d'agent, sans dépendance produit
ou CI. Il compare les deux sens à `zlib`, teste des flux fragmentés et invalides,
et consigne les tailles du corpus ainsi que les empreintes des flux dans
`compression-oracle.json`. Une évolution volontaire de l'encodage peut régénérer
les références avec `--update-wire-fixtures` ; les fichiers ne sont écrits
qu'après réussite de cette validation indépendante.

La compression n'ajoute aucune dépendance. Le lot ajoute séparément la
dépendance locale `atelier-render → atelier-geom` dans `Cargo.lock`.

## Correctif : limitation des longueurs de code Huffman

`make_lengths` ramenait les codes à 15 bits (7 pour l'arbre des longueurs) en
reprenant le décompte de débordement de zlib, qui suppose que les ancêtres
d'une feuille trop profonde sont aussi tronqués. Ici les profondeurs étaient
vraies : dès qu'une feuille se trouvait à plus d'un niveau sous la limite, le
code devenait sur-souscrit (somme de Kraft supérieure à 1) et le flux, écrit
sans erreur, était rejeté par tout décodeur (`InvalidDeflate` côté ZIP, et
« invalid literal/lengths set » côté `zlib`). Cela survenait sur de grandes
entrées très répétitives (environ 2,5 Mo de XML WordprocessingML), dont les
fréquences de symboles sont très inégales. La somme de Kraft est désormais
mesurée à partir des effectifs par longueur, puis réparée (allongement des
codes les plus profonds possibles, puis raccourcissement pour rester complet).
Tests : `crates/compress/tests/large_inputs.rs`, `crates/zip/tests/large_entries.rs`.
