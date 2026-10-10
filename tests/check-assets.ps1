# SPDX-License-Identifier: AGPL-3.0-or-later
# Native PowerShell integrity check; no downloaded tool or product dependency.
param([string]$RootPath = (Split-Path -Parent $PSScriptRoot))
$ErrorActionPreference = 'Stop'

function Assert-Asset($Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Check-File([string]$Base, [string]$Relative, [string]$Digest) {
    Assert-Asset (-not [string]::IsNullOrWhiteSpace($Relative)) 'Missing asset path'
    Assert-Asset ($Digest -cmatch '^[0-9a-f]{64}$') "Invalid SHA-256: $Relative"
    $basePath = [IO.Path]::GetFullPath($Base)
    $fullPath = [IO.Path]::GetFullPath((Join-Path $basePath $Relative))
    $prefix = $basePath.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    Assert-Asset ($fullPath.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) 'Asset path escapes its resource directory'
    Assert-Asset (Test-Path -LiteralPath $fullPath -PathType Leaf) "Missing asset: $Relative"
    $actual = (Get-FileHash -LiteralPath $fullPath -Algorithm SHA256).Hash.ToLowerInvariant()
    Assert-Asset ($actual -ceq $Digest) "SHA-256 mismatch: $Relative"
    return (Get-Item -LiteralPath $fullPath).Length
}

try {
    $fontRoot = Join-Path $RootPath 'data/fonts'
    $fonts = Get-Content -LiteralPath (Join-Path $fontRoot 'manifest.json') -Raw -Encoding utf8 | ConvertFrom-Json -AsHashtable
    Assert-Asset ($fonts.schema_version -eq 1 -and $fonts.assets -is [array] -and $fonts.assets.Count -eq 20) 'Invalid font inventory'
    $seen = @{}
    $total = 0L
    foreach ($font in $fonts.assets) {
        Assert-Asset ($font.family -in @('Carlito','Caladea','Liberation Sans','Liberation Serif','Liberation Mono')) 'Unknown font family'
        Assert-Asset (($font.weight -is [long] -or $font.weight -is [int]) -and $font.weight -in @(400,700)) 'Invalid font weight'
        Assert-Asset ($font.style -in @('normal','italic')) 'Invalid font style'
        $key = '{0}/{1}/{2}' -f $font.family,$font.weight,$font.style
        Assert-Asset (-not $seen.ContainsKey($key)) 'Duplicate font face'
        $seen[$key] = $true
        Assert-Asset ($font.upstream.commit -cmatch '^[0-9a-f]{40}$' -and $font.upstream.source_url -cmatch '^https://') 'Unpinned font source'
        Assert-Asset ($font.path -is [string] -and $font.path.StartsWith('data/fonts/')) 'Invalid font path'
        $length = Check-File $fontRoot $font.path.Substring(11) $font.sha256
        Assert-Asset ($length -eq $font.bytes) 'Font length mismatch'
        $total += $length
        $licensePath = [IO.Path]::GetFullPath((Join-Path $fontRoot $font.license_file))
        Assert-Asset ($licensePath.StartsWith([IO.Path]::GetFullPath((Join-Path $fontRoot 'licenses')) + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) 'Invalid license path'
        Assert-Asset (Test-Path -LiteralPath $licensePath -PathType Leaf) 'Missing font license'
    }
    Assert-Asset ($total -eq $fonts.total_bytes -and $fonts.asset_count -eq 20) 'Font inventory totals mismatch'

    $imageRoot = Join-Path $RootPath 'tests/image-reference'
    $images = Get-Content -LiteralPath (Join-Path $imageRoot 'manifest.json') -Raw -Encoding utf8 | ConvertFrom-Json -AsHashtable
    Assert-Asset ($images.cases -is [array] -and $images.cases.Count -eq 201) 'Invalid image inventory'
    $lines = [Collections.Generic.List[string]]::new()
    $seen = @{}
    foreach ($case in $images.cases) {
        Assert-Asset ($case.format -in @('png','gif','bmp') -and $case.valid -is [bool]) 'Invalid image case type'
        Assert-Asset (-not $seen.ContainsKey($case.file)) 'Duplicate image case'
        $seen[$case.file] = $true
        $null = Check-File $imageRoot $case.file $case.sha256
        $reference = '-'
        if ($case.valid) {
            Assert-Asset (($case.width -is [long] -or $case.width -is [int]) -and $case.width -gt 0 -and ($case.height -is [long] -or $case.height -is [int]) -and $case.height -gt 0) 'Invalid image dimensions'
            $length = Check-File $imageRoot $case.reference $case.reference_sha256
            Assert-Asset ($length -eq 4L * $case.width * $case.height) 'RGBA reference length mismatch'
            $reference = $case.reference
        } else {
            Assert-Asset ($case.format -eq 'png' -and $case.width -eq 0 -and $case.height -eq 0) 'Invalid rejection case'
        }
        $line = ("{0}`t{1}`t{2}`t{3}`t{4}" -f $case.format,$case.file,$case.width,$case.height,$reference)
        $lines.Add($line)
    }
    $tsv = Get-Content -LiteralPath (Join-Path $imageRoot 'cases.tsv') -Raw -Encoding utf8
    Assert-Asset ($tsv.Replace("`r`n", "`n") -ceq (($lines -join "`n") + "`n")) 'Cargo corpus and manifest disagree'
    Write-Output "Verified 20 font hashes, 201 image hashes and all 187 RGBA reference hashes."
    exit 0
} catch {
    Write-Error $_ -ErrorAction Continue
    exit 1
}
