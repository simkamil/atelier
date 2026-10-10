# Epic 0.5 Context: Images

<!-- Compiled from planning artifacts. Edit freely. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Build the first safe image codecs used by document display and rendering. This epic begins with PNG decoding, then PNG encoding and JPEG/GIF/BMP decoding; it enables later image rendering and insertion work while keeping the image crate independent of document formats.

## Stories

- Story 0.40: PNG decoder
- Story 0.41: PNG encoder
- Story 0.42: JPEG decoder
- Story 0.43: GIF and BMP decoders

## Requirements & Constraints

Treat every image file as untrusted: enforce configurable size limits and return errors rather than panicking on malformed input. PNG decoding must cover the legal color/depth combinations, Adam7, palette transparency, and chunk CRCs, and be checked against PngSuite. PNG encoding is limited to RGBA8 and grayscale initially. Keep the crate dependency-free, safe Rust, and reusable by document and rendering components.

## Technical Decisions

The image crate is the format-codec boundary. The workspace forbids external Rust dependencies and `unsafe` in ordinary crates. PNG uses the existing in-workspace DEFLATE/zlib implementation. Keep decoded pixels in a single documented representation so downstream consumers do not need format-specific branching.

## Cross-Story Dependencies

US-040 depends on US-020 (zlib/DEFLATE). US-041 depends on US-040 and US-021 (compression). US-003 reference-image comparisons depend on US-041 and US-082. Later document image insertion depends on PNG, JPEG, GIF, and BMP decoding.
