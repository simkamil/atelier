# US-042 — JPEG decoder

`atelier-image` exposes `decode_jpeg(data, limits) -> Result<Image, ImageError>`
for pixels and `decode_jpeg_with_metadata(data, limits) ->
Result<DecodedJpeg, ImageError>` when the EXIF orientation is also needed.
`DecodedJpeg.orientation` is always 1–8; a missing EXIF orientation yields 1.
The decoder returns the encoded top-down raster and does not apply orientation.
Applications that display an image should apply the returned EXIF transform at
their presentation boundary. `decode_jpeg` deliberately keeps the same `Image`
contract as the PNG, GIF, and BMP decoders.

The decoder accepts 8-bit Huffman baseline and progressive frames, grayscale,
RGB/YCbCr, Adobe CMYK, and Adobe YCCK. It supports sampling factors used by
4:4:4, 4:2:2, and 4:2:0 images, byte stuffing, restart intervals, progressive
DC/AC scans and successive-approximation refinements. EXIF TIFF byte order may
be little-endian or big-endian. Arithmetic, lossless, differential, and sample
precisions other than 8 bits return a typed unsupported/invalid-data error.
Color profiles and ICC metadata are not applied.

The decoder checks the caller's `DecodeLimits` before allocating coefficient
and pixel buffers. It also caps coefficient storage at 256 MiB, marker segments
at 4,096, scans at 1,024, decoded block work at 64 Mi blocks, and restart
markers at 1 Mi. Invalid marker sequences, Huffman tables, scan parameters,
truncated data, and out-of-order progressive refinements return `ImageError`;
input is never trusted to provide indexing or allocation sizes.

## Reference corpus and verification

The dedicated corpus is in `crates/image/tests/fixtures/jpeg/`. Pillow 12.3.0
generates deterministic baseline/progressive, grayscale, sampling, noise,
quality, restart, and Adobe CMYK JPEGs. The reference RGBA bytes are decoded
from those saved JPEG bytes by Pillow. The YCCK sample is assembled from four
baseline grayscale component scans by the same generator and decoded to RGBA
by Pillow. Its small dimensions keep the fixture reproducible while exercising
the Adobe transform flag and four-component conversion. The frozen `.jpg` and
`.rgba` files are the test inputs; Python and Pillow are not runtime or test
dependencies. The generator and precise provenance are recorded beside them.

`cargo test -p atelier-image --locked --offline` passes the JPEG corpus and the
existing PNG, GIF, and BMP suites. All corpus dimensions match, and the mean
absolute error is at most one level for each RGBA channel. Tests also cover all
eight EXIF orientations in both TIFF byte orders, malformed EXIF and scan
headers, restart markers, pixel/input budgets, every truncated prefix of a
fixture, and single-byte mutations under `catch_unwind` with small decode
limits.

## Validation d’intégration — 2026-10-12

La [passe finale](../_bmad-output/implementation-artifacts/styles-formats-raster-validation.json)
réussit 41 contrôles, 507 tests workspace et 55 tests natifs release,
avec sources et lock inchangés. Les 57 assertions de l’éditeur utilisent le vrai
Worker/WASM. Clippy Windows/WASM, format, licences et builds Windows/WASM/musl
passent ; les tests musl sont compilés localement et exécutés en CI Linux.
La [copie portable](../_bmad-output/implementation-artifacts/styles-formats-raster-portable.json)
fonctionne avec le seul atelier.exe et ses 18 ressources embarquées.
Les limites des US parentes et l’échec strict de handles US-111 restent ouverts.
