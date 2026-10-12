<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-301 — Import DOCX

`atelier-fmt-ooxml::import_docx` reads an OPC package and returns an
`ImportedDocx` containing the original `Package`, a validated `RichDocument`,
the US-201 `StyleEngine`, resolved theme data, settings, latent-style and font
table metadata, and owned embedded-font bytes. Main document, styles, theme,
settings and font table parts are located through relationships, so callers
must not assume that parts live at `word/document.xml` or use a particular XML
prefix. No external relationship is fetched.

```rust,ignore
use atelier_fmt_ooxml::import_docx;
use atelier_font::manager::FontManager;

let mut imported = import_docx(&bytes)?;
let paragraph_id = imported.paragraph_ids()[0];
let (paragraph_with_effective_runs, defaults) =
    imported.resolved_paragraph(paragraph_id)?;
let mut fonts = FontManager::empty();
imported.register_fonts(&mut fonts)?;
```

The returned paragraph runs contain effective character properties, including
paragraph and run style inheritance and direct overrides. The accompanying
`ResolvedProperties` contains the paragraph's effective paragraph properties
and default character properties. `resolved_paragraph` is lazy: missing style
references, wrong style kinds, cycles, and unresolved theme slots surface as a
typed `DocxError::Style` when the paragraph is resolved. Direct `false` values
remain explicit. Runs split inside an extended grapheme are normalized to the
grapheme's outer UTF-8 byte boundaries; the XML run owning the first scalar
provides formatting for the whole grapheme. Later text keeps its own style.

The text subset handles `w:t` with its whitespace, tabs, line breaks and their
types, symbols, nonbreaking/soft hyphens, visible runs inside known inline
containers such as hyperlinks, character and paragraph properties,
document defaults, based-on and linked styles, latent styles, theme colors and
fonts, default tab interval, compatibility settings, and embedded ODTTF fonts.
ODTTF bytes are deobfuscated and parsed by `atelier-font` before the import is
published. `register_fonts` then registers the owned bytes without a
self-referential structure.

The rich model does not yet represent every Word structure. Unsupported
elements and relationships are listed in `notices`; their complete source
parts remain in `package` and can be written back unchanged. Paragraphs inside
currently unsupported containers are discovered recursively and exposed in
document order, while the notice makes that flattening visible. Import errors
are typed and no partial document is returned.

The rich model cannot currently express the cascade-reset semantics of
`w:color w:val="auto"` or `w:highlight w:val="none"`. These source values are
retained as opaque attributes and produce `UnsupportedProperty` notices. An
inherited highlight can therefore remain visible in `resolved_paragraph` even
when a direct `none` was imported; callers must treat the notice as a limitation
until the shared model gains explicit reset operations. `w:szCs` is likewise
retained opaquely and does not override the Latin `w:sz` value.

`DocxLimits` bounds package/XML parsing, paragraphs, runs, aggregate text,
styles, embedded fonts, and font bytes. The defaults inherit OPC limits and
allow at most 1,000,000 paragraphs, 4,000,000 runs, 256 MiB of text, 100,000
styles, and 1,024 fonts / 256 MiB of embedded font data. XML event, depth,
namespace and text limits remain enforced by `atelier-xml`.

The font obfuscation key and byte order follow [ECMA-376 Part 1 §17.8.1](https://ecma-international.org/publications-and-standards/standards/ecma-376/)
and Microsoft's [Font Part specification](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/ea097c57-5794-4624-b08e-017b47051b1d).
The independent fixture and its expected values are generated and reparsed by
[`tools/generate-docx-reference.py`](../tools/generate-docx-reference.py),
using the repository's OFL-licensed Carlito font.

Validation du 12 octobre 2026 : 12 tests DOCX, 14 tests OPC, 16 tests de lignes
riches et 5 tests d’intégration DOCX vers layout. Les [15 contrôles finaux](../_bmad-output/implementation-artifacts/docx-rich-layout-validation.json)
passent avec 540 tests workspace et sources/lock inchangés. La [revue](../_bmad-output/implementation-artifacts/docx-rich-layout-review.md)
conserve chaque constat et son traitement. Cette livraison expose des API de
bibliothèque; le raccordement au parcours d’ouverture de l’éditeur riche reste séparé.
