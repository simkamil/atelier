# Synthetic CFF outline oracle

`cff-oracle.otf` is a small generated OpenType/CFF1 fixture containing a line-only triangle, a cubic curve, and a nested `seac` composite used as an accent. It is source-controlled so the Rust test suite runs without Python or FontTools.

To regenerate the font, point manifest, and development receipt, run:

```powershell
py tools/generate-cff-reference.py
```

To verify the checked-in files without changing them, run:

```powershell
py tools/generate-cff-reference.py --check
```

The generator uses FontTools `DecomposingRecordingPen` over `TTFont.getGlyphSet()` to record every move, line, cubic control point, endpoint, and close operation, including the decomposed nested `seac` components. The manifest records the FontTools version, fixed OpenType timestamps, and SHA-256 hashes. No downloaded font is used. This fixture checks ordinary Type 2 line and cubic operators plus this nested `seac` case; it does not cover CID fonts or FontMatrix transforms.
