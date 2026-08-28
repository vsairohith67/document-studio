Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$workflowPath = Join-Path $repositoryRoot '.github\workflows\g04d-c-libreoffice-runtime-proof.yml'
$workflow = Get-Content -LiteralPath $workflowPath -Raw
$allProof = @(Get-ChildItem -LiteralPath $PSScriptRoot -File | Where-Object { $_.Name -cne 'verify-g04d-c-boundaries.ps1' } | ForEach-Object { Get-Content -LiteralPath $_.FullName -Raw }) -join "`n"

foreach ($required in @(
    'workflow_dispatch:', 'admin-image:', 'minimal-msi:', 'decision:', 'windows-2025',
    'LibreOffice_26.2.5_Win_x86-64.msi', 'f15ba07bfcb0186986cf3171063506f5d207c11f8cc051ba0d135209e9e915f9',
    'ADMIN_IMAGE_CANDIDATE', 'MINIMAL_MSI_CANDIDATE', 'LIBREOFFICE_RUNTIME_UNSUPPORTED'
)) {
    if (!$workflow.Contains($required) -and !$allProof.Contains($required)) { throw "G04D-C proof boundary is missing: $required" }
}
$adminProof = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'Invoke-G04DCAdminImageProof.ps1') -Raw
$minimalProof = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'Invoke-G04DCMinimalMsiProof.ps1') -Raw
$sandboxProof = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'Invoke-G04DCSandboxSmoke.ps1') -Raw
$commonProof = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'G04DC.Common.psm1') -Raw
$runtimeManifestProof = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'New-G04DCRuntimeManifest.ps1') -Raw
$decisionProof = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'New-G04DCCandidateDecision.ps1') -Raw
$tests = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'Test-G04DCBoundaries.ps1') -Raw
$registryRowsDefinition = '$protectedRegistryRows = @($analysis.protectedOwnership.allRegistryRows)'
$firstRegistryRowsUse = '$before = Get-G04DCMachineState -ProtectedRegistryRows $protectedRegistryRows -ProtectedFontFileNames $protectedFontFileNames'
if (!$adminProof.Contains($registryRowsDefinition) -or !$adminProof.Contains($firstRegistryRowsUse) -or
    $adminProof.IndexOf($registryRowsDefinition, [StringComparison]::Ordinal) -gt $adminProof.IndexOf($firstRegistryRowsUse, [StringComparison]::Ordinal)) {
    throw 'G04D-C administrative-image proof must derive protected registry rows before its first strict-mode state capture.'
}
if (!$minimalProof.Contains($registryRowsDefinition)) {
    throw 'G04D-C minimal-MSI proof must watch every Registry-table row, including excluded-feature/custom-action ownership.'
}
if ((Test-Path -LiteralPath (Join-Path $PSScriptRoot 'Invoke-G04DCDirectSmoke.ps1')) -or
    $adminProof.Contains('Invoke-G04DCDirectSmoke') -or $minimalProof.Contains('Invoke-G04DCDirectSmoke')) {
    throw 'G04D-C candidate paths may not retain or call an unsandboxed LibreOffice smoke.'
}
foreach ($runtimeBoundary in @('--version', '--convert-to', 'Invoke-G04DCZeroCapabilityProbe', 'zero-capability AppContainer only')) {
    if (!$sandboxProof.Contains($runtimeBoundary) -and !$adminProof.Contains($runtimeBoundary) -and !$minimalProof.Contains($runtimeBoundary)) {
        throw "G04D-C sandbox-only runtime boundary is missing: $runtimeBoundary"
    }
}
foreach ($machineSeal in @('serviceCatalogSha256', 'serviceRegistryCatalogSha256', 'scheduledTaskCatalogSha256', 'firewallCatalogSha256', 'installedProductCatalogSha256', 'otherInstalledProductCatalogSha256', 'installerCacheCatalogSha256', 'classRegistryCatalogSha256', 'shortcutCatalogSha256', 'environmentCatalogSha256', 'pendingReboot')) {
    if (!$commonProof.Contains($machineSeal)) { throw "G04D-C full machine-state digest is missing: $machineSeal" }
}
foreach ($startupSeal in @('RunOnce', 'WOW6432Node\Microsoft\Windows\CurrentVersion\Run', 'SpecialFolder]::Startup', 'ProgramData')) {
    if (!$commonProof.Contains($startupSeal)) { throw "G04D-C startup catalog boundary is missing: $startupSeal" }
}
foreach ($negativePathCase in @('sibling-prefix process escape', 'unresolved process identity', 'missed short-lived descendant')) {
    if (!$tests.Contains($negativePathCase)) { throw "G04D-C process ownership regression is missing: $negativePathCase" }
}
foreach ($lifecycleBoundary in @('$process.ExitCode -eq 0', '$uninstall.ExitCode -eq 0', 'exactProductRegistration', 'REBOOT_REQUIRED')) {
    if (!$minimalProof.Contains($lifecycleBoundary)) { throw "G04D-C minimal MSI lifecycle boundary is missing: $lifecycleBoundary" }
}
foreach ($minimalBoundary in @('INSTALLLEVEL=0', 'REMOVE=$removeFeatureList', 'MsiConditionEvaluator', 'Resolve-G04DCExpectedComponentStates', 'Get-G04DCMutationClosure', 'Assert-G04DCMinimalMutationClosure', 'Get-G04DCInstalledFeatureStates', 'Assert-G04DCInstalledFeatureStates', 'Assert-G04DCInstalledComponentStates', 'Assert-G04DCInstalledFileOwnership', 'Assert-G04DCMsiRegistrationInstalled')) {
    if (!$minimalProof.Contains($minimalBoundary)) { throw "G04D-C minimal feature/file boundary is missing: $minimalBoundary" }
}
if (!$minimalProof.Contains('post-uninstall-install-root-residue.json') -or !$minimalProof.Contains('-and ![bool]$installRootResidue.present') -or
    $minimalProof.IndexOf('post-uninstall-install-root-residue.json', [StringComparison]::Ordinal) -gt $minimalProof.LastIndexOf('$cleanup = Remove-G04DCOwnedRoot', [StringComparison]::Ordinal)) {
    throw 'G04D-C minimal uninstall must reject install-root residue before marker-owned cleanup.'
}
$postSmokeComparison = '$smokeComparison = Compare-G04DCMachineState -Before $before -After $afterSmoke'
$sandboxPass = '$sandboxPassed = $true'
if (!$minimalProof.Contains($postSmokeComparison) -or !$minimalProof.Contains($sandboxPass) -or
    $minimalProof.IndexOf($postSmokeComparison, [StringComparison]::Ordinal) -gt $minimalProof.IndexOf($sandboxPass, [StringComparison]::Ordinal)) {
    throw 'G04D-C minimal candidate must complete the post-smoke machine comparison before setting sandboxPassed.'
}
foreach ($processBoundary in @('TotalAssignedProcesses', 'totalAssignedProcesses', 'SandboxRunException')) {
    if (!$allProof.Contains($processBoundary)) { throw "G04D-C complete process/failure evidence is missing: $processBoundary" }
}
foreach ($moduleBoundary in @('loadedModules', 'moduleInventoryComplete', 'Get-G04DCLoadBearingModuleEvidence', 'sandbox-load-bearing-modules.json', 'signerThumbprint -cne $expected.SignerThumbprint', 'O=Microsoft Corporation')) {
    if (!$allProof.Contains($moduleBoundary)) { throw "G04D-C dynamic module provenance boundary is missing: $moduleBoundary" }
}
if (!$runtimeManifestProof.Contains('potentialExecutable') -or !$runtimeManifestProof.Contains('staticallyAdmittedEntryPoint') -or $runtimeManifestProof.Contains('invalidLoadBearing')) {
    throw 'G04D-C runtime manifest must distinguish static PE inventory from dynamically proven load-bearing modules.'
}
foreach ($identityChainBoundary in @('signerChainElements', 'timestampChainElements', '@($Identity.signerChain).Count -ge 2')) {
    if (!$commonProof.Contains($identityChainBoundary)) { throw "G04D-C MSI certificate-chain evidence is missing: $identityChainBoundary" }
}
foreach ($networkBoundary in @("'LoopbackExempt' '-s'", 'TCP and UDP owned sockets/listeners sampled', 'loopbackExemptBefore', 'loopbackExemptAfter')) {
    if (!$allProof.Contains($networkBoundary)) { throw "G04D-C bounded network proof is missing: $networkBoundary" }
}
foreach ($fileAccessBoundary in @('ownedWritablePathInventory', 'appContainerExternalStorageAbsent', 'appContainerRegistryResidueAbsent', 'runtimeTreeUnchanged', 'GetAppContainerFolderPath', 'actualAccessTelemetryCaptured', 'effectiveDenialOutsideAllowedRootsProven')) {
    if (!$sandboxProof.Contains($fileAccessBoundary) -and !$commonProof.Contains($fileAccessBoundary)) { throw "G04D-C file-access boundary is missing: $fileAccessBoundary" }
}
if (!$sandboxProof.Contains('actualAccessTelemetryCaptured = $false') -or !$sandboxProof.Contains('effectiveDenialOutsideAllowedRootsProven = $false') -or !$sandboxProof.Contains('Assert-G04DCFileAccessEvidence')) {
    throw 'G04D-C must reject candidate admission when empirical effective file-access telemetry is unavailable.'
}
foreach ($registrationBoundary in @('ProductState', 'LocalPackage', 'Installer\Products', 'Installer\Features', 'Installer\UpgradeCodes', 'Installer\UserData\S-1-5-18\Components', 'Assert-G04DCMsiRegistrationAbsent')) {
    if (!$commonProof.Contains($registrationBoundary)) { throw "G04D-C authoritative Windows Installer boundary is missing: $registrationBoundary" }
}
foreach ($effectBoundary in @('RemoveRegistry', 'Extension', 'ProgId', 'MIME', 'Verb', 'Class', 'AppId', 'Shortcut', 'Environment', 'AdminExecuteSequence', 'AdminUISequence', 'unboundedInstallCustomActions', 'unboundedAdminCustomActions')) {
    if (!$commonProof.Contains($effectBoundary)) { throw "G04D-C MSI effect/action model is missing: $effectBoundary" }
}
foreach ($cleanupBoundary in @('markerOwnedPathsOnly', 'reparseEntryCount', 'markerSha256', 'Remove-G04DCOwnedRoot')) {
    if (!$commonProof.Contains($cleanupBoundary)) { throw "G04D-C marker-owned cleanup boundary is missing: $cleanupBoundary" }
}
if (!$adminProof.Contains('Remove-G04DCOwnedRoot') -or !$minimalProof.Contains('Remove-G04DCOwnedRoot')) {
    throw 'G04D-C modes must use the exact marker/non-reparse owned-root cleanup helper on all terminal paths.'
}
if (!$adminProof.Contains('Assert-G04DCRunnerIsolation -State $before') -or !$minimalProof.Contains('Assert-G04DCRunnerIsolation -State $before')) {
    throw 'G04D-C modes must fail closed on a pre-existing ordinary profile or LibreOffice process.'
}
if ($workflow -match '(?m)^\s+(push|pull_request|schedule):') { throw 'G04D-C expensive proof workflow must be workflow_dispatch only.' }
if ([regex]::Matches($workflow, 'persist-credentials:\s*false').Count -ne 3) { throw 'G04D-C checkouts must not persist the read-scoped GitHub token.' }
if (!$workflow.Contains('if: ${{ always() }}') -or !$workflow.Contains('PROOF_PROVENANCE_OR_INFRASTRUCTURE_FAILURE') -or !$decisionProof.Contains('Assert-G04DCArtifactManifest')) {
    throw 'G04D-C decision must always run, separate infrastructure failure, and validate both artifact manifests.'
}
foreach ($sourceBoundary in @('AllowAutoRedirect = $false', "Host -cne 'download.documentfoundation.org'", 'Uri.Port -ne 443', 'MSI_ACQUISITION_SOURCE_REJECTED', 'mirrors are prohibited')) {
    if (!$commonProof.Contains($sourceBoundary)) { throw "G04D-C same-origin acquisition boundary is missing: $sourceBoundary" }
}
if ($commonProof.Contains('Invoke-WebRequest')) { throw 'G04D-C acquisition may not silently follow a redirect to a mirror.' }
foreach ($uses in [regex]::Matches($workflow, '(?m)^\s*-\s+uses:\s*([^\s]+)')) {
    if ($uses.Groups[1].Value -notmatch '@[0-9a-f]{40}$') { throw "G04D-C action is not pinned to a full commit: $($uses.Groups[1].Value)" }
}
foreach ($prohibited in @('PortableApps', 'winget install', 'ms-windows-store:', 'Invoke-Expression', 'cmd.exe', '--accept')) {
    if ($allProof -match [regex]::Escape($prohibited)) { throw "G04D-C proof contains prohibited acquisition or execution path: $prohibited" }
}
foreach ($required in @(
    'ServiceInstall', 'ServiceControl', 'FeatureComponents', 'CustomAction',
    'CreateAppContainerProfile', 'AssignProcessToJobObject', 'CREATE_SUSPENDED',
    'JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE', 'JOB_OBJECT_LIMIT_ACTIVE_PROCESS', 'JOB_OBJECT_LIMIT_JOB_MEMORY',
    'qpdfStrict', 'pdfjsOpened', 'ordinaryProfile', 'ADDLOCAL=', 'REMOVE=', '/a', '/x'
)) {
    if (!$allProof.Contains($required)) { throw "G04D-C proof implementation is missing: $required" }
}
foreach ($regression in @('ambiguous MSI effect ownership', 'enabled MSI shortcut mutation', 'unbounded install custom action', 'unbounded administrative custom action', 'Windows Installer registration residue', 'Windows Installer cache residue', 'service registry configuration change', 'full classes registry change', 'desktop or start-menu shortcut change', 'environment registry change', 'file-access observation incomplete')) {
    if (!$tests.Contains($regression)) { throw "G04D-C fail-closed regression is missing: $regression" }
}

$allowed = @(
    '.github/workflows/g04d-c-libreoffice-runtime-proof.yml',
    'docs/adr/ADR-018-non-mutating-libreoffice-runtime-acquisition.md',
    'docs/implementation-log/G04D-C-libreoffice-runtime-admission.md',
    'scripts/g04d-c/',
    'scripts/validate_repo.py'
)
$base = 'origin/main'
$changed = @(git -C $repositoryRoot diff --name-only $base --)
foreach ($path in $changed) {
    $normalized = $path.Replace('\', '/')
    if (@($allowed | Where-Object { $normalized -ceq $_ -or ($_.EndsWith('/') -and $normalized.StartsWith($_, [StringComparison]::Ordinal)) }).Count -eq 0) {
        throw "G04D-C branch modified out-of-scope path: $normalized"
    }
}

& (Join-Path $PSScriptRoot 'Test-G04DCBoundaries.ps1')
Write-Output 'G04D-C workflow trigger, pinned actions, exact provenance, table derivation, separate runners, AppContainer/Job boundary, output verification, failure cases, and proof-only scope verified.'
