<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-081 — Backend SVG

`atelier_render::render_svg` converts one validated `DisplayList` into a
deterministic, standalone SVG document. It performs no file, system-font, or
network access. Font outlines and encoded image bytes are supplied through the
synchronous `SvgResourceProvider` trait.

```rust
use atelier_render::{render_svg, SvgImage, SvgLimits, SvgResourceProvider};

struct Resources;
impl SvgResourceProvider for Resources {
    fn glyph_outline(&self, _font_id: u64, _glyph_id: u32)
        -> Option<&atelier_render::DisplayPath> { None }
    fn image(&self, _image_id: u64) -> Option<SvgImage<'_>> { None }
}

// let svg = render_svg(&display_list, &Resources, SvgLimits::default())?;
```

Glyph outlines are paths in page units relative to their glyph origin. The
renderer validates each supplied outline, writes one definition per
font/glyph pair, then positions references with `<use>`. Image resources borrow
their canonical MIME type and encoded bytes. Only `image/png`, `image/jpeg`,
`image/gif`, and `image/bmp` are accepted; the corresponding initial signature
is checked before bytes are embedded in a Base64 `data:` URI. Signature checks
do not decode an image or prove that its complete contents are valid.

The default `SvgLimits` cap the document at 64 MiB, each encoded image at 8 MiB,
and total raw embedded image bytes at 32 MiB. Resolved outlines are limited to
1,000,000 unique glyphs and 1,000,000 path elements cumulatively. Callers may
provide smaller limits. Arithmetic and output growth are checked before the
renderer reserves or appends bytes. `SvgError` distinguishes missing resources,
invalid bytes, unsupported MIME values, invalid lists/outlines, and limit
failures; an error returns no partial document.

The resource preflight checks every image and glyph referenced by the display
list, including drawings later hidden by an empty clip. The raw image budget
therefore conservatively counts every image command. Unsupported image formats
must be converted by the caller before supplying resources.

The SVG keeps page coordinates and viewBox bounds. Drawing commands retain
their paint order, RGBA alpha, fill rules, line caps/joins/mitre limits, and
line, quadratic, cubic, and close path elements. Each drawing command receives
the current page-space clip outside its composite transformation, so the
independent state stacks remain independent when pushes and pops cross. Links
and structure markers are retained in escaped, inert metadata; link targets
never become navigation attributes.

## Validation — 2026-10-10

US développée, revue et validée. Le [suivi du lot](EDITING-PROGRESS.md) relie les tests, les comparaisons indépendantes, les correctifs de revue et leurs limites.
