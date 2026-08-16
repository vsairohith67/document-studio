param(
    [Parameter(Mandatory = $true)]
    [string]$Executable
)

$ErrorActionPreference = 'Stop'
$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
$temporaryRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$testDirectory = [System.IO.Path]::GetFullPath(
    [System.IO.Path]::Combine($temporaryRoot, "document-studio-single-instance-$([guid]::NewGuid())")
)
if (-not $testDirectory.StartsWith($temporaryRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'The isolated test directory escaped the operating-system temporary directory.'
}

$previousValue = [Environment]::GetEnvironmentVariable('DOCUMENT_STUDIO_TEST_APP_DATA', 'Process')
$primary = $null
try {
    New-Item -ItemType Directory -Path $testDirectory | Out-Null
    [Environment]::SetEnvironmentVariable('DOCUMENT_STUDIO_TEST_APP_DATA', $testDirectory, 'Process')
    $primary = Start-Process -FilePath $resolvedExecutable -PassThru -WindowStyle Hidden

    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    $primaryMarker = Join-Path $testDirectory "runtime-started-$($primary.Id)"
    while (-not (Test-Path -LiteralPath $primaryMarker)) {
        if ($primary.HasExited) {
            throw "The primary instance exited before runtime setup completed (exit $($primary.ExitCode))."
        }
        if ([DateTime]::UtcNow -ge $deadline) {
            throw 'The primary instance did not reach runtime setup within 30 seconds.'
        }
        Start-Sleep -Milliseconds 100
        $primary.Refresh()
    }

    $secondary = Start-Process -FilePath $resolvedExecutable -PassThru -WindowStyle Hidden
    if (-not $secondary.WaitForExit(10000)) {
        Stop-Process -Id $secondary.Id -Force
        throw 'The secondary instance did not exit within 10 seconds.'
    }
    if ($secondary.ExitCode -ne 0) {
        throw "The secondary instance exited with code $($secondary.ExitCode)."
    }

    $markers = @(Get-ChildItem -LiteralPath $testDirectory -Filter 'runtime-started-*' -File)
    if ($markers.Count -ne 1 -or $markers[0].FullName -ne $primaryMarker) {
        throw 'A secondary process reached runtime setup; single-instance enforcement failed.'
    }
    Write-Output 'Single-instance smoke passed: the secondary exited before runtime setup.'
}
finally {
    if ($null -ne $primary -and -not $primary.HasExited) {
        Stop-Process -Id $primary.Id -Force
        $primary.WaitForExit(10000) | Out-Null
    }
    [Environment]::SetEnvironmentVariable(
        'DOCUMENT_STUDIO_TEST_APP_DATA',
        $previousValue,
        'Process'
    )
    if (Test-Path -LiteralPath $testDirectory) {
        $resolvedTestDirectory = [System.IO.Path]::GetFullPath((Resolve-Path -LiteralPath $testDirectory).Path)
        if (-not $resolvedTestDirectory.StartsWith($temporaryRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw 'Refusing to remove a test directory outside the operating-system temporary directory.'
        }
        Remove-Item -LiteralPath $resolvedTestDirectory -Recurse -Force
    }
}
