param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [ValidateSet('None', 'PrimaryRuntimeNotReady', 'SecondaryNotExited', 'SecondaryReachedRuntime')]
    [string]$FailureInjection = 'None',
    [switch]$SkipFailureSelfTests
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$environmentNames = @(
    'DOCUMENT_STUDIO_TEST_APP_DATA',
    'DOCUMENT_STUDIO_TEST_WEBVIEW2_DATA_DIR',
    'DOCUMENT_STUDIO_TEST_CDP_PORT',
    'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS',
    'WEBVIEW2_USER_DATA_FOLDER'
)
$environmentState = @{}
foreach ($name in $environmentNames) {
    $environmentState[$name] = @{
        Exists = Test-Path -LiteralPath "Env:$name"
        Value = [Environment]::GetEnvironmentVariable($name, 'Process')
    }
}

$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
$repositoryRoot = [System.IO.Path]::GetFullPath((Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path)
$temporaryRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()).TrimEnd('\')
$ownedRoot = [System.IO.Path]::GetFullPath(
    [System.IO.Path]::Combine($temporaryRoot, "document-studio-single-instance-$([guid]::NewGuid().ToString('N'))")
)
$appDataDirectory = Join-Path $ownedRoot 'app-data'
$primaryUdfDirectory = Join-Path $appDataDirectory 'primary-webview2-user-data'
$secondaryUdfDirectory = Join-Path $appDataDirectory 'secondary-webview2-user-data'
$primaryBrowserDataDirectory = Join-Path $primaryUdfDirectory 'EBWebView'
$secondaryBrowserDataDirectory = Join-Path $secondaryUdfDirectory 'EBWebView'
$primaryStdout = Join-Path $ownedRoot 'primary.stdout.log'
$primaryStderr = Join-Path $ownedRoot 'primary.stderr.log'
$secondaryStdout = Join-Path $ownedRoot 'secondary.stdout.log'
$secondaryStderr = Join-Path $ownedRoot 'secondary.stderr.log'
$metadataPath = Join-Path $appDataDirectory 'metadata.sqlite3'
$workspacePath = Join-Path $appDataDirectory 'workspaces'
$devUrl = 'http://localhost:1420'

$firstProgressDeadlineMs = 15000
$setupDeadlineMs = 20000
$absoluteDeadlineMs = 45000
$secondaryExitDeadlineMs = 10000
if ($FailureInjection -eq 'PrimaryRuntimeNotReady') {
    $setupDeadlineMs = 3000
    $absoluteDeadlineMs = 10000
}
if ($FailureInjection -eq 'SecondaryNotExited') {
    $secondaryExitDeadlineMs = 3000
}

$primary = $null
$secondary = $null
$primaryMarker = $null
$secondaryMarker = $null
$startupWatch = [System.Diagnostics.Stopwatch]::new()
$secondaryWatch = [System.Diagnostics.Stopwatch]::new()
$cleanupWatch = [System.Diagnostics.Stopwatch]::new()
$phase = 'INITIALIZING'
$phaseStartedMs = 0L
$failureCode = $null
$failureMessage = $null
$diagnosticsPrinted = $false
$cleanupComplete = $false
$primaryMarkerSuppressed = $false
$metrics = [ordered]@{
    processLaunchMs = $null
    metadataCreatedMs = $null
    workspaceCreatedMs = $null
    webviewChildCreatedMs = $null
    udfPopulatedMs = $null
    runtimeMarkerCreatedMs = $null
    secondaryExitMs = $null
    cleanupMs = $null
    firstProgressDeadlineMs = $firstProgressDeadlineMs
    setupDeadlineMs = $setupDeadlineMs
    absoluteDeadlineMs = $absoluteDeadlineMs
    secondaryExitDeadlineMs = $secondaryExitDeadlineMs
}

function Convert-NormalizedPath {
    param([Parameter(Mandatory = $true)][string]$Path)

    $normalized = [System.IO.Path]::GetFullPath($Path)
    if ($normalized.StartsWith('\\?\UNC\', [System.StringComparison]::OrdinalIgnoreCase)) {
        $normalized = "\\$($normalized.Substring(8))"
    }
    elseif ($normalized.StartsWith('\\?\', [System.StringComparison]::OrdinalIgnoreCase)) {
        $normalized = $normalized.Substring(4)
    }
    return $normalized.TrimEnd('\')
}

function Test-PathWithin {
    param(
        [Parameter(Mandatory = $true)][string]$Candidate,
        [Parameter(Mandatory = $true)][string]$Boundary,
        [switch]$AllowEqual
    )

    $candidatePath = Convert-NormalizedPath -Path $Candidate
    $boundaryPath = Convert-NormalizedPath -Path $Boundary
    if ($AllowEqual -and $candidatePath.Equals($boundaryPath, [System.StringComparison]::OrdinalIgnoreCase)) {
        return $true
    }
    return $candidatePath.StartsWith(
        "$boundaryPath\",
        [System.StringComparison]::OrdinalIgnoreCase
    )
}

function Assert-OwnedDirectory {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [switch]$MustBeEmpty
    )

    $resolved = [System.IO.Path]::GetFullPath((Resolve-Path -LiteralPath $Path).Path)
    if (-not (Test-PathWithin -Candidate $resolved -Boundary $ownedRoot -AllowEqual)) {
        throw "Owned test path escaped its boundary: $resolved"
    }
    $attributes = [System.IO.File]::GetAttributes($resolved)
    if (($attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "Owned test path is a reparse point: $resolved"
    }
    if ($MustBeEmpty -and @(Get-ChildItem -LiteralPath $resolved -Force).Count -ne 0) {
        throw "Owned test path is not empty: $resolved"
    }
    return $resolved
}

function Set-TestEnvironmentValue {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [AllowNull()][string]$Value
    )

    if ($null -eq $Value) {
        Remove-Item -LiteralPath "Env:$Name" -ErrorAction SilentlyContinue
    }
    else {
        [Environment]::SetEnvironmentVariable($Name, $Value, 'Process')
    }
}

function Restore-TestEnvironment {
    foreach ($name in $environmentNames) {
        $saved = $environmentState[$name]
        if ($saved.Exists) {
            [Environment]::SetEnvironmentVariable($name, $saved.Value, 'Process')
        }
        else {
            Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue
        }
    }
}

function Throw-HarnessFailure {
    param(
        [Parameter(Mandatory = $true)][string]$Code,
        [Parameter(Mandatory = $true)][string]$Message
    )

    $exception = [System.InvalidOperationException]::new($Message)
    $exception.Data['FailureCode'] = $Code
    throw $exception
}

function Test-DirectoryPopulated {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        return $false
    }
    return @(Get-ChildItem -LiteralPath $Path -Force -ErrorAction SilentlyContinue).Count -gt 0
}

function Get-WebViewProcesses {
    param([string[]]$BrowserDataDirectories)

    $normalizedDirectories = @(
        $BrowserDataDirectories |
            ForEach-Object { Convert-NormalizedPath -Path $_ }
    )
    $matches = @()
    foreach ($process in @(Get-CimInstance Win32_Process -Filter "Name = 'msedgewebview2.exe'" -ErrorAction SilentlyContinue)) {
        $commandLine = [string]$process.CommandLine
        if ([string]::IsNullOrWhiteSpace($commandLine)) {
            continue
        }
        $match = [regex]::Match($commandLine, '(?i)--user-data-dir=(?:"([^"]+)"|([^\s]+))')
        if (-not $match.Success) {
            continue
        }
        $actualValue = $match.Groups[1].Value
        if ([string]::IsNullOrEmpty($actualValue)) {
            $actualValue = $match.Groups[2].Value
        }
        try {
            $actualPath = Convert-NormalizedPath -Path $actualValue
        }
        catch {
            continue
        }
        $owned = $false
        foreach ($expected in $normalizedDirectories) {
            if ($actualPath.Equals($expected, [System.StringComparison]::OrdinalIgnoreCase)) {
                $owned = $true
                break
            }
        }
        if ($owned) {
            $matches += $process
        }
    }
    return @($matches)
}

function Get-DescendantProcessIds {
    param([int[]]$RootIds)

    $all = @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue)
    $owned = [System.Collections.Generic.HashSet[int]]::new()
    $frontier = [System.Collections.Generic.Queue[int]]::new()
    foreach ($rootId in $RootIds) {
        if ($rootId -gt 0 -and $owned.Add($rootId)) {
            $frontier.Enqueue($rootId)
        }
    }
    while ($frontier.Count -gt 0) {
        $parentId = $frontier.Dequeue()
        foreach ($child in $all) {
            if ([int]$child.ParentProcessId -eq $parentId) {
                $childId = [int]$child.ProcessId
                if ($owned.Add($childId)) {
                    $frontier.Enqueue($childId)
                }
            }
        }
    }
    return @($owned)
}

function Get-ProcessDiagnosticState {
    param([AllowNull()][System.Diagnostics.Process]$Process)

    if ($null -eq $Process) {
        return [ordered]@{ pid = $null; status = 'not-started'; exitCode = $null }
    }
    try {
        $Process.Refresh()
        if ($Process.HasExited) {
            return [ordered]@{ pid = $Process.Id; status = 'exited'; exitCode = $Process.ExitCode }
        }
        return [ordered]@{ pid = $Process.Id; status = 'running'; exitCode = $null }
    }
    catch {
        return [ordered]@{ pid = $Process.Id; status = 'unavailable'; exitCode = $null }
    }
}

function Convert-ToStableLabel {
    param([AllowNull()][string]$Text)

    if ($null -eq $Text) {
        return $null
    }
    return $Text.Replace($ownedRoot, '<OWNED>').Replace($repositoryRoot, '<REPOSITORY>')
}

function Get-RelevantWebViewSwitches {
    param([object[]]$Processes)

    $result = @()
    foreach ($process in $Processes) {
        $commandLine = [string]$process.CommandLine
        $typeMatch = [regex]::Match($commandLine, '(?i)--type=([^\s"]+)')
        $userDataMatch = [regex]::Match($commandLine, '(?i)--user-data-dir=(?:"([^"]+)"|([^\s]+))')
        $userData = $null
        if ($userDataMatch.Success) {
            $userData = $userDataMatch.Groups[1].Value
            if ([string]::IsNullOrEmpty($userData)) {
                $userData = $userDataMatch.Groups[2].Value
            }
        }
        $remoteSwitches = @(
            [regex]::Matches($commandLine, '(?i)--remote-(?:debugging|allow-origins)[^\s"]*') |
                ForEach-Object { $_.Value }
        )
        $result += [ordered]@{
            pid = [int]$process.ProcessId
            parentPid = [int]$process.ParentProcessId
            type = $(if ($typeMatch.Success) { $typeMatch.Groups[1].Value } else { 'browser' })
            userDataDir = Convert-ToStableLabel -Text $userData
            remoteDebugging = $remoteSwitches
        }
    }
    return @($result)
}

function Get-BoundedLog {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return '<absent>'
    }
    $lines = @(Get-Content -LiteralPath $Path -Tail 200 -ErrorAction SilentlyContinue)
    $text = $lines -join [Environment]::NewLine
    while ([System.Text.Encoding]::UTF8.GetByteCount($text) -gt 65536 -and $text.Length -gt 1) {
        $text = $text.Substring([Math]::Floor($text.Length / 4))
    }
    return Convert-ToStableLabel -Text $text
}

function Test-DevUrlReachable {
    try {
        $response = Invoke-WebRequest -Uri $devUrl -UseBasicParsing -TimeoutSec 1 -ErrorAction Stop
        return [int]$response.StatusCode -eq 200
    }
    catch {
        return $false
    }
}

function Get-MarkerNames {
    if (-not (Test-Path -LiteralPath $appDataDirectory -PathType Container)) {
        return @()
    }
    return @(
        Get-ChildItem -LiteralPath $appDataDirectory -Filter 'runtime-started-*' -File -ErrorAction SilentlyContinue |
            Select-Object -ExpandProperty Name
    )
}

function Write-FailureDiagnostics {
    param(
        [Parameter(Mandatory = $true)][string]$Code,
        [Parameter(Mandatory = $true)][string]$Message
    )

    $primaryWebViews = @(Get-WebViewProcesses -BrowserDataDirectories @($primaryBrowserDataDirectory))
    $secondaryWebViews = @(Get-WebViewProcesses -BrowserDataDirectories @($secondaryBrowserDataDirectory))
    $markerNames = @(Get-MarkerNames)
    $metadataSize = $null
    if (Test-Path -LiteralPath $metadataPath -PathType Leaf) {
        $metadataSize = (Get-Item -LiteralPath $metadataPath).Length
    }
    $phaseElapsed = 0L
    if ($startupWatch.IsRunning) {
        $phaseElapsed = $startupWatch.ElapsedMilliseconds - $phaseStartedMs
    }
    elseif ($secondaryWatch.IsRunning) {
        $phaseElapsed = $secondaryWatch.ElapsedMilliseconds
    }
    $diagnostic = [ordered]@{
        failureCode = $Code
        message = Convert-ToStableLabel -Text $Message
        primaryStartupElapsedMs = $startupWatch.ElapsedMilliseconds
        phase = $phase
        phaseElapsedMs = $phaseElapsed
        primary = Get-ProcessDiagnosticState -Process $primary
        secondary = Get-ProcessDiagnosticState -Process $secondary
        primaryMarkerPresent = $(if ($null -ne $primaryMarker) { Test-Path -LiteralPath $primaryMarker } else { $false })
        secondaryMarkerPresent = $(if ($null -ne $secondaryMarker) { Test-Path -LiteralPath $secondaryMarker } else { $false })
        runtimeMarkers = $markerNames
        runtimeMarkerCount = $markerNames.Count
        metadata = [ordered]@{ present = Test-Path -LiteralPath $metadataPath; bytes = $metadataSize }
        primaryUdf = [ordered]@{ label = '<OWNED>\app-data\primary-webview2-user-data'; populated = Test-DirectoryPopulated -Path $primaryUdfDirectory }
        secondaryUdf = [ordered]@{ label = '<OWNED>\app-data\secondary-webview2-user-data'; populated = Test-DirectoryPopulated -Path $secondaryUdfDirectory }
        ownedWebViewProcesses = @(
            Get-RelevantWebViewSwitches -Processes @($primaryWebViews + $secondaryWebViews)
        )
        devUrl = [ordered]@{ url = $devUrl; reachable = Test-DevUrlReachable }
        logs = [ordered]@{
            primaryStdout = Get-BoundedLog -Path $primaryStdout
            primaryStderr = Get-BoundedLog -Path $primaryStderr
            secondaryStdout = Get-BoundedLog -Path $secondaryStdout
            secondaryStderr = Get-BoundedLog -Path $secondaryStderr
        }
    }
    Write-Output 'SINGLE_INSTANCE_FAILURE_DIAGNOSTICS_BEGIN'
    Write-Output ($diagnostic | ConvertTo-Json -Depth 7 -Compress)
    Write-Output 'SINGLE_INSTANCE_FAILURE_DIAGNOSTICS_END'
    $script:diagnosticsPrinted = $true
}

function Stop-OwnedProcesses {
    $rootIds = @()
    foreach ($process in @($secondary, $primary)) {
        if ($null -ne $process) {
            try {
                $process.Refresh()
                if (-not $process.HasExited) {
                    $rootIds += $process.Id
                }
            }
            catch {
            }
        }
    }
    $ownedIds = [System.Collections.Generic.HashSet[int]]::new()
    foreach ($processId in @(Get-DescendantProcessIds -RootIds $rootIds)) {
        [void]$ownedIds.Add([int]$processId)
    }
    foreach ($webView in @(Get-WebViewProcesses -BrowserDataDirectories @($primaryBrowserDataDirectory, $secondaryBrowserDataDirectory))) {
        [void]$ownedIds.Add([int]$webView.ProcessId)
    }
    foreach ($processId in @($ownedIds | Sort-Object -Descending)) {
        if ($processId -eq $PID) {
            continue
        }
        Stop-Process -Id $processId -Force -ErrorAction SilentlyContinue
    }
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        $remaining = @()
        foreach ($processId in $ownedIds) {
            if (Get-Process -Id $processId -ErrorAction SilentlyContinue) {
                $remaining += $processId
            }
        }
        if ($remaining.Count -eq 0) {
            return
        }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "Owned processes remained after cleanup: $($remaining -join ',')"
}

function Remove-OwnedDirectory {
    if (-not (Test-Path -LiteralPath $ownedRoot)) {
        return
    }
    $resolved = [System.IO.Path]::GetFullPath((Resolve-Path -LiteralPath $ownedRoot).Path)
    if (-not (Test-PathWithin -Candidate $resolved -Boundary $temporaryRoot)) {
        throw 'Refusing to remove a test directory outside the operating-system temporary directory.'
    }
    if (-not ([System.IO.Path]::GetFileName($resolved)).StartsWith('document-studio-single-instance-', [System.StringComparison]::Ordinal)) {
        throw 'Refusing to remove a test directory with an unexpected name.'
    }
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        try {
            [System.IO.Directory]::Delete($resolved, $true)
        }
        catch {
            if ([DateTime]::UtcNow -ge $deadline) {
                throw
            }
            Start-Sleep -Milliseconds 200
        }
    } while (Test-Path -LiteralPath $resolved)
}

function Get-MetadataSnapshot {
    if (-not (Test-Path -LiteralPath $metadataPath -PathType Leaf)) {
        return $null
    }
    $item = Get-Item -LiteralPath $metadataPath
    $stream = $null
    $sha256 = $null
    try {
        $stream = [System.IO.FileStream]::new(
            $metadataPath,
            [System.IO.FileMode]::Open,
            [System.IO.FileAccess]::Read,
            ([System.IO.FileShare]::ReadWrite -bor [System.IO.FileShare]::Delete)
        )
        $sha256 = [System.Security.Cryptography.SHA256]::Create()
        $hash = [System.BitConverter]::ToString($sha256.ComputeHash($stream)).Replace('-', '')
        return [ordered]@{
            length = $item.Length
            lastWriteUtc = $item.LastWriteTimeUtc.Ticks
            sha256 = $hash
        }
    }
    finally {
        if ($null -ne $sha256) {
            $sha256.Dispose()
        }
        if ($null -ne $stream) {
            $stream.Dispose()
        }
    }
}

function Assert-EnvironmentRestored {
    foreach ($name in $environmentNames) {
        $expected = $environmentState[$name]
        $exists = Test-Path -LiteralPath "Env:$name"
        $value = [Environment]::GetEnvironmentVariable($name, 'Process')
        if ($exists -ne $expected.Exists -or $value -ne $expected.Value) {
            throw "The harness did not restore process environment variable $name."
        }
    }
}

function Invoke-FailureSelfTests {
    $hostExecutable = (Get-Process -Id $PID).Path
    $cases = @(
        @{ Injection = 'PrimaryRuntimeNotReady'; Code = 'PRIMARY_RUNTIME_NOT_READY' },
        @{ Injection = 'SecondaryNotExited'; Code = 'SECONDARY_NOT_EXITED' },
        @{ Injection = 'SecondaryReachedRuntime'; Code = 'SECONDARY_REACHED_RUNTIME' }
    )
    $beforeDirectories = @(
        Get-ChildItem -LiteralPath $temporaryRoot -Directory -Filter 'document-studio-single-instance-*' -ErrorAction SilentlyContinue |
            Select-Object -ExpandProperty FullName
    )
    foreach ($case in $cases) {
        $arguments = @(
            '-NoLogo',
            '-NoProfile',
            '-ExecutionPolicy', 'Bypass',
            '-File', $PSCommandPath,
            '-Executable', $resolvedExecutable,
            '-FailureInjection', $case.Injection,
            '-SkipFailureSelfTests'
        )
        $previousErrorActionPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = 'Continue'
            $output = @(& $hostExecutable @arguments 2>&1 | ForEach-Object { [string]$_ })
            $exitCode = $LASTEXITCODE
        }
        finally {
            $ErrorActionPreference = $previousErrorActionPreference
        }
        if ($exitCode -eq 0) {
            throw "Failure self-test $($case.Injection) unexpectedly passed."
        }
        $joined = $output -join [Environment]::NewLine
        if (-not $joined.Contains("FAILURE_CODE=$($case.Code)")) {
            throw "Failure self-test $($case.Injection) did not report $($case.Code)."
        }
        if (-not $joined.Contains('SINGLE_INSTANCE_FAILURE_DIAGNOSTICS_BEGIN') -or
            -not $joined.Contains('SINGLE_INSTANCE_FAILURE_DIAGNOSTICS_END')) {
            throw "Failure self-test $($case.Injection) did not print bounded diagnostics."
        }
        if (-not $joined.Contains('CLEANUP_STATUS=complete')) {
            throw "Failure self-test $($case.Injection) did not prove cleanup."
        }
        Assert-EnvironmentRestored
    }
    $afterDirectories = @(
        Get-ChildItem -LiteralPath $temporaryRoot -Directory -Filter 'document-studio-single-instance-*' -ErrorAction SilentlyContinue |
            Select-Object -ExpandProperty FullName
    )
    $unexpectedDirectories = @($afterDirectories | Where-Object { $beforeDirectories -notcontains $_ })
    if ($unexpectedDirectories.Count -ne 0) {
        throw "Failure self-tests left owned directories: $($unexpectedDirectories -join ',')"
    }
    $leftoverWebViews = @(
        Get-CimInstance Win32_Process -Filter "Name = 'msedgewebview2.exe'" -ErrorAction SilentlyContinue |
            Where-Object { ([string]$_.CommandLine).Contains('document-studio-single-instance-') }
    )
    if ($leftoverWebViews.Count -ne 0) {
        throw 'Failure self-tests left owned WebView2 processes.'
    }
    Write-Output 'Single-instance failure-path self-tests passed: 3 cases produced bounded diagnostics and complete cleanup.'
}

try {
    if (Test-Path -LiteralPath $ownedRoot) {
        Throw-HarnessFailure -Code 'CLEANUP_FAILED' -Message 'The newly generated owned test root already exists.'
    }
    if (-not (Test-PathWithin -Candidate $ownedRoot -Boundary $temporaryRoot)) {
        Throw-HarnessFailure -Code 'CLEANUP_FAILED' -Message 'The isolated test root escaped the operating-system temporary directory.'
    }

    $productionProfiles = @()
    if (-not [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        $productionProfiles += [System.IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'studio.document.app')).TrimEnd('\')
    }
    foreach ($productionProfile in $productionProfiles) {
        if ((Test-PathWithin -Candidate $ownedRoot -Boundary $productionProfile -AllowEqual) -or
            (Test-PathWithin -Candidate $productionProfile -Boundary $ownedRoot -AllowEqual)) {
            Throw-HarnessFailure -Code 'CLEANUP_FAILED' -Message 'The owned test root overlaps a production application profile.'
        }
    }

    New-Item -ItemType Directory -Path $ownedRoot | Out-Null
    New-Item -ItemType Directory -Path $appDataDirectory | Out-Null
    New-Item -ItemType Directory -Path $primaryUdfDirectory | Out-Null
    [void](Assert-OwnedDirectory -Path $ownedRoot)
    [void](Assert-OwnedDirectory -Path $appDataDirectory)
    [void](Assert-OwnedDirectory -Path $primaryUdfDirectory -MustBeEmpty)

    Set-TestEnvironmentValue -Name 'DOCUMENT_STUDIO_TEST_APP_DATA' -Value $appDataDirectory
    Set-TestEnvironmentValue -Name 'DOCUMENT_STUDIO_TEST_WEBVIEW2_DATA_DIR' -Value $primaryUdfDirectory
    Set-TestEnvironmentValue -Name 'DOCUMENT_STUDIO_TEST_CDP_PORT' -Value $null
    Set-TestEnvironmentValue -Name 'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS' -Value $null
    Set-TestEnvironmentValue -Name 'WEBVIEW2_USER_DATA_FOLDER' -Value $null

    $phase = 'PRIMARY_STARTUP'
    $startupWatch.Start()
    $primary = Start-Process -FilePath $resolvedExecutable -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput $primaryStdout -RedirectStandardError $primaryStderr
    [void]$primary.Handle
    $metrics.processLaunchMs = $startupWatch.ElapsedMilliseconds
    $primaryMarker = Join-Path $appDataDirectory "runtime-started-$($primary.Id)"
    $firstProgressMs = $null

    while ($true) {
        $primary.Refresh()
        if ($primary.HasExited) {
            Throw-HarnessFailure -Code 'PRIMARY_EXITED' -Message "The primary exited before runtime setup completed (exit $($primary.ExitCode))."
        }

        $elapsed = $startupWatch.ElapsedMilliseconds
        if ($null -eq $metrics.metadataCreatedMs -and (Test-Path -LiteralPath $metadataPath -PathType Leaf)) {
            $metrics.metadataCreatedMs = $elapsed
        }
        if ($null -eq $metrics.workspaceCreatedMs -and (Test-Path -LiteralPath $workspacePath -PathType Container)) {
            $metrics.workspaceCreatedMs = $elapsed
        }
        $primaryWebViews = @(Get-WebViewProcesses -BrowserDataDirectories @($primaryBrowserDataDirectory))
        if ($null -eq $metrics.webviewChildCreatedMs -and $primaryWebViews.Count -gt 0) {
            $metrics.webviewChildCreatedMs = $elapsed
        }
        if ($null -eq $metrics.udfPopulatedMs -and (Test-DirectoryPopulated -Path $primaryUdfDirectory)) {
            $metrics.udfPopulatedMs = $elapsed
        }

        $markerPresent = Test-Path -LiteralPath $primaryMarker -PathType Leaf
        if ($markerPresent -and $FailureInjection -eq 'PrimaryRuntimeNotReady') {
            Remove-Item -LiteralPath $primaryMarker -Force
            $primaryMarkerSuppressed = $true
            $markerPresent = $false
        }
        if ($markerPresent) {
            $metrics.runtimeMarkerCreatedMs = $elapsed
            break
        }

        $hasProgress = ($null -ne $metrics.metadataCreatedMs) -or
            ($null -ne $metrics.workspaceCreatedMs) -or
            ($null -ne $metrics.webviewChildCreatedMs) -or
            ($null -ne $metrics.udfPopulatedMs) -or
            $primaryMarkerSuppressed
        if ($hasProgress -and $null -eq $firstProgressMs) {
            $firstProgressMs = $elapsed
            $phase = 'PRIMARY_SETUP'
            $phaseStartedMs = $elapsed
        }
        if ($null -eq $firstProgressMs -and $elapsed -ge $firstProgressDeadlineMs) {
            Throw-HarnessFailure -Code 'PRIMARY_NO_PROGRESS' -Message "The primary made no observable startup progress within $firstProgressDeadlineMs milliseconds."
        }
        if ($null -ne $firstProgressMs -and ($elapsed - $firstProgressMs) -ge $setupDeadlineMs) {
            Throw-HarnessFailure -Code 'PRIMARY_RUNTIME_NOT_READY' -Message "The primary made startup progress but did not create its runtime marker within the $setupDeadlineMs millisecond setup phase."
        }
        if ($elapsed -ge $absoluteDeadlineMs) {
            Throw-HarnessFailure -Code 'PRIMARY_RUNTIME_NOT_READY' -Message "The primary did not create its runtime marker before the $absoluteDeadlineMs millisecond hard ceiling."
        }
        Start-Sleep -Milliseconds 100
    }
    $startupWatch.Stop()

    $primaryWebViews = @(Get-WebViewProcesses -BrowserDataDirectories @($primaryBrowserDataDirectory))
    if ($primaryWebViews.Count -eq 0) {
        Throw-HarnessFailure -Code 'PRIMARY_RUNTIME_NOT_READY' -Message 'The primary runtime marker appeared without an owned WebView2 browser process.'
    }
    foreach ($webView in $primaryWebViews) {
        if (([string]$webView.CommandLine) -match '(?i)--remote-(debugging|allow-origins)') {
            Throw-HarnessFailure -Code 'PRIMARY_RUNTIME_NOT_READY' -Message 'The single-instance primary unexpectedly enabled remote debugging.'
        }
    }
    $markersBeforeSecondary = @(Get-MarkerNames)
    if ($markersBeforeSecondary.Count -ne 1 -or $markersBeforeSecondary[0] -ne [System.IO.Path]::GetFileName($primaryMarker)) {
        Throw-HarnessFailure -Code 'PRIMARY_RUNTIME_NOT_READY' -Message 'The primary did not own exactly one runtime marker.'
    }

    New-Item -ItemType Directory -Path $secondaryUdfDirectory | Out-Null
    [void](Assert-OwnedDirectory -Path $secondaryUdfDirectory -MustBeEmpty)
    Set-TestEnvironmentValue -Name 'DOCUMENT_STUDIO_TEST_WEBVIEW2_DATA_DIR' -Value $secondaryUdfDirectory
    $metadataBeforeSecondary = Get-MetadataSnapshot

    $phase = 'SECONDARY_EXIT'
    $phaseStartedMs = 0L
    $secondaryWatch.Start()
    if ($FailureInjection -eq 'SecondaryNotExited') {
        $hostExecutable = (Get-Process -Id $PID).Path
        $secondary = Start-Process -FilePath $hostExecutable -PassThru -WindowStyle Hidden `
            -ArgumentList @('-NoLogo', '-NoProfile', '-Command', 'Start-Sleep -Seconds 60') `
            -RedirectStandardOutput $secondaryStdout -RedirectStandardError $secondaryStderr
    }
    else {
        $secondary = Start-Process -FilePath $resolvedExecutable -PassThru -WindowStyle Hidden `
            -RedirectStandardOutput $secondaryStdout -RedirectStandardError $secondaryStderr
    }
    [void]$secondary.Handle
    $secondaryMarker = Join-Path $appDataDirectory "runtime-started-$($secondary.Id)"
    if ($FailureInjection -eq 'SecondaryReachedRuntime') {
        [System.IO.File]::WriteAllText($secondaryMarker, 'simulated test-only secondary marker')
    }

    while (-not $secondary.HasExited) {
        if (Test-Path -LiteralPath $secondaryMarker -PathType Leaf) {
            Throw-HarnessFailure -Code 'SECONDARY_REACHED_RUNTIME' -Message 'The secondary created a runtime marker.'
        }
        $secondaryWebViews = @(Get-WebViewProcesses -BrowserDataDirectories @($secondaryBrowserDataDirectory))
        if ($secondaryWebViews.Count -gt 0 -or (Test-DirectoryPopulated -Path $secondaryUdfDirectory)) {
            Throw-HarnessFailure -Code 'SECONDARY_CREATED_WEBVIEW' -Message 'The secondary initialized its isolated WebView2 profile.'
        }
        if ($secondaryWatch.ElapsedMilliseconds -ge $secondaryExitDeadlineMs) {
            Throw-HarnessFailure -Code 'SECONDARY_NOT_EXITED' -Message "The secondary did not exit within $secondaryExitDeadlineMs milliseconds."
        }
        Start-Sleep -Milliseconds 50
        $secondary.Refresh()
    }
    $secondaryWatch.Stop()
    $secondary.WaitForExit()
    $secondary.Refresh()
    $metrics.secondaryExitMs = $secondaryWatch.ElapsedMilliseconds

    if ($secondary.ExitCode -ne 0) {
        Throw-HarnessFailure -Code 'SECONDARY_EXIT_NONZERO' -Message "The secondary exited with code $($secondary.ExitCode)."
    }
    if (Test-Path -LiteralPath $secondaryMarker -PathType Leaf) {
        Throw-HarnessFailure -Code 'SECONDARY_REACHED_RUNTIME' -Message 'The secondary reached runtime setup before exiting.'
    }
    if (@(Get-WebViewProcesses -BrowserDataDirectories @($secondaryBrowserDataDirectory)).Count -gt 0 -or
        (Test-DirectoryPopulated -Path $secondaryUdfDirectory)) {
        Throw-HarnessFailure -Code 'SECONDARY_CREATED_WEBVIEW' -Message 'The secondary created or retained WebView2 state.'
    }

    $primary.Refresh()
    if ($primary.HasExited) {
        Throw-HarnessFailure -Code 'PRIMARY_EXITED' -Message 'The primary exited while the secondary was being tested.'
    }
    if (-not (Test-Path -LiteralPath $primaryMarker -PathType Leaf)) {
        Throw-HarnessFailure -Code 'PRIMARY_EXITED' -Message 'The primary runtime marker disappeared while the secondary was being tested.'
    }
    $markersAfterSecondary = @(Get-MarkerNames)
    if ($markersAfterSecondary.Count -ne 1 -or $markersAfterSecondary[0] -ne [System.IO.Path]::GetFileName($primaryMarker)) {
        Throw-HarnessFailure -Code 'SECONDARY_REACHED_RUNTIME' -Message 'Exactly one primary runtime marker was not preserved.'
    }
    $metadataAfterSecondary = Get-MetadataSnapshot
    if (($null -eq $metadataBeforeSecondary) -or ($null -eq $metadataAfterSecondary) -or
        (($metadataBeforeSecondary | ConvertTo-Json -Compress) -ne ($metadataAfterSecondary | ConvertTo-Json -Compress))) {
        Throw-HarnessFailure -Code 'SECONDARY_REACHED_RUNTIME' -Message 'Metadata runtime state changed during the secondary launch.'
    }
}
catch {
    if ($_.Exception.Data.Contains('FailureCode')) {
        $failureCode = [string]$_.Exception.Data['FailureCode']
    }
    else {
        $failureCode = 'HARNESS_ERROR'
    }
    $failureMessage = $_.Exception.Message
    Write-FailureDiagnostics -Code $failureCode -Message $failureMessage
}
finally {
    $cleanupWatch.Start()
    try {
        Stop-OwnedProcesses
        Restore-TestEnvironment
        Remove-OwnedDirectory
        Assert-EnvironmentRestored
        if (Test-Path -LiteralPath $ownedRoot) {
            throw 'The owned test directory remained after cleanup.'
        }
        if (@(Get-WebViewProcesses -BrowserDataDirectories @($primaryBrowserDataDirectory, $secondaryBrowserDataDirectory)).Count -ne 0) {
            throw 'Owned WebView2 processes remained after cleanup.'
        }
        $cleanupComplete = $true
    }
    catch {
        if ($null -eq $failureCode) {
            $failureCode = 'CLEANUP_FAILED'
            $failureMessage = $_.Exception.Message
            if (-not $diagnosticsPrinted) {
                Write-FailureDiagnostics -Code $failureCode -Message $failureMessage
            }
        }
        else {
            $failureCode = 'CLEANUP_FAILED'
            $failureMessage = "Cleanup failed after the original harness failure: $($_.Exception.Message)"
        }
    }
    $cleanupWatch.Stop()
    $metrics.cleanupMs = $cleanupWatch.ElapsedMilliseconds
}

if ($cleanupComplete) {
    Write-Output 'CLEANUP_STATUS=complete'
}
if ($null -ne $failureCode) {
    Write-Output "FAILURE_CODE=$failureCode"
    throw "$failureCode`: $failureMessage"
}

Write-Output "SINGLE_INSTANCE_TIMING=$($metrics | ConvertTo-Json -Compress)"
Write-Output 'Single-instance smoke passed: isolated primary and secondary profiles, one runtime marker, no secondary runtime or WebView initialization.'

if (-not $SkipFailureSelfTests) {
    Invoke-FailureSelfTests
}
