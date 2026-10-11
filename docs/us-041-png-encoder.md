# US-041 — Encodeur PNG

`atelier-image::encode_png` encode des pixels 8 bits orientés de haut en bas en PNG non entrelacé. `PngColor::Gray8` (1 octet par pixel, type couleur 0) et `PngColor::Rgba8` (4 octets, alpha droite, type 6) sont acceptés ; `encode_png_rgba` encode directement un `Image` du décodeur.

```rust
pub fn encode_png(width: u32, height: u32, color: PngColor, pixels: &[u8], limits: DecodeLimits)
    -> Result<Vec<u8>, ImageError>;
pub fn encode_png_rgba(image: &Image, limits: DecodeLimits) -> Result<Vec<u8>, ImageError>;
```

Chaque ligne reçoit le filtre (None, Sub, Up, Average, Paeth) dont la somme des valeurs absolues des octets filtrés, lus comme signés, est minimale ; à égalité, le type le plus petit gagne. Le flux zlib vient d’`atelier-compress` (`compress_zlib`, niveau normal), découpé en chunks IDAT de 64 Kio ; les CRC sont calculés par `crc32`. Aucun chunk auxiliaire n’est écrit et la sortie est déterministe : mêmes entrées, mêmes octets.

## Limites et erreurs

L’encodeur réutilise `DecodeLimits` et `ImageError` : le PNG produit est ainsi décodable avec les mêmes plafonds. Une dimension nulle ou supérieure à 2^31-1 ou un tampon de longueur inexacte donnent `InvalidData` ; un nombre de pixels supérieur à `max_pixels`, un tampon ou un PNG résultant supérieur à `max_input_bytes` donnent `LimitExceeded`. Les dimensions, la longueur du tampon et le nombre de pixels sont vérifiés avant les allocations. La taille du PNG résultant est vérifiée après compression ; cette limite borne la sortie retournée, sans garantir un plafond équivalent pour la mémoire temporaire utilisée pendant l’encodage. Les calculs de taille utilisent des opérations vérifiées. Les niveaux de gris sont décodés en RGBA (gris répliqué, alpha 255). Hors périmètre : 16 bits, palette, entrelacement, chunks de métadonnées.

## Vérification

Les tests (`crates/image/tests/png_encode.rs` et tests unitaires du module) couvrent : aller-retour avec `decode_png` pour 1×1, lignes impaires et larges, images pseudo-aléatoires à graine fixe (xorshift maison), dégradés, transparence et pixels transparents conservés ; chacun des cinq filtres forcé puis décodé, et choix adaptatif de score minimal avec départage vers le plus petit type ; CRC et structure des chunks, plusieurs IDAT ; déterminisme ; dimensions nulles, tampons faux, dépassements de limites. Les 162 PNG valides du [corpus](../tests/image-reference/README.md) sont décodés, ré-encodés (en gris aussi lorsque l’image est grise opaque) et redécodés à l’identique.

```powershell
cargo test -p atelier-image --locked --offline
cargo check -p atelier-image --target wasm32-unknown-unknown --locked --offline
```
