Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$sourceGatePath = Join-Path $PSScriptRoot 'Test-G04DCPowerShell51Source.ps1'
& powershell.exe -NoProfile -ExecutionPolicy Bypass `
    -File $sourceGatePath `
    -RepositoryRoot $repositoryRoot `
    -SourceRoot $PSScriptRoot
if ($LASTEXITCODE -ne 0) { throw 'G04D-C source compatibility gate failed.' }
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
$classRegistryDigestProof = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'ClassRegistryDigest.cs') -Raw
$runtimeManifestProof = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'New-G04DCRuntimeManifest.ps1') -Raw
$decisionProof = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'New-G04DCCandidateDecision.ps1') -Raw
$precheckProof = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'Invoke-G04DCMachineStatePrecheck.ps1') -Raw
$tests = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'Test-G04DCBoundaries.ps1') -Raw
$provenanceHelper = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'AuthenticodeProvenance.cs') -Raw
$provenanceModule = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'G04DC.Provenance.psm1') -Raw
$onlineProvenance = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'New-G04DCOnlineProvenanceRecord.ps1') -Raw
$combinedProvenance = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'New-G04DCCombinedProvenanceAttestation.ps1') -Raw
$offlineProvenance = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'Test-G04DCOfflineProvenanceAttestation.ps1') -Raw
$provenanceTests = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'Test-G04DCProvenanceBoundaries.ps1') -Raw
foreach ($c13Boundary in @(
    'WinVerifyTrust', 'WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT', 'CertGetCertificateChain',
    'CERT_CHAIN_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT', 'CERT_CHAIN_REVOCATION_ACCUMULATIVE_TIMEOUT',
    'CertCreateCertificateChainEngine', 'hExclusiveRoot', 'CERT_CHAIN_CACHE_ONLY_URL_RETRIEVAL',
    'CERT_CHAIN_POLICY_AUTHENTICODE', 'AUTHENTICODE_TS', 'CryptSIPGetSignedDataMsg',
    'CryptSIPVerifyIndirectData', 'SignedCms', 'CheckSignature(true)',
    'Cryptographic APIs', 'PurposeEkuValid', 'purposeEkuValid', 'RSASSA-PKCS1-v1_5-SHA256', 'attestation-private-key.cspblob',
    'two-independent-online-verifier-results', 'ONLINE_PROVENANCE_VERIFIERS_DISAGREE',
    'OFFLINE_PROVENANCE_VERIFIED', 'signed-payload-manifest.json', 'globalCertificateStoreUnchanged',
    'diagnosticDefaultChainAcceptedAsTrust = $false', 'diagnosticOfflineRevocationAcceptedAsTrust = $false'
)) {
    if (!$provenanceHelper.Contains($c13Boundary) -and !$provenanceModule.Contains($c13Boundary) -and
        !$onlineProvenance.Contains($c13Boundary) -and !$combinedProvenance.Contains($c13Boundary) -and !$offlineProvenance.Contains($c13Boundary)) {
        throw "G04D-C13 offline provenance boundary is missing: $c13Boundary"
    }
}
foreach ($onlineBoundary in @(
    "@('verify', '/pa', '/all', '/v', '/tw'", 'ONLINE_PROVENANCE_TARGET_OS_MISMATCH',
    "targetOsEnforcement = 'exact-verifier-host-build'", 'Number of signatures successfully Verified:',
    'Number of warnings:', 'Number of errors:', 'VerifyOnlineFileTrust', 'BuildOnlineChain',
    'urlRetrievalTimeoutMilliseconds', 'signerChainExcludeRoot = ''good''',
    'timestampChainExcludeRoot = ''good''', 'canonicalRecordContainsRawConsoleOutput = $false',
    'canonicalRecordContainsCertificateSubjectText = $false', 'injected-windows-sdk-10.0.26100.0-x64'
)) {
    if (!$onlineProvenance.Contains($onlineBoundary)) { throw "G04D-C13 online verifier boundary is missing: $onlineBoundary" }
}
foreach ($attestationBoundary in @(
    "[Security.Cryptography.RSACryptoServiceProvider]::new(3072)", 'C:\DocumentStudioLab\G04D-C12L\credentials',
    'SetAccessRuleProtection($true, $false)', 'privateKeyCopiedToGuest', 'signedPayloadManifestSha256',
    'minimumOnlineVerifierCount = 2', 'maximumLifetimeHours', 'AddHours($AttestationLifetimeHours)'
)) {
    if (!$combinedProvenance.Contains($attestationBoundary) -and !$offlineProvenance.Contains($attestationBoundary)) { throw "G04D-C13 attestation boundary is missing: $attestationBoundary" }
}
foreach ($offlineBoundary in @(
    'VerifyOfflineFileDigestAndSignature', 'BuildOfflineExclusiveChain', 'Get-G04DCCertificateStoreDigest',
    'Get-G04DCOfflineMsiIdentity', 'ATTESTATION_PRIVATE_KEY_LEAK', 'connectedAdapterCount -ne 0',
    'defaultRoutes', 'noLoopbackListenerUsedForProvenance', 'activeDnsServerCount', 'proxyFree',
    'ExpectedPublicKeySha256', 'MaximumClockSkewMinutes = 5',
    'ATTESTATION_FUTURE_DATED', 'ATTESTATION_EXPIRED', 'ATTESTATION_SIGNATURE_INVALID'
)) {
    if (!$offlineProvenance.Contains($offlineBoundary)) { throw "G04D-C13 offline verifier boundary is missing: $offlineBoundary" }
}
foreach ($negativeCase in @(
    'online revoked signer rejected', 'online revoked timestamp signer rejected', 'online revocation unknown rejected',
    'online offline revocation rejected', 'online partial chain rejected', 'online untrusted root rejected',
    'two verifier MSI hash disagreement', 'two verifier signer disagreement', 'two verifier timestamp disagreement',
    'two verifier intermediate disagreement', 'two verifier root disagreement', 'two verifier revocation disagreement',
    'attestation altered JSON rejected', 'attestation altered signature rejected', 'attestation altered public key rejected',
    'offline altered signed file rejected', 'offline missing intermediate rejected', 'offline substituted root rejected',
    'offline wrong EKU rejected', 'offline no network retrieval', 'privacy no private-key material in guest package',
    'Expected 60 G04D-C13 provenance cases'
)) {
    if (!$provenanceTests.Contains($negativeCase)) { throw "G04D-C13 provenance regression is missing: $negativeCase" }
}
if ($provenanceHelper -match '(?i)CERT_CHAIN_POLICY_IGNORE|WTD_REVOCATION_CHECK_NONE\s*\||RevocationMode\s*=\s*NoCheck' -or
    $onlineProvenance -match '(?i)ignore.*revocation|PartialChain.*accepted|OfflineRevocation.*accepted') {
    throw 'G04D-C13 may not ignore revocation or accept partial/offline online trust.'
}
$offlineGate = $adminProof.IndexOf('Test-G04DCOfflineProvenanceAttestation.ps1', [StringComparison]::Ordinal)
$adminExtraction = $adminProof.IndexOf("@('/a',", [StringComparison]::Ordinal)
if ($offlineGate -lt 0 -or $adminExtraction -lt 0 -or $offlineGate -gt $adminExtraction) {
    throw 'G04D-C13 offline provenance must pass before administrative extraction is prepared.'
}
foreach ($telemetryBoundary in @(
    'New-G04DCMachineStateCaptureContext', 'Write-G04DCMachineStateProgressRecord',
    'Write-G04DCMachineStateSubstageRecord', 'Start-G04DCMachineStateSubstage',
    'Complete-G04DCMachineStateSubstage',
    'Start-G04DCMachineStatePhase', 'Assert-G04DCMachineStateCaptureBudget',
    'Complete-G04DCMachineStatePhase', 'Complete-G04DCMachineStateCapture',
    'machine-state-progress.ndjson', 'machine-state-performance',
    'MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED', 'captureTargetMilliseconds',
    'hardCeilingMilliseconds', 'phaseCeilingMilliseconds', 'Flush($true)',
    'canonical-state-finalization', 'state-serialization', 'Write-G04DCBoundedMachineStateJson',
    'Get-G04DCBoundedFileSha256', 'Invoke-G04DCBoundedCaptureProcess',
    'KillOnCloseJob', 'TerminateAndVerify', 'Assert-G04DCMachineStatePerformanceEvidence'
)) {
    if (!$commonProof.Contains($telemetryBoundary) -and !$allProof.Contains($telemetryBoundary)) { throw "G04D-C machine-state telemetry boundary is missing: $telemetryBoundary" }
}
foreach ($classRegistryBoundary in @(
    'ClassRegistryDigestCollector', 'BoundedTextCapture', 'BeginRead', 'AppendStderrText',
    'StringComparer.CurrentCultureIgnoreCase', 'SHA256.Create()', 'REGISTRY_DIGEST_TIMEOUT',
    'REGISTRY_DIGEST_RAW_BYTE_CEILING', 'REGISTRY_DIGEST_ROW_CEILING',
    'REGISTRY_DIGEST_CANONICAL_BYTE_CEILING', 'REGISTRY_DIGEST_STDERR_CEILING',
    'CanonicalUtf8 = new UTF8Encoding(false, true)', 'nativeOutputEncoding',
    'CanonicalUtf8.GetByteCount', 'CanonicalUtf8.GetBytes'
)) {
    if (!$classRegistryDigestProof.Contains($classRegistryBoundary)) { throw "G04D-C class-registry streaming boundary is missing: $classRegistryBoundary" }
}
foreach ($directRegistryBoundary in @(
    'DirectClassRegistryDigestCollector', 'RegistryHive.ClassesRoot', 'RegistryView.Registry64',
    'RegQueryValueExW', 'CollectClassesRoot64', 'REGISTRY_TRAVERSAL_ACCESS_DENIED',
    'REGISTRY_TRAVERSAL_UNSTABLE', 'REGISTRY_TRAVERSAL_KEY_CEILING',
    'REGISTRY_TRAVERSAL_VALUE_CEILING', 'REGISTRY_TRAVERSAL_DEPTH_CEILING',
    'REGISTRY_TRAVERSAL_VALUE_BYTE_CEILING', 'REGISTRY_TRAVERSAL_CANONICAL_BYTE_CEILING',
    'public int SchemaVersion { get { return 2; } }'
)) {
    if (!$classRegistryDigestProof.Contains($directRegistryBoundary)) { throw "G04D-C direct HKCR boundary is missing: $directRegistryBoundary" }
}
if ($classRegistryDigestProof -match '(?i)CreateSubKey|SetValue|Delete(SubKey|Value)|WriteAll(Text|Bytes)|FileMode\.Create|RegistryKey\.OpenRemoteBaseKey') {
    throw 'G04D-C class-registry helper may not mutate registry, use remote registry, or create raw-output artifacts.'
}
foreach ($classRegistryProcessBoundary in @(
    'native-query-startup', 'native-query-read', 'row-normalization', 'canonical-hash', 'helper-cleanup',
    'MaximumRawBytes = 134217728', 'MaximumRows = 1000000', 'StandardOutputEncoding',
    'ActiveHandleCount', 'nativeCleanupFailureStage', 'secondaryNativeCleanupFailure',
    'MACHINE_STATE_CAPTURE_EVIDENCE_FAILED', 'LifecycleTestHooks',
    'DisposeStdoutReader', 'DisposeStdoutStream', 'DisposeStderrReader', 'DisposeStderrStream',
    'DisposeStdoutTask', 'DisposeStderrTask'
)) {
    if (!$commonProof.Contains($classRegistryProcessBoundary)) { throw "G04D-C class-registry process boundary is missing: $classRegistryProcessBoundary" }
}
foreach ($classRegistryRegression in @(
    'current native-row normalization equivalence', 'incremental hash equality',
    '64236-row registry digest scale', 'explicit UTF-8 native output encoding',
    'split multibyte character across stream buffers', 'truncated multibyte native output rejected',
    'class registry substage ordering', 'class registry helper termination',
    'registry key creation mutation equivalence', 'registry default absent-empty distinction',
    'registry value-kind mutation equivalence', 'registry Unicode mutation equivalence',
    'optimized class registry collector performs no mutation', 'C7 class registry target is 180000 ms',
    'direct HKCR Registry64 merged-view fixture', 'direct HKCR access denial fails closed',
    'direct HKCR disappearing key fails closed', 'direct HKCR disappearing value fails closed',
    'class registry digest schema change', 'class registry key-count change', 'class registry value-count change',
    'direct HKCR traversal timeout', 'direct HKCR handles disposed for owned cleanup',
    'shortcut catalog transient sharing retry preserves full entry',
    'shortcut catalog persistent sharing failure remains fail closed',
    'P1-A cleanup error precedence preserves secondary evidence',
    'P1-A redirected readers streams and tasks are disposed',
    'P1-A production cleanup uses no global process termination',
    'P1-B canonical hash independent of Console OutputEncoding',
    'P1-B accepted canonical fixture remains byte-identical', 'Expected 315 fail-closed cases'
)) {
    if (!$tests.Contains($classRegistryRegression)) { throw "G04D-C class-registry regression is missing: $classRegistryRegression" }
}
if (!$precheckProof.Contains('classRegistryDigestTargetMilliseconds = 180000L') -or !$precheckProof.Contains('CLASS_REGISTRY_DIGEST_TARGET_EXCEEDED')) {
    throw 'G04D-C PRECHECK class-registry target is not fixed at 180000 ms.'
}
foreach ($directoryRetryBoundary in @(
    '$maximumAttempts = 20', 'Start-Sleep -Milliseconds 500', 'DIRECTORY_TREE_CAPTURE_UNSTABLE',
    '$afterItem.LastWriteTimeUtc', "'IOException', 'ItemNotFoundException'"
)) {
    if (!$commonProof.Contains($directoryRetryBoundary)) { throw "G04D-C bounded directory retry boundary is missing: $directoryRetryBoundary" }
}
foreach ($phase in @(
    'associations', 'font-registry-catalog', 'protected-font-files', 'external-runtime-targets',
    'service-catalog', 'service-registry-digest', 'app-paths', 'class-key-catalog',
    'class-registry-digest', 'startup', 'scheduled-tasks', 'shortcut-catalog',
    'environment-catalog', 'firewall-catalog', 'protected-registry-targets', 'process-catalog',
    'installed-product-catalog', 'installer-cache-catalog', 'pending-reboot', 'msi-registration'
)) {
    if (!$commonProof.Contains("-Phase '$phase'")) { throw "G04D-C machine-state phase is not independently timed: $phase" }
}
foreach ($registrationBoundary in @(
    'Get-G04DCMsiComponentRegistrationState', 'RegistryView]::Registry64', 'OpenBaseKey',
    'OpenComponentKey', 'OpenSubKey($PackedComponent, $false)', 'DoNotExpandEnvironmentNames',
    'systemProductValueState', 'userProductValueState'
)) {
    if (!$commonProof.Contains($registrationBoundary)) { throw "G04D-C optimized MSI registration boundary is missing: $registrationBoundary" }
}
$componentCollectorStart = $commonProof.IndexOf('function Get-G04DCMsiComponentRegistrationState', [StringComparison]::Ordinal)
$registrationCollectorStart = $commonProof.IndexOf('function Get-G04DCMsiRegistrationState', [StringComparison]::Ordinal)
if ($componentCollectorStart -lt 0 -or $registrationCollectorStart -le $componentCollectorStart -or
    $commonProof.Substring($componentCollectorStart, $registrationCollectorStart - $componentCollectorStart).Contains('Get-G04DCRegistryValueState')) {
    throw 'G04D-C optimized component collector may not invoke the Registry provider value helper per component.'
}
foreach ($registryStateBoundary in @(
    'Get-G04DCRegistryValueState', 'Get-G04DCRegistryDefaultValueState',
    'keyExists', 'defaultValuePresent', 'defaultValueType', 'defaultValue',
    'valuePresent', 'valueType', 'DoNotExpandEnvironmentNames',
    'REGISTRY_STATE_CAPTURE_FAILED', '1048576'
)) {
    if (!$commonProof.Contains($registryStateBoundary)) { throw "G04D-C typed registry-state boundary is missing: $registryStateBoundary" }
}
if ($commonProof.Contains('Get-ItemPropertyValue')) {
    throw 'G04D-C monitored registry values may not use the missing-value-ambiguous Get-ItemPropertyValue collector pattern.'
}
foreach ($registryRegression in @(
    'registry key missing state', 'registry key exists without default state', 'empty-string registry default present',
    'normal REG_SZ registry default', 'typed non-string registry default', 'registry value disappears during read',
    'registry access denied simulation', 'unexpected registry provider failure', '.odt exact missing-default runner condition',
    '.ods and .odp missing-default handling', 'registry state serialization distinction',
    'identical absent registry defaults compare equal', 'registry no-default to value transition detected',
    'registry value to no-default transition detected', 'registry collector performs no mutation'
)) {
    if (!$tests.Contains($registryRegression)) { throw "G04D-C registry-state regression is missing: $registryRegression" }
}
foreach ($scheduledTaskBoundary in @(
    'Get-G04DCSafePropertyState', 'ConvertTo-G04DCScheduledTaskSensitiveActionValueEvidence', 'ConvertTo-G04DCScheduledTaskActionEvidence',
    'ConvertTo-G04DCScheduledTaskEvidence', 'Get-G04DCScheduledTaskCatalogEvidence',
    'SCHEDULED_TASK_ACTION_CAPTURE_FAILED', 'SCHEDULED_TASK_DEFINITION_CAPTURE_FAILED',
    'cimClass', 'actionKind', 'properties', 'definitionSha256',
    "'exec'", "'comHandler'", "'email'", "'showMessage'", "'other'",
    "@('Id', 'Execute', 'Arguments', 'WorkingDirectory')",
    "@('Id', 'ClassId', 'Data')", "Get-ScheduledTask -ErrorAction Stop",
    'Export-ScheduledTask', 'maximumStringCharacters = 16384', 'maximumArrayMembers = 128',
    "'comHandler' { @('Data') }", "'showMessage' { @('Title', 'Message') }"
)) {
    if (!$commonProof.Contains($scheduledTaskBoundary)) { throw "G04D-C heterogeneous scheduled-task evidence boundary is missing: $scheduledTaskBoundary" }
}
if ($commonProof -match '\$_\.Execute|\$action\.Execute|\$Action\.Execute') {
    throw 'G04D-C scheduled-task catalog may not directly assume an Execute property.'
}
if ($commonProof -match '(?i)(Register|Set|Unregister|Disable|Enable)-ScheduledTask') {
    throw 'G04D-C scheduled-task evidence collection may not mutate scheduled tasks.'
}
foreach ($scheduledTaskRegression in @(
    'normal scheduled task Exec action', 'scheduled task Exec empty Arguments',
    'scheduled task Exec absent WorkingDirectory', 'scheduled task COM handler without Execute',
    'scheduled task COM ClassId and Data', 'scheduled task different valid property set',
    'scheduled task scalar and array shapes preserved', 'scheduled task sensitive email values are hash only',
    'scheduled task sensitive COM data is hash only', 'changed sensitive scheduled task value changes hash evidence',
    'mixed scheduled task Exec and COM actions', 'scheduled task original action order preserved',
    'scheduled task property absent versus present null', 'scheduled task property present empty string',
    'unknown identifiable scheduled task CIM class', 'unknown scheduled task action retains XML hash coverage',
    'scheduled task action class cannot be identified', 'scheduled task Export-ScheduledTask failure',
    'scheduled task property getter failure', 'scheduled task bounded string overflow',
    'scheduled task bounded array overflow', 'scheduled task recursive object serialization',
    'scheduled task non-finite primitive serialization',
    'scheduled task deterministic repeated serialization', 'changed scheduled task action changes definition evidence',
    'unchanged heterogeneous scheduled task action compares equal', 'scheduled task collection performs no mutation',
    'GitHub runner shaped non-Exec scheduled task action', 'no direct Execute assumption in scheduled task catalog',
    'scheduled task TaskPath and TaskName ordering', 'Expected 315 fail-closed cases'
)) {
    if (!$tests.Contains($scheduledTaskRegression)) { throw "G04D-C scheduled-task regression is missing: $scheduledTaskRegression" }
}
$registryRowsDefinition = '$protectedRegistryRows = @($analysis.protectedOwnership.allRegistryRows)'
$firstRegistryRowsUse = '$before = Get-G04DCAdminMachineState -Label ''pre'''
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
foreach ($machineSeal in @('serviceCatalogSha256', 'serviceRegistryCatalogSha256', 'scheduledTaskCatalogSha256', 'firewallCatalogSha256', 'installedProductCatalogSha256', 'otherInstalledProductCatalogSha256', 'installerCacheCatalogSha256', 'classRegistryCatalogSchemaVersion', 'classRegistryKeyCount', 'classRegistryValueCount', 'classRegistryCatalogSha256', 'shortcutCatalogSha256', 'environmentCatalogSha256', 'pendingReboot')) {
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
if ([regex]::Matches($workflow, 'persist-credentials:\s*false').Count -ne 4) { throw 'G04D-C checkouts must not persist the read-scoped GitHub token.' }
if (!$workflow.Contains('if: ${{ always() }}') -or !$workflow.Contains('PROOF_PROVENANCE_OR_INFRASTRUCTURE_FAILURE') -or !$decisionProof.Contains('Assert-G04DCArtifactManifest')) {
    throw 'G04D-C decision must always run, separate infrastructure failure, and validate both artifact manifests.'
}
if (!$workflow.Contains('proofMode:') -or !$workflow.Contains('- precheck') -or !$workflow.Contains('- full') -or
    !$workflow.Contains("inputs.proofMode == 'precheck'") -or !$workflow.Contains("inputs.proofMode == 'full'")) {
    throw 'G04D-C workflow must expose bounded precheck/full dispatch modes.'
}
if ([regex]::Matches($workflow, '(?m)^\s+timeout-minutes:\s*90\s*$').Count -ne 2 -or
    [regex]::Matches($workflow, '(?m)^\s+timeout-minutes:\s*20\s*$').Count -ne 1) {
    throw 'G04D-C PRECHECK must be 20 minutes and both FULL proof jobs must remain 90 minutes.'
}
if ($precheckProof -match '(?i)msiexec|/a|soffice|Invoke-G04DCSandboxSmoke|AppContainer') {
    throw 'G04D-C PRECHECK may not extract, install, or launch the runtime.'
}
foreach ($precheckBoundary in @(
    'machine-pre.json', 'machine-state-progress.ndjson', 'machine-state-performance.json',
    'MACHINE_STATE_PRECHECK_PASS', 'MACHINE_STATE_PRECHECK_BLOCKED',
    'Assert-G04DCRunnerIsolation', 'Assert-G04DCMsiRegistrationAbsent',
    'candidateClassificationProduced = $false', 'bootstrap-source-validation.json',
    'precheckScriptStarted', 'PRECHECK_BOOTSTRAP_INVALID'
)) {
    if (!$precheckProof.Contains($precheckBoundary)) { throw "G04D-C PRECHECK boundary is missing: $precheckBoundary" }
}
foreach ($sourceCompatibilityBoundary in @(
    'Test-G04DCPowerShell51Source.ps1', 'g04d_c_powershell_source_policy.py',
    'ASCII-byte gate result: PASS', 'Windows PowerShell 5.1 parser gate result: PASS',
    'Initialize fail-safe PRECHECK bootstrap evidence', 'bootstrap-source-validation.json',
    'sourceFileCount', 'asciiGateStatus', 'parserGateStatus', 'precheckScriptStarted'
)) {
    if (!$workflow.Contains($sourceCompatibilityBoundary) -and !$allProof.Contains($sourceCompatibilityBoundary)) {
        throw "G04D-C source/bootstrap compatibility boundary is missing: $sourceCompatibilityBoundary"
    }
}
foreach ($sourceBoundary in @(
    'TcpClient]::new', '$tcp.Connect($selected, 443)', 'AuthenticateAsClient', 'RemoteEndPoint',
    'Assert-G04DCPinnedRemoteEndpoint', 'CanonicalFirstRequest', 'maximumRedirects = 8', 'Redirect loop detected',
    'Raw IP-literal acquisition hosts are prohibited', 'Test-G04DCRestrictedIpAddress', 'FileMode]::CreateNew',
    'FileShare]::None', 'Assert-G04DCBoundedDownloadLength', 'Assert-G04DCFailedDownloadCleanup',
    'mirrorHostnameIsTrustAnchor = $false', 'MSI_ACQUISITION_SOURCE_REJECTED'
)) {
    if (!$commonProof.Contains($sourceBoundary)) { throw "G04D-C MirrorBrain acquisition boundary is missing: $sourceBoundary" }
}
if ($commonProof.Contains('Invoke-WebRequest') -or $commonProof.Contains('HttpWebRequest')) { throw 'G04D-C acquisition may not delegate DNS or redirect handling to an unpinned HTTP client.' }
foreach ($redirectRegression in @(
    'exact canonical first request', 'accepted HTTPS cross-origin MirrorBrain redirect', 'pinned HTTPS remote endpoint',
    'DNS rebinding remote endpoint', 'multi-hop HTTPS redirect',
    'exact eight-hop redirect boundary', 'ninth-hop rejection', 'redirect loop', 'redirect missing Location',
    'HTTP downgrade', 'non-default port', 'acquisition URI userinfo', 'acquisition URI empty userinfo', 'acquisition URI fragment', 'localhost target',
    'IPv4 loopback target', 'IPv6 loopback target', 'RFC1918 private target', 'link-local target',
    'multicast target', 'reserved target', 'unspecified target', 'raw IP literal target',
    'unexpected final filename or path', 'truncated acquisition body', 'oversized acquisition body', 'bounded chunked acquisition body',
    'failed-download cleanup ownership', 'complete redirect-chain evidence', 'no production acquisition path'
)) {
    if (!$tests.Contains($redirectRegression)) { throw "G04D-C MirrorBrain regression is missing: $redirectRegression" }
}
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
    '.github/workflows/ci.yml',
    '.github/workflows/g04d-c-libreoffice-runtime-proof.yml',
    'docs/adr/ADR-018-non-mutating-libreoffice-runtime-acquisition.md',
    'docs/implementation-log/G04D-C-libreoffice-runtime-admission.md',
    'scripts/g04d-c/',
    'scripts/g04d_c_powershell_source_policy.py',
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
