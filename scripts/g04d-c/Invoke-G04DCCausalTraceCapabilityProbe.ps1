param(
    [Parameter(Mandatory = $true)] [string]$RepositoryRoot,
    [Parameter(Mandatory = $true)] [string]$EvidenceDirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$blockedStatus = 'G04D-C10 CAUSAL TRACE CAPABILITY BLOCKED - CONTROLLED DISPOSABLE WINDOWS VM OR TRACE-TOOL OWNER GATE REQUIRED'
$passStatus = 'CAUSAL_TRACE_CAPABILITY_PASS'
$maximumTraceBytes = 268435456L
$maximumRawEvidenceBytes = 536870912L
$maximumDecodedRows = 1000000
$commandTimeoutMilliseconds = 120000
$canaryTimeoutMilliseconds = 30000

function Write-G04DCTraceJson {
    param(
        [Parameter(Mandatory = $true)] [string]$Path,
        [Parameter(Mandatory = $true)] $Value
    )
    [IO.File]::WriteAllText(
        $Path,
        ($Value | ConvertTo-Json -Depth 16) + [Environment]::NewLine,
        [Text.UTF8Encoding]::new($false)
    )
    $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try { $stream.Flush($true) }
    finally { $stream.Dispose() }
}

function Get-G04DCTraceFileSeal {
    param(
        [Parameter(Mandatory = $true)] [string]$Path,
        [Parameter(Mandatory = $true)] [string]$TokenPath
    )
    $item = Get-Item -LiteralPath $Path -ErrorAction Stop
    return [pscustomobject][ordered]@{
        path = $TokenPath
        sizeBytes = [long]$item.Length
        sha256 = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}

function ConvertTo-G04DCWindowsArgument {
    param([Parameter(Mandatory = $true)] [AllowEmptyString()] [string]$Value)
    if ($Value -notmatch '[\s"]') { return $Value }
    $builder = [Text.StringBuilder]::new()
    [void]$builder.Append('"')
    $backslashes = 0
    foreach ($character in $Value.ToCharArray()) {
        if ($character -ceq '\') {
            $backslashes++
            continue
        }
        if ($character -ceq '"') {
            [void]$builder.Append(('\' * (($backslashes * 2) + 1)))
            [void]$builder.Append('"')
            $backslashes = 0
            continue
        }
        if ($backslashes -gt 0) {
            [void]$builder.Append(('\' * $backslashes))
            $backslashes = 0
        }
        [void]$builder.Append($character)
    }
    if ($backslashes -gt 0) { [void]$builder.Append(('\' * ($backslashes * 2))) }
    [void]$builder.Append('"')
    return $builder.ToString()
}

function Get-G04DCTokenizedArgument {
    param(
        [Parameter(Mandatory = $true)] [string]$Value,
        [Parameter(Mandatory = $true)] [string]$RawRoot,
        [Parameter(Mandatory = $true)] [string]$EvidenceRoot
    )
    $tokenized = $Value.Replace($RawRoot, '$RAW_ROOT')
    return $tokenized.Replace($EvidenceRoot, '$EVIDENCE_ROOT')
}

function Invoke-G04DCTraceCommand {
    param(
        [Parameter(Mandatory = $true)] [string]$Name,
        [Parameter(Mandatory = $true)] [string]$ExecutablePath,
        [Parameter(Mandatory = $true)] [string[]]$Arguments,
        [Parameter(Mandatory = $true)] [string]$RawRoot,
        [Parameter(Mandatory = $true)] [string]$EvidenceRoot,
        [int]$TimeoutMilliseconds = 120000
    )
    $stdoutPath = Join-Path $RawRoot ($Name + '.stdout.txt')
    $stderrPath = Join-Path $RawRoot ($Name + '.stderr.txt')
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $ExecutablePath
    $startInfo.Arguments = (($Arguments | ForEach-Object { ConvertTo-G04DCWindowsArgument -Value $_ }) -join ' ')
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    $startedAt = [DateTime]::UtcNow
    try {
        if (!$process.Start()) { throw "Trace command did not start: $Name" }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        if (!$process.WaitForExit($TimeoutMilliseconds)) {
            $process.Kill()
            [void]$process.WaitForExit(5000)
            throw "Trace command exceeded its bounded timeout: $Name"
        }
        $stdout = $stdoutTask.GetAwaiter().GetResult()
        $stderr = $stderrTask.GetAwaiter().GetResult()
        [IO.File]::WriteAllText($stdoutPath, $stdout, [Text.UTF8Encoding]::new($false))
        [IO.File]::WriteAllText($stderrPath, $stderr, [Text.UTF8Encoding]::new($false))
        return [pscustomobject][ordered]@{
            name = $Name
            executable = $ExecutablePath
            arguments = @($Arguments | ForEach-Object { Get-G04DCTokenizedArgument -Value $_ -RawRoot $RawRoot -EvidenceRoot $EvidenceRoot })
            exitCode = [int]$process.ExitCode
            elapsedMilliseconds = [long]([DateTime]::UtcNow - $startedAt).TotalMilliseconds
            stdout = Get-G04DCTraceFileSeal -Path $stdoutPath -TokenPath ('$RAW_ROOT\' + [IO.Path]::GetFileName($stdoutPath))
            stderr = Get-G04DCTraceFileSeal -Path $stderrPath -TokenPath ('$RAW_ROOT\' + [IO.Path]::GetFileName($stderrPath))
        }
    }
    finally { $process.Dispose() }
}

function Get-G04DCTraceToolIdentity {
    param([Parameter(Mandatory = $true)] [string]$Name)
    $command = Get-Command $Name -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if (!$command) {
        return [pscustomobject][ordered]@{ name = $Name; available = $false }
    }
    $path = [IO.Path]::GetFullPath([string]$command.Source)
    $item = Get-Item -LiteralPath $path -ErrorAction Stop
    $signature = Get-AuthenticodeSignature -LiteralPath $path
    return [pscustomobject][ordered]@{
        name = $Name
        available = $true
        canonicalPath = $path
        sizeBytes = [long]$item.Length
        sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        fileVersion = [string]$item.VersionInfo.FileVersion
        productVersion = [string]$item.VersionInfo.ProductVersion
        signatureStatus = [string]$signature.Status
        signerSubject = if ($signature.SignerCertificate) { [string]$signature.SignerCertificate.Subject } else { $null }
        signerThumbprint = if ($signature.SignerCertificate) { [string]$signature.SignerCertificate.Thumbprint } else { $null }
    }
}

function New-G04DCTraceChildCode {
    param(
        [Parameter(Mandatory = $true)] [string]$EventName,
        [Parameter(Mandatory = $true)] [string]$RegistrySubKey,
        [Parameter(Mandatory = $true)] [string]$FilePath,
        [switch]$OwnedCanary
    )
    $eventLiteral = $EventName.Replace("'", "''")
    $registryLiteral = $RegistrySubKey.Replace("'", "''")
    $fileLiteral = $FilePath.Replace("'", "''")
    $networkCode = if ($OwnedCanary) {
        @'
$client=[Net.Sockets.TcpClient]::new();try{$async=$client.BeginConnect('127.0.0.1',9,$null,$null);if($async.AsyncWaitHandle.WaitOne(250)){try{$client.EndConnect($async)}catch{}}}finally{$client.Dispose()}
'@
    }
    else { '' }
    return @"
`$ErrorActionPreference='Stop'
`$event=[Threading.EventWaitHandle]::OpenExisting('$eventLiteral')
try{if(!`$event.WaitOne(10000)){exit 91}}finally{`$event.Dispose()}
`$key=[Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('$registryLiteral')
try{`$key.SetValue('State','created',[Microsoft.Win32.RegistryValueKind]::String);`$key.SetValue('State','modified',[Microsoft.Win32.RegistryValueKind]::String);`$key.DeleteValue('State',$false)}finally{`$key.Dispose()}
[Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree('$registryLiteral',$false)
[IO.File]::WriteAllBytes('$fileLiteral',[byte[]]@(1,2,3,4))
[IO.File]::WriteAllBytes('$fileLiteral',[byte[]]@(5,6,7,8,9))
$networkCode
exit 0
"@
}

function Start-G04DCTraceChild {
    param(
        [Parameter(Mandatory = $true)] [string]$PowerShellPath,
        [Parameter(Mandatory = $true)] [string]$Code
    )
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($Code))
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $PowerShellPath
    $startInfo.Arguments = "-NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $encoded"
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (!$process.Start()) { $process.Dispose(); throw 'Synthetic trace child did not start.' }
    return $process
}

function Invoke-G04DCTraceCanaryPair {
    param(
        [Parameter(Mandatory = $true)] [string]$RawRoot,
        [Parameter(Mandatory = $true)] [string]$AttemptName,
        [int]$TimeoutMilliseconds = 30000
    )
    $canaryId = [guid]::NewGuid().ToString('N')
    $comparisonId = [guid]::NewGuid().ToString('N')
    $canaryRoot = Join-Path $RawRoot ($AttemptName + '-canary')
    [IO.Directory]::CreateDirectory($canaryRoot) | Out-Null
    $ownedFile = Join-Path $canaryRoot ($canaryId + '.bin')
    $comparisonFile = Join-Path $canaryRoot ($comparisonId + '.bin')
    $ownedRegistry = "Software\DocumentStudioG04DCC10\$canaryId"
    $comparisonRegistry = "Software\DocumentStudioG04DCC10\$comparisonId"
    $ownedEventName = 'Local\DocumentStudioG04DCC10Owned' + $canaryId
    $comparisonEventName = 'Local\DocumentStudioG04DCC10Comparison' + $comparisonId
    $ownedEvent = [Threading.EventWaitHandle]::new($false, [Threading.EventResetMode]::ManualReset, $ownedEventName)
    $comparisonEvent = [Threading.EventWaitHandle]::new($false, [Threading.EventResetMode]::ManualReset, $comparisonEventName)
    $powershellPath = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $job = [DocumentStudio.G04DC.KillOnCloseJob]::new()
    $ownedProcess = $comparisonProcess = $null
    $startedAt = [DateTime]::UtcNow
    try {
        $ownedCode = New-G04DCTraceChildCode -EventName $ownedEventName -RegistrySubKey $ownedRegistry -FilePath $ownedFile -OwnedCanary
        $comparisonCode = New-G04DCTraceChildCode -EventName $comparisonEventName -RegistrySubKey $comparisonRegistry -FilePath $comparisonFile
        $ownedProcess = Start-G04DCTraceChild -PowerShellPath $powershellPath -Code $ownedCode
        $job.Assign($ownedProcess)
        $comparisonProcess = Start-G04DCTraceChild -PowerShellPath $powershellPath -Code $comparisonCode
        [void]$ownedEvent.Set()
        [void]$comparisonEvent.Set()
        if (!$ownedProcess.WaitForExit($TimeoutMilliseconds)) {
            $job.TerminateAndVerify($ownedProcess, 5000)
            throw 'Owned synthetic trace canary exceeded its timeout.'
        }
        if (!$comparisonProcess.WaitForExit($TimeoutMilliseconds)) {
            $comparisonProcess.Kill()
            [void]$comparisonProcess.WaitForExit(5000)
            throw 'Comparison synthetic trace process exceeded its timeout.'
        }
        if ($ownedProcess.ExitCode -ne 0 -or $comparisonProcess.ExitCode -ne 0) {
            throw "Synthetic trace child failed: owned=$($ownedProcess.ExitCode), comparison=$($comparisonProcess.ExitCode)"
        }
        return [pscustomobject][ordered]@{
            canaryId = $canaryId
            comparisonId = $comparisonId
            ownedProcessId = [int]$ownedProcess.Id
            comparisonProcessId = [int]$comparisonProcess.Id
            parentProcessId = [int]$PID
            imagePath = $powershellPath
            startedUtc = $startedAt.ToString('o')
            completedUtc = [DateTime]::UtcNow.ToString('o')
            ownedRegistryTarget = '$HKCU\Software\DocumentStudioG04DCC10\' + $canaryId
            ownedFileTarget = '$RAW_ROOT\' + [IO.Path]::GetFileName($canaryRoot) + '\' + [IO.Path]::GetFileName($ownedFile)
            comparisonRegistryTarget = '$HKCU\Software\DocumentStudioG04DCC10\' + $comparisonId
            comparisonFileTarget = '$RAW_ROOT\' + [IO.Path]::GetFileName($canaryRoot) + '\' + [IO.Path]::GetFileName($comparisonFile)
            expectedOwnedOperations = @('registry-create', 'registry-set-created', 'registry-set-modified', 'registry-delete-value', 'registry-delete-key', 'file-create', 'file-modify', 'network-loopback-attempt')
            expectedComparisonOperations = @('registry-create', 'registry-set-created', 'registry-set-modified', 'registry-delete-value', 'registry-delete-key', 'file-create', 'file-modify')
            jobObjectOwned = $true
        }
    }
    finally {
        if ($ownedProcess -and !$ownedProcess.HasExited) { try { $job.TerminateAndVerify($ownedProcess, 5000) } catch { } }
        if ($comparisonProcess -and !$comparisonProcess.HasExited) { try { $comparisonProcess.Kill(); [void]$comparisonProcess.WaitForExit(5000) } catch { } }
        if ($ownedProcess) { $ownedProcess.Dispose() }
        if ($comparisonProcess) { $comparisonProcess.Dispose() }
        $job.Dispose()
        $ownedEvent.Dispose()
        $comparisonEvent.Dispose()
    }
}

function Test-G04DCPidValue {
    param(
        [AllowNull()] [string]$Value,
        [Parameter(Mandatory = $true)] [int]$ProcessId
    )
    if ([string]::IsNullOrWhiteSpace($Value)) { return $false }
    $trimmed = $Value.Trim()
    if ($trimmed -match '^0x[0-9a-f]+$') {
        try { return [Convert]::ToInt64($trimmed.Substring(2), 16) -eq $ProcessId } catch { return $false }
    }
    $number = 0L
    return [long]::TryParse($trimmed, [ref]$number) -and $number -eq $ProcessId
}

function Get-G04DCDecodedTraceCapability {
    param(
        [Parameter(Mandatory = $true)] [string]$CsvPath,
        [Parameter(Mandatory = $true)] $Canary,
        [Parameter(Mandatory = $true)] [string]$LossText,
        [int]$MaximumRows = 1000000
    )
    $rows = @(Import-Csv -LiteralPath $CsvPath)
    if ($rows.Count -gt $MaximumRows) { throw 'Decoded trace exceeded its row ceiling.' }
    $headers = if ($rows.Count -gt 0) { @($rows[0].PSObject.Properties.Name) } else { @() }
    $pidHeaders = @($headers | Where-Object { $_ -match '(?i)(^|[^a-z])(pid|process[ _-]?id)([^a-z]|$)' })
    $eventHeaders = @($headers | Where-Object { $_ -match '(?i)event|opcode|task|type' })
    $timeHeaders = @($headers | Where-Object { $_ -match '(?i)time|clock' })
    $schemaHeaders = @($headers | Where-Object { $_ -match '(?i)provider|event[ _-]?id|opcode|task' })
    $resultHeaders = @($headers | Where-Object { $_ -match '(?i)status|result|return' })
    $matched = [System.Collections.Generic.List[object]]::new()
    $ownedRegistryRows = 0
    $ownedFileRows = 0
    $ownedProcessStartRows = 0
    $ownedProcessEndRows = 0
    $ownedImageRows = 0
    $ownedNetworkRows = 0
    $comparisonTargetRows = 0
    $comparisonMisattributedRows = 0
    $ownedResultRows = 0
    for ($index = 0; $index -lt $rows.Count; $index++) {
        $row = $rows[$index]
        $ownedPid = @($pidHeaders | Where-Object { Test-G04DCPidValue -Value ([string]$row.$_) -ProcessId $Canary.ownedProcessId }).Count -gt 0
        $comparisonPid = @($pidHeaders | Where-Object { Test-G04DCPidValue -Value ([string]$row.$_) -ProcessId $Canary.comparisonProcessId }).Count -gt 0
        $values = @($row.PSObject.Properties | ForEach-Object { [string]$_.Value })
        $rowText = $values -join '|'
        $eventName = (@($eventHeaders | ForEach-Object { [string]$row.$_ } | Where-Object { ![string]::IsNullOrWhiteSpace($_) }) -join '|')
        $timeValue = @($timeHeaders | ForEach-Object { [string]$row.$_ } | Where-Object { ![string]::IsNullOrWhiteSpace($_) } | Select-Object -First 1)
        $resultValue = @($resultHeaders | ForEach-Object { [string]$row.$_ } | Where-Object { ![string]::IsNullOrWhiteSpace($_) } | Select-Object -First 1)
        $targetToken = $null
        if ($rowText.IndexOf([string]$Canary.canaryId, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
            if ($rowText -match '(?i)reg') { $ownedRegistryRows++ ; $targetToken = 'OWNED_REGISTRY_TARGET' }
            if ($rowText -match '(?i)file|io') { $ownedFileRows++ ; $targetToken = 'OWNED_FILE_TARGET' }
        }
        if ($rowText.IndexOf([string]$Canary.comparisonId, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
            $comparisonTargetRows++
            if ($ownedPid -and !$comparisonPid) { $comparisonMisattributedRows++ }
        }
        if ($ownedPid -and $rowText -match '(?i)process' -and $rowText -match '(?i)start|dcstart') { $ownedProcessStartRows++ }
        if ($ownedPid -and $rowText -match '(?i)process' -and $rowText -match '(?i)end|stop|dcend') { $ownedProcessEndRows++ }
        if ($ownedPid -and $rowText -match '(?i)image' -and $rowText -match '(?i)powershell[.]exe') { $ownedImageRows++ }
        if ($ownedPid -and $rowText -match '(?i)network|tcp|127[.]0[.]0[.]1') { $ownedNetworkRows++ }
        if ($ownedPid -and $resultValue.Count -gt 0) { $ownedResultRows++ }
        if (($ownedPid -or $comparisonPid) -and $matched.Count -lt 64) {
            $matched.Add([pscustomobject][ordered]@{
                sequenceIndex = $index
                processId = if ($ownedPid) { [int]$Canary.ownedProcessId } else { [int]$Canary.comparisonProcessId }
                event = if ([string]::IsNullOrWhiteSpace($eventName)) { 'UNNAMED' } else { $eventName }
                timestamp = if ($timeValue.Count -gt 0) { [string]$timeValue[0] } else { $null }
                result = if ($resultValue.Count -gt 0) { [string]$resultValue[0] } else { $null }
                targetToken = $targetToken
            })
        }
    }
    $lossMatches = @([regex]::Matches($LossText, '(?im)(dropped event|events lost|buffers lost)\s*[:=]\s*([0-9]+)'))
    $lossValues = @($lossMatches | ForEach-Object { [long]$_.Groups[2].Value })
    $lossCounterAvailable = $lossValues.Count -gt 0
    $zeroLoss = $lossCounterAvailable -and @($lossValues | Where-Object { $_ -ne 0 }).Count -eq 0
    $capabilities = [pscustomobject][ordered]@{
        decoderSchemaAvailable = $schemaHeaders.Count -ge 2
        processLifetime = $ownedProcessStartRows -gt 0 -and $ownedProcessEndRows -gt 0
        parentProcessAttribution = $rows.Count -gt 0 -and (@($rows | Where-Object { ($_.PSObject.Properties.Value -join '|') -match [regex]::Escape([string]$Canary.parentProcessId) }).Count -gt 0)
        imageLoadAttribution = $ownedImageRows -gt 0
        registryTargetAttribution = $ownedRegistryRows -ge 3
        fileTargetAttribution = $ownedFileRows -ge 2
        networkTargetAttribution = $ownedNetworkRows -gt 0
        operationResultAvailable = $ownedResultRows -gt 0
        unrelatedProcessDistinguished = $comparisonTargetRows -gt 0 -and $comparisonMisattributedRows -eq 0
        lossCounterAvailable = $lossCounterAvailable
        zeroEventsLost = $zeroLoss
    }
    $passed = @($capabilities.PSObject.Properties | Where-Object { ![bool]$_.Value }).Count -eq 0
    return [pscustomobject][ordered]@{
        passed = $passed
        decodedRowCount = $rows.Count
        headerCount = $headers.Count
        pidFieldCount = $pidHeaders.Count
        schemaFieldCount = $schemaHeaders.Count
        matchedOwnedRegistryRowCount = $ownedRegistryRows
        matchedOwnedFileRowCount = $ownedFileRows
        matchedOwnedProcessStartRowCount = $ownedProcessStartRows
        matchedOwnedProcessEndRowCount = $ownedProcessEndRows
        matchedOwnedImageRowCount = $ownedImageRows
        matchedOwnedNetworkRowCount = $ownedNetworkRows
        matchedComparisonTargetRowCount = $comparisonTargetRows
        comparisonMisattributionCount = $comparisonMisattributedRows
        matchedOwnedResultRowCount = $ownedResultRows
        lossCounters = $lossValues
        capabilities = $capabilities
        sanitizedEvents = @($matched.ToArray())
    }
}

function Invoke-G04DCDecodeAttempt {
    param(
        [Parameter(Mandatory = $true)] [string]$AttemptName,
        [Parameter(Mandatory = $true)] [string]$TracePath,
        [Parameter(Mandatory = $true)] [string]$LossText,
        [Parameter(Mandatory = $true)] $Canary,
        [Parameter(Mandatory = $true)] $TracerptTool,
        [Parameter(Mandatory = $true)] [string]$RawRoot,
        [Parameter(Mandatory = $true)] [string]$EvidenceRoot,
        [Parameter(Mandatory = $true)] [System.Collections.Generic.List[object]]$Commands
    )
    $traceItem = Get-Item -LiteralPath $TracePath -ErrorAction Stop
    if ($traceItem.Length -gt $maximumTraceBytes) { throw 'Raw ETL exceeded its hard byte ceiling.' }
    $csvPath = Join-Path $RawRoot ($AttemptName + '.csv')
    $summaryPath = Join-Path $RawRoot ($AttemptName + '.summary.txt')
    $decode = Invoke-G04DCTraceCommand -Name ($AttemptName + '-tracerpt') -ExecutablePath $TracerptTool.canonicalPath -Arguments @($TracePath, '-o', $csvPath, '-of', 'CSV', '-lr', '-summary', $summaryPath, '-y') -RawRoot $RawRoot -EvidenceRoot $EvidenceRoot -TimeoutMilliseconds $commandTimeoutMilliseconds
    $Commands.Add($decode)
    if ($decode.exitCode -ne 0 -or !(Test-Path -LiteralPath $csvPath -PathType Leaf)) {
        throw 'The installed Microsoft tracerpt decoder did not produce CSV evidence.'
    }
    $summaryText = if (Test-Path -LiteralPath $summaryPath -PathType Leaf) { Get-Content -LiteralPath $summaryPath -Raw } else { '' }
    $decoded = Get-G04DCDecodedTraceCapability -CsvPath $csvPath -Canary $Canary -LossText ($LossText + [Environment]::NewLine + $summaryText) -MaximumRows $maximumDecodedRows
    return [pscustomobject][ordered]@{
        name = $AttemptName
        trace = Get-G04DCTraceFileSeal -Path $TracePath -TokenPath ('$RAW_ROOT\' + [IO.Path]::GetFileName($TracePath))
        decoder = 'tracerpt'
        canary = $Canary
        decoded = $decoded
    }
}

$resolvedRepository = (Resolve-Path -LiteralPath $RepositoryRoot).Path
$resolvedEvidence = (Resolve-Path -LiteralPath $EvidenceDirectory).Path
if (![IO.Path]::GetFullPath($resolvedEvidence).StartsWith([IO.Path]::GetFullPath($env:RUNNER_TEMP), [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Trace capability evidence must be under RUNNER_TEMP.'
}
$reportPath = Join-Path $resolvedEvidence 'causal-trace-capability.json'
if (Test-Path -LiteralPath $reportPath) { throw "Refusing to overwrite trace capability evidence: $reportPath" }
$rawRoot = Join-Path $resolvedEvidence ('.trace-capability-raw-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($rawRoot) | Out-Null
$rawMarkerPath = Join-Path $rawRoot '.g04d-c10-trace-raw-root'
$rawMarkerText = 'DOCUMENT_STUDIO_G04D_C10_TRACE_RAW_V1'
[IO.File]::WriteAllText($rawMarkerPath, $rawMarkerText, [Text.Encoding]::ASCII)
Import-Module (Join-Path $resolvedRepository 'scripts\g04d-c\G04DC.Common.psm1') -Force

$commands = [System.Collections.Generic.List[object]]::new()
$attempts = [System.Collections.Generic.List[object]]::new()
$blockers = [System.Collections.Generic.List[string]]::new()
$tools = @(
    Get-G04DCTraceToolIdentity -Name 'wpr.exe'
    Get-G04DCTraceToolIdentity -Name 'logman.exe'
    Get-G04DCTraceToolIdentity -Name 'tracerpt.exe'
)
$report = [ordered]@{
    schemaVersion = 1
    status = 'TRACE_CAPABILITY_PROBE_RUNNING'
    checkedOutSha = $env:GITHUB_SHA
    runnerImage = $env:ImageOS
    operatingSystem = [Environment]::OSVersion.VersionString
    processArchitecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
    isAdministrator = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    toolPreferenceOrder = @('wpr.exe', 'built-in-wpr-profiles', 'logman.exe-kernel-session', 'tracerpt.exe')
    logicalPathTokens = [ordered]@{ rawRoot = '$RAW_ROOT'; evidenceRoot = '$EVIDENCE_ROOT'; currentUserHive = '$HKCU' }
    tools = $tools
    commands = @()
    providerIdentifiers = @()
    builtInProfiles = @()
    selectedProfiles = @()
    attempts = @()
    rawArtifacts = @()
    rawArtifactCount = 0
    rawArtifactByteCount = 0L
    rawArtifactsRemoved = $false
    blockerReasons = @()
}
Write-G04DCTraceJson -Path $reportPath -Value $report

$wprActive = $false
$logmanActive = $false
$logmanSession = 'DocumentStudioG04DC10' + [guid]::NewGuid().ToString('N')
try {
    $wpr = @($tools | Where-Object { $_.name -ceq 'wpr.exe' })[0]
    $logman = @($tools | Where-Object { $_.name -ceq 'logman.exe' })[0]
    $tracerpt = @($tools | Where-Object { $_.name -ceq 'tracerpt.exe' })[0]
    if (!$tracerpt.available) { $blockers.Add('TRACERPT_UNAVAILABLE') }

    if ($wpr.available) {
        $profilesCommand = Invoke-G04DCTraceCommand -Name 'wpr-profiles' -ExecutablePath $wpr.canonicalPath -Arguments @('-profiles') -RawRoot $rawRoot -EvidenceRoot $resolvedEvidence -TimeoutMilliseconds $commandTimeoutMilliseconds
        $commands.Add($profilesCommand)
        $profilesText = Get-Content -LiteralPath (Join-Path $rawRoot 'wpr-profiles.stdout.txt') -Raw
        $knownProfiles = @('GeneralProfile', 'FileIO', 'Registry', 'Network')
        $availableProfiles = @($knownProfiles | Where-Object { $profilesText -match ('(?im)^\s*' + [regex]::Escape($_) + '\s+') })
        $report.builtInProfiles = $availableProfiles
        $report.selectedProfiles = $availableProfiles
        if ($profilesCommand.exitCode -eq 0 -and $availableProfiles.Count -eq $knownProfiles.Count -and $tracerpt.available) {
            $profileDetails = Invoke-G04DCTraceCommand -Name 'wpr-profiledetails' -ExecutablePath $wpr.canonicalPath -Arguments @('-profiledetails', ($availableProfiles -join '+')) -RawRoot $rawRoot -EvidenceRoot $resolvedEvidence -TimeoutMilliseconds $commandTimeoutMilliseconds
            $commands.Add($profileDetails)
            $startArguments = [System.Collections.Generic.List[string]]::new()
            foreach ($profile in $availableProfiles) { $startArguments.Add('-start'); $startArguments.Add($profile) }
            $wprStart = Invoke-G04DCTraceCommand -Name 'wpr-start' -ExecutablePath $wpr.canonicalPath -Arguments $startArguments.ToArray() -RawRoot $rawRoot -EvidenceRoot $resolvedEvidence -TimeoutMilliseconds $commandTimeoutMilliseconds
            $commands.Add($wprStart)
            if ($wprStart.exitCode -eq 0) {
                $wprActive = $true
                $canary = Invoke-G04DCTraceCanaryPair -RawRoot $rawRoot -AttemptName 'wpr' -TimeoutMilliseconds $canaryTimeoutMilliseconds
                $wprStatus = Invoke-G04DCTraceCommand -Name 'wpr-status' -ExecutablePath $wpr.canonicalPath -Arguments @('-status', 'collectors', '-details') -RawRoot $rawRoot -EvidenceRoot $resolvedEvidence -TimeoutMilliseconds $commandTimeoutMilliseconds
                $commands.Add($wprStatus)
                $wprStatusText = (Get-Content -LiteralPath (Join-Path $rawRoot 'wpr-status.stdout.txt') -Raw) + (Get-Content -LiteralPath (Join-Path $rawRoot 'wpr-status.stderr.txt') -Raw)
                $wprTrace = Join-Path $rawRoot 'wpr.etl'
                $wprStop = Invoke-G04DCTraceCommand -Name 'wpr-stop' -ExecutablePath $wpr.canonicalPath -Arguments @('-stop', $wprTrace, '-skipPdbGen') -RawRoot $rawRoot -EvidenceRoot $resolvedEvidence -TimeoutMilliseconds $commandTimeoutMilliseconds
                $commands.Add($wprStop)
                $wprActive = $false
                if ($wprStop.exitCode -eq 0 -and (Test-Path -LiteralPath $wprTrace -PathType Leaf)) {
                    try { $attempts.Add((Invoke-G04DCDecodeAttempt -AttemptName 'wpr' -TracePath $wprTrace -LossText $wprStatusText -Canary $canary -TracerptTool $tracerpt -RawRoot $rawRoot -EvidenceRoot $resolvedEvidence -Commands $commands)) }
                    catch { $blockers.Add('WPR_DECODE_OR_ATTRIBUTION_INSUFFICIENT') }
                }
                else { $blockers.Add('WPR_TRACE_STOP_FAILED') }
            }
            else { $blockers.Add('WPR_TRACE_START_FAILED') }
        }
        else { $blockers.Add('WPR_BUILT_IN_PROFILE_SET_INCOMPLETE') }
    }
    else { $blockers.Add('WPR_UNAVAILABLE') }

    $wprPassed = @($attempts.ToArray() | Where-Object { $_.name -ceq 'wpr' -and $_.decoded.passed }).Count -gt 0
    if (!$wprPassed -and $logman.available -and $tracerpt.available) {
        $providerQuery = Invoke-G04DCTraceCommand -Name 'logman-kernel-provider' -ExecutablePath $logman.canonicalPath -Arguments @('query', 'providers', 'Windows Kernel Trace') -RawRoot $rawRoot -EvidenceRoot $resolvedEvidence -TimeoutMilliseconds $commandTimeoutMilliseconds
        $commands.Add($providerQuery)
        $providerText = (Get-Content -LiteralPath (Join-Path $rawRoot 'logman-kernel-provider.stdout.txt') -Raw) + (Get-Content -LiteralPath (Join-Path $rawRoot 'logman-kernel-provider.stderr.txt') -Raw)
        $report.providerIdentifiers = @([regex]::Matches($providerText, '(?i)[{][0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}[}]') | ForEach-Object { $_.Value.ToLowerInvariant() } | Sort-Object -Unique)
        $logmanTrace = Join-Path $rawRoot 'logman.etl'
        $kernelFlags = '0x06030205'
        $logmanStart = Invoke-G04DCTraceCommand -Name 'logman-start' -ExecutablePath $logman.canonicalPath -Arguments @('create', 'trace', $logmanSession, '-ow', '-o', $logmanTrace, '-p', 'Windows Kernel Trace', $kernelFlags, '0xff', '-nb', '64', '256', '-bs', '1024', '-f', 'bincirc', '-max', '128', '-ets') -RawRoot $rawRoot -EvidenceRoot $resolvedEvidence -TimeoutMilliseconds $commandTimeoutMilliseconds
        $commands.Add($logmanStart)
        if ($providerQuery.exitCode -eq 0 -and $logmanStart.exitCode -eq 0) {
            $logmanActive = $true
            $canary = Invoke-G04DCTraceCanaryPair -RawRoot $rawRoot -AttemptName 'logman' -TimeoutMilliseconds $canaryTimeoutMilliseconds
            $logmanStatus = Invoke-G04DCTraceCommand -Name 'logman-status' -ExecutablePath $logman.canonicalPath -Arguments @('query', $logmanSession, '-ets') -RawRoot $rawRoot -EvidenceRoot $resolvedEvidence -TimeoutMilliseconds $commandTimeoutMilliseconds
            $commands.Add($logmanStatus)
            $logmanStatusText = (Get-Content -LiteralPath (Join-Path $rawRoot 'logman-status.stdout.txt') -Raw) + (Get-Content -LiteralPath (Join-Path $rawRoot 'logman-status.stderr.txt') -Raw)
            $logmanStop = Invoke-G04DCTraceCommand -Name 'logman-stop' -ExecutablePath $logman.canonicalPath -Arguments @('stop', $logmanSession, '-ets') -RawRoot $rawRoot -EvidenceRoot $resolvedEvidence -TimeoutMilliseconds $commandTimeoutMilliseconds
            $commands.Add($logmanStop)
            $logmanActive = $false
            if ($logmanStop.exitCode -eq 0 -and (Test-Path -LiteralPath $logmanTrace -PathType Leaf)) {
                try { $attempts.Add((Invoke-G04DCDecodeAttempt -AttemptName 'logman' -TracePath $logmanTrace -LossText $logmanStatusText -Canary $canary -TracerptTool $tracerpt -RawRoot $rawRoot -EvidenceRoot $resolvedEvidence -Commands $commands)) }
                catch { $blockers.Add('LOGMAN_DECODE_OR_ATTRIBUTION_INSUFFICIENT') }
            }
            else { $blockers.Add('LOGMAN_TRACE_STOP_FAILED') }
        }
        else { $blockers.Add('LOGMAN_KERNEL_SESSION_START_FAILED') }
    }
}
catch {
    $blockers.Add('TRACE_CAPABILITY_PROBE_INTERNAL_FAILURE')
}
finally {
    if ($wprActive) {
        try {
            $wprTool = @($tools | Where-Object { $_.name -ceq 'wpr.exe' })[0]
            $commands.Add((Invoke-G04DCTraceCommand -Name 'wpr-cancel' -ExecutablePath $wprTool.canonicalPath -Arguments @('-cancel') -RawRoot $rawRoot -EvidenceRoot $resolvedEvidence -TimeoutMilliseconds $commandTimeoutMilliseconds))
        }
        catch { $blockers.Add('WPR_CLEANUP_FAILED') }
    }
    if ($logmanActive) {
        try {
            $logmanTool = @($tools | Where-Object { $_.name -ceq 'logman.exe' })[0]
            $commands.Add((Invoke-G04DCTraceCommand -Name 'logman-cleanup-stop' -ExecutablePath $logmanTool.canonicalPath -Arguments @('stop', $logmanSession, '-ets') -RawRoot $rawRoot -EvidenceRoot $resolvedEvidence -TimeoutMilliseconds $commandTimeoutMilliseconds))
        }
        catch { $blockers.Add('LOGMAN_CLEANUP_FAILED') }
    }
}

$rawArtifacts = @()
$rawTotalBytes = 0L
foreach ($file in @(Get-ChildItem -LiteralPath $rawRoot -Recurse -File | Sort-Object FullName)) {
    $relative = $file.FullName.Substring($rawRoot.Length).TrimStart('\').Replace('\', '/')
    $seal = Get-G04DCTraceFileSeal -Path $file.FullName -TokenPath ('$RAW_ROOT/' + $relative)
    $rawArtifacts += $seal
    $rawTotalBytes += [long]$seal.sizeBytes
}
if ($rawTotalBytes -gt $maximumRawEvidenceBytes) { $blockers.Add('RAW_TRACE_EVIDENCE_BYTE_CEILING') }
$passedAttempt = @($attempts.ToArray() | Where-Object { $_.decoded.passed } | Select-Object -First 1)
$report.status = if ($passedAttempt.Count -eq 1) { $passStatus } else { $blockedStatus }
$report.commands = @($commands.ToArray())
$report.attempts = @($attempts.ToArray())
$report.rawArtifacts = $rawArtifacts
$report.rawArtifactCount = $rawArtifacts.Count
$report.rawArtifactByteCount = $rawTotalBytes
$report.blockerReasons = @($blockers.ToArray() | Sort-Object -Unique)
Write-G04DCTraceJson -Path $reportPath -Value $report

$rawFull = [IO.Path]::GetFullPath($rawRoot)
$evidenceFull = [IO.Path]::GetFullPath($resolvedEvidence).TrimEnd('\') + '\'
if (!$rawFull.StartsWith($evidenceFull, [StringComparison]::OrdinalIgnoreCase) -or
    !(Test-Path -LiteralPath $rawMarkerPath -PathType Leaf) -or
    [IO.File]::ReadAllText($rawMarkerPath, [Text.Encoding]::ASCII) -cne $rawMarkerText) {
    throw 'Raw trace cleanup ownership verification failed.'
}
Remove-Item -LiteralPath $rawRoot -Recurse -Force
if (Test-Path -LiteralPath $rawRoot) { throw 'Raw trace evidence cleanup did not reach terminal state.' }
$report.rawArtifactsRemoved = $true
Write-G04DCTraceJson -Path $reportPath -Value $report

if ($report.status -cne $passStatus) { throw $blockedStatus }
Write-Output $passStatus
