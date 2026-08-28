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
[IO.File]::WriteAllText((Join-Path $evidence 'MARKER.md'), "# G04D-C administrative-image proof evidence`r`n", [Text.UTF8Encoding]::new($false))
$ownedRoot = Join-Path $env:RUNNER_TEMP ('document-studio-g04d-c-admin-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $ownedRoot | Out-Null
$ownedMarkerContent = "DOCUMENT-STUDIO-G04D-C-ADMIN-OWNED`n"
$ownedMarker = Join-Path $ownedRoot '.g04d-c-owned-root'
[IO.File]::WriteAllText($ownedMarker, $ownedMarkerContent, [Text.UTF8Encoding]::new($false))
$download = Join-Path $ownedRoot 'download\LibreOffice_26.2.5_Win_x86-64.msi'
$adminImage = Join-Path $ownedRoot 'runtime'
$work = Join-Path $ownedRoot 'work'
New-Item -ItemType Directory -Path $work | Out-Null

function Complete-G04DCAdminModelRejection {
    param([Parameter(Mandatory = $true)] [string[]]$Reasons, [Parameter(Mandatory = $true)] $Identity)
    $cleanup = Remove-G04DCOwnedRoot -OwnedRoot $ownedRoot -MarkerPath $ownedMarker -MarkerContent $ownedMarkerContent -RequiredParent $env:RUNNER_TEMP
    $cleanup | Add-Member -NotePropertyName preExtractionModelRejection -NotePropertyValue $true
    if (!$cleanup.removed) { $Reasons += '[CLEANUP_OWNERSHIP_MISMATCH] Rejected administrative-image model owned root was not removed.' }
    Write-G04DCJson -Path (Join-Path $evidence 'cleanup.json') -Value $cleanup
    $result = [ordered]@{
        mode = 'ADMIN_IMAGE'
        classification = 'REJECTED'
        candidate = $false
        extractionAttempted = $false
        reasons = $Reasons
        msiSha256 = $Identity.sha256
    }
    Write-G04DCJson -Path (Join-Path $evidence 'candidate-result.json') -Value $result
    New-G04DCArtifactManifest -EvidenceDirectory $evidence | Out-Null
    Write-Output ($result | ConvertTo-Json -Compress)
}

try { $identity = Invoke-G04DCAcquireMsi -Destination $download -EvidenceDirectory $evidence }
catch {
    $cleanup = Remove-G04DCOwnedRoot -OwnedRoot $ownedRoot -MarkerPath $ownedMarker -MarkerContent $ownedMarkerContent -RequiredParent $env:RUNNER_TEMP
    Write-G04DCJson -Path (Join-Path $evidence 'cleanup.json') -Value $cleanup
    New-G04DCArtifactManifest -EvidenceDirectory $evidence | Out-Null
    throw
}
$analysis = Export-G04DCMsiDatabase -MsiPath $download -OutputDirectory (Join-Path $evidence 'msi-tables')
$adminMutationClosure = [pscustomobject][ordered]@{
    sequencedAdminCustomActions = @($analysis.protectedOwnership.sequencedCustomActions | Where-Object { $_.sequenceTable -in @('AdminExecuteSequence', 'AdminUISequence') })
    unboundedAdminCustomActions = @($analysis.protectedOwnership.unboundedAdminCustomActions)
    administrativeActionModelClosed = @($analysis.protectedOwnership.unboundedAdminCustomActions).Count -eq 0
}
Write-G04DCJson -Path (Join-Path $evidence 'administrative-action-model.json') -Value $adminMutationClosure
try { Assert-G04DCAdminMutationClosure -Closure $adminMutationClosure | Out-Null }
catch {
    Complete-G04DCAdminModelRejection -Reasons @($_.Exception.Message) -Identity $identity
    return
}
$protectedRegistryRows = @($analysis.protectedOwnership.allRegistryRows)
$protectedFontFileNames = @($analysis.ownershipByCategory.systemFonts.fontFiles | ForEach-Object { $_.installedFileName })
$externalRuntimeFilePaths = @(Get-G04DCExternalRuntimeTargetPaths -FileComponentOwnership @($analysis.fileComponentOwnership) -WindowsRoot $env:SystemRoot)
$msiComponentCodes = @($analysis.componentInstallOwnership | ForEach-Object { $_.componentId })
Write-G04DCJson -Path (Join-Path $evidence 'external-runtime-target-derivation.json') -Value ([ordered]@{
    derivedFromDirectoryComponentFileTables = $true
    candidatePublicProperties = $analysis.candidatePublicProperties
    targetPaths = $externalRuntimeFilePaths
})
$before = Get-G04DCMachineState -ProtectedRegistryRows $protectedRegistryRows -ProtectedFontFileNames $protectedFontFileNames -ProtectedExternalFilePaths $externalRuntimeFilePaths -ProtectedMsiComponentCodes $msiComponentCodes
Write-G04DCJson -Path (Join-Path $evidence 'machine-pre.json') -Value $before
if (@($before.installedProduct).Count -ne 0) { throw '[PREEXISTING_PRODUCT] Disposable runner already has the accepted ProductCode.' }
Assert-G04DCRunnerIsolation -State $before | Out-Null
Assert-G04DCMsiRegistrationAbsent -State $before.msiRegistration | Out-Null

$log = Join-Path $evidence 'administrative-extraction-msiexec.log'
$arguments = @('/a', $download, "TARGETDIR=$adminImage", '/qn', '/norestart', '/L*V', $log)
Write-G04DCJson -Path (Join-Path $evidence 'administrative-extraction-command.json') -Value ([ordered]@{ executable = (Join-Path $env:SystemRoot 'System32\msiexec.exe'); arguments = $arguments })
$started = [DateTime]::UtcNow
$process = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\msiexec.exe') -ArgumentList $arguments -Wait -PassThru -WindowStyle Hidden
$ended = [DateTime]::UtcNow
$extractResult = [ordered]@{
    startedAtUtc = $started.ToString('o')
    endedAtUtc = $ended.ToString('o')
    durationMilliseconds = [long]($ended - $started).TotalMilliseconds
    exitCode = $process.ExitCode
    rebootRequired = $process.ExitCode -in @(1641, 3010)
    logSizeBytes = if (Test-Path -LiteralPath $log) { (Get-Item -LiteralPath $log).Length } else { 0 }
    logSha256 = if (Test-Path -LiteralPath $log) { Get-G04DCSha256 -Path $log } else { $null }
}
Write-G04DCJson -Path (Join-Path $evidence 'administrative-extraction-result.json') -Value $extractResult

$candidate = $false
$reasons = [System.Collections.Generic.List[string]]::new()
$sandboxPassed = $false
$extractionComparison = [pscustomobject][ordered]@{ protectedMutation = $true; changes = @([pscustomobject]@{ boundary = 'administrativeExtractionExit'; before = 0; after = $process.ExitCode }) }
if ($process.ExitCode -ne 0) {
    $reasons.Add("[ADMIN_EXTRACTION_FAILED] msiexec /a exited $($process.ExitCode).")
}
else {
    $afterExtraction = Get-G04DCMachineState -ProtectedRegistryRows $protectedRegistryRows -ProtectedFontFileNames $protectedFontFileNames -ProtectedExternalFilePaths $externalRuntimeFilePaths -ProtectedMsiComponentCodes $msiComponentCodes
    Write-G04DCJson -Path (Join-Path $evidence 'machine-post-extraction.json') -Value $afterExtraction
    $extractionComparison = Compare-G04DCMachineState -Before $before -After $afterExtraction -IncludeInstalledProductCatalog -IncludeAcceptedMsiRegistration -IncludeInstallerCacheCatalog
    Write-G04DCJson -Path (Join-Path $evidence 'machine-extraction-comparison.json') -Value $extractionComparison
    try {
        Assert-G04DCNonMutation -Comparison $extractionComparison -Code 'ADMINISTRATIVE_EXTRACTION_MUTATION' | Out-Null
        if (@($afterExtraction.installedProduct).Count -ne 0) { throw '[ADMINISTRATIVE_EXTRACTION_MUTATION] ProductCode was registered by msiexec /a.' }
        Assert-G04DCRunnerIsolation -State $afterExtraction -Code 'ADMINISTRATIVE_EXTRACTION_MUTATION' | Out-Null
        & (Join-Path $PSScriptRoot 'New-G04DCRuntimeManifest.ps1') -RuntimeRoot $adminImage -OutputPath (Join-Path $evidence 'runtime-manifest.json') | Out-Null
        & (Join-Path $PSScriptRoot 'Invoke-G04DCSandboxSmoke.ps1') -RuntimeRoot $adminImage -WorkRoot $work -EvidenceDirectory $evidence -RepositoryRoot $repo | Out-Null
        $afterSmoke = Get-G04DCMachineState -ProtectedRegistryRows $protectedRegistryRows -ProtectedFontFileNames $protectedFontFileNames -ProtectedExternalFilePaths $externalRuntimeFilePaths -ProtectedMsiComponentCodes $msiComponentCodes
        Write-G04DCJson -Path (Join-Path $evidence 'machine-post-smoke.json') -Value $afterSmoke
        $smokeComparison = Compare-G04DCMachineState -Before $before -After $afterSmoke -IncludeInstalledProductCatalog -IncludeAcceptedMsiRegistration -IncludeInstallerCacheCatalog
        Write-G04DCJson -Path (Join-Path $evidence 'machine-smoke-comparison.json') -Value $smokeComparison
        Assert-G04DCNonMutation -Comparison $smokeComparison -Code 'ADMINISTRATIVE_RUNTIME_MUTATION' | Out-Null
        Assert-G04DCRunnerIsolation -State $afterSmoke -Code 'ADMINISTRATIVE_RUNTIME_MUTATION' | Out-Null
        $sandboxPassed = $true
        $candidate = $true
    }
    catch { $reasons.Add($_.Exception.Message) }
}
$afterRuntimeAttempt = Get-G04DCMachineState -ProtectedRegistryRows $protectedRegistryRows -ProtectedFontFileNames $protectedFontFileNames -ProtectedExternalFilePaths $externalRuntimeFilePaths -ProtectedMsiComponentCodes $msiComponentCodes
Write-G04DCJson -Path (Join-Path $evidence 'machine-post-runtime-attempt.json') -Value $afterRuntimeAttempt
$runtimeAttemptComparison = Compare-G04DCMachineState -Before $before -After $afterRuntimeAttempt -IncludeInstalledProductCatalog -IncludeAcceptedMsiRegistration -IncludeInstallerCacheCatalog
Write-G04DCJson -Path (Join-Path $evidence 'machine-runtime-attempt-comparison.json') -Value $runtimeAttemptComparison
if ([bool]$runtimeAttemptComparison.protectedMutation) { $reasons.Add('[ADMINISTRATIVE_RUNTIME_MUTATION] Runtime attempt changed protected machine state.'); $candidate = $false; $sandboxPassed = $false }
try { Assert-G04DCRunnerIsolation -State $afterRuntimeAttempt -Code 'ADMINISTRATIVE_RUNTIME_MUTATION' | Out-Null } catch { $reasons.Add($_.Exception.Message); $candidate = $false; $sandboxPassed = $false }

$cleanup = Remove-G04DCOwnedRoot -OwnedRoot $ownedRoot -MarkerPath $ownedMarker -MarkerContent $ownedMarkerContent -RequiredParent $env:RUNNER_TEMP
if (!$cleanup.removed) { $reasons.Add('[CLEANUP_OWNERSHIP_MISMATCH] Marker-owned administrative image was not removed.'); $candidate = $false }
try { Assert-G04DCCleanupEvidence -Evidence $cleanup | Out-Null } catch { $reasons.Add($_.Exception.Message); $candidate = $false }
Write-G04DCJson -Path (Join-Path $evidence 'cleanup.json') -Value $cleanup
$finalState = Get-G04DCMachineState -ProtectedRegistryRows $protectedRegistryRows -ProtectedFontFileNames $protectedFontFileNames -ProtectedExternalFilePaths $externalRuntimeFilePaths -ProtectedMsiComponentCodes $msiComponentCodes
$finalComparison = Compare-G04DCMachineState -Before $before -After $finalState -IncludeInstalledProductCatalog -IncludeAcceptedMsiRegistration -IncludeInstallerCacheCatalog
Write-G04DCJson -Path (Join-Path $evidence 'machine-final-comparison.json') -Value $finalComparison
if ($finalComparison.protectedMutation) { $reasons.Add('[ADMINISTRATIVE_RUNTIME_MUTATION] Final runner state differs from pre-state.'); $candidate = $false }
try { Assert-G04DCRunnerIsolation -State $finalState -Code 'ADMINISTRATIVE_RUNTIME_MUTATION' | Out-Null } catch { $reasons.Add($_.Exception.Message); $candidate = $false }
$result = [ordered]@{
    mode = 'ADMIN_IMAGE'
    classification = if ($candidate) { 'ADMIN_IMAGE_CANDIDATE' } else { 'REJECTED' }
    candidate = $candidate
    extractionNonMutating = ![bool]$extractionComparison.protectedMutation
    runtimeSmokeNetworkBoundary = 'zero-capability AppContainer only'
    sandboxVersionPassed = $sandboxPassed
    appContainerJobSmokePassed = $sandboxPassed
    cleanupPassed = [bool]$cleanup.removed -and ![bool]$finalComparison.protectedMutation
    reasons = @($reasons.ToArray())
    msiSha256 = $identity.sha256
}
Write-G04DCJson -Path (Join-Path $evidence 'candidate-result.json') -Value $result
New-G04DCArtifactManifest -EvidenceDirectory $evidence | Out-Null
Write-Output ($result | ConvertTo-Json -Compress)
