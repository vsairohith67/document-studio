param(
    [Parameter(Mandatory = $true)] [string]$AdminEvidenceDirectory,
    [Parameter(Mandatory = $true)] [string]$MinimalEvidenceDirectory,
    [Parameter(Mandatory = $true)] [string]$OutputDirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'G04DC.Common.psm1') -Force

if (Test-Path -LiteralPath $OutputDirectory) { throw "Refusing to overwrite decision directory: $OutputDirectory" }
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
$adminPath = Join-Path $AdminEvidenceDirectory 'candidate-result.json'
$minimalPath = Join-Path $MinimalEvidenceDirectory 'candidate-result.json'
if (!(Test-Path -LiteralPath $adminPath) -or !(Test-Path -LiteralPath $minimalPath)) {
    throw '[MISSING_CANDIDATE_EVIDENCE] Both fresh-runner candidate results are required.'
}
Assert-G04DCArtifactManifest -EvidenceDirectory $AdminEvidenceDirectory | Out-Null
Assert-G04DCArtifactManifest -EvidenceDirectory $MinimalEvidenceDirectory | Out-Null
$admin = Get-Content -LiteralPath $adminPath -Raw | ConvertFrom-Json
$minimal = Get-Content -LiteralPath $minimalPath -Raw | ConvertFrom-Json
if ([string]$admin.mode -cne 'ADMIN_IMAGE' -or [string]$minimal.mode -cne 'MINIMAL_MSI') {
    throw '[CANDIDATE_EVIDENCE_INVALID] Candidate evidence mode identity is invalid.'
}
if (([bool]$admin.candidate -and [string]$admin.classification -cne 'ADMIN_IMAGE_CANDIDATE') -or
    (![bool]$admin.candidate -and [string]$admin.classification -cne 'REJECTED') -or
    ([bool]$minimal.candidate -and [string]$minimal.classification -cne 'MINIMAL_MSI_CANDIDATE') -or
    (![bool]$minimal.candidate -and [string]$minimal.classification -cne 'REJECTED')) {
    throw '[CANDIDATE_EVIDENCE_INVALID] Candidate boolean and classification disagree.'
}
if ($admin.msiSha256 -cne $minimal.msiSha256 -or $admin.msiSha256 -cne (Get-G04DCExpectedMsi).Sha256) {
    throw '[MSI_IDENTITY_MISMATCH] Candidate jobs did not prove the same exact MSI.'
}
$classification = if ([bool]$admin.candidate) {
    'ADMIN_IMAGE_CANDIDATE'
}
elseif ([bool]$minimal.candidate) {
    'MINIMAL_MSI_CANDIDATE'
}
else {
    'LIBREOFFICE_RUNTIME_UNSUPPORTED'
}
$decision = [ordered]@{
    schemaVersion = 1
    classification = $classification
    preferredModelReason = if ($classification -ceq 'ADMIN_IMAGE_CANDIDATE') {
        'Administrative image has less host lifecycle ownership than an MSI product registration.'
    } elseif ($classification -ceq 'MINIMAL_MSI_CANDIDATE') {
        'Administrative image failed; the minimal MSI candidate alone passed non-mutation, sandbox, smoke, uninstall, and restoration gates.'
    } else {
        'Neither exact official-MSI acquisition model passed every non-mutation, runtime, AppContainer, Job Object, network, output, and cleanup gate.'
    }
    productionSupportDeclared = $false
    adminImage = $admin
    minimalMsi = $minimal
}
Write-G04DCJson -Path (Join-Path $OutputDirectory 'g04d-c-candidate-decision.json') -Value $decision
New-G04DCArtifactManifest -EvidenceDirectory $OutputDirectory | Out-Null
Write-Output ($decision | ConvertTo-Json -Compress -Depth 10)
