# SPDX-License-Identifier: AGPL-3.0-or-later
# Functional CI gate; strict acceptance remains atelier --webview-self-check.
param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$TargetDirectory,
    [switch]$AllowKnownRuntimeHandleGrowth
)
$ErrorActionPreference = 'Stop'
$startInfo = [System.Diagnostics.ProcessStartInfo]::new()
$startInfo.FileName = (Resolve-Path -LiteralPath $Executable).Path
$startInfo.UseShellExecute = $false
$startInfo.CreateNoWindow = $true
$startInfo.RedirectStandardOutput = $true
$startInfo.RedirectStandardError = $true
$startInfo.ArgumentList.Add('--webview-self-check')
$startInfo.ArgumentList.Add($TargetDirectory)
$process = [System.Diagnostics.Process]::new()
$process.StartInfo = $startInfo
try {
    if (-not $process.Start()) { throw 'Could not start the native diagnostic.' }
    # Explicit redirected process ownership also waits for the release GUI subsystem.
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit(240000)) {
        $process.Kill($true)
        $process.WaitForExit()
        throw 'Native WebView2 diagnostic timed out.'
    }
    $code = $process.ExitCode
    $output = ($stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult()).Trim()
} finally {
    $process.Dispose()
}
$strictPassed = $code -eq 0 -and $output -match '^WebView2 OK; warm-up=1; cycles=100; callback objects 0->0; process handles baseline=\d+, samples=\[.+\], peak=\d+, final=\d+; DevTools=(True|False|true|false)$'
$functionalOnly = $code -eq 1 -and $output -match '^WebView2 functional OK; cycles=100; callback objects 0->0; DevTools=(True|False|true|false); process handle count grew: baseline \d+, samples \[.+\], final \d+, peak \d+$'
$report = [ordered]@{
    functional_passed = ($strictPassed -or $functionalOnly)
    handle_acceptance_passed = $strictPassed
    strict_exit_code = $code
    result = $output
    upstream_report = 'https://github.com/MicrosoftEdge/WebView2Feedback/issues/5704'
}
$report | ConvertTo-Json -Depth 3
if ($strictPassed) { exit 0 }
if ($functionalOnly -and $AllowKnownRuntimeHandleGrowth) {
    [Console]::Error.WriteLine('US-111 remains open: 100 functional cycles passed, but strict process-handle acceptance failed. See the JSON diagnostic and upstream report.')
    exit 0
}
if ($code -eq 0) { exit 1 }
exit $code
