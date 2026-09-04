[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)] [string]$BundleDirectory,
    [Parameter(Mandatory = $true)] [string]$MsiPath,
    [Parameter(Mandatory = $true)] [string]$EvidenceDirectory,
    [Parameter(Mandatory = $true)] [string]$TrustedHostUtc,
    [Parameter(Mandatory = $true)] [ValidatePattern('^[0-9a-f]{64}$')] [string]$ExpectedPublicKeySha256,
    [ValidateRange(1, 5)] [int]$MaximumClockSkewMinutes = 5
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

function Get-G04DCSha256Bytes {
    param([Parameter(Mandatory = $true)] [byte[]]$Bytes)
    $algorithm = [Security.Cryptography.SHA256]::Create()
    try { return (($algorithm.ComputeHash($Bytes) | ForEach-Object { $_.ToString('x2') }) -join '') }
    finally { $algorithm.Dispose() }
}

function Get-G04DCOfflineMsiIdentity {
    param([Parameter(Mandatory = $true)] [string]$Path)
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or [bool]($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw '[MSI_IDENTITY_MISMATCH] MSI is not a regular non-reparse file.' }
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $database = $installer.OpenDatabase($item.FullName, 0)
    try {
        $properties = @{}
        foreach ($name in @('ProductVersion', 'ProductCode', 'UpgradeCode')) {
            $view = $database.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='$name'")
            try { [void]$view.Execute(); $record = $view.Fetch(); $properties[$name] = if ($record) { [string]$record.StringData(1) } else { $null } }
            finally { [void]$view.Close() }
        }
        $summary = $database.SummaryInformation(0)
        try {
            $template = [string]$summary.Property(7)
            return [pscustomobject][ordered]@{
                filename = $item.Name
                sizeBytes = [long]$item.Length
                sha256 = Get-G04DCSha256 -Path $item.FullName
                version = [string]$properties.ProductVersion
                architecture = if ($template -match '(^|;)x64($|;)') { 'x64' } else { $template }
                productCode = ([string]$properties.ProductCode).ToUpperInvariant()
                upgradeCode = ([string]$properties.UpgradeCode).ToUpperInvariant()
                packageCode = ([string]$summary.Property(9)).ToUpperInvariant()
            }
        }
        finally { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($summary) }
    }
    finally {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($database)
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer)
    }
}

function Get-G04DCCertificateStoreDigest {
    $rows = [System.Collections.Generic.List[string]]::new()
    foreach ($location in @([Security.Cryptography.X509Certificates.StoreLocation]::CurrentUser, [Security.Cryptography.X509Certificates.StoreLocation]::LocalMachine)) {
        foreach ($name in @('Root', 'CA', 'AuthRoot', 'TrustedPeople', 'My', 'Disallowed')) {
            $store = [Security.Cryptography.X509Certificates.X509Store]::new($name, $location)
            try {
                $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadOnly -bor [Security.Cryptography.X509Certificates.OpenFlags]::OpenExistingOnly)
                foreach ($certificate in @($store.Certificates | Sort-Object Thumbprint)) {
                    $rows.Add(('{0}|{1}|{2}|{3}' -f [string]$location, $name, $certificate.Thumbprint.ToUpperInvariant(), (Get-G04DCSha256Bytes -Bytes $certificate.RawData)))
                }
            }
            finally { $store.Close() }
        }
    }
    return Get-G04DCCanonicalHash -Rows @($rows.ToArray())
}

function Get-G04DCNetworkState {
    $adapters = @(Get-NetAdapter -IncludeHidden -ErrorAction Stop | Sort-Object ifIndex | ForEach-Object {
        [pscustomobject][ordered]@{ ifIndex = [int]$_.ifIndex; status = [string]$_.Status; linkSpeed = [string]$_.LinkSpeed }
    })
    # A physically disconnected adapter legitimately has no default-route
    # instances. Querying the absent prefixes directly turns that safe state
    # into a terminating CIM error, so enumerate and filter instead.
    $defaultRoutes = @(Get-NetRoute -ErrorAction Stop | Where-Object {
        $_.DestinationPrefix -in @('0.0.0.0/0', '::/0') -and $_.State -ne 'Invalid'
    } | ForEach-Object {
        [pscustomobject][ordered]@{ ifIndex = [int]$_.ifIndex; destinationPrefix = [string]$_.DestinationPrefix; nextHop = [string]$_.NextHop }
    })
    $tcpState = @(Get-NetTCPConnection -ErrorAction Stop)
    $tcp = @($tcpState | Where-Object { $_.State -eq 'Established' } | ForEach-Object { '{0}|{1}|{2}|{3}|{4}' -f $_.OwningProcess, $_.LocalAddress, $_.LocalPort, $_.RemoteAddress, $_.RemotePort } | Sort-Object)
    $ownedLoopbackListeners = @($tcpState | Where-Object {
        $_.State -eq 'Listen' -and [int]$_.OwningProcess -eq $PID -and $_.LocalAddress -in @('127.0.0.1', '::1')
    })
    $activeInterfaceIndexes = @($adapters | Where-Object { $_.status -in @('Up', 'Connected') } | ForEach-Object { [int]$_.ifIndex })
    $activeDnsServerCount = @(Get-DnsClientServerAddress -ErrorAction Stop | Where-Object {
        $activeInterfaceIndexes -contains [int]$_.InterfaceIndex
    } | ForEach-Object { @($_.ServerAddresses) } | Where-Object { ![string]::IsNullOrWhiteSpace([string]$_) }).Count
    $physicalAdapterCount = @(Get-NetAdapter -Physical -ErrorAction Stop).Count
    $proxy = [DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::GetProxyConfiguration()
    $proxyEnvironmentCount = @(@('HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY') | Where-Object {
        ![string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($_, 'Process')) -or
        ![string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($_, 'User')) -or
        ![string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($_, 'Machine'))
    }).Count
    return [pscustomobject][ordered]@{
        adapters = $adapters
        physicalAdapterCount = $physicalAdapterCount
        connectedAdapterCount = @($adapters | Where-Object { $_.status -in @('Up', 'Connected') }).Count
        defaultRoutes = $defaultRoutes
        activeDnsServerCount = $activeDnsServerCount
        establishedTcpSha256 = Get-G04DCCanonicalHash -Rows $tcp
        establishedTcpCount = $tcp.Count
        currentProcessLoopbackListenerCount = $ownedLoopbackListeners.Count
        proxyFree = [bool]$proxy.proxyFree -and $proxyEnvironmentCount -eq 0
        proxyEnvironmentCount = $proxyEnvironmentCount
    }
}

function Get-G04DCCertificateChainFromRecord {
    param(
        [Parameter(Mandatory = $true)] $Record,
        [Parameter(Mandatory = $true)] [ValidateSet('signerChain', 'timestampChain')] [string]$ChainName,
        [Parameter(Mandatory = $true)] [string]$VerifierRoot
    )
    $bytes = [System.Collections.Generic.List[byte[]]]::new()
    $metadata = [System.Collections.Generic.List[object]]::new()
    $position = 0
    foreach ($certificate in @($Record.authenticode.$ChainName)) {
        Assert-G04DCExactProperties -Value $certificate -Names @('chainPosition', 'path', 'derSha256', 'thumbprint', 'serialNumber', 'subjectNameSha256', 'issuerNameSha256', 'publicKeyAlgorithmOid', 'publicKeySizeBits', 'signatureAlgorithmOid', 'ekuOids', 'notBeforeUtc', 'notAfterUtc', 'selfSignedName') -Code 'OFFLINE_PROVENANCE_CERTIFICATE_INVALID'
        $relative = [string]$certificate.path
        if ([int]$certificate.chainPosition -ne $position -or $relative -notmatch '^certificates/(signer|timestamp)-[0-9]{2}-[0-9a-f]{64}\.cer$' -or $relative -match '(^|/)\.\.(/|$)') {
            throw '[OFFLINE_PROVENANCE_CERTIFICATE_INVALID] Certificate record path or order is invalid.'
        }
        $path = [IO.Path]::GetFullPath((Join-Path $VerifierRoot $relative.Replace('/', '\')))
        $prefix = [IO.Path]::GetFullPath($VerifierRoot).TrimEnd('\') + '\'
        if (!$path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { throw '[OFFLINE_PROVENANCE_CERTIFICATE_INVALID] Certificate path escapes verifier root.' }
        $item = Get-Item -LiteralPath $path -Force
        if ($item.PSIsContainer -or [bool]($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.Length -gt 1048576 -or
            (Get-G04DCSha256 -Path $path) -cne [string]$certificate.derSha256) {
            throw '[OFFLINE_PROVENANCE_CERTIFICATE_INVALID] Certificate DER failed exact identity.'
        }
        $der = [IO.File]::ReadAllBytes($path)
        $observed = [DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::GetCertificateEvidence($der, $position)
        $observedRecord = [pscustomobject][ordered]@{
            chainPosition = [int]$observed.chainPosition; path = $relative; derSha256 = [string]$observed.derSha256
            thumbprint = [string]$observed.thumbprint; serialNumber = [string]$observed.serialNumber
            subjectNameSha256 = [string]$observed.subjectNameSha256; issuerNameSha256 = [string]$observed.issuerNameSha256
            publicKeyAlgorithmOid = [string]$observed.publicKeyAlgorithmOid; publicKeySizeBits = [int]$observed.publicKeySizeBits
            signatureAlgorithmOid = [string]$observed.signatureAlgorithmOid; ekuOids = @($observed.ekuOids | ForEach-Object { [string]$_ })
            notBeforeUtc = [string]$observed.notBeforeUtc; notAfterUtc = [string]$observed.notAfterUtc; selfSignedName = [bool]$observed.selfSignedName
        }
        if (($observedRecord | ConvertTo-Json -Depth 8 -Compress) -cne ($certificate | ConvertTo-Json -Depth 8 -Compress)) {
            throw '[OFFLINE_PROVENANCE_CERTIFICATE_INVALID] Certificate DER metadata does not match the signed record.'
        }
        $bytes.Add($der)
        $metadata.Add($observedRecord)
        $position++
    }
    if ($bytes.Count -lt 2 -or !$metadata[$metadata.Count - 1].selfSignedName) { throw '[OFFLINE_PROVENANCE_CERTIFICATE_INVALID] Chain is incomplete or does not end at a self-issued pinned root.' }
    return [pscustomobject][ordered]@{ bytes = [byte[][]]$bytes.ToArray(); metadata = @($metadata.ToArray()) }
}

function Assert-G04DCRecordPolicy {
    param([Parameter(Mandatory = $true)] $Record, [Parameter(Mandatory = $true)] [ValidateSet('A', 'B')] [string]$Id)
    $expected = Get-G04DCExpectedMsi
    if ([int]$Record.schemaVersion -ne 1 -or [string]$Record.recordType -cne 'g04d-c13-online-authenticode-verifier' -or
        [string]$Record.verifier.id -cne $Id -or ![bool]$Record.verification.accepted -or ![bool]$Record.verification.signTool.accepted -or
        ![bool]$Record.verification.winVerifyTrust.accepted -or ![bool]$Record.verification.signerChain.accepted -or
        ![bool]$Record.verification.timestampChain.accepted -or ![bool]$Record.verification.signerChain.revocationKnownGood -or
        ![bool]$Record.verification.timestampChain.revocationKnownGood -or [string]$Record.authenticode.revocation.signerChainExcludeRoot -cne 'good' -or
        [string]$Record.authenticode.revocation.timestampChainExcludeRoot -cne 'good' -or [string]$Record.subject.sha256 -cne [string]$expected.Sha256 -or
        [long]$Record.subject.sizeBytes -ne [long]$expected.SizeBytes -or [string]$Record.authenticode.signerLeafThumbprint -cne [string]$expected.SignerThumbprint -or
        [string]$Record.authenticode.timestampLeafThumbprint -cne [string]$expected.TimestampSignerThumbprint) {
        throw '[OFFLINE_PROVENANCE_RECORD_INVALID] Online verifier record is not accepted by the offline policy.'
    }
}

$startedAt = [DateTime]::UtcNow
[DateTime]$trustedHost = [DateTime]::MinValue
if (![DateTime]::TryParse($TrustedHostUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AdjustToUniversal, [ref]$trustedHost)) {
    throw '[OFFLINE_PROVENANCE_CLOCK_INVALID] Trusted host UTC is invalid.'
}
$trustedHost = $trustedHost.ToUniversalTime()
if ([Math]::Abs(($startedAt - $trustedHost).TotalMinutes) -gt $MaximumClockSkewMinutes) { throw '[OFFLINE_PROVENANCE_CLOCK_INVALID] Guest UTC differs from trusted host UTC by more than five minutes.' }

$bundleItem = Get-Item -LiteralPath $BundleDirectory -Force
$msiItem = Get-Item -LiteralPath $MsiPath -Force
if (!$bundleItem.PSIsContainer -or [bool]($bundleItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
    $msiItem.PSIsContainer -or [bool]($msiItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw '[ATTESTATION_PACKAGE_INVALID] Provenance bundle or MSI root is unsafe.'
}
$bundle = $bundleItem.FullName
$msi = $msiItem.FullName
if (Test-Path -LiteralPath $EvidenceDirectory) { throw '[OFFLINE_PROVENANCE_EVIDENCE_INVALID] Refusing to overwrite offline evidence.' }
$bundleEntries = @(Get-ChildItem -LiteralPath $bundle -Recurse -Force)
if (@($bundleEntries | Where-Object { [bool]($_.Attributes -band [IO.FileAttributes]::ReparsePoint) }).Count -ne 0) { throw '[ATTESTATION_PACKAGE_INVALID] Provenance bundle contains a reparse point.' }
$bundleFiles = @($bundleEntries | Where-Object { !$_.PSIsContainer })
if ($bundleFiles.Count -gt 64 -or ($bundleFiles | Measure-Object -Property Length -Sum).Sum -gt 16777216) { throw '[ATTESTATION_PACKAGE_INVALID] Provenance bundle exceeds its file-count or byte ceiling.' }
$privateMaterial = @($bundleFiles | Where-Object { $_.Name -match '(?i)(private|\.pfx$|\.p12$|\.key$|\.pem$)' })
if ($privateMaterial.Count -ne 0) { throw '[ATTESTATION_PRIVATE_KEY_LEAK] Provenance bundle contains prohibited private-key material.' }
Assert-G04DCCanonicalArtifactManifest -EvidenceDirectory $bundle | Out-Null
Assert-G04DCCanonicalArtifactManifest -EvidenceDirectory (Join-Path $bundle 'verifiers\A') | Out-Null
Assert-G04DCCanonicalArtifactManifest -EvidenceDirectory (Join-Path $bundle 'verifiers\B') | Out-Null

$attestationPath = Join-Path $bundle 'combined-provenance-attestation.json'
$signaturePath = Join-Path $bundle 'combined-provenance-attestation.sig'
$publicKeyPath = Join-Path $bundle 'attestation-public-key.cspblob'
$observedPublicKeySha256 = Get-G04DCSha256 -Path $publicKeyPath
if ($observedPublicKeySha256 -cne $ExpectedPublicKeySha256) { throw '[ATTESTATION_PUBLIC_KEY_INVALID] Public key does not match the host launch binding.' }
$publicKey = [IO.File]::ReadAllBytes($publicKeyPath)
if ($publicKey.Length -lt 16 -or $publicKey.Length -gt 8192 -or $publicKey[0] -ne 0x06) { throw '[ATTESTATION_PUBLIC_KEY_INVALID] Public key blob is not a bounded RSA PUBLICKEYBLOB.' }
$attestationBytes = [IO.File]::ReadAllBytes($attestationPath)
$signatureBytes = [IO.File]::ReadAllBytes($signaturePath)
$rsa = [Security.Cryptography.RSACryptoServiceProvider]::new()
$signatureValid = $false
try {
    $rsa.ImportCspBlob($publicKey)
    if ($rsa.KeySize -ne 3072 -or $signatureBytes.Length -ne ($rsa.KeySize / 8)) { throw '[ATTESTATION_PUBLIC_KEY_INVALID] Host attestation key or detached signature size is invalid.' }
    $signatureValid = $rsa.VerifyData($attestationBytes, 'SHA256', $signatureBytes)
}
finally { $rsa.Clear(); $rsa.Dispose() }
if (!$signatureValid) { throw '[ATTESTATION_SIGNATURE_INVALID] Detached attestation signature is invalid.' }

$attestation = Read-G04DCCanonicalJson -Path $attestationPath -Code 'ATTESTATION_SCHEMA_INVALID'
Assert-G04DCExactProperties -Value $attestation -Names @('schemaVersion', 'attestationType', 'subject', 'onlineVerification', 'freshness', 'proofPolicy', 'integrity', 'agreement') -Code 'ATTESTATION_SCHEMA_INVALID'
Assert-G04DCExactProperties -Value $attestation.subject -Names @('filename', 'sizeBytes', 'sha256', 'version', 'architecture', 'productCode', 'upgradeCode', 'packageCode') -Code 'ATTESTATION_SCHEMA_INVALID'
Assert-G04DCExactProperties -Value $attestation.onlineVerification -Names @(
    'verifierARecordPath', 'verifierARecordSha256', 'verifierBRecordPath', 'verifierBRecordSha256',
    'verifiedAtEarliestUtc', 'verifiedAtLatestUtc', 'verifierMaximumSeparationMinutes', 'signerChain', 'timestampChain',
    'signatureDigestAlgorithm', 'timestampType', 'timestampUtc', 'revocation'
) -Code 'ATTESTATION_SCHEMA_INVALID'
Assert-G04DCExactProperties -Value $attestation.onlineVerification.revocation -Names @('signerChainExcludeRoot', 'timestampChainExcludeRoot', 'evidenceMode', 'rawCrlOrOcspObjectsRetained') -Code 'ATTESTATION_SCHEMA_INVALID'
Assert-G04DCExactProperties -Value $attestation.freshness -Names @('createdAtUtc', 'expiresAtUtc', 'maximumLifetimeHours', 'revocationNextUpdateBoundaryAvailable') -Code 'ATTESTATION_SCHEMA_INVALID'
Assert-G04DCExactProperties -Value $attestation.proofPolicy -Names @('minimumOnlineVerifierCount', 'offlineCloneNetworkRequired', 'offlineUrlRetrievalPermitted', 'globalCertificateStoreMutationPermitted', 'partialChainAcceptedAsTrust', 'offlineRevocationAcceptedAsTrust') -Code 'ATTESTATION_SCHEMA_INVALID'
Assert-G04DCExactProperties -Value $attestation.integrity -Names @('detachedSignatureAlgorithm', 'rsaKeySizeBits', 'privateKeyCopiedToGuest', 'publicKeyPath', 'publicKeySha256', 'signedPayloadManifestPath', 'signedPayloadManifestSha256', 'purpose') -Code 'ATTESTATION_SCHEMA_INVALID'
Assert-G04DCExactProperties -Value $attestation.agreement -Names @('accepted', 'fields') -Code 'ATTESTATION_SCHEMA_INVALID'
Assert-G04DCExactProperties -Value $attestation.agreement.fields -Names @('subject', 'signatureDigestAlgorithm', 'timestampType', 'timestampUtc', 'signerLeaf', 'timestampLeaf', 'signerChain', 'timestampChain', 'roots', 'revocation') -Code 'ATTESTATION_SCHEMA_INVALID'
if ([int]$attestation.schemaVersion -ne 1 -or [string]$attestation.attestationType -cne 'g04d-c13-offline-authenticode-provenance' -or
    [int]$attestation.proofPolicy.minimumOnlineVerifierCount -ne 2 -or [bool]$attestation.proofPolicy.offlineCloneNetworkRequired -or
    [bool]$attestation.proofPolicy.offlineUrlRetrievalPermitted -or [bool]$attestation.proofPolicy.globalCertificateStoreMutationPermitted -or
    [bool]$attestation.proofPolicy.partialChainAcceptedAsTrust -or [bool]$attestation.proofPolicy.offlineRevocationAcceptedAsTrust -or
    [string]$attestation.integrity.detachedSignatureAlgorithm -cne 'RSASSA-PKCS1-v1_5-SHA256' -or [int]$attestation.integrity.rsaKeySizeBits -lt 3072 -or
    [bool]$attestation.integrity.privateKeyCopiedToGuest -or [string]$attestation.integrity.publicKeyPath -cne 'attestation-public-key.cspblob' -or
    [string]$attestation.integrity.publicKeySha256 -cne (Get-G04DCSha256 -Path $publicKeyPath) -or
    [string]$attestation.integrity.purpose -cne 'host transport and integrity binding only; not vendor provenance' -or
    [string]$attestation.onlineVerification.verifierARecordPath -cne 'verifiers/A/online-verifier-A.json' -or
    [string]$attestation.onlineVerification.verifierBRecordPath -cne 'verifiers/B/online-verifier-B.json' -or
    [string]$attestation.onlineVerification.verifierARecordSha256 -notmatch '^[0-9a-f]{64}$' -or
    [string]$attestation.onlineVerification.verifierBRecordSha256 -notmatch '^[0-9a-f]{64}$' -or
    [int]$attestation.onlineVerification.verifierMaximumSeparationMinutes -ne 30 -or
    [string]$attestation.onlineVerification.signatureDigestAlgorithm -cne 'sha256' -or
    [string]$attestation.onlineVerification.timestampType -cne 'embedded-authenticode-timestamp' -or
    [string]$attestation.onlineVerification.revocation.signerChainExcludeRoot -cne 'good' -or
    [string]$attestation.onlineVerification.revocation.timestampChainExcludeRoot -cne 'good' -or
    [string]$attestation.onlineVerification.revocation.evidenceMode -cne 'two-independent-online-verifier-results' -or
    [bool]$attestation.onlineVerification.revocation.rawCrlOrOcspObjectsRetained -or
    [bool]$attestation.freshness.revocationNextUpdateBoundaryAvailable -or ![bool]$attestation.agreement.accepted -or
    @($attestation.agreement.fields.PSObject.Properties | Where-Object { ![bool]$_.Value }).Count -ne 0) {
    throw '[ATTESTATION_POLICY_INVALID] Attestation policy or public-key binding is invalid.'
}
$payloadManifestPath = Join-Path $bundle 'signed-payload-manifest.json'
if ([string]$attestation.integrity.signedPayloadManifestPath -cne 'signed-payload-manifest.json' -or
    (Get-G04DCSha256 -Path $payloadManifestPath) -cne [string]$attestation.integrity.signedPayloadManifestSha256) {
    throw '[ATTESTATION_PACKAGE_INVALID] Signed payload manifest binding is invalid.'
}
$payloadManifest = Read-G04DCCanonicalJson -Path $payloadManifestPath -Code 'ATTESTATION_PACKAGE_INVALID'
Assert-G04DCExactProperties -Value $payloadManifest -Names @('schemaVersion', 'files') -Code 'ATTESTATION_PACKAGE_INVALID'
$declaredPayload = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($file in @($payloadManifest.files)) {
    Assert-G04DCExactProperties -Value $file -Names @('path', 'sizeBytes', 'sha256') -Code 'ATTESTATION_PACKAGE_INVALID'
    $relative = ([string]$file.path).Replace('\', '/')
    if (($relative -cne 'attestation-public-key.cspblob' -and !$relative.StartsWith('verifiers/', [StringComparison]::Ordinal)) -or
        $relative -match '(^|/)\.\.(/|$)' -or !$declaredPayload.Add($relative)) { throw '[ATTESTATION_PACKAGE_INVALID] Signed payload path is unsafe or outside the closed payload set.' }
    $candidate = [IO.Path]::GetFullPath((Join-Path $bundle $relative.Replace('/', '\')))
    $item = Get-Item -LiteralPath $candidate -Force
    if ($item.PSIsContainer -or [bool]($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or [long]$item.Length -ne [long]$file.sizeBytes -or
        (Get-G04DCSha256 -Path $item.FullName) -cne [string]$file.sha256) { throw '[ATTESTATION_PACKAGE_INVALID] Signed payload file identity is invalid.' }
}
$actualPayload = @(Get-ChildItem -LiteralPath $bundle -File -Recurse -Force | ForEach-Object { $_.FullName.Substring($bundle.Length + 1).Replace('\', '/') } | Where-Object {
    $_ -ceq 'attestation-public-key.cspblob' -or $_.StartsWith('verifiers/', [StringComparison]::Ordinal)
})
if (@($actualPayload | Where-Object { !$declaredPayload.Contains($_) }).Count -ne 0 -or @($declaredPayload | Where-Object { $actualPayload -cnotcontains $_ }).Count -ne 0) {
    throw '[ATTESTATION_PACKAGE_INVALID] Signed payload manifest is incomplete or contains an extra file.'
}
$allowedRootFiles = @('artifact-manifest.json', 'attestation-public-key.cspblob', 'combined-provenance-attestation.json', 'combined-provenance-attestation.sig', 'signed-payload-manifest.json')
$unexpectedRootFiles = @(Get-ChildItem -LiteralPath $bundle -File -Force | Where-Object { $allowedRootFiles -cnotcontains $_.Name })
$unexpectedRootDirectories = @(Get-ChildItem -LiteralPath $bundle -Directory -Force | Where-Object { $_.Name -cne 'verifiers' })
if ($unexpectedRootFiles.Count -ne 0 -or $unexpectedRootDirectories.Count -ne 0) { throw '[ATTESTATION_PACKAGE_INVALID] Provenance bundle contains an unexpected root entry.' }
[DateTime]$created = [DateTime]::Parse([string]$attestation.freshness.createdAtUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AdjustToUniversal).ToUniversalTime()
[DateTime]$expires = [DateTime]::Parse([string]$attestation.freshness.expiresAtUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AdjustToUniversal).ToUniversalTime()
[DateTime]$earliest = [DateTime]::Parse([string]$attestation.onlineVerification.verifiedAtEarliestUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AdjustToUniversal).ToUniversalTime()
[DateTime]$latest = [DateTime]::Parse([string]$attestation.onlineVerification.verifiedAtLatestUtc, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AdjustToUniversal).ToUniversalTime()
if ($created -gt $startedAt.AddMinutes($MaximumClockSkewMinutes) -or $earliest -gt $startedAt.AddMinutes($MaximumClockSkewMinutes) -or
    $latest -gt $startedAt.AddMinutes($MaximumClockSkewMinutes)) { throw '[ATTESTATION_FUTURE_DATED] Attestation or verifier record is future-dated.' }
if ($startedAt -ge $expires -or $expires -le $created -or $expires -gt $earliest.AddHours(12) -or [int]$attestation.freshness.maximumLifetimeHours -gt 12 -or
    [Math]::Abs(($latest - $earliest).TotalMinutes) -gt 30) { throw '[ATTESTATION_EXPIRED] Attestation freshness or two-verifier time window is invalid.' }
Assert-G04DCProvenanceFreshnessModel -NowUtc $startedAt -CreatedUtc $created -ExpiresUtc $expires -VerifiedEarliestUtc $earliest -VerifiedLatestUtc $latest -MaximumLifetimeHours ([int]$attestation.freshness.maximumLifetimeHours) -ClockSkewMinutes $MaximumClockSkewMinutes | Out-Null

$aPath = Join-Path $bundle 'verifiers\A\online-verifier-A.json'
$bPath = Join-Path $bundle 'verifiers\B\online-verifier-B.json'
if ((Get-G04DCSha256 -Path $aPath) -cne [string]$attestation.onlineVerification.verifierARecordSha256 -or
    (Get-G04DCSha256 -Path $bPath) -cne [string]$attestation.onlineVerification.verifierBRecordSha256) { throw '[ATTESTATION_RECORD_BINDING_INVALID] Verifier record hash binding failed.' }
$a = Read-G04DCCanonicalJson -Path $aPath -Code 'OFFLINE_PROVENANCE_RECORD_INVALID'
$b = Read-G04DCCanonicalJson -Path $bPath -Code 'OFFLINE_PROVENANCE_RECORD_INVALID'
Assert-G04DCRecordPolicy -Record $a -Id 'A'
Assert-G04DCRecordPolicy -Record $b -Id 'B'
Assert-G04DCOnlineProvenanceRecordModel -Record $a -ExpectedId 'A' -ExpectedMsi (Get-G04DCExpectedMsi) | Out-Null
Assert-G04DCOnlineProvenanceRecordModel -Record $b -ExpectedId 'B' -ExpectedMsi (Get-G04DCExpectedMsi) | Out-Null
if ([string]$a.verifier.instanceId -ceq [string]$b.verifier.instanceId -or [string]$a.verifier.vmUuid -ceq [string]$b.verifier.vmUuid -or [string]$a.verifier.diskUuid -ceq [string]$b.verifier.diskUuid) {
    throw '[ONLINE_PROVENANCE_INDEPENDENCE_INVALID] Online records are not independent.'
}
$aSemantic = [ordered]@{
    subject = $a.subject
    signatureDigestAlgorithm = $a.authenticode.signatureDigestAlgorithm
    signatureDigestAlgorithmOid = $a.authenticode.signatureDigestAlgorithmOid
    timestampType = $a.authenticode.timestampType
    timestampUtc = $a.authenticode.timestampUtc
    signerLeafDerSha256 = $a.authenticode.signerLeafDerSha256
    signerLeafThumbprint = $a.authenticode.signerLeafThumbprint
    timestampLeafDerSha256 = $a.authenticode.timestampLeafDerSha256
    timestampLeafThumbprint = $a.authenticode.timestampLeafThumbprint
    signerChain = @($a.authenticode.signerChain | Select-Object -Property * -ExcludeProperty path)
    timestampChain = @($a.authenticode.timestampChain | Select-Object -Property * -ExcludeProperty path)
    revocation = $a.authenticode.revocation
}
$bSemantic = [ordered]@{
    subject = $b.subject
    signatureDigestAlgorithm = $b.authenticode.signatureDigestAlgorithm
    signatureDigestAlgorithmOid = $b.authenticode.signatureDigestAlgorithmOid
    timestampType = $b.authenticode.timestampType
    timestampUtc = $b.authenticode.timestampUtc
    signerLeafDerSha256 = $b.authenticode.signerLeafDerSha256
    signerLeafThumbprint = $b.authenticode.signerLeafThumbprint
    timestampLeafDerSha256 = $b.authenticode.timestampLeafDerSha256
    timestampLeafThumbprint = $b.authenticode.timestampLeafThumbprint
    signerChain = @($b.authenticode.signerChain | Select-Object -Property * -ExcludeProperty path)
    timestampChain = @($b.authenticode.timestampChain | Select-Object -Property * -ExcludeProperty path)
    revocation = $b.authenticode.revocation
}
if (($aSemantic | ConvertTo-Json -Depth 30 -Compress) -cne ($bSemantic | ConvertTo-Json -Depth 30 -Compress)) { throw '[ONLINE_PROVENANCE_VERIFIERS_DISAGREE] Online records disagree semantically.' }
Compare-G04DCOnlineProvenanceRecordModels -VerifierA $a -VerifierB $b | Out-Null
if (($attestation.onlineVerification.signerChain | ConvertTo-Json -Depth 20 -Compress) -cne ($a.authenticode.signerChain | ConvertTo-Json -Depth 20 -Compress) -or
    ($attestation.onlineVerification.timestampChain | ConvertTo-Json -Depth 20 -Compress) -cne ($a.authenticode.timestampChain | ConvertTo-Json -Depth 20 -Compress) -or
    [string]$attestation.onlineVerification.signatureDigestAlgorithm -cne [string]$a.authenticode.signatureDigestAlgorithm -or
    [string]$attestation.onlineVerification.timestampType -cne [string]$a.authenticode.timestampType -or
    [string]$attestation.onlineVerification.timestampUtc -cne [string]$a.authenticode.timestampUtc) {
    throw '[ATTESTATION_RECORD_BINDING_INVALID] Combined attestation does not exactly reproduce the agreed verifier result.'
}

if (!('DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier' -as [type])) { Add-Type -Path (Join-Path $PSScriptRoot 'AuthenticodeProvenance.cs') -ReferencedAssemblies 'System.Security.dll' -ErrorAction Stop }
$storeBefore = Get-G04DCCertificateStoreDigest
$networkBefore = Get-G04DCNetworkState
if ($networkBefore.physicalAdapterCount -ne 1 -or $networkBefore.connectedAdapterCount -ne 0 -or @($networkBefore.defaultRoutes).Count -ne 0 -or
    $networkBefore.activeDnsServerCount -ne 0 -or $networkBefore.establishedTcpCount -ne 0 -or
    $networkBefore.currentProcessLoopbackListenerCount -ne 0 -or !$networkBefore.proxyFree) {
    throw '[OFFLINE_PROVENANCE_NETWORK_INVALID] Proof clone is not physically disconnected before verification.'
}

$msiIdentity = Get-G04DCOfflineMsiIdentity -Path $msi
$expected = Get-G04DCExpectedMsi
if (($msiIdentity | ConvertTo-Json -Compress) -cne ($attestation.subject | ConvertTo-Json -Compress) -or
    [string]$msiIdentity.sha256 -cne [string]$expected.Sha256 -or [long]$msiIdentity.sizeBytes -ne [long]$expected.SizeBytes) {
    throw '[MSI_IDENTITY_MISMATCH] Offline MSI identity does not match the signed attestation.'
}

$signerA = Get-G04DCCertificateChainFromRecord -Record $a -ChainName 'signerChain' -VerifierRoot (Join-Path $bundle 'verifiers\A')
$timestampA = Get-G04DCCertificateChainFromRecord -Record $a -ChainName 'timestampChain' -VerifierRoot (Join-Path $bundle 'verifiers\A')
$signerB = Get-G04DCCertificateChainFromRecord -Record $b -ChainName 'signerChain' -VerifierRoot (Join-Path $bundle 'verifiers\B')
$timestampB = Get-G04DCCertificateChainFromRecord -Record $b -ChainName 'timestampChain' -VerifierRoot (Join-Path $bundle 'verifiers\B')
if ((@($signerA.metadata.derSha256) -join "`n") -cne (@($signerB.metadata.derSha256) -join "`n") -or
    (@($timestampA.metadata.derSha256) -join "`n") -cne (@($timestampB.metadata.derSha256) -join "`n")) {
    throw '[ONLINE_PROVENANCE_VERIFIERS_DISAGREE] Certificate DER chains disagree.'
}

$fileTrust = [DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::VerifyOfflineFileDigestAndSignature($msi)
Assert-G04DCOfflineEmbeddedIdentityModel -FileTrust $fileTrust -OnlineRecord $a | Out-Null
$signerStructural = [DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::BuildOfflineExclusiveChain($signerA.bytes, 'signer', [string]$a.authenticode.timestampUtc)
$timestampStructural = [DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::BuildOfflineExclusiveChain($timestampA.bytes, 'timestamp', [string]$a.authenticode.timestampUtc)
if (!$signerStructural.valid -or !$signerStructural.exclusiveRoot -or !$signerStructural.urlRetrievalDisabled -or
    !$timestampStructural.valid -or !$timestampStructural.exclusiveRoot -or !$timestampStructural.urlRetrievalDisabled -or
    (@($signerStructural.certificates.derSha256) -join "`n") -cne (@($signerA.metadata.derSha256) -join "`n") -or
    (@($timestampStructural.certificates.derSha256) -join "`n") -cne (@($timestampA.metadata.derSha256) -join "`n")) {
    throw '[OFFLINE_PROVENANCE_STRUCTURAL_CHAIN_INVALID] Exclusive-root structural chain verification failed.'
}

$storeAfter = Get-G04DCCertificateStoreDigest
$networkAfter = Get-G04DCNetworkState
if ($storeBefore -cne $storeAfter) { throw '[OFFLINE_PROVENANCE_STORE_MUTATION] Global certificate stores changed during verification.' }
if ($networkAfter.physicalAdapterCount -ne 1 -or $networkAfter.connectedAdapterCount -ne 0 -or @($networkAfter.defaultRoutes).Count -ne 0 -or
    $networkAfter.activeDnsServerCount -ne 0 -or $networkAfter.establishedTcpCount -ne 0 -or
    $networkAfter.currentProcessLoopbackListenerCount -ne 0 -or !$networkAfter.proxyFree -or
    $networkBefore.establishedTcpSha256 -cne $networkAfter.establishedTcpSha256) {
    throw '[OFFLINE_PROVENANCE_NETWORK_INVALID] Network state changed or a connection appeared during verification.'
}

New-Item -ItemType Directory -Path $EvidenceDirectory | Out-Null
$evidence = (Resolve-Path -LiteralPath $EvidenceDirectory).Path
[IO.File]::WriteAllText((Join-Path $evidence 'MARKER.md'), "# G04D-C13 offline provenance evidence`r`n", [Text.UTF8Encoding]::new($false))
$result = [pscustomobject][ordered]@{
    schemaVersion = 1
    status = 'OFFLINE_PROVENANCE_VERIFIED'
    accepted = $true
    verifiedAtUtc = [DateTime]::UtcNow.ToString('o')
    trustedHostUtc = $trustedHost.ToString('o')
    clockSkewSeconds = [Math]::Round(($startedAt - $trustedHost).TotalSeconds, 3)
    bundleManifestVerified = $true
    detachedSignatureVerified = $true
    attestationFresh = $true
    twoIndependentOnlineVerifiers = $true
    onlineRevocationDecision = 'two-independent-online-verifier-results'
    msiIdentity = $msiIdentity
    embeddedAuthenticode = [ordered]@{ hashAndSignatureValid = [bool]$fileTrust.passed; signerCount = [int]$fileTrust.signerCount; timestampSignerCount = [int]$fileTrust.timestampSignerCount; timestampUtc = [string]$fileTrust.timestampUtc }
    signerStructuralChain = [ordered]@{ accepted = [bool]$signerStructural.valid; exclusiveRoot = [bool]$signerStructural.exclusiveRoot; urlRetrievalDisabled = [bool]$signerStructural.urlRetrievalDisabled; purposeEkuValid = [bool]$signerStructural.purposeEkuValid; certificateDerSha256 = @($signerStructural.certificates.derSha256) }
    timestampStructuralChain = [ordered]@{ accepted = [bool]$timestampStructural.valid; exclusiveRoot = [bool]$timestampStructural.exclusiveRoot; urlRetrievalDisabled = [bool]$timestampStructural.urlRetrievalDisabled; purposeEkuValid = [bool]$timestampStructural.purposeEkuValid; certificateDerSha256 = @($timestampStructural.certificates.derSha256) }
    diagnosticDefaultChainAcceptedAsTrust = $false
    diagnosticOfflineRevocationAcceptedAsTrust = $false
    globalCertificateStoreSha256Before = $storeBefore
    globalCertificateStoreSha256After = $storeAfter
    globalCertificateStoreUnchanged = $storeBefore -ceq $storeAfter
    networkCanary = [ordered]@{
        exactlyOnePhysicalAdapter = $networkAfter.physicalAdapterCount -eq 1
        adaptersDisconnected = $networkAfter.connectedAdapterCount -eq 0
        noDefaultRoute = @($networkAfter.defaultRoutes).Count -eq 0
        noActiveDnsServer = $networkAfter.activeDnsServerCount -eq 0
        noEstablishedTcp = $networkAfter.establishedTcpCount -eq 0
        noLoopbackListenerUsedForProvenance = $networkAfter.currentProcessLoopbackListenerCount -eq 0
        noProxy = [bool]$networkAfter.proxyFree
    }
    privateKeyMaterialAbsent = $true
}
Write-G04DCJson -Path (Join-Path $evidence 'offline-provenance-result.json') -Value $result -Depth 20
New-G04DCCanonicalArtifactManifest -EvidenceDirectory $evidence | Out-Null
Assert-G04DCCanonicalArtifactManifest -EvidenceDirectory $evidence | Out-Null
Write-Output 'OFFLINE_PROVENANCE_VERIFIED'
