[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)] [string]$VerifierAEvidenceDirectory,
    [Parameter(Mandatory = $true)] [ValidatePattern('^[0-9a-fA-F]{64}$')] [string]$ExpectedVerifierAManifestSha256,
    [Parameter(Mandatory = $true)] [string]$VerifierBEvidenceDirectory,
    [Parameter(Mandatory = $true)] [ValidatePattern('^[0-9a-fA-F]{64}$')] [string]$ExpectedVerifierBManifestSha256,
    [Parameter(Mandatory = $true)] [string]$OutputDirectory,
    [Parameter(Mandatory = $true)] [string]$KeyDirectory,
    [ValidateRange(1, 12)] [int]$AttestationLifetimeHours = 12
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'G04DC.Common.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'G04DC.Provenance.psm1') -Force

function Assert-G04DCExactProperties {
    param([Parameter(Mandatory = $true)] $Value, [Parameter(Mandatory = $true)] [string[]]$Names, [Parameter(Mandatory = $true)] [string]$Code)
    if ($null -eq $Value) { throw "[$Code] Required object is missing." }
    $actual = @($Value.PSObject.Properties | ForEach-Object { $_.Name } | Sort-Object)
    $expected = @($Names | Sort-Object)
    if (($actual -join "`n") -cne ($expected -join "`n")) { throw "[$Code] Object fields are missing, duplicated, or unexpected." }
}

function Get-G04DCSemanticJson {
    param([Parameter(Mandatory = $true)] $Value)
    return ($Value | ConvertTo-Json -Depth 30 -Compress)
}

function Assert-G04DCOnlineRecord {
    param([Parameter(Mandatory = $true)] $Record, [Parameter(Mandatory = $true)] [ValidateSet('A', 'B')] [string]$ExpectedId)
    Assert-G04DCExactProperties -Value $Record -Names @('schemaVersion', 'recordType', 'verifier', 'subject', 'authenticode', 'verification', 'tooling', 'privacy') -Code 'ONLINE_PROVENANCE_RECORD_INVALID'
    Assert-G04DCExactProperties -Value $Record.verifier -Names @('id', 'instanceId', 'vmUuid', 'diskUuid', 'verifiedAtUtc', 'osCaptionSha256', 'osVersion', 'osBuild', 'osArchitecture') -Code 'ONLINE_PROVENANCE_RECORD_INVALID'
    Assert-G04DCExactProperties -Value $Record.subject -Names @('filename', 'sizeBytes', 'sha256', 'version', 'architecture', 'productCode', 'upgradeCode', 'packageCode') -Code 'ONLINE_PROVENANCE_RECORD_INVALID'
    Assert-G04DCExactProperties -Value $Record.authenticode -Names @('signatureDigestAlgorithm', 'signatureDigestAlgorithmOid', 'timestampType', 'timestampUtc', 'signerLeafDerSha256', 'signerLeafThumbprint', 'timestampLeafDerSha256', 'timestampLeafThumbprint', 'signerChain', 'timestampChain', 'revocation') -Code 'ONLINE_PROVENANCE_RECORD_INVALID'
    Assert-G04DCExactProperties -Value $Record.authenticode.revocation -Names @('signerChainExcludeRoot', 'timestampChainExcludeRoot', 'evidenceMode') -Code 'ONLINE_PROVENANCE_RECORD_INVALID'
    $expected = Get-G04DCExpectedMsi
    $valid = [int]$Record.schemaVersion -eq 1 -and [string]$Record.recordType -ceq 'g04d-c13-online-authenticode-verifier' -and
        [string]$Record.verifier.id -ceq $ExpectedId -and [string]$Record.subject.filename -ceq [string]$expected.FileName -and
        [long]$Record.subject.sizeBytes -eq [long]$expected.SizeBytes -and [string]$Record.subject.sha256 -ceq [string]$expected.Sha256 -and
        [string]$Record.subject.version -ceq [string]$expected.ProductVersion -and [string]$Record.subject.architecture -ceq [string]$expected.Architecture -and
        [string]$Record.subject.productCode -ceq [string]$expected.ProductCode -and [string]$Record.subject.upgradeCode -ceq [string]$expected.UpgradeCode -and
        [string]$Record.subject.packageCode -ceq [string]$expected.PackageCode -and
        [string]$Record.authenticode.signerLeafThumbprint -ceq [string]$expected.SignerThumbprint -and
        [string]$Record.authenticode.timestampLeafThumbprint -ceq [string]$expected.TimestampSignerThumbprint -and
        [string]$Record.authenticode.signatureDigestAlgorithm -ceq 'sha256' -and
        [string]$Record.authenticode.signatureDigestAlgorithmOid -ceq '2.16.840.1.101.3.4.2.1' -and
        [string]$Record.authenticode.revocation.signerChainExcludeRoot -ceq 'good' -and
        [string]$Record.authenticode.revocation.timestampChainExcludeRoot -ceq 'good' -and
        [bool]$Record.verification.accepted -and [bool]$Record.verification.signTool.accepted -and
        [bool]$Record.verification.winVerifyTrust.accepted -and [bool]$Record.verification.signerChain.accepted -and
        [bool]$Record.verification.timestampChain.accepted -and [bool]$Record.verification.signerChain.revocationKnownGood -and
        [bool]$Record.verification.timestampChain.revocationKnownGood -and [string]$Record.verification.signerChain.errorStatusHex -ceq '0x00000000' -and
        [string]$Record.verification.timestampChain.errorStatusHex -ceq '0x00000000' -and [string]$Record.verification.signerChain.policyErrorHex -ceq '0x00000000' -and
        [string]$Record.verification.timestampChain.policyErrorHex -ceq '0x00000000' -and @($Record.authenticode.signerChain).Count -ge 2 -and
        @($Record.authenticode.timestampChain).Count -ge 2 -and ![bool]$Record.privacy.canonicalRecordContainsRawConsoleOutput -and
        ![bool]$Record.privacy.canonicalRecordContainsCertificateSubjectText -and ![bool]$Record.privacy.canonicalRecordContainsUsernameOrProfilePath -and
        ![bool]$Record.privacy.canonicalRecordContainsPrivateKeyMaterial
    if (!$valid) { throw '[ONLINE_PROVENANCE_RECORD_INVALID] Online verifier record did not pass the frozen fail-closed policy.' }
    [DateTime]$verified = [DateTime]::MinValue
    [DateTime]$timestamp = [DateTime]::MinValue
    if (![DateTime]::TryParse([string]$Record.verifier.verifiedAtUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AdjustToUniversal, [ref]$verified) -or
        ![DateTime]::TryParse([string]$Record.authenticode.timestampUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AdjustToUniversal, [ref]$timestamp)) {
        throw '[ONLINE_PROVENANCE_RECORD_INVALID] Online verifier timestamps are invalid.'
    }
    foreach ($chainName in @('signerChain', 'timestampChain')) {
        $position = 0
        foreach ($certificate in @($Record.authenticode.$chainName)) {
            Assert-G04DCExactProperties -Value $certificate -Names @('chainPosition', 'path', 'derSha256', 'thumbprint', 'serialNumber', 'subjectNameSha256', 'issuerNameSha256', 'publicKeyAlgorithmOid', 'publicKeySizeBits', 'signatureAlgorithmOid', 'ekuOids', 'notBeforeUtc', 'notAfterUtc', 'selfSignedName') -Code 'ONLINE_PROVENANCE_RECORD_INVALID'
            if ([int]$certificate.chainPosition -ne $position -or [string]$certificate.path -notmatch '^certificates/(signer|timestamp)-[0-9]{2}-[0-9a-f]{64}\.cer$' -or
                [string]$certificate.derSha256 -notmatch '^[0-9a-f]{64}$' -or [string]$certificate.thumbprint -notmatch '^[0-9A-F]{40}$') {
                throw '[ONLINE_PROVENANCE_RECORD_INVALID] Certificate chain metadata is not canonical.'
            }
            $position++
        }
    }
    return $true
}

function Copy-G04DCSealedVerifierEvidence {
    param(
        [Parameter(Mandatory = $true)] [string]$Source,
        [Parameter(Mandatory = $true)] [string]$Destination,
        [Parameter(Mandatory = $true)] [ValidateSet('A', 'B')] [string]$ExpectedId,
        [Parameter(Mandatory = $true)] [ValidatePattern('^[0-9a-fA-F]{64}$')] [string]$ExpectedManifestSha256
    )
    $sourceRoot = (Resolve-Path -LiteralPath $Source).Path
    Assert-G04DCExpectedArtifactManifest -EvidenceDirectory $sourceRoot -ExpectedManifestSha256 $ExpectedManifestSha256 | Out-Null
    if (@(Get-ChildItem -LiteralPath $sourceRoot -Recurse -Force | Where-Object { [bool]($_.Attributes -band [IO.FileAttributes]::ReparsePoint) }).Count -ne 0) {
        throw '[ONLINE_PROVENANCE_RECORD_INVALID] Verifier evidence contains a reparse point.'
    }
    $recordName = "online-verifier-$ExpectedId.json"
    $recordPath = Join-Path $sourceRoot $recordName
    $record = Read-G04DCCanonicalJson -Path $recordPath -Code 'ONLINE_PROVENANCE_RECORD_INVALID'
    Assert-G04DCOnlineRecord -Record $record -ExpectedId $ExpectedId | Out-Null
    $recordCertificatePaths = @(@($record.authenticode.signerChain) + @($record.authenticode.timestampChain) | ForEach-Object { [string]$_.path })
    if (@($recordCertificatePaths | Sort-Object -Unique).Count -ne $recordCertificatePaths.Count) {
        throw '[ONLINE_PROVENANCE_RECORD_INVALID] Verifier record repeats a certificate path.'
    }
    $sourceCertificatePaths = @(Get-ChildItem -LiteralPath (Join-Path $sourceRoot 'certificates') -File -Recurse -Force | ForEach-Object {
        $_.FullName.Substring($sourceRoot.Length + 1).Replace('\', '/')
    })
    $recordCertificateInventory = (@($recordCertificatePaths | Sort-Object) -join "`n")
    $sourceCertificateInventory = (@($sourceCertificatePaths | Sort-Object) -join "`n")
    if ($recordCertificateInventory -cne $sourceCertificateInventory) {
        throw '[ONLINE_PROVENANCE_RECORD_INVALID] Verifier certificate inventory is incomplete or contains an extra file.'
    }
    New-Item -ItemType Directory -Path (Join-Path $Destination 'certificates') -Force | Out-Null
    $destinationRecordPath = Join-Path $Destination $recordName
    Copy-Item -LiteralPath $recordPath -Destination $destinationRecordPath
    if ((Get-G04DCSha256 -Path $destinationRecordPath) -cne (Get-G04DCSha256 -Path $recordPath)) {
        throw '[ONLINE_PROVENANCE_RECORD_INVALID] Copied verifier record does not match the protected source snapshot.'
    }
    $copiedRecord = Read-G04DCCanonicalJson -Path $destinationRecordPath -Code 'ONLINE_PROVENANCE_RECORD_INVALID'
    Assert-G04DCOnlineRecord -Record $copiedRecord -ExpectedId $ExpectedId | Out-Null
    if ((Get-G04DCSemanticJson -Value $copiedRecord) -cne (Get-G04DCSemanticJson -Value $record)) {
        throw '[ONLINE_PROVENANCE_RECORD_INVALID] Copied verifier record changed semantically.'
    }
    foreach ($certificate in @(@($record.authenticode.signerChain) + @($record.authenticode.timestampChain))) {
        $relative = [string]$certificate.path
        $sourcePath = Join-Path $sourceRoot $relative.Replace('/', '\')
        $item = Get-Item -LiteralPath $sourcePath -Force
        if ($item.PSIsContainer -or [bool]($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
            (Get-G04DCSha256 -Path $sourcePath) -cne [string]$certificate.derSha256) {
            throw '[ONLINE_PROVENANCE_RECORD_INVALID] Verifier certificate file does not match its canonical record.'
        }
        Copy-Item -LiteralPath $sourcePath -Destination (Join-Path $Destination $relative.Replace('/', '\'))
    }
    New-G04DCCanonicalArtifactManifest -EvidenceDirectory $Destination | Out-Null
    Assert-G04DCCanonicalArtifactManifest -EvidenceDirectory $Destination | Out-Null
    Assert-G04DCExpectedArtifactManifest -EvidenceDirectory $sourceRoot -ExpectedManifestSha256 $ExpectedManifestSha256 | Out-Null
}

function Set-G04DCOwnerOnlyAcl {
    param([Parameter(Mandatory = $true)] [string]$Path)
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $security = [Security.AccessControl.DirectorySecurity]::new()
    $security.SetAccessRuleProtection($true, $false)
    $rule = [Security.AccessControl.FileSystemAccessRule]::new(
        $identity.User,
        [Security.AccessControl.FileSystemRights]::FullControl,
        [Security.AccessControl.InheritanceFlags]'ContainerInherit, ObjectInherit',
        [Security.AccessControl.PropagationFlags]::None,
        [Security.AccessControl.AccessControlType]::Allow
    )
    $security.AddAccessRule($rule)
    [IO.Directory]::SetAccessControl($Path, $security)
    $observed = [IO.Directory]::GetAccessControl($Path, [Security.AccessControl.AccessControlSections]::Access)
    $rules = @($observed.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]))
    if (!$observed.AreAccessRulesProtected -or $rules.Count -ne 1 -or $rules[0].IdentityReference.Value -cne $identity.User.Value -or
        ($rules[0].FileSystemRights -band [Security.AccessControl.FileSystemRights]::FullControl) -ne [Security.AccessControl.FileSystemRights]::FullControl) {
        throw '[ATTESTATION_KEY_ACL_INVALID] Ephemeral key directory is not owner-only.'
    }
}

function Remove-G04DCOwnedSourceSnapshot {
    param(
        [Parameter(Mandatory = $true)] [string]$Path,
        [Parameter(Mandatory = $true)] [string]$OutputRoot
    )
    $candidate = [IO.Path]::GetFullPath($Path).TrimEnd('\')
    $outputCanonical = [IO.Path]::GetFullPath($OutputRoot).TrimEnd('\')
    $leaf = [IO.Path]::GetFileName($candidate)
    if ([IO.Path]::GetDirectoryName($candidate) -cne $outputCanonical -or
        $leaf -notmatch '^\.source-snapshots-[0-9a-f-]{36}$' -or
        !(Test-Path -LiteralPath (Join-Path $candidate 'MARKER.md') -PathType Leaf)) {
        throw '[ONLINE_PROVENANCE_SNAPSHOT_CLEANUP_INVALID] Refusing to remove an unowned source snapshot.'
    }
    Remove-Item -LiteralPath $candidate -Recurse -Force
}

$keyParent = [IO.Path]::GetFullPath('C:\DocumentStudioLab\G04D-C12L\credentials').TrimEnd('\')
$keyCanonical = [IO.Path]::GetFullPath($KeyDirectory).TrimEnd('\')
if ($keyCanonical -cne $keyParent -and !$keyCanonical.StartsWith($keyParent + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw '[ATTESTATION_KEY_PATH_INVALID] Private key directory must be beneath the retained lab credentials root.'
}
if (Test-Path -LiteralPath $OutputDirectory) { throw '[ATTESTATION_PACKAGE_INVALID] Refusing to overwrite provenance bundle.' }
if (Test-Path -LiteralPath $keyCanonical) { throw '[ATTESTATION_KEY_PATH_INVALID] Refusing to reuse an existing proof key directory.' }

$sourceARoot = (Resolve-Path -LiteralPath $VerifierAEvidenceDirectory).Path
$sourceBRoot = (Resolve-Path -LiteralPath $VerifierBEvidenceDirectory).Path
$expectedAManifestSha256 = $ExpectedVerifierAManifestSha256.ToLowerInvariant()
$expectedBManifestSha256 = $ExpectedVerifierBManifestSha256.ToLowerInvariant()
if ($expectedAManifestSha256 -ceq $expectedBManifestSha256) {
    throw '[ONLINE_PROVENANCE_INDEPENDENCE_INVALID] Verifier source manifest bindings must be distinct.'
}
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
$output = (Resolve-Path -LiteralPath $OutputDirectory).Path
Set-G04DCOwnerOnlyAcl -Path $output
$snapshotRoot = Join-Path $output ('.source-snapshots-' + [guid]::NewGuid().ToString('D'))
New-Item -ItemType Directory -Path $snapshotRoot | Out-Null
Set-G04DCOwnerOnlyAcl -Path $snapshotRoot
[IO.File]::WriteAllText((Join-Path $snapshotRoot 'MARKER.md'), "G04D-C13 marker-owned verifier snapshots`r`n", [Text.UTF8Encoding]::new($false))
try {
$aRoot = Copy-G04DCExpectedArtifactSnapshot -EvidenceDirectory $sourceARoot -ExpectedManifestSha256 $expectedAManifestSha256 -DestinationDirectory (Join-Path $snapshotRoot 'A')
$bRoot = Copy-G04DCExpectedArtifactSnapshot -EvidenceDirectory $sourceBRoot -ExpectedManifestSha256 $expectedBManifestSha256 -DestinationDirectory (Join-Path $snapshotRoot 'B')
Assert-G04DCExpectedArtifactManifest -EvidenceDirectory $aRoot -ExpectedManifestSha256 $expectedAManifestSha256 | Out-Null
Assert-G04DCExpectedArtifactManifest -EvidenceDirectory $bRoot -ExpectedManifestSha256 $expectedBManifestSha256 | Out-Null
$aPath = Join-Path $aRoot 'online-verifier-A.json'
$bPath = Join-Path $bRoot 'online-verifier-B.json'
if (!(Test-Path -LiteralPath $aPath -PathType Leaf) -or !(Test-Path -LiteralPath $bPath -PathType Leaf)) { throw '[ONLINE_PROVENANCE_RECORD_INVALID] Both verifier records are required.' }
$a = Read-G04DCCanonicalJson -Path $aPath -Code 'ONLINE_PROVENANCE_RECORD_INVALID'
$b = Read-G04DCCanonicalJson -Path $bPath -Code 'ONLINE_PROVENANCE_RECORD_INVALID'
Assert-G04DCOnlineRecord -Record $a -ExpectedId 'A' | Out-Null
Assert-G04DCOnlineRecord -Record $b -ExpectedId 'B' | Out-Null
Assert-G04DCOnlineProvenanceRecordModel -Record $a -ExpectedId 'A' -ExpectedMsi (Get-G04DCExpectedMsi) | Out-Null
Assert-G04DCOnlineProvenanceRecordModel -Record $b -ExpectedId 'B' -ExpectedMsi (Get-G04DCExpectedMsi) | Out-Null

if ([string]$a.verifier.instanceId -ceq [string]$b.verifier.instanceId -or [string]$a.verifier.vmUuid -ceq [string]$b.verifier.vmUuid -or
    [string]$a.verifier.diskUuid -ceq [string]$b.verifier.diskUuid) {
    throw '[ONLINE_PROVENANCE_INDEPENDENCE_INVALID] Verifiers must use distinct instance, VM, and differencing-disk identities.'
}
$timeA = [DateTime]::Parse([string]$a.verifier.verifiedAtUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AdjustToUniversal).ToUniversalTime()
$timeB = [DateTime]::Parse([string]$b.verifier.verifiedAtUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AdjustToUniversal).ToUniversalTime()
if ([Math]::Abs(($timeA - $timeB).TotalMinutes) -gt 30) { throw '[ONLINE_PROVENANCE_TIME_WINDOW_INVALID] Verifier records differ by more than 30 minutes.' }

$agreementFields = [ordered]@{
    subject = (Get-G04DCSemanticJson -Value $a.subject) -ceq (Get-G04DCSemanticJson -Value $b.subject)
    signatureDigestAlgorithm = [string]$a.authenticode.signatureDigestAlgorithm -ceq [string]$b.authenticode.signatureDigestAlgorithm
    timestampType = [string]$a.authenticode.timestampType -ceq [string]$b.authenticode.timestampType
    timestampUtc = [string]$a.authenticode.timestampUtc -ceq [string]$b.authenticode.timestampUtc
    signerLeafDerSha256 = [string]$a.authenticode.signerLeafDerSha256 -ceq [string]$b.authenticode.signerLeafDerSha256
    signerLeafThumbprint = [string]$a.authenticode.signerLeafThumbprint -ceq [string]$b.authenticode.signerLeafThumbprint
    timestampLeafDerSha256 = [string]$a.authenticode.timestampLeafDerSha256 -ceq [string]$b.authenticode.timestampLeafDerSha256
    timestampLeafThumbprint = [string]$a.authenticode.timestampLeafThumbprint -ceq [string]$b.authenticode.timestampLeafThumbprint
    signerChain = (Get-G04DCSemanticJson -Value @($a.authenticode.signerChain | Select-Object -Property * -ExcludeProperty path)) -ceq (Get-G04DCSemanticJson -Value @($b.authenticode.signerChain | Select-Object -Property * -ExcludeProperty path))
    timestampChain = (Get-G04DCSemanticJson -Value @($a.authenticode.timestampChain | Select-Object -Property * -ExcludeProperty path)) -ceq (Get-G04DCSemanticJson -Value @($b.authenticode.timestampChain | Select-Object -Property * -ExcludeProperty path))
    revocation = (Get-G04DCSemanticJson -Value $a.authenticode.revocation) -ceq (Get-G04DCSemanticJson -Value $b.authenticode.revocation)
}
$disagreement = @($agreementFields.GetEnumerator() | Where-Object { !$_.Value } | ForEach-Object { $_.Key })
if ($disagreement.Count -ne 0) { throw "[ONLINE_PROVENANCE_VERIFIERS_DISAGREE] Semantic verifier disagreement: $($disagreement -join ', ')" }
$sharedAgreement = Compare-G04DCOnlineProvenanceRecordModels -VerifierA $a -VerifierB $b

Copy-G04DCSealedVerifierEvidence -Source $aRoot -Destination (Join-Path $output 'verifiers\A') -ExpectedId 'A' -ExpectedManifestSha256 $expectedAManifestSha256
Copy-G04DCSealedVerifierEvidence -Source $bRoot -Destination (Join-Path $output 'verifiers\B') -ExpectedId 'B' -ExpectedManifestSha256 $expectedBManifestSha256
Assert-G04DCExpectedArtifactManifest -EvidenceDirectory $aRoot -ExpectedManifestSha256 $expectedAManifestSha256 | Out-Null
Assert-G04DCExpectedArtifactManifest -EvidenceDirectory $bRoot -ExpectedManifestSha256 $expectedBManifestSha256 | Out-Null
$copiedAPath = Join-Path $output 'verifiers\A\online-verifier-A.json'
$copiedBPath = Join-Path $output 'verifiers\B\online-verifier-B.json'
if ((Get-G04DCSha256 -Path $copiedAPath) -cne (Get-G04DCSha256 -Path $aPath) -or
    (Get-G04DCSha256 -Path $copiedBPath) -cne (Get-G04DCSha256 -Path $bPath)) {
    throw '[ONLINE_PROVENANCE_RECORD_INVALID] Final verifier records do not match their protected source snapshots.'
}
Remove-G04DCOwnedSourceSnapshot -Path $snapshotRoot -OutputRoot $output
$snapshotRoot = $null

New-Item -ItemType Directory -Path $keyCanonical | Out-Null
Set-G04DCOwnerOnlyAcl -Path $keyCanonical
$privateKeyPath = Join-Path $keyCanonical 'attestation-private-key.cspblob'
$rsa = [Security.Cryptography.RSACryptoServiceProvider]::new(3072)
$rsa.PersistKeyInCsp = $false
$privateBlob = $null
try {
    $privateBlob = $rsa.ExportCspBlob($true)
    $publicBlob = $rsa.ExportCspBlob($false)
    $privateStream = [IO.File]::Open($privateKeyPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $privateStream.Write($privateBlob, 0, $privateBlob.Length); $privateStream.Flush($true) }
    finally { $privateStream.Dispose() }
    [Array]::Clear($privateBlob, 0, $privateBlob.Length)
    $privateBlob = $null
    $publicKeyPath = Join-Path $output 'attestation-public-key.cspblob'
    [IO.File]::WriteAllBytes($publicKeyPath, $publicBlob)
    $publicKeySha256 = Get-G04DCSha256 -Path $publicKeyPath
    $payloadFiles = @(Get-ChildItem -LiteralPath $output -File -Recurse -Force | Where-Object {
        $_.FullName -eq $publicKeyPath -or $_.FullName.StartsWith((Join-Path $output 'verifiers') + '\', [StringComparison]::OrdinalIgnoreCase)
    } | Sort-Object FullName | ForEach-Object {
        [pscustomobject][ordered]@{
            path = $_.FullName.Substring($output.Length + 1).Replace('\', '/')
            sizeBytes = [long]$_.Length
            sha256 = Get-G04DCSha256 -Path $_.FullName
        }
    })
    $payloadManifestPath = Join-Path $output 'signed-payload-manifest.json'
    Write-G04DCCanonicalJson -Path $payloadManifestPath -Value ([ordered]@{ schemaVersion = 1; files = $payloadFiles }) -Depth 10
    $payloadManifestSha256 = Get-G04DCSha256 -Path $payloadManifestPath
    $earliest = if ($timeA -le $timeB) { $timeA } else { $timeB }
    $latest = if ($timeA -ge $timeB) { $timeA } else { $timeB }
    $created = [DateTime]::UtcNow
    $expires = $earliest.AddHours($AttestationLifetimeHours)
    if ($created -gt $expires) { throw '[ATTESTATION_EXPIRED] Online verifier evidence expired before attestation creation.' }
    Assert-G04DCProvenanceFreshnessModel -NowUtc $created -CreatedUtc $created -ExpiresUtc $expires -VerifiedEarliestUtc $earliest -VerifiedLatestUtc $latest -MaximumLifetimeHours $AttestationLifetimeHours | Out-Null
    $attestation = [pscustomobject][ordered]@{
        schemaVersion = 1
        attestationType = 'g04d-c13-offline-authenticode-provenance'
        subject = $a.subject
        onlineVerification = [ordered]@{
            verifierARecordPath = 'verifiers/A/online-verifier-A.json'
            verifierARecordSha256 = Get-G04DCSha256 -Path $copiedAPath
            verifierASourceManifestSha256 = $expectedAManifestSha256
            verifierBRecordPath = 'verifiers/B/online-verifier-B.json'
            verifierBRecordSha256 = Get-G04DCSha256 -Path $copiedBPath
            verifierBSourceManifestSha256 = $expectedBManifestSha256
            verifiedAtEarliestUtc = $earliest.ToString('o')
            verifiedAtLatestUtc = $latest.ToString('o')
            verifierMaximumSeparationMinutes = 30
            signerChain = $a.authenticode.signerChain
            timestampChain = $a.authenticode.timestampChain
            signatureDigestAlgorithm = [string]$a.authenticode.signatureDigestAlgorithm
            timestampType = [string]$a.authenticode.timestampType
            timestampUtc = [string]$a.authenticode.timestampUtc
            revocation = [ordered]@{
                signerChainExcludeRoot = 'good'
                timestampChainExcludeRoot = 'good'
                evidenceMode = 'two-independent-online-verifier-results'
                rawCrlOrOcspObjectsRetained = $false
            }
        }
        freshness = [ordered]@{
            createdAtUtc = $created.ToString('o')
            expiresAtUtc = $expires.ToString('o')
            maximumLifetimeHours = $AttestationLifetimeHours
            revocationNextUpdateBoundaryAvailable = $false
        }
        proofPolicy = [ordered]@{
            minimumOnlineVerifierCount = 2
            offlineCloneNetworkRequired = $false
            offlineUrlRetrievalPermitted = $false
            globalCertificateStoreMutationPermitted = $false
            partialChainAcceptedAsTrust = $false
            offlineRevocationAcceptedAsTrust = $false
        }
        integrity = [ordered]@{
            detachedSignatureAlgorithm = 'RSASSA-PKCS1-v1_5-SHA256'
            rsaKeySizeBits = 3072
            privateKeyCopiedToGuest = $false
            publicKeyPath = 'attestation-public-key.cspblob'
            publicKeySha256 = $publicKeySha256
            signedPayloadManifestPath = 'signed-payload-manifest.json'
            signedPayloadManifestSha256 = $payloadManifestSha256
            purpose = 'host transport and integrity binding only; not vendor provenance'
        }
        agreement = [ordered]@{ accepted = $true; fields = $sharedAgreement }
    }
    $attestationPath = Join-Path $output 'combined-provenance-attestation.json'
    Write-G04DCCanonicalJson -Path $attestationPath -Value $attestation -Depth 30
    $attestationBytes = [IO.File]::ReadAllBytes($attestationPath)
    $signatureBytes = $rsa.SignData($attestationBytes, 'SHA256')
    [IO.File]::WriteAllBytes((Join-Path $output 'combined-provenance-attestation.sig'), $signatureBytes)
    New-G04DCCanonicalArtifactManifest -EvidenceDirectory $output | Out-Null
    Assert-G04DCCanonicalArtifactManifest -EvidenceDirectory $output | Out-Null
    Write-Output $attestationPath
}
finally {
    if ($null -ne $privateBlob) { [Array]::Clear($privateBlob, 0, $privateBlob.Length) }
    $rsa.Clear()
    $rsa.Dispose()
}
}
finally {
    if ($null -ne $snapshotRoot -and (Test-Path -LiteralPath $snapshotRoot)) {
        Remove-G04DCOwnedSourceSnapshot -Path $snapshotRoot -OutputRoot $output
    }
}
