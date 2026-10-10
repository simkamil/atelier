# SPDX-License-Identifier: AGPL-3.0-or-later
param([string]$RootPath = (Split-Path -Parent $PSScriptRoot))
$ErrorActionPreference = 'Stop'
function Require($Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Verify([string]$Base, [string]$Relative, [string]$Digest) {
    Require ($Relative -is [string] -and -not [string]::IsNullOrWhiteSpace($Relative)) 'Missing relative path'
    Require ($Digest -cmatch '^[0-9a-f]{64}$') "Invalid digest: $Relative"
    $prefix = [IO.Path]::GetFullPath($Base).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    $full = [IO.Path]::GetFullPath((Join-Path $Base $Relative))
    Require ($full.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) 'Path escapes corpus'
    Require (Test-Path -LiteralPath $full -PathType Leaf) "Missing source: $Relative"
    Require ((Get-FileHash -LiteralPath $full -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $Digest) "SHA-256 mismatch: $Relative"
    return (Get-Item -LiteralPath $full).Length
}
try {
    $base = Join-Path $RootPath 'data/ucd/17.0.0'
    $manifest = Get-Content -LiteralPath (Join-Path $base 'manifest.json') -Raw -Encoding utf8 | ConvertFrom-Json -AsHashtable
    Require ($manifest.unicode_version -ceq '17.0.0') 'Unexpected UCD version'
    Require ($manifest.source_url -ceq 'https://www.unicode.org/Public/17.0.0/ucd/UCD.zip') 'Unexpected UCD source'
    Require ($manifest.archive_sha256 -ceq '2066d1909b2ea93916ce092da1c0ee4808ea3ef8407c94b4f14f5b7eb263d28e') 'Unexpected UCD archive'
    Require ($manifest.files -is [array] -and $manifest.files.Count -eq 71) 'Incomplete UCD inventory'
    $seen = @{}
    foreach ($file in $manifest.files) {
        Require (-not $seen.ContainsKey($file.path)) 'Duplicate UCD path'
        $seen[$file.path] = $true
        $length = Verify $base $file.path $file.sha256
        Require ($length -eq $file.bytes) "Source length mismatch: $($file.path)"
    }
    Require ($manifest.license.path -ceq 'LICENSE.txt') 'Missing Unicode license'
    $null = Verify $base $manifest.license.path $manifest.license.sha256
    $seen['LICENSE.txt'] = $true
    $seen['manifest.json'] = $true
    foreach ($file in Get-ChildItem -LiteralPath $base -Recurse -File) {
        $relative = [IO.Path]::GetRelativePath($base, $file.FullName).Replace('\','/')
        Require ($seen.ContainsKey($relative)) "Unlisted UCD source: $relative"
    }
    $references = Join-Path $RootPath 'tests/font-outline-reference'
    $golden = Get-Content -LiteralPath (Join-Path $references 'manifest.json') -Raw -Encoding utf8 | ConvertFrom-Json -AsHashtable
    Require ($golden.schema_version -eq 1 -and $golden.oracle -ceq 'FontTools' -and $golden.faces -eq 20 -and $golden.cases -eq 300) 'Invalid outline reference inventory'
    $null = Verify $references $golden.file $golden.sha256
    $unicodeReferences = Join-Path $RootPath 'tests/unicode-reference'
    $unicodeGolden = Get-Content -LiteralPath (Join-Path $unicodeReferences 'manifest.json') -Raw -Encoding utf8 | ConvertFrom-Json -AsHashtable
    Require ($unicodeGolden.unicode_version -ceq '17.0.0' -and $unicodeGolden.scalars -eq 1112064 -and $unicodeGolden.fields -eq 30) 'Invalid exhaustive Unicode reference'
    $null = Verify $unicodeReferences $unicodeGolden.file $unicodeGolden.sha256
    Write-Output 'Verified 71 official Unicode sources, license, 300 outline references and exhaustive Unicode fingerprints.'
} catch { Write-Error $_; exit 1 }
