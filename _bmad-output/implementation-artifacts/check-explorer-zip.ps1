# SPDX-License-Identifier: AGPL-3.0-or-later
# Agent-only check through the same compressed-folder handler as Explorer.
$ErrorActionPreference = 'Stop'
$atelierRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$atelierTempParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$atelierTemp = Join-Path $atelierTempParent ('atelier-explorer-' + [guid]::NewGuid())
$atelierZipSource = Join-Path $atelierRoot 'crates/zip/src/lib.rs'
$atelierOracleSource = Join-Path $PSScriptRoot 'format-oracle.rs'
$atelierZipSourceHash = (Get-FileHash -LiteralPath $atelierZipSource -Algorithm SHA256).Hash.ToLowerInvariant()
$atelierOracleSourceHash = (Get-FileHash -LiteralPath $atelierOracleSource -Algorithm SHA256).Hash.ToLowerInvariant()
$atelierOracleExe = Join-Path $atelierRoot 'target/format-oracle.exe'
$atelierOracleExeHash = (Get-FileHash -LiteralPath $atelierOracleExe -Algorithm SHA256).Hash.ToLowerInvariant()
$atelierResults = @()
try {
    New-Item -ItemType Directory -Path $atelierTemp | Out-Null
    $atelierSource = Join-Path $atelierTemp 'source.bin'
    $atelierExpected = [Text.Encoding]::UTF8.GetBytes('Explorer interop é🙂')
    [IO.File]::WriteAllBytes($atelierSource, $atelierExpected)
    $atelierShell = New-Object -ComObject Shell.Application
    foreach ($atelierMode in @('32', '64')) {
        $atelierZipPath = Join-Path $atelierTemp ('archive-' + $atelierMode + '.zip')
        & $atelierOracleExe zip-write $atelierZipPath $atelierMode 'readme.txt' '8' $atelierSource 'été.txt' '0' $atelierSource
        if ($LASTEXITCODE -ne 0) { throw 'ZIP generation failed' }
        $atelierArchive = $atelierShell.NameSpace($atelierZipPath)
        if ($null -eq $atelierArchive) { throw "Explorer did not recognize ZIP $atelierMode" }
        $atelierNames = @($atelierArchive.Items() | ForEach-Object { $_.Name })
        $atelierDestination = Join-Path $atelierTemp ('extract-' + $atelierMode)
        New-Item -ItemType Directory -Path $atelierDestination | Out-Null
        $atelierFolder = $atelierShell.NameSpace($atelierDestination)
        $atelierFolder.CopyHere($atelierArchive.Items(), 4 + 16 + 512 + 1024)
        $atelierComplete = $false
        for ($atelierAttempt = 0; $atelierAttempt -lt 30; $atelierAttempt++) {
            $atelierFiles = @(Get-ChildItem -LiteralPath $atelierDestination -File)
            if ($atelierFiles.Count -eq 2 -and @($atelierFiles | Where-Object { $_.Length -ne $atelierExpected.Length }).Count -eq 0) {
                $atelierComplete = $true
                break
            }
            Start-Sleep -Milliseconds 200
        }
        if (-not $atelierComplete) { throw "Explorer extraction incomplete for ZIP $atelierMode" }
        foreach ($atelierName in @('readme.txt', 'été.txt')) {
            $atelierActual = [IO.File]::ReadAllBytes((Join-Path $atelierDestination $atelierName))
            if ([Convert]::ToBase64String($atelierActual) -ne [Convert]::ToBase64String($atelierExpected)) { throw 'Extracted bytes differ' }
        }
        $atelierResults += @{ mode = $atelierMode; names = $atelierNames; count = 2; extracted_bytes_equal = $true }
    }
    $atelierZipSourceAfter = (Get-FileHash -LiteralPath $atelierZipSource -Algorithm SHA256).Hash.ToLowerInvariant()
    $atelierOracleSourceAfter = (Get-FileHash -LiteralPath $atelierOracleSource -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($atelierZipSourceAfter -ne $atelierZipSourceHash -or $atelierOracleSourceAfter -ne $atelierOracleSourceHash) { throw 'Oracle source changed during Explorer validation' }
    $atelierEvidence = @{ date = '2026-10-10'; handler = 'Shell.Application compressed folders'; results = $atelierResults; source_sha256 = @{ zip_crate = $atelierZipSourceHash; oracle_rust = $atelierOracleSourceHash; oracle_exe = $atelierOracleExeHash }; success = $true }
} catch {
    $atelierEvidence = @{ date = '2026-10-10'; handler = 'Shell.Application compressed folders'; results = $atelierResults; success = $false; error = $_.Exception.Message }
} finally {
    $atelierResolved = [IO.Path]::GetFullPath($atelierTemp)
    if (-not $atelierResolved.StartsWith($atelierTempParent, [StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($atelierResolved) -notlike 'atelier-explorer-*') { throw 'Unexpected cleanup target' }
    if (Test-Path -LiteralPath $atelierResolved) { Remove-Item -LiteralPath $atelierResolved -Recurse -Force }
}
$atelierEvidence | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'explorer-zip.json') -Encoding utf8
$atelierEvidence | ConvertTo-Json -Depth 6
if (-not $atelierEvidence.success) { exit 1 }
