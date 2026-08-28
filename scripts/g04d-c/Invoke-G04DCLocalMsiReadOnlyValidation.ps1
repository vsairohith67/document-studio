param(
    [Parameter(Mandatory = $true)] [string]$MsiPath,
    [Parameter(Mandatory = $true)] [string]$EvidenceDirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'G04DC.Common.psm1') -Force

if (Test-Path -LiteralPath $EvidenceDirectory) { throw "Refusing to overwrite evidence directory: $EvidenceDirectory" }
New-Item -ItemType Directory -Path $EvidenceDirectory | Out-Null
$evidence = (Resolve-Path -LiteralPath $EvidenceDirectory).Path
$markerContent = "# G04D-C exact MSI read-only validation`r`n"
[IO.File]::WriteAllText((Join-Path $evidence 'MARKER.md'), $markerContent, [Text.UTF8Encoding]::new($false))

$identity = Get-G04DCMsiIdentity -MsiPath $MsiPath
Assert-G04DCMsiIdentity -Identity $identity | Out-Null
Write-G04DCJson -Path (Join-Path $evidence 'identity.json') -Value $identity
$analysis = Export-G04DCMsiDatabase -MsiPath $MsiPath -OutputDirectory (Join-Path $evidence 'tables')
Add-Type -Path (Join-Path $PSScriptRoot 'G04DC.MsiCondition.cs')
$propertyNames = @('VC_REDIST', 'CREATEDESKTOPLINK', 'WRITE_REGISTRY', 'ISCHECKFORPRODUCTUPDATES', 'REGISTER_NO_MSO_TYPES')
$propertyValues = @('0', '0', '0', '0', '1')
$conditions = @($analysis.componentInstallOwnership | Where-Object {
    @($_.selectedOwners).Count -ne 0 -and ![string]::IsNullOrWhiteSpace([string]$_.condition)
} | ForEach-Object { $_.condition } | Sort-Object -Unique)
$evaluations = @([DocumentStudio.G04DC.Proof.MsiConditionEvaluator]::Evaluate(
    (Resolve-Path -LiteralPath $MsiPath).Path, $propertyNames, $propertyValues, $conditions
))
$componentOwnership = @(Resolve-G04DCExpectedComponentStates -ComponentOwnership @($analysis.componentInstallOwnership) -ConditionEvaluations $evaluations)
$mutationClosure = Get-G04DCMutationClosure -Analysis $analysis -ComponentOwnership $componentOwnership
Write-G04DCJson -Path (Join-Path $evidence 'condition-validation.json') -Value ([ordered]@{
    propertyNames = $propertyNames
    propertyValues = $propertyValues
    evaluations = $evaluations
    expectedComponentOwnership = $componentOwnership
})
Write-G04DCJson -Path (Join-Path $evidence 'mutation-closure.json') -Value $mutationClosure
Write-G04DCJson -Path (Join-Path $evidence 'validation-summary.json') -Value ([ordered]@{
    readOnly = $true
    msiSha256 = $identity.sha256
    selectedFeatureCount = @($analysis.candidateMinimumFeatureSet).Count
    selectedComponentCount = @($componentOwnership | Where-Object { @($_.selectedOwners).Count -ne 0 }).Count
    mutationTableRowCount = @($mutationClosure.rows).Count
    ambiguousMutationRowCount = @($mutationClosure.ambiguousRows).Count
    enabledMutationRowCount = @($mutationClosure.enabledMutationRows).Count
    unboundedInstallCustomActionCount = @($mutationClosure.unboundedInstallCustomActions).Count
    unboundedAdminCustomActionCount = @($mutationClosure.unboundedAdminCustomActions).Count
    minimalInstallModelClosed = [bool]$mutationClosure.minimalInstallModelClosed
    administrativeActionModelClosed = [bool]$mutationClosure.administrativeActionModelClosed
})
New-G04DCArtifactManifest -EvidenceDirectory $evidence | Out-Null
Write-Output (Join-Path $evidence 'validation-summary.json')
