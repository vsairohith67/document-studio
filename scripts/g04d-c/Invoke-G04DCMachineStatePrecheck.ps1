param(
    [Parameter(Mandatory = $true)] [string]$RepositoryRoot,
    [Parameter(Mandatory = $true)] [string]$EvidenceDirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'G04DC.Common.psm1') -Force

$repo = (Resolve-Path -LiteralPath $RepositoryRoot).Path
if (Test-Path -LiteralPath $EvidenceDirectory) { throw "Refusing to overwrite evidence directory: $EvidenceDirectory" }
New-Item -ItemType Directory -Path $EvidenceDirectory | Out-Null
$evidence = (Resolve-Path -LiteralPath $EvidenceDirectory).Path
[IO.File]::WriteAllText((Join-Path $evidence 'MARKER.md'), "# G04D-C machine-state precheck evidence`r`n", [Text.UTF8Encoding]::new($false))
$ownedRoot = Join-Path $env:RUNNER_TEMP ('document-studio-g04d-c-precheck-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $ownedRoot | Out-Null
$ownedMarkerContent = "DOCUMENT-STUDIO-G04D-C-PRECHECK-OWNED`n"
$ownedMarker = Join-Path $ownedRoot '.g04d-c-owned-root'
[IO.File]::WriteAllText($ownedMarker, $ownedMarkerContent, [Text.UTF8Encoding]::new($false))
$download = Join-Path $ownedRoot 'download\LibreOffice_26.2.5_Win_x86-64.msi'
$progressPath = Join-Path $evidence 'machine-state-progress.ndjson'
$performancePath = Join-Path $evidence 'machine-state-performance.json'

try {
    $identity = Invoke-G04DCAcquireMsi -Destination $download -EvidenceDirectory $evidence
    $analysis = Export-G04DCMsiDatabase -MsiPath $download -OutputDirectory (Join-Path $evidence 'msi-tables')
    $protectedRegistryRows = @($analysis.protectedOwnership.allRegistryRows)
    $protectedFontFileNames = @($analysis.ownershipByCategory.systemFonts.fontFiles | ForEach-Object { $_.installedFileName })
    $externalRuntimeFilePaths = @(Get-G04DCExternalRuntimeTargetPaths -FileComponentOwnership @($analysis.fileComponentOwnership) -WindowsRoot $env:SystemRoot)
    $msiComponentCodes = @($analysis.componentInstallOwnership | ForEach-Object { $_.componentId })
    Write-G04DCJson -Path (Join-Path $evidence 'machine-state-input-scale.json') -Value ([ordered]@{
        schemaVersion = 1
        protectedRegistryRowCount = $protectedRegistryRows.Count
        protectedFontFileCount = $protectedFontFileNames.Count
        externalRuntimeTargetCount = $externalRuntimeFilePaths.Count
        expectedMsiComponentCount = $msiComponentCodes.Count
        msiSha256 = $identity.sha256
    })

    $before = Get-G04DCMachineState `
        -ProtectedRegistryRows $protectedRegistryRows `
        -ProtectedFontFileNames $protectedFontFileNames `
        -ProtectedExternalFilePaths $externalRuntimeFilePaths `
        -ProtectedMsiComponentCodes $msiComponentCodes `
        -CaptureLabel 'pre' `
        -ProgressPath $progressPath `
        -PerformancePath $performancePath `
        -StateOutputPath (Join-Path $evidence 'machine-pre.json') `
        -CaptureTargetMilliseconds 480000 `
        -OverallBudgetMilliseconds 720000 `
        -PhaseBudgetMilliseconds 240000
    if (@($before.installedProduct).Count -ne 0) { throw '[PREEXISTING_PRODUCT] Disposable runner already has the accepted ProductCode.' }
    Assert-G04DCRunnerIsolation -State $before | Out-Null
    Assert-G04DCMsiRegistrationAbsent -State $before.msiRegistration | Out-Null

    $performance = Assert-G04DCMachineStatePerformanceEvidence -Path $performancePath -RequiredPhase 'state-serialization'
    $cleanup = Remove-G04DCOwnedRoot -OwnedRoot $ownedRoot -MarkerPath $ownedMarker -MarkerContent $ownedMarkerContent -RequiredParent $env:RUNNER_TEMP
    Write-G04DCJson -Path (Join-Path $evidence 'cleanup.json') -Value $cleanup
    if (!$cleanup.removed) { throw '[CLEANUP_OWNERSHIP_MISMATCH] Precheck owned root was not removed.' }
    $result = [ordered]@{
        schemaVersion = 1
        status = 'MACHINE_STATE_PRECHECK_PASS'
        candidateClassificationProduced = $false
        machinePreProduced = $true
        totalElapsedMilliseconds = [long]$performance.totalElapsedMilliseconds
        captureHardCeilingMilliseconds = 720000
        phaseCeilingMilliseconds = 240000
        runnerIsolationPassed = $true
        msiRegistrationAbsent = $true
    }
    Write-G04DCJson -Path (Join-Path $evidence 'machine-state-precheck-result.json') -Value $result
    New-G04DCArtifactManifest -EvidenceDirectory $evidence | Out-Null
    Write-Output 'MACHINE_STATE_PRECHECK_PASS'
}
catch {
    $original = $_
    $phase = 'acquisition-or-analysis'
    if ($original.Exception.Message -match 'phase=([a-z0-9-]+)') { $phase = $Matches[1] }
    elseif (Test-Path -LiteralPath $performancePath) {
        try {
            $failedPerformance = Get-Content -LiteralPath $performancePath -Raw | ConvertFrom-Json
            if (![string]::IsNullOrWhiteSpace([string]$failedPerformance.failurePhase)) { $phase = [string]$failedPerformance.failurePhase }
        }
        catch {}
    }
    $reasonCode = if ($original.Exception.Message -match '^\[([A-Z0-9_]+)\]') { $Matches[1] } else { 'PRECHECK_INFRASTRUCTURE_FAILURE' }
    $cleanup = if (Test-Path -LiteralPath $ownedRoot) {
        Remove-G04DCOwnedRoot -OwnedRoot $ownedRoot -MarkerPath $ownedMarker -MarkerContent $ownedMarkerContent -RequiredParent $env:RUNNER_TEMP
    }
    else { [pscustomobject][ordered]@{ markerOwnedPathsOnly = $true; removed = $true } }
    Write-G04DCJson -Path (Join-Path $evidence 'cleanup.json') -Value $cleanup
    Write-G04DCJson -Path (Join-Path $evidence 'machine-state-precheck-result.json') -Value ([ordered]@{
        schemaVersion = 1
        status = 'MACHINE_STATE_PRECHECK_BLOCKED'
        displayStatus = "MACHINE_STATE_PRECHECK_BLOCKED — $phase"
        phase = $phase
        reasonCode = $reasonCode
        candidateClassificationProduced = $false
        machinePreProduced = Test-Path -LiteralPath (Join-Path $evidence 'machine-pre.json')
    })
    New-G04DCArtifactManifest -EvidenceDirectory $evidence | Out-Null
    Write-Output "MACHINE_STATE_PRECHECK_BLOCKED — $phase"
    throw "[MACHINE_STATE_PRECHECK_BLOCKED] phase=$phase reasonCode=$reasonCode"
}
