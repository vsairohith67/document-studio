Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-G04DCProvenanceExactProperties {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] $Value, [Parameter(Mandatory = $true)] [string[]]$Names, [string]$Code = 'ONLINE_PROVENANCE_RECORD_INVALID')
    if ($null -eq $Value) { throw "[$Code] Required object is missing." }
    $actual = @($Value.PSObject.Properties | ForEach-Object { $_.Name } | Sort-Object)
    $expected = @($Names | Sort-Object)
    if (($actual -join "`n") -cne ($expected -join "`n")) { throw "[$Code] Object fields are missing, duplicated, or unexpected." }
    return $true
}

function Write-G04DCCanonicalJson {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [string]$Path,
        [Parameter(Mandatory = $true)] [AllowNull()] $Value,
        [ValidateRange(2, 50)] [int]$Depth = 30
    )
    $parent = Split-Path -Parent $Path
    if ($parent -and !(Test-Path -LiteralPath $parent)) { New-Item -ItemType Directory -Path $parent | Out-Null }
    $json = ($Value | ConvertTo-Json -Depth $Depth -Compress) + "`n"
    [IO.File]::WriteAllText($Path, $json, [Text.UTF8Encoding]::new($false, $true))
}

function Read-G04DCCanonicalJson {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [string]$Path,
        [ValidateRange(1, 1048576)] [int]$MaximumBytes = 1048576,
        [ValidateRange(2, 50)] [int]$Depth = 30,
        [string]$Code = 'ATTESTATION_SCHEMA_INVALID'
    )
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or [bool]($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.Length -lt 3 -or $item.Length -gt $MaximumBytes) {
        throw "[$Code] Canonical JSON file is missing, unsafe, empty, or oversized."
    }
    $bytes = [IO.File]::ReadAllBytes($item.FullName)
    if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) { throw "[$Code] Canonical JSON must be BOM-free UTF-8." }
    try { $text = [Text.UTF8Encoding]::new($false, $true).GetString($bytes) }
    catch { throw "[$Code] Canonical JSON is not valid UTF-8." }
    try { $value = $text | ConvertFrom-Json -ErrorAction Stop }
    catch { throw "[$Code] Canonical JSON is invalid." }
    $expected = ($value | ConvertTo-Json -Depth $Depth -Compress) + "`n"
    if ($text -cne $expected) { throw "[$Code] JSON is non-canonical or contains duplicate fields." }
    return $value
}

function New-G04DCCanonicalArtifactManifest {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] [string]$EvidenceDirectory)
    $rootItem = Get-Item -LiteralPath $EvidenceDirectory -Force
    if (!$rootItem.PSIsContainer -or [bool]($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw '[ARTIFACT_MANIFEST_INVALID] Evidence root is unsafe.' }
    $root = $rootItem.FullName.TrimEnd('\')
    $rootManifestPath = Join-Path $root 'artifact-manifest.json'
    $files = @(Get-ChildItem -LiteralPath $root -File -Recurse -Force | Where-Object { $_.FullName -cne $rootManifestPath } | Sort-Object FullName | ForEach-Object {
        [pscustomobject][ordered]@{
            path = $_.FullName.Substring($root.Length + 1).Replace('\', '/')
            sizeBytes = [long]$_.Length
            sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        }
    })
    if ($files.Count -eq 0) { throw '[ARTIFACT_MANIFEST_INVALID] Evidence root contains no files.' }
    $manifest = [pscustomobject][ordered]@{ schemaVersion = 1; files = $files }
    Write-G04DCCanonicalJson -Path (Join-Path $root 'artifact-manifest.json') -Value $manifest -Depth 10
    return $manifest
}

function Assert-G04DCCanonicalArtifactManifest {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] [string]$EvidenceDirectory)
    $manifest = Assert-G04DCArtifactManifest -EvidenceDirectory $EvidenceDirectory
    $canonical = Read-G04DCCanonicalJson -Path (Join-Path $EvidenceDirectory 'artifact-manifest.json') -MaximumBytes 1048576 -Depth 10 -Code 'ARTIFACT_MANIFEST_INVALID'
    Assert-G04DCProvenanceExactProperties -Value $canonical -Names @('schemaVersion', 'files') -Code 'ARTIFACT_MANIFEST_INVALID' | Out-Null
    foreach ($file in @($canonical.files)) {
        Assert-G04DCProvenanceExactProperties -Value $file -Names @('path', 'sizeBytes', 'sha256') -Code 'ARTIFACT_MANIFEST_INVALID' | Out-Null
    }
    return $manifest
}

function Test-G04DCCanonicalGuid {
    param([Parameter(Mandatory = $true)] [string]$Value)
    [guid]$parsed = [guid]::Empty
    return [guid]::TryParseExact($Value, 'D', [ref]$parsed) -and $parsed -ne [guid]::Empty -and $Value -ceq $Value.ToLowerInvariant()
}

function Assert-G04DCCertificateChainRecord {
    param(
        [Parameter(Mandatory = $true)] [object[]]$Certificates,
        [Parameter(Mandatory = $true)] [ValidateSet('signer', 'timestamp')] [string]$Purpose,
        [Parameter(Mandatory = $true)] [DateTime]$VerificationTimeUtc
    )
    if ($Certificates.Count -lt 2 -or $Certificates.Count -gt 16) { throw '[ONLINE_PROVENANCE_RECORD_INVALID] Certificate chain length is invalid.' }
    $acceptedSignatureAlgorithms = @(
        '1.2.840.113549.1.1.11', '1.2.840.113549.1.1.12', '1.2.840.113549.1.1.13',
        '1.2.840.10045.4.3.2', '1.2.840.10045.4.3.3', '1.2.840.10045.4.3.4'
    )
    for ($position = 0; $position -lt $Certificates.Count; $position++) {
        $certificate = $Certificates[$position]
        Assert-G04DCProvenanceExactProperties -Value $certificate -Names @(
            'chainPosition', 'path', 'derSha256', 'thumbprint', 'serialNumber', 'subjectNameSha256', 'issuerNameSha256',
            'publicKeyAlgorithmOid', 'publicKeySizeBits', 'signatureAlgorithmOid', 'ekuOids', 'notBeforeUtc', 'notAfterUtc', 'selfSignedName'
        ) | Out-Null
        [DateTime]$notBefore = [DateTime]::MinValue
        [DateTime]$notAfter = [DateTime]::MinValue
        $expectedPath = 'certificates/{0}-{1:D2}-{2}.cer' -f $Purpose, $position, [string]$certificate.derSha256
        $isRsa = [string]$certificate.publicKeyAlgorithmOid -ceq '1.2.840.113549.1.1.1'
        $isEc = [string]$certificate.publicKeyAlgorithmOid -ceq '1.2.840.10045.2.1'
        $valid = [int]$certificate.chainPosition -eq $position -and [string]$certificate.path -ceq $expectedPath -and
            [string]$certificate.derSha256 -match '^[0-9a-f]{64}$' -and [string]$certificate.thumbprint -match '^[0-9A-F]{40}$' -and
            [string]$certificate.serialNumber -match '^[0-9A-F]+$' -and [string]$certificate.subjectNameSha256 -match '^[0-9a-f]{64}$' -and
            [string]$certificate.issuerNameSha256 -match '^[0-9a-f]{64}$' -and [string]$certificate.signatureAlgorithmOid -in $acceptedSignatureAlgorithms -and
            (($isRsa -and [int]$certificate.publicKeySizeBits -ge 2048) -or ($isEc -and [int]$certificate.publicKeySizeBits -ge 256)) -and
            @($certificate.ekuOids | Where-Object { [string]$_ -notmatch '^[0-9]+(?:\.[0-9]+)+$' }).Count -eq 0 -and
            [DateTime]::TryParse([string]$certificate.notBeforeUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AdjustToUniversal, [ref]$notBefore) -and
            [DateTime]::TryParse([string]$certificate.notAfterUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AdjustToUniversal, [ref]$notAfter) -and
            $VerificationTimeUtc -ge $notBefore.ToUniversalTime() -and $VerificationTimeUtc -le $notAfter.ToUniversalTime()
        if (!$valid) { throw '[ONLINE_PROVENANCE_RECORD_INVALID] Certificate chain metadata or policy is invalid.' }
        if ($position -lt $Certificates.Count - 1 -and [string]$certificate.issuerNameSha256 -cne [string]$Certificates[$position + 1].subjectNameSha256) {
            throw '[ONLINE_PROVENANCE_RECORD_INVALID] Certificate issuer and subject linkage is invalid.'
        }
    }
    $root = $Certificates[-1]
    if (![bool]$root.selfSignedName -or [string]$root.subjectNameSha256 -cne [string]$root.issuerNameSha256) {
        throw '[ONLINE_PROVENANCE_RECORD_INVALID] Certificate chain root identity is invalid.'
    }
    $requiredEku = if ($Purpose -ceq 'signer') { '1.3.6.1.5.5.7.3.3' } else { '1.3.6.1.5.5.7.3.8' }
    if (@($Certificates[0].ekuOids) -notcontains $requiredEku) { throw '[ONLINE_PROVENANCE_RECORD_INVALID] Certificate leaf EKU is invalid.' }
    return $true
}

function Assert-G04DCOfflineEmbeddedIdentityModel {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] $FileTrust,
        [Parameter(Mandatory = $true)] $OnlineRecord
    )
    if (![bool]$FileTrust.passed -or ![bool]$FileTrust.hashOnly -or ![bool]$FileTrust.cacheOnlyUrlRetrieval -or
        [int]$FileTrust.signerCount -ne 1 -or [int]$FileTrust.timestampSignerCount -ne 1 -or
        [int]$FileTrust.providerSignerError -ne 0 -or [int]$FileTrust.providerTimestampError -ne 0 -or
        [string]$FileTrust.signerLeafDerSha256 -cne [string]$OnlineRecord.authenticode.signerLeafDerSha256 -or
        [string]$FileTrust.timestampLeafDerSha256 -cne [string]$OnlineRecord.authenticode.timestampLeafDerSha256 -or
        [string]$FileTrust.timestampUtc -cne [string]$OnlineRecord.authenticode.timestampUtc) {
        throw '[OFFLINE_PROVENANCE_SIGNATURE_INVALID] Embedded Authenticode digest, signer, timestamp, or signer cardinality failed.'
    }
    return $true
}

function Assert-G04DCOnlineProvenanceRecordModel {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] $Record,
        [Parameter(Mandatory = $true)] [ValidateSet('A', 'B')] [string]$ExpectedId,
        [Parameter(Mandatory = $true)] $ExpectedMsi
    )
    Assert-G04DCProvenanceExactProperties -Value $Record -Names @('schemaVersion', 'recordType', 'verifier', 'subject', 'authenticode', 'verification', 'tooling', 'privacy') | Out-Null
    Assert-G04DCProvenanceExactProperties -Value $Record.verifier -Names @('id', 'instanceId', 'vmUuid', 'diskUuid', 'verifiedAtUtc', 'osCaptionSha256', 'osVersion', 'osBuild', 'osArchitecture') | Out-Null
    Assert-G04DCProvenanceExactProperties -Value $Record.subject -Names @('filename', 'sizeBytes', 'sha256', 'version', 'architecture', 'productCode', 'upgradeCode', 'packageCode') | Out-Null
    Assert-G04DCProvenanceExactProperties -Value $Record.authenticode -Names @('signatureDigestAlgorithm', 'signatureDigestAlgorithmOid', 'timestampType', 'timestampUtc', 'signerLeafDerSha256', 'signerLeafThumbprint', 'timestampLeafDerSha256', 'timestampLeafThumbprint', 'signerChain', 'timestampChain', 'revocation') | Out-Null
    Assert-G04DCProvenanceExactProperties -Value $Record.authenticode.revocation -Names @('signerChainExcludeRoot', 'timestampChainExcludeRoot', 'evidenceMode') | Out-Null
    Assert-G04DCProvenanceExactProperties -Value $Record.verification -Names @('accepted', 'signTool', 'winVerifyTrust', 'signerChain', 'timestampChain', 'urlRetrievalTimeoutMilliseconds', 'identityChecks') | Out-Null
    Assert-G04DCProvenanceExactProperties -Value $Record.verification.signTool -Names @('accepted', 'exitCode', 'targetOs', 'targetOsEnforcement', 'allEmbeddedSignatures', 'timestampRequired', 'warningCount') | Out-Null
    Assert-G04DCProvenanceExactProperties -Value $Record.verification.winVerifyTrust -Names @('accepted', 'statusHex', 'revocationChecks', 'cacheOnly') | Out-Null
    foreach ($chainName in @('signerChain', 'timestampChain')) {
        Assert-G04DCProvenanceExactProperties -Value $Record.verification.$chainName -Names @('accepted', 'errorStatusHex', 'errorStatusNames', 'policyErrorHex', 'policy', 'certificateSignaturesValid', 'revocationKnownGood') | Out-Null
    }
    Assert-G04DCProvenanceExactProperties -Value $Record.verification.identityChecks -Names @(
        'regularFile', 'nonReparse', 'sizeBytes', 'sha256', 'authenticode', 'signerThumbprint', 'timestampThumbprint',
        'signerLeafDer', 'timestampLeafDer', 'version', 'architecture', 'productCode', 'upgradeCode', 'packageCode'
    ) | Out-Null
    Assert-G04DCProvenanceExactProperties -Value $Record.tooling -Names @('signTool', 'provenanceHelper') | Out-Null
    Assert-G04DCProvenanceExactProperties -Value $Record.tooling.signTool -Names @('pathCategory', 'sizeBytes', 'sha256', 'fileVersion', 'productVersion', 'signerLeafThumbprint', 'signerLeafDerSha256') | Out-Null
    Assert-G04DCProvenanceExactProperties -Value $Record.tooling.provenanceHelper -Names @('sourceSha256', 'runtime') | Out-Null
    Assert-G04DCProvenanceExactProperties -Value $Record.privacy -Names @('canonicalRecordContainsRawConsoleOutput', 'canonicalRecordContainsCertificateSubjectText', 'canonicalRecordContainsUsernameOrProfilePath', 'canonicalRecordContainsPrivateKeyMaterial') | Out-Null
    [DateTime]$verifiedAt = [DateTime]::MinValue
    [DateTime]$timestampAt = [DateTime]::MinValue
    if (![DateTime]::TryParse([string]$Record.verifier.verifiedAtUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AdjustToUniversal, [ref]$verifiedAt) -or
        ![DateTime]::TryParse([string]$Record.authenticode.timestampUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AdjustToUniversal, [ref]$timestampAt)) {
        throw '[ONLINE_PROVENANCE_RECORD_INVALID] Verifier or embedded timestamp time is invalid.'
    }
    $timestampAt = $timestampAt.ToUniversalTime()
    Assert-G04DCCertificateChainRecord -Certificates @($Record.authenticode.signerChain) -Purpose signer -VerificationTimeUtc $timestampAt | Out-Null
    Assert-G04DCCertificateChainRecord -Certificates @($Record.authenticode.timestampChain) -Purpose timestamp -VerificationTimeUtc $timestampAt | Out-Null
    $allIdentityChecks = @($Record.verification.identityChecks.PSObject.Properties | Where-Object { ![bool]$_.Value }).Count -eq 0
    $valid = [int]$Record.schemaVersion -eq 1 -and [string]$Record.recordType -ceq 'g04d-c13-online-authenticode-verifier' -and
        [string]$Record.verifier.id -ceq $ExpectedId -and (Test-G04DCCanonicalGuid -Value ([string]$Record.verifier.instanceId)) -and
        (Test-G04DCCanonicalGuid -Value ([string]$Record.verifier.vmUuid)) -and (Test-G04DCCanonicalGuid -Value ([string]$Record.verifier.diskUuid)) -and
        [string]$Record.verifier.osCaptionSha256 -match '^[0-9a-f]{64}$' -and [string]$Record.verifier.osVersion -match '^10\.0\.26100(?:\.|$)' -and
        [string]$Record.verifier.osBuild -ceq '26100' -and [string]$Record.verifier.osArchitecture -match '64' -and
        [string]$Record.subject.filename -ceq [string]$ExpectedMsi.FileName -and [long]$Record.subject.sizeBytes -eq [long]$ExpectedMsi.SizeBytes -and
        [string]$Record.subject.sha256 -ceq [string]$ExpectedMsi.Sha256 -and [string]$Record.subject.version -ceq [string]$ExpectedMsi.ProductVersion -and
        [string]$Record.subject.architecture -ceq [string]$ExpectedMsi.Architecture -and [string]$Record.subject.productCode -ceq [string]$ExpectedMsi.ProductCode -and
        [string]$Record.subject.upgradeCode -ceq [string]$ExpectedMsi.UpgradeCode -and [string]$Record.subject.packageCode -ceq [string]$ExpectedMsi.PackageCode -and
        [string]$Record.authenticode.signerLeafThumbprint -ceq [string]$ExpectedMsi.SignerThumbprint -and
        [string]$Record.authenticode.timestampLeafThumbprint -ceq [string]$ExpectedMsi.TimestampSignerThumbprint -and
        [string]$Record.authenticode.signatureDigestAlgorithm -ceq 'sha256' -and [string]$Record.authenticode.signatureDigestAlgorithmOid -ceq '2.16.840.1.101.3.4.2.1' -and
        [string]$Record.authenticode.timestampType -ceq 'embedded-authenticode-timestamp' -and
        [string]$Record.authenticode.revocation.signerChainExcludeRoot -ceq 'good' -and [string]$Record.authenticode.revocation.timestampChainExcludeRoot -ceq 'good' -and
        [string]$Record.authenticode.revocation.evidenceMode -ceq 'fresh-windows-online-chain-results' -and
        [string]$Record.authenticode.signerLeafDerSha256 -ceq [string]$Record.authenticode.signerChain[0].derSha256 -and
        [string]$Record.authenticode.signerLeafThumbprint -ceq [string]$Record.authenticode.signerChain[0].thumbprint -and
        [string]$Record.authenticode.timestampLeafDerSha256 -ceq [string]$Record.authenticode.timestampChain[0].derSha256 -and
        [string]$Record.authenticode.timestampLeafThumbprint -ceq [string]$Record.authenticode.timestampChain[0].thumbprint -and
        [bool]$Record.verification.accepted -and [bool]$Record.verification.signTool.accepted -and [int]$Record.verification.signTool.exitCode -eq 0 -and
        [string]$Record.verification.signTool.targetOs -ceq '2:10.0.26100.0' -and [string]$Record.verification.signTool.targetOsEnforcement -ceq 'exact-verifier-host-build' -and
        [bool]$Record.verification.signTool.allEmbeddedSignatures -and
        [bool]$Record.verification.signTool.timestampRequired -and [int]$Record.verification.signTool.warningCount -eq 0 -and
        [bool]$Record.verification.winVerifyTrust.accepted -and [string]$Record.verification.winVerifyTrust.statusHex -ceq '0x00000000' -and
        [string]$Record.verification.winVerifyTrust.revocationChecks -ceq 'whole-chain-excluding-root' -and ![bool]$Record.verification.winVerifyTrust.cacheOnly -and
        [bool]$Record.verification.signerChain.accepted -and [bool]$Record.verification.timestampChain.accepted -and
        [bool]$Record.verification.signerChain.certificateSignaturesValid -and [bool]$Record.verification.timestampChain.certificateSignaturesValid -and
        [bool]$Record.verification.signerChain.revocationKnownGood -and [bool]$Record.verification.timestampChain.revocationKnownGood -and
        [string]$Record.verification.signerChain.errorStatusHex -ceq '0x00000000' -and [string]$Record.verification.timestampChain.errorStatusHex -ceq '0x00000000' -and
        @($Record.verification.signerChain.errorStatusNames).Count -eq 0 -and @($Record.verification.timestampChain.errorStatusNames).Count -eq 0 -and
        [string]$Record.verification.signerChain.policyErrorHex -ceq '0x00000000' -and [string]$Record.verification.timestampChain.policyErrorHex -ceq '0x00000000' -and
        [string]$Record.verification.signerChain.policy -ceq 'AUTHENTICODE' -and [string]$Record.verification.timestampChain.policy -ceq 'AUTHENTICODE_TS' -and
        [int]$Record.verification.urlRetrievalTimeoutMilliseconds -ge 1000 -and [int]$Record.verification.urlRetrievalTimeoutMilliseconds -le 120000 -and
        $allIdentityChecks -and [string]$Record.tooling.signTool.pathCategory -ceq 'injected-windows-sdk-10.0.26100.0-x64' -and
        [long]$Record.tooling.signTool.sizeBytes -gt 0 -and [string]$Record.tooling.signTool.sha256 -match '^[0-9a-f]{64}$' -and
        ![string]::IsNullOrWhiteSpace([string]$Record.tooling.signTool.fileVersion) -and ![string]::IsNullOrWhiteSpace([string]$Record.tooling.signTool.productVersion) -and
        [string]$Record.tooling.signTool.signerLeafThumbprint -match '^[0-9A-F]{40}$' -and [string]$Record.tooling.signTool.signerLeafDerSha256 -match '^[0-9a-f]{64}$' -and
        [string]$Record.tooling.provenanceHelper.sourceSha256 -match '^[0-9a-f]{64}$' -and
        [string]$Record.tooling.provenanceHelper.runtime -ceq '.NET Framework and Windows cryptographic APIs' -and
        ![bool]$Record.privacy.canonicalRecordContainsRawConsoleOutput -and ![bool]$Record.privacy.canonicalRecordContainsCertificateSubjectText -and
        ![bool]$Record.privacy.canonicalRecordContainsUsernameOrProfilePath -and ![bool]$Record.privacy.canonicalRecordContainsPrivateKeyMaterial
    if (!$valid) { throw '[ONLINE_PROVENANCE_RECORD_INVALID] Online verifier record failed the frozen policy.' }
    return $true
}

function Compare-G04DCOnlineProvenanceRecordModels {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] $VerifierA, [Parameter(Mandatory = $true)] $VerifierB)
    if ([string]$VerifierA.verifier.instanceId -ceq [string]$VerifierB.verifier.instanceId -or [string]$VerifierA.verifier.vmUuid -ceq [string]$VerifierB.verifier.vmUuid -or
        [string]$VerifierA.verifier.diskUuid -ceq [string]$VerifierB.verifier.diskUuid) { throw '[ONLINE_PROVENANCE_INDEPENDENCE_INVALID] Verifiers are not independent.' }
    $timeA = [DateTime]::Parse([string]$VerifierA.verifier.verifiedAtUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AdjustToUniversal).ToUniversalTime()
    $timeB = [DateTime]::Parse([string]$VerifierB.verifier.verifiedAtUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AdjustToUniversal).ToUniversalTime()
    if ([Math]::Abs(($timeA - $timeB).TotalMinutes) -gt 30) { throw '[ONLINE_PROVENANCE_TIME_WINDOW_INVALID] Verifier times differ by more than 30 minutes.' }
    $fields = [ordered]@{
        subject = ($VerifierA.subject | ConvertTo-Json -Compress) -ceq ($VerifierB.subject | ConvertTo-Json -Compress)
        signatureDigestAlgorithm = [string]$VerifierA.authenticode.signatureDigestAlgorithm -ceq [string]$VerifierB.authenticode.signatureDigestAlgorithm -and [string]$VerifierA.authenticode.signatureDigestAlgorithmOid -ceq [string]$VerifierB.authenticode.signatureDigestAlgorithmOid
        timestampType = [string]$VerifierA.authenticode.timestampType -ceq [string]$VerifierB.authenticode.timestampType
        timestampUtc = [string]$VerifierA.authenticode.timestampUtc -ceq [string]$VerifierB.authenticode.timestampUtc
        signerLeaf = [string]$VerifierA.authenticode.signerLeafDerSha256 -ceq [string]$VerifierB.authenticode.signerLeafDerSha256 -and [string]$VerifierA.authenticode.signerLeafThumbprint -ceq [string]$VerifierB.authenticode.signerLeafThumbprint
        timestampLeaf = [string]$VerifierA.authenticode.timestampLeafDerSha256 -ceq [string]$VerifierB.authenticode.timestampLeafDerSha256 -and [string]$VerifierA.authenticode.timestampLeafThumbprint -ceq [string]$VerifierB.authenticode.timestampLeafThumbprint
        signerChain = (@($VerifierA.authenticode.signerChain | Select-Object -Property * -ExcludeProperty path) | ConvertTo-Json -Depth 10 -Compress) -ceq (@($VerifierB.authenticode.signerChain | Select-Object -Property * -ExcludeProperty path) | ConvertTo-Json -Depth 10 -Compress)
        timestampChain = (@($VerifierA.authenticode.timestampChain | Select-Object -Property * -ExcludeProperty path) | ConvertTo-Json -Depth 10 -Compress) -ceq (@($VerifierB.authenticode.timestampChain | Select-Object -Property * -ExcludeProperty path) | ConvertTo-Json -Depth 10 -Compress)
        roots = [string]$VerifierA.authenticode.signerChain[-1].derSha256 -ceq [string]$VerifierB.authenticode.signerChain[-1].derSha256 -and [string]$VerifierA.authenticode.timestampChain[-1].derSha256 -ceq [string]$VerifierB.authenticode.timestampChain[-1].derSha256
        revocation = ($VerifierA.authenticode.revocation | ConvertTo-Json -Compress) -ceq ($VerifierB.authenticode.revocation | ConvertTo-Json -Compress)
    }
    $failed = @($fields.GetEnumerator() | Where-Object { !$_.Value } | ForEach-Object { $_.Key })
    if ($failed.Count -ne 0) { throw "[ONLINE_PROVENANCE_VERIFIERS_DISAGREE] $($failed -join ', ')" }
    return [pscustomobject]$fields
}

function Assert-G04DCProvenanceFreshnessModel {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [DateTime]$NowUtc,
        [Parameter(Mandatory = $true)] [DateTime]$CreatedUtc,
        [Parameter(Mandatory = $true)] [DateTime]$ExpiresUtc,
        [Parameter(Mandatory = $true)] [DateTime]$VerifiedEarliestUtc,
        [Parameter(Mandatory = $true)] [DateTime]$VerifiedLatestUtc,
        [ValidateRange(1, 12)] [int]$MaximumLifetimeHours = 12,
        [ValidateRange(1, 5)] [int]$ClockSkewMinutes = 5
    )
    $now = $NowUtc.ToUniversalTime(); $created = $CreatedUtc.ToUniversalTime(); $expires = $ExpiresUtc.ToUniversalTime()
    $earliest = $VerifiedEarliestUtc.ToUniversalTime(); $latest = $VerifiedLatestUtc.ToUniversalTime()
    if ($created -gt $now.AddMinutes($ClockSkewMinutes) -or $earliest -gt $now.AddMinutes($ClockSkewMinutes) -or $latest -gt $now.AddMinutes($ClockSkewMinutes)) {
        throw '[ATTESTATION_FUTURE_DATED] Attestation is future-dated.'
    }
    if ($now -ge $expires -or $expires -le $created -or $expires -gt $earliest.AddHours($MaximumLifetimeHours)) { throw '[ATTESTATION_EXPIRED] Attestation freshness is invalid.' }
    if ([Math]::Abs(($latest - $earliest).TotalMinutes) -gt 30) { throw '[ONLINE_PROVENANCE_TIME_WINDOW_INVALID] Verifier times differ by more than 30 minutes.' }
    return $true
}

Export-ModuleMember -Function @(
    'Assert-G04DCProvenanceExactProperties', 'Assert-G04DCOnlineProvenanceRecordModel',
    'Compare-G04DCOnlineProvenanceRecordModels', 'Assert-G04DCProvenanceFreshnessModel',
    'Write-G04DCCanonicalJson', 'Read-G04DCCanonicalJson', 'Assert-G04DCOfflineEmbeddedIdentityModel',
    'New-G04DCCanonicalArtifactManifest', 'Assert-G04DCCanonicalArtifactManifest'
)
