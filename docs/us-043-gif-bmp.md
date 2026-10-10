# US-043 — Décodeurs GIF et BMP

Le crate `atelier-image` expose `decode_gif(data, limits)` et `decode_bmp(data, limits)` qui retournent `Image` : pixels RGBA8 droits, ordre des rangées de haut en bas. Les décodeurs n’effectuent pas de conversion de profil ou de gamma. Ils refusent les entrées dépassant `DecodeLimits`, défaut de 16 777 216 pixels et 64 MiB de données, avec `ImageError`; aucune entrée malformée ne doit provoquer de panic.

## GIF

GIF87a et GIF89a sont acceptés. Le décodeur ignore les extensions non graphiques, applique le Graphic Control Extension précédant la première image, puis décode uniquement le premier descripteur d’image. L’extension Plain Text est ignorée après validation de son bloc d’en-tête obligatoire de 12 octets et consomme tout GCE en attente; les commentaires et extensions Application ne consomment pas ce GCE. Il utilise la palette locale, sinon la palette globale; un index qui sort de la palette est invalide. L’image est posée à son offset dans l’écran logique. Une image nulle ou qui dépasse cet écran est rejetée.

Le canevas est initialisé avec la couleur de fond de l’écran (palette globale) lorsqu’aucune transparence n’est indiquée par le GCE; sinon il reste transparent. L’index transparent garde l’alpha à zéro. Les images entrelacées sont replacées selon les quatre passes GIF. LZW doit commencer par Clear, contenir End et produire exactement largeur × hauteur indices. La taille du dictionnaire reste limitée à 4096 codes et l’expansion au nombre de pixels de l’image.

## BMP

Le parseur prend en charge les en-têtes DIB Windows 40, 52, 56, 108 et 124 octets, les profondeurs 1, 4, 8, 16, 24 et 32 bits, et les compressions BI_RGB, BI_BITFIELDS, BI_RLE4 et BI_RLE8 lorsque profondeur et compression sont compatibles. Les lignes non compressées peuvent être bottom-up ou top-down; RLE doit être bottom-up. Les headers OS/2, autres modes, compressions incompatibles, dimensions invalides, palettes/masques malformés et coordonnées RLE hors image sont rejetés.

Les pixels indexés utilisent la table de couleurs et les octets de rangée alignés sur 4 octets. BI_RGB16 est interprété en RGB555, BI_RGB24 en BGR, et BI_RGB32 en BGRX avec alpha 255. BITFIELDS utilise des masques RGB contigus et disjoints; le masque alpha non nul présent dans un header assez grand est honoré. RLE4/RLE8 prennent en charge runs, EOL, EOB, delta, séquences absolues et bourrage à l’octet requis; les pixels non peints utilisent l’entrée de palette 0. Un flux RLE incomplet sans EOB ou dont `biSizeImage` dépasse les données disponibles est tronqué.

Les erreurs distinguent troncature, données incohérentes, mode non pris en charge et dépassement de limite. Les consommateurs de formats de document doivent conserver les octets originaux si l’image source doit être préservée; `Image` ne transporte que les pixels décodés.
