# SPDX-License-Identifier: AGPL-3.0-or-later
$ErrorActionPreference = 'Stop'
$resourceRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../data/webview2'))
$manifest = Get-Content -Raw -Encoding UTF8 -LiteralPath (Join-Path $resourceRoot 'manifest.json') | ConvertFrom-Json
if ($manifest.package -ne 'Microsoft.Web.WebView2' -or $manifest.version -ne '1.0.4258.31') {
    throw 'Unexpected WebView2 SDK identity'
}
$expectedFiles = @('LICENSE.txt', 'include/WebView2.h', 'x64/WebView2LoaderStatic.lib')
if (@($manifest.files.PSObject.Properties).Count -ne $expectedFiles.Count) { throw 'Unexpected resource count' }
foreach ($name in $expectedFiles) {
    $entry = $manifest.files.$name
    $path = Join-Path $resourceRoot $name
    if (-not $entry -or -not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing WebView2 resource $name" }
    if ((Get-Item -LiteralPath $path).Length -ne $entry.size) { throw "Size mismatch for $name" }
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.sha256) {
        throw "SHA-256 mismatch for $name"
    }
}
Write-Output 'Pinned Microsoft WebView2 loader, ABI header and license verified.'
