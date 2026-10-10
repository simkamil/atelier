# Tables UCD Unicode 17.0.0

`atelier-unicode::properties` exposes immutable Unicode Character Database values from the vendored Unicode 17.0.0 source corpus. Properties are table lookups; this module does not implement grapheme/word segmentation, line breaking, normalization, or contextual casing.

The scalar properties use two-level tables with 256-code-point pages. Identical pages are stored once. APIs return the original UCD property value as a static string, numeric canonical combining classes, or boolean membership. Names and variable-length case/decomposition mappings are sorted tables backed by generated static data. `name` includes algorithmic names expanded from `extracted/DerivedName.txt`, including Hangul and CJK names; NameAliases values have a separate accessor, so `<control>` placeholders are never exposed as names. Conditional SpecialCasing rows are preserved as typed data and are not applied by this module. Default and Turkic case-fold mappings remain separately available.

The original archive is `https://www.unicode.org/Public/17.0.0/ucd/UCD.zip`; source file hashes and archive provenance are recorded in `data/ucd/17.0.0/manifest.json`. The complete Unicode License V3 is included in that directory. The archive SHA-256 is `2066d1909b2ea93916ce092da1c0ee4808ea3ef8407c94b4f14f5b7eb263d28e`.

To regenerate the checked-in source offline from the vendored data:

```sh
cargo run -p atelier-ucd-gen --locked --offline -- --ucd data/ucd/17.0.0 --output crates/unicode/src/generated.rs
```

To verify reproducibility without writing the output:

```sh
cargo run -p atelier-ucd-gen --locked --offline -- --ucd data/ucd/17.0.0 --output crates/unicode/src/generated.rs --check
```

The crate build independently regenerates to Cargo's `OUT_DIR` and compares every byte with `crates/unicode/src/generated.rs`; it never modifies checked-in source files. The generator accepts only a local UCD directory, validates Unicode 17.0.0, rejects malformed scalars and overlapping ranges, and writes through a temporary file after the complete output has been built in memory. Input and output bounds are `MAX_INPUT_BYTES` (64 MiB per input file) and `MAX_OUTPUT_BYTES` (128 MiB).
