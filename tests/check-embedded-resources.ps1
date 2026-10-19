# SPDX-License-Identifier: AGPL-3.0-or-later
# Hidden real-runtime probe; explicitly waits for the release GUI subsystem too.
param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$TargetDirectory
)
$ErrorActionPreference = 'Stop'
$startInfo = [System.Diagnostics.ProcessStartInfo]::new()
$startInfo.FileName = (Resolve-Path -LiteralPath $Executable).Path
$startInfo.UseShellExecute = $false
$startInfo.CreateNoWindow = $true
$startInfo.RedirectStandardOutput = $true
$startInfo.RedirectStandardError = $true
$startInfo.ArgumentList.Add('--resources-self-check')
$startInfo.ArgumentList.Add($TargetDirectory)
$process = [System.Diagnostics.Process]::new()
$process.StartInfo = $startInfo
try {
    if (-not $process.Start()) { throw 'Could not start the embedded resource diagnostic.' }
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit(90000)) {
        $process.Kill($true)
        $process.WaitForExit()
        throw 'Embedded resource diagnostic timed out.'
    }
    $code = $process.ExitCode
    $output = ($stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult()).Trim()
} finally {
    $process.Dispose()
}
$passed = $code -eq 0 -and $output -match '^embedded resources: assets=90; worker-ping=echo; editor=typed\+ink\+undo\+redo; encoded-path=403; CSP fetch\+WebSocket\+image\+frame=blocked; attempts=\d+; rotated=\d+; callback objects 0->0$'
[ordered]@{ passed = $passed; exit_code = $code; result = $output } | ConvertTo-Json -Depth 4
if ($passed) { exit 0 }
exit 1
