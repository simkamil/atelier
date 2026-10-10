# Embedded fonts

Atelier ships 20 unchanged TrueType faces (7,426,636 bytes / 7.08 MiB of raw TTF data). The payload contributes approximately this amount before executable/package compression; the final executable size depends on link-time section retention and packaging. No font is downloaded at build or runtime.

Each family provides Regular (400), Bold (700), Italic (400) and Bold Italic (700): Carlito, Caladea, Liberation Sans, Liberation Serif and Liberation Mono. The font files are grouped under their family directories. Full OFL 1.1 texts, including upstream copyright and reserved-name notices, are in `licenses/` and are exposed through each `EmbeddedFont::license` value.

`manifest.json` records each face's source file, immutable upstream commit, upstream font version, byte length and SHA-256. The Liberation faces come from the upstream 2.1.5 TTF release archive (archive SHA-256: `7191c669bf38899f73a2094ed00f7b800553364f90e2637010a69c0e268f25d0`); Carlito and Caladea are fetched from their pinned official Git commits. To verify payload integrity from the repository root with PowerShell:

```powershell
$manifest = Get-Content data/fonts/manifest.json -Raw | ConvertFrom-Json
foreach ($asset in $manifest.assets) {
  $actual = (Get-FileHash $asset.path -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($actual -ne $asset.sha256) { throw "SHA-256 mismatch: $($asset.path)" }
}
```

The files are redistributed under SIL OFL 1.1. The application must include the corresponding full notice text with each distribution. Font binaries are upstream assets and must remain byte-for-byte unchanged.

Le job Windows vérifie également les SHA-256 et l’inventaire typé avec
`tests/check-assets.ps1`; les tests Rust valident le catalogue compilé et les
métadonnées OpenType de chaque face.
