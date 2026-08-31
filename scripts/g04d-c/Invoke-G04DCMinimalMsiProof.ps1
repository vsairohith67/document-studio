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
[IO.File]::WriteAllText((Join-Path $evidence 'MARKER.md'), "# G04D-C minimal-MSI proof evidence`r`n", [Text.UTF8Encoding]::new($false))
$ownedRoot = Join-Path $env:RUNNER_TEMP ('document-studio-g04d-c-minimal-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $ownedRoot | Out-Null
$ownedMarkerContent = "DOCUMENT-STUDIO-G04D-C-MINIMAL-OWNED`n"
$ownedMarker = Join-Path $ownedRoot '.g04d-c-owned-root'
[IO.File]::WriteAllText($ownedMarker, $ownedMarkerContent, [Text.UTF8Encoding]::new($false))
$download = Join-Path $ownedRoot 'download\LibreOffice_26.2.5_Win_x86-64.msi'
$installRoot = Join-Path $ownedRoot 'runtime'
$work = Join-Path $ownedRoot 'work'
New-Item -ItemType Directory -Path $work | Out-Null

function Complete-G04DCPreinstallModelRejection {
    param([Parameter(Mandatory = $true)] [string[]]$Reasons, [Parameter(Mandatory = $true)] $Identity)
    $cleanup = Remove-G04DCOwnedRoot -OwnedRoot $ownedRoot -MarkerPath $ownedMarker -MarkerContent $ownedMarkerContent -RequiredParent $env:RUNNER_TEMP
    $cleanup | Add-Member -NotePropertyName preinstallModelRejection -NotePropertyValue $true
    if (!$cleanup.removed) { $Reasons += '[CLEANUP_OWNERSHIP_MISMATCH] Rejected minimal model owned root was not removed.' }
    Write-G04DCJson -Path (Join-Path $evidence 'cleanup.json') -Value $cleanup
    $result = [ordered]@{
        mode = 'MINIMAL_MSI'
        classification = 'REJECTED'
        candidate = $false
        installationAttempted = $false
        uninstallAttempted = $false
        featureSelection = @($analysis.candidateMinimumFeatureSet)
        explicitlyRemovedFeatures = @($analysis.explicitlyExcludedFeatures)
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
Add-Type -Path (Join-Path $PSScriptRoot 'G04DC.MsiCondition.cs')
$conditionPropertyNames = @('VC_REDIST', 'CREATEDESKTOPLINK', 'WRITE_REGISTRY', 'ISCHECKFORPRODUCTUPDATES', 'REGISTER_NO_MSO_TYPES')
$conditionPropertyValues = @('0', '0', '0', '0', '1')
$modelRejections = [System.Collections.Generic.List[string]]::new()
try {
    Assert-G04DCFeatureAnalysis -Analysis $analysis | Out-Null
    if (![bool]$analysis.candidatePublicProperties.suppressionSafe -or [string]$analysis.candidatePublicProperties.VC_REDIST -cne '0') {
        throw '[AMBIGUOUS_FEATURE_OWNERSHIP] Selected external components are not safely disabled by the exact MSI table model.'
    }
    if ([string]$analysis.candidatePublicProperties.CREATEDESKTOPLINK -cne '0' -or [string]$analysis.candidatePublicProperties.WRITE_REGISTRY -cne '0') {
        throw '[AMBIGUOUS_FEATURE_OWNERSHIP] Desktop-link and registry suppression properties were not derived from exact MSI defaults.'
    }
    $selectedConditions = @($analysis.componentInstallOwnership | Where-Object { @($_.selectedOwners).Count -ne 0 -and ![string]::IsNullOrWhiteSpace([string]$_.condition) } | ForEach-Object { $_.condition } | Sort-Object -Unique)
    $conditionEvaluations = @([DocumentStudio.G04DC.Proof.MsiConditionEvaluator]::Evaluate($download, $conditionPropertyNames, $conditionPropertyValues, $selectedConditions))
    $expectedComponentOwnership = @(Resolve-G04DCExpectedComponentStates -ComponentOwnership @($analysis.componentInstallOwnership) -ConditionEvaluations $conditionEvaluations)
    if (@($expectedComponentOwnership | Where-Object { @($_.selectedOwners).Count -ne 0 -and ![bool]$_.underInstallLocation -and [int]$_.expectedInstallState -ne 2 }).Count -ne 0) {
        throw '[AMBIGUOUS_FEATURE_OWNERSHIP] A selected external component remains enabled under the derived public properties.'
    }
    $mutationClosure = Get-G04DCMutationClosure -Analysis $analysis -ComponentOwnership $expectedComponentOwnership
    Write-G04DCJson -Path (Join-Path $evidence 'preinstall-condition-evaluations.json') -Value ([ordered]@{
        propertyNames = $conditionPropertyNames
        propertyValues = $conditionPropertyValues
        evaluations = $conditionEvaluations
        expectedComponentOwnership = $expectedComponentOwnership
    })
    Write-G04DCJson -Path (Join-Path $evidence 'minimal-mutation-model.json') -Value $mutationClosure
    Assert-G04DCMinimalMutationClosure -Closure $mutationClosure | Out-Null
}
catch { $modelRejections.Add($_.Exception.Message) }
if ($modelRejections.Count -ne 0) {
    Complete-G04DCPreinstallModelRejection -Reasons @($modelRejections.ToArray()) -Identity $identity
    return
}
$featureList = @($analysis.candidateMinimumFeatureSet) -join ','
$removeFeatureList = @($analysis.explicitlyExcludedFeatures) -join ','
if ([string]::IsNullOrWhiteSpace($removeFeatureList)) { throw '[AMBIGUOUS_FEATURE_OWNERSHIP] Derived explicit REMOVE feature list is empty.' }
$protectedRegistryRows = @($analysis.protectedOwnership.allRegistryRows)
$protectedFontFileNames = @($analysis.ownershipByCategory.systemFonts.fontFiles | ForEach-Object { $_.installedFileName })
$externalRuntimeFilePaths = @(Get-G04DCExternalRuntimeTargetPaths -FileComponentOwnership @($analysis.fileComponentOwnership) -WindowsRoot $env:SystemRoot)
$msiComponentCodes = @($analysis.componentInstallOwnership | ForEach-Object { $_.componentId })
Write-G04DCJson -Path (Join-Path $evidence 'external-runtime-target-derivation.json') -Value ([ordered]@{
    derivedFromDirectoryComponentFileTables = $true
    candidatePublicProperties = $analysis.candidatePublicProperties
    targetPaths = $externalRuntimeFilePaths
})
$machineStateProgressPath = Join-Path $evidence 'machine-state-progress.ndjson'
function Get-G04DCMinimalMachineState {
    param(
        [Parameter(Mandatory = $true)] [string]$Label,
        [AllowNull()] [string]$EvidenceFileName
    )
    $performancePath = Join-Path $evidence "machine-state-performance-$Label.json"
    $arguments = @{
        ProtectedRegistryRows = $protectedRegistryRows
        ProtectedFontFileNames = $protectedFontFileNames
        ProtectedExternalFilePaths = $externalRuntimeFilePaths
        ProtectedMsiComponentCodes = $msiComponentCodes
        CaptureLabel = $Label
        ProgressPath = $machineStateProgressPath
        PerformancePath = $performancePath
        CaptureTargetMilliseconds = 480000
        OverallBudgetMilliseconds = 720000
        PhaseBudgetMilliseconds = 240000
    }
    if (![string]::IsNullOrWhiteSpace($EvidenceFileName)) { $arguments.StateOutputPath = Join-Path $evidence $EvidenceFileName }
    $state = Get-G04DCMachineState @arguments
    Assert-G04DCMachineStatePerformanceEvidence -Path $performancePath -RequiredPhase $(if ($arguments.ContainsKey('StateOutputPath')) { 'state-serialization' } else { $null }) | Out-Null
    return $state
}
$before = Get-G04DCMinimalMachineState -Label 'pre' -EvidenceFileName 'machine-pre.json'
if (@($before.installedProduct).Count -ne 0) { throw '[PREEXISTING_PRODUCT] Disposable runner already has the accepted ProductCode.' }
Assert-G04DCRunnerIsolation -State $before | Out-Null
Assert-G04DCExternalRuntimeDependencies -State $before | Out-Null
Assert-G04DCMsiRegistrationAbsent -State $before.msiRegistration | Out-Null

$log = Join-Path $evidence 'minimal-install-msiexec.log'
$arguments = @('/i', $download, '/qn', '/norestart', "INSTALLLOCATION=$installRoot", 'INSTALLLEVEL=0', "ADDLOCAL=$featureList", "REMOVE=$removeFeatureList", 'VC_REDIST=0', 'CREATEDESKTOPLINK=0', 'WRITE_REGISTRY=0', 'ISCHECKFORPRODUCTUPDATES=0', 'REGISTER_NO_MSO_TYPES=1', 'REBOOT=ReallySuppress', '/L*V', $log)
$argumentVectorCharacters = (($arguments | ForEach-Object { '"' + [string]$_ + '"' }) -join ' ').Length
if ($argumentVectorCharacters -gt 30000) { throw "[ARGUMENT_VECTOR_INVALID] MSI argument vector is $argumentVectorCharacters characters." }
Write-G04DCJson -Path (Join-Path $evidence 'minimal-install-command.json') -Value ([ordered]@{
    executable = (Join-Path $env:SystemRoot 'System32\msiexec.exe')
    arguments = $arguments
    derivedFromMsiFeatureGraph = $true
    addLocalFeatures = @($analysis.candidateMinimumFeatureSet)
    removeFeatures = @($analysis.explicitlyExcludedFeatures)
    derivedPublicProperties = $analysis.candidatePublicProperties
    argumentVectorCharacters = $argumentVectorCharacters
    maximumArgumentVectorCharacters = 30000
})
$started = [DateTime]::UtcNow
$process = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\msiexec.exe') -ArgumentList $arguments -Wait -PassThru -WindowStyle Hidden
$ended = [DateTime]::UtcNow
$installResult = [ordered]@{
    startedAtUtc = $started.ToString('o')
    endedAtUtc = $ended.ToString('o')
    durationMilliseconds = [long]($ended - $started).TotalMilliseconds
    exitCode = $process.ExitCode
    rebootRequired = $process.ExitCode -in @(1641, 3010)
    logSizeBytes = if (Test-Path -LiteralPath $log) { (Get-Item -LiteralPath $log).Length } else { 0 }
    logSha256 = if (Test-Path -LiteralPath $log) { Get-G04DCSha256 -Path $log } else { $null }
}
Write-G04DCJson -Path (Join-Path $evidence 'minimal-install-result.json') -Value $installResult

$candidate = $false
$reasons = [System.Collections.Generic.List[string]]::new()
$sandboxPassed = $false
$installNonMutating = $false
$uninstallRestored = $false
$registrationExact = $false
$installExitClean = $process.ExitCode -eq 0 -and !$installResult.rebootRequired
$uninstallExitClean = $false
$installed = $process.ExitCode -in @(0, 1641, 3010)
$afterInstall = Get-G04DCMinimalMachineState -Label 'post-install' -EvidenceFileName 'machine-post-install.json'
$installComparison = Compare-G04DCMachineState -Before $before -After $afterInstall
Write-G04DCJson -Path (Join-Path $evidence 'machine-install-comparison.json') -Value $installComparison
$requiresUninstall = $installed -or @($afterInstall.installedProduct).Count -ne 0
try { Assert-G04DCRunnerIsolation -State $afterInstall -Code 'MINIMAL_INSTALL_MUTATION' | Out-Null } catch { $reasons.Add($_.Exception.Message) }
if (!$installed) {
    $reasons.Add("[MINIMAL_INSTALL_FAILED] msiexec exited $($process.ExitCode).")
    if ([bool]$installComparison.protectedMutation) { $reasons.Add('[MINIMAL_INSTALL_MUTATION] Failed installation changed protected machine state.') }
}
if ($requiresUninstall) {
    if (!$installExitClean) { $reasons.Add("[REBOOT_REQUIRED] Minimal install exited $($process.ExitCode); a candidate requires exit 0 with no reboot.") }
    if (@($afterInstall.installedProduct).Count -eq 1) {
        $registeredValues = @($afterInstall.installedProduct[0].values)
        $registeredVersion = [string](@($registeredValues | Where-Object { $_.name -ceq 'DisplayVersion' } | Select-Object -First 1).value)
        $registeredPublisher = [string](@($registeredValues | Where-Object { $_.name -ceq 'Publisher' } | Select-Object -First 1).value)
        $registrationExact = $afterInstall.installedProduct[0].path.EndsWith('{3B467719-C25B-478C-8F4C-8E2EDA0E2093}', [StringComparison]::OrdinalIgnoreCase) -and
            $registeredVersion -ceq '26.2.5.2' -and $registeredPublisher -match 'The Document Foundation'
    }
    if (!$registrationExact) { $reasons.Add('[PRODUCT_REGISTRATION_INVALID] Accepted ProductCode/version/publisher was not registered exactly once.') }
    try {
        if (!$installExitClean) { throw '[REBOOT_REQUIRED] Runtime smoke is prohibited after a reboot-requiring install result.' }
        if (!$registrationExact) { throw '[PRODUCT_REGISTRATION_INVALID] Runtime smoke is prohibited without one exact accepted product registration.' }
        Assert-G04DCRunnerIsolation -State $afterInstall -Code 'MINIMAL_INSTALL_MUTATION' | Out-Null
        Assert-G04DCNonMutation -Comparison $installComparison -Code 'MINIMAL_INSTALL_MUTATION' | Out-Null
        $installNonMutating = $true
        $featureStates = @(Get-G04DCInstalledFeatureStates -ProductCode '{3B467719-C25B-478C-8F4C-8E2EDA0E2093}' -FeatureNames @($analysis.featureTree | ForEach-Object { $_.feature }))
        Write-G04DCJson -Path (Join-Path $evidence 'installed-feature-states.json') -Value ([ordered]@{
            installStateAbsent = 2
            installStateLocal = 3
            selectedFeatures = @($analysis.candidateMinimumFeatureSet)
            explicitlyRemovedFeatures = @($analysis.explicitlyExcludedFeatures)
            states = $featureStates
        })
        Assert-G04DCInstalledFeatureStates -States $featureStates -SelectedFeatures @($analysis.candidateMinimumFeatureSet) | Out-Null
        $componentStates = @(Get-G04DCInstalledComponentStates -ProductCode '{3B467719-C25B-478C-8F4C-8E2EDA0E2093}' -ComponentCodes @($analysis.componentInstallOwnership | ForEach-Object { $_.componentId }))
        Write-G04DCJson -Path (Join-Path $evidence 'installed-component-states.json') -Value ([ordered]@{
            installStateAbsent = 2
            installStateLocal = 3
            states = $componentStates
            ownership = $expectedComponentOwnership
        })
        Assert-G04DCInstalledComponentStates -States $componentStates -ComponentOwnership $expectedComponentOwnership | Out-Null
        Assert-G04DCMsiRegistrationInstalled -State $afterInstall.msiRegistration -ExpectedComponents $expectedComponentOwnership | Out-Null
        $runtimeManifestPath = Join-Path $evidence 'runtime-manifest.json'
        & (Join-Path $PSScriptRoot 'New-G04DCRuntimeManifest.ps1') -RuntimeRoot $installRoot -OutputPath $runtimeManifestPath | Out-Null
        $runtimeManifest = Get-Content -LiteralPath $runtimeManifestPath -Raw | ConvertFrom-Json
        Assert-G04DCInstalledFileOwnership -RuntimeManifest $runtimeManifest -FileComponentOwnership @($analysis.fileComponentOwnership) -ComponentOwnership $expectedComponentOwnership | Out-Null
        Write-G04DCJson -Path (Join-Path $evidence 'installed-file-ownership.json') -Value ([ordered]@{
            runtimeFileCount = @($runtimeManifest.files).Count
            selectedFileTableRowCount = @($analysis.fileComponentOwnership | Where-Object { @($_.selectedOwners).Count -ne 0 }).Count
            exactSelectedComponentPathOwnershipPassed = $true
        })
        & (Join-Path $PSScriptRoot 'Invoke-G04DCSandboxSmoke.ps1') -RuntimeRoot $installRoot -WorkRoot $work -EvidenceDirectory $evidence -RepositoryRoot $repo | Out-Null
        $afterSmoke = Get-G04DCMinimalMachineState -Label 'post-smoke' -EvidenceFileName 'machine-post-smoke.json'
        $smokeComparison = Compare-G04DCMachineState -Before $before -After $afterSmoke
        Write-G04DCJson -Path (Join-Path $evidence 'machine-smoke-comparison.json') -Value $smokeComparison
        Assert-G04DCNonMutation -Comparison $smokeComparison -Code 'MINIMAL_RUNTIME_MUTATION' | Out-Null
        Assert-G04DCRunnerIsolation -State $afterSmoke -Code 'MINIMAL_RUNTIME_MUTATION' | Out-Null
        $sandboxPassed = $true
    }
    catch {
        $reasons.Add($_.Exception.Message)
    }

    $afterRuntimeAttempt = Get-G04DCMinimalMachineState -Label 'post-runtime-attempt' -EvidenceFileName 'machine-post-runtime-attempt.json'
    $runtimeAttemptComparison = Compare-G04DCMachineState -Before $before -After $afterRuntimeAttempt
    Write-G04DCJson -Path (Join-Path $evidence 'machine-runtime-attempt-comparison.json') -Value $runtimeAttemptComparison
    if ([bool]$runtimeAttemptComparison.protectedMutation) { $reasons.Add('[MINIMAL_RUNTIME_MUTATION] Runtime attempt changed protected machine state.'); $sandboxPassed = $false }
    try { Assert-G04DCRunnerIsolation -State $afterRuntimeAttempt -Code 'MINIMAL_RUNTIME_MUTATION' | Out-Null } catch { $reasons.Add($_.Exception.Message); $sandboxPassed = $false }

    $uninstallLog = Join-Path $evidence 'minimal-uninstall-msiexec.log'
    $uninstallArguments = @('/x', '{3B467719-C25B-478C-8F4C-8E2EDA0E2093}', '/qn', '/norestart', 'REBOOT=ReallySuppress', '/L*V', $uninstallLog)
    Write-G04DCJson -Path (Join-Path $evidence 'minimal-uninstall-command.json') -Value ([ordered]@{ executable = (Join-Path $env:SystemRoot 'System32\msiexec.exe'); arguments = $uninstallArguments })
    $uninstallStarted = [DateTime]::UtcNow
    $uninstall = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\msiexec.exe') -ArgumentList $uninstallArguments -Wait -PassThru -WindowStyle Hidden
    $uninstallEnded = [DateTime]::UtcNow
    Write-G04DCJson -Path (Join-Path $evidence 'minimal-uninstall-result.json') -Value ([ordered]@{
        startedAtUtc = $uninstallStarted.ToString('o')
        endedAtUtc = $uninstallEnded.ToString('o')
        durationMilliseconds = [long]($uninstallEnded - $uninstallStarted).TotalMilliseconds
        exitCode = $uninstall.ExitCode
        rebootRequired = $uninstall.ExitCode -in @(1641, 3010)
        logSizeBytes = if (Test-Path -LiteralPath $uninstallLog) { (Get-Item -LiteralPath $uninstallLog).Length } else { 0 }
        logSha256 = if (Test-Path -LiteralPath $uninstallLog) { Get-G04DCSha256 -Path $uninstallLog } else { $null }
    })
    $uninstallExitClean = $uninstall.ExitCode -eq 0 -and $uninstall.ExitCode -notin @(1641, 3010)
    if (!$uninstallExitClean) { $reasons.Add("[UNINSTALL_FAILED] Exact uninstall must exit 0 without reboot; msiexec exited $($uninstall.ExitCode).") }
    $afterUninstall = Get-G04DCMinimalMachineState -Label 'post-uninstall' -EvidenceFileName 'machine-post-uninstall.json'
    $uninstallComparison = Compare-G04DCMachineState -Before $before -After $afterUninstall -IncludeInstalledProductCatalog -IncludeAcceptedMsiRegistration -IncludeInstallerCacheCatalog
    Write-G04DCJson -Path (Join-Path $evidence 'machine-uninstall-comparison.json') -Value $uninstallComparison
    $installRootResidue = if (Test-Path -LiteralPath $installRoot) {
        [pscustomobject][ordered]@{
            present = $true
            entries = @(Get-ChildItem -LiteralPath $installRoot -Recurse -Force | ForEach-Object {
                [pscustomobject][ordered]@{ path = $_.FullName.Substring($installRoot.Length).TrimStart('\'); directory = $_.PSIsContainer; reparsePoint = [bool]($_.Attributes -band [IO.FileAttributes]::ReparsePoint); sizeBytes = if ($_.PSIsContainer) { 0 } else { [long]$_.Length } }
            })
        }
    } else { [pscustomobject][ordered]@{ present = $false; entries = @() } }
    Write-G04DCJson -Path (Join-Path $evidence 'post-uninstall-install-root-residue.json') -Value $installRootResidue
    $uninstallRestored = $uninstallExitClean -and !$uninstallComparison.protectedMutation -and @($afterUninstall.installedProduct).Count -eq 0 -and ![bool]$installRootResidue.present
    try { Assert-G04DCMsiRegistrationAbsent -State $afterUninstall.msiRegistration | Out-Null } catch { $reasons.Add($_.Exception.Message); $uninstallRestored = $false }
    try { Assert-G04DCRunnerIsolation -State $afterUninstall -Code 'UNINSTALL_RESIDUE' | Out-Null } catch { $reasons.Add($_.Exception.Message); $uninstallRestored = $false }
    if (!$uninstallRestored) { $reasons.Add('[UNINSTALL_RESIDUE] Minimal MSI uninstall did not restore complete pre-state.') }
    $candidate = $installExitClean -and $registrationExact -and $installNonMutating -and $sandboxPassed -and $uninstallRestored
}

$preCleanupInstallRootResidue = if (Test-Path -LiteralPath $installRoot) {
    [pscustomobject][ordered]@{
        present = $true
        entries = @(Get-ChildItem -LiteralPath $installRoot -Recurse -Force | ForEach-Object {
            [pscustomobject][ordered]@{
                path = $_.FullName.Substring($installRoot.Length).TrimStart('\')
                directory = $_.PSIsContainer
                reparsePoint = [bool]($_.Attributes -band [IO.FileAttributes]::ReparsePoint)
                sizeBytes = if ($_.PSIsContainer) { 0 } else { [long]$_.Length }
                sha256 = if (!$_.PSIsContainer -and ![bool]($_.Attributes -band [IO.FileAttributes]::ReparsePoint)) { Get-G04DCSha256 -Path $_.FullName } else { $null }
            }
        })
    }
} else { [pscustomobject][ordered]@{ present = $false; entries = @() } }
Write-G04DCJson -Path (Join-Path $evidence 'pre-cleanup-install-root-residue.json') -Value $preCleanupInstallRootResidue
if ([bool]$preCleanupInstallRootResidue.present) { $reasons.Add('[UNINSTALL_RESIDUE] Install root remains before marker-owned cleanup.'); $candidate = $false }

$cleanup = Remove-G04DCOwnedRoot -OwnedRoot $ownedRoot -MarkerPath $ownedMarker -MarkerContent $ownedMarkerContent -RequiredParent $env:RUNNER_TEMP
if (!$cleanup.removed) { $reasons.Add('[CLEANUP_OWNERSHIP_MISMATCH] Marker-owned minimal runtime was not removed.'); $candidate = $false }
try { Assert-G04DCCleanupEvidence -Evidence $cleanup | Out-Null } catch { $reasons.Add($_.Exception.Message); $candidate = $false }
Write-G04DCJson -Path (Join-Path $evidence 'cleanup.json') -Value $cleanup
$finalState = Get-G04DCMinimalMachineState -Label 'final'
$finalComparison = Compare-G04DCMachineState -Before $before -After $finalState -IncludeInstalledProductCatalog -IncludeAcceptedMsiRegistration -IncludeInstallerCacheCatalog
Write-G04DCJson -Path (Join-Path $evidence 'machine-final-comparison.json') -Value $finalComparison
if ($finalComparison.protectedMutation) { $reasons.Add('[UNINSTALL_RESIDUE] Final runner state differs from pre-state.'); $candidate = $false }
try { Assert-G04DCRunnerIsolation -State $finalState -Code 'UNINSTALL_RESIDUE' | Out-Null } catch { $reasons.Add($_.Exception.Message); $candidate = $false }
$result = [ordered]@{
    mode = 'MINIMAL_MSI'
    classification = if ($candidate) { 'MINIMAL_MSI_CANDIDATE' } else { 'REJECTED' }
    candidate = $candidate
    featureSelection = @($analysis.candidateMinimumFeatureSet)
    protectedFeatureExclusions = @($analysis.explicitlyExcludedFeatures)
    installNonMutating = $installNonMutating
    installExitClean = $installExitClean
    exactProductRegistration = $registrationExact
    runtimeSmokeNetworkBoundary = 'zero-capability AppContainer only'
    sandboxVersionPassed = $sandboxPassed
    appContainerJobSmokePassed = $sandboxPassed
    uninstallRestoredPreState = $uninstallRestored
    uninstallExitClean = $uninstallExitClean
    cleanupPassed = [bool]$cleanup.removed -and ![bool]$finalComparison.protectedMutation
    reasons = @($reasons.ToArray())
    msiSha256 = $identity.sha256
}
Write-G04DCJson -Path (Join-Path $evidence 'candidate-result.json') -Value $result
New-G04DCArtifactManifest -EvidenceDirectory $evidence | Out-Null
Write-Output ($result | ConvertTo-Json -Compress)
