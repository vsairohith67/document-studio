[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)] [ValidateSet('A', 'B')] [string]$VerifierId,
    [Parameter(Mandatory = $true)] [ValidatePattern('^[0-9a-fA-F-]{36}$')] [string]$VerifierInstanceId,
    [Parameter(Mandatory = $true)] [ValidatePattern('^[0-9a-fA-F-]{36}$')] [string]$VmUuid,
    [Parameter(Mandatory = $true)] [ValidatePattern('^[0-9a-fA-F-]{36}$')] [string]$DiskUuid,
    [Parameter(Mandatory = $true)] [string]$MsiPath,
    [Parameter(Mandatory = $true)] [string]$SignToolPath,
    [Parameter(Mandatory = $true)] [string]$EvidenceDirectory,
    [ValidateRange(1000, 120000)] [int]$UrlRetrievalTimeoutMilliseconds = 15000
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'G04DC.Common.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'G04DC.Provenance.psm1') -Force

function Get-G04DCCertificateRecord {
    param(
        [Parameter(Mandatory = $true)] $Certificate,
        [Parameter(Mandatory = $true)] [string]$Purpose,
        [Parameter(Mandatory = $true)] [string]$CertificateDirectory
    )
    $fileName = '{0}-{1:D2}-{2}.cer' -f $Purpose, [int]$Certificate.chainPosition, [string]$Certificate.derSha256
    $path = Join-Path $CertificateDirectory $fileName
    [IO.File]::WriteAllBytes($path, [byte[]]$Certificate.der)
    return [pscustomobject][ordered]@{
        chainPosition = [int]$Certificate.chainPosition
        path = 'certificates/' + $fileName
        derSha256 = [string]$Certificate.derSha256
        thumbprint = [string]$Certificate.thumbprint
        serialNumber = [string]$Certificate.serialNumber
        subjectNameSha256 = [string]$Certificate.subjectNameSha256
        issuerNameSha256 = [string]$Certificate.issuerNameSha256
        publicKeyAlgorithmOid = [string]$Certificate.publicKeyAlgorithmOid
        publicKeySizeBits = [int]$Certificate.publicKeySizeBits
        signatureAlgorithmOid = [string]$Certificate.signatureAlgorithmOid
        ekuOids = @($Certificate.ekuOids | ForEach-Object { [string]$_ })
        notBeforeUtc = [string]$Certificate.notBeforeUtc
        notAfterUtc = [string]$Certificate.notAfterUtc
        selfSignedName = [bool]$Certificate.selfSignedName
    }
}

function Get-G04DCToolIdentity {
    param([Parameter(Mandatory = $true)] [string]$Path, [Parameter(Mandatory = $true)] [string]$PathCategory)
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or [bool]($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw '[ONLINE_PROVENANCE_TOOL_INVALID] Verification tool must be a regular non-reparse file.'
    }
    $signature = Get-AuthenticodeSignature -LiteralPath $item.FullName
    if ([string]$signature.Status -cne 'Valid' -or !$signature.SignerCertificate) {
        throw '[ONLINE_PROVENANCE_TOOL_INVALID] Verification tool Authenticode identity is invalid.'
    }
    return [pscustomobject][ordered]@{
        pathCategory = $PathCategory
        sizeBytes = [long]$item.Length
        sha256 = Get-G04DCSha256 -Path $item.FullName
        fileVersion = [string]$item.VersionInfo.FileVersion
        productVersion = [string]$item.VersionInfo.ProductVersion
        signerLeafThumbprint = $signature.SignerCertificate.Thumbprint.ToUpperInvariant()
        signerLeafDerSha256 = Get-G04DCSha256Bytes -Bytes $signature.SignerCertificate.RawData
    }
}

function Get-G04DCSha256Bytes {
    param([Parameter(Mandatory = $true)] [byte[]]$Bytes)
    $algorithm = [Security.Cryptography.SHA256]::Create()
    try { return (($algorithm.ComputeHash($Bytes) | ForEach-Object { $_.ToString('x2') }) -join '') }
    finally { $algorithm.Dispose() }
}

function Write-G04DCSanitizedLog {
    param([Parameter(Mandatory = $true)] [string]$Path, [Parameter(Mandatory = $true)] [string]$Text, [Parameter(Mandatory = $true)] [string[]]$SensitivePaths)
    if ($Text.Length -gt 262144) { throw '[ONLINE_PROVENANCE_TOOL_INVALID] SignTool output exceeded the 256 KiB ceiling.' }
    $sanitized = $Text
    foreach ($sensitivePath in @($SensitivePaths | Where-Object { ![string]::IsNullOrWhiteSpace($_) } | Sort-Object Length -Descending)) {
        $sanitized = $sanitized.Replace($sensitivePath, '<redacted-path>')
    }
    if (![string]::IsNullOrWhiteSpace($env:USERNAME)) { $sanitized = $sanitized.Replace($env:USERNAME, '<redacted-user>') }
    [IO.File]::WriteAllText($Path, $sanitized, [Text.UTF8Encoding]::new($false))
}

$expected = Get-G04DCExpectedMsi
$msiItem = Get-Item -LiteralPath $MsiPath -Force
$signToolItem = Get-Item -LiteralPath $SignToolPath -Force
if ($msiItem.PSIsContainer -or [bool]($msiItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw '[MSI_IDENTITY_MISMATCH] Online provenance input is not a regular non-reparse MSI.'
}
if (Test-Path -LiteralPath $EvidenceDirectory) { throw '[ONLINE_PROVENANCE_EVIDENCE_INVALID] Refusing to overwrite verifier evidence.' }
New-Item -ItemType Directory -Path $EvidenceDirectory | Out-Null
$evidence = (Resolve-Path -LiteralPath $EvidenceDirectory).Path
$certificateDirectory = Join-Path $evidence 'certificates'
New-Item -ItemType Directory -Path $certificateDirectory | Out-Null
[IO.File]::WriteAllText((Join-Path $evidence 'MARKER.md'), "# G04D-C13 online verifier $VerifierId evidence`r`n", [Text.UTF8Encoding]::new($false))

if (!('DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier' -as [type])) {
    Add-Type -Path (Join-Path $PSScriptRoot 'AuthenticodeProvenance.cs') -ReferencedAssemblies 'System.Security.dll' -ErrorAction Stop
}

$identity = Get-G04DCMsiIdentity -MsiPath $msiItem.FullName
$signature = Get-AuthenticodeSignature -LiteralPath $msiItem.FullName
if (!$signature.SignerCertificate -or !$signature.TimeStamperCertificate) {
    throw '[ONLINE_PROVENANCE_SIGNATURE_INVALID] Embedded signer and timestamp signer certificates are required.'
}
$signerLeafDerSha256 = Get-G04DCSha256Bytes -Bytes $signature.SignerCertificate.RawData
$timestampLeafDerSha256 = Get-G04DCSha256Bytes -Bytes $signature.TimeStamperCertificate.RawData

$os = Get-CimInstance Win32_OperatingSystem
if ([string]$os.Version -notmatch '^10\.0\.26100(?:\.|$)' -or [string]$os.BuildNumber -cne '26100' -or [string]$os.OSArchitecture -notmatch '64') {
    throw '[ONLINE_PROVENANCE_TARGET_OS_MISMATCH] SignTool verification must execute on the Windows Server 2025 build 26100 x64 verifier.'
}
# SignTool rejects /o together with /pa. Bind the target OS by requiring the verifier
# itself to be build 26100, then use the default authentication policy for the MSI.
$signToolArguments = @('verify', '/pa', '/all', '/v', '/tw', $msiItem.FullName)
$signToolOutput = @(& $signToolItem.FullName @signToolArguments 2>&1 | ForEach-Object { [string]$_ })
$signToolExitCode = $LASTEXITCODE
$signToolText = ($signToolOutput -join "`r`n") + "`r`n"
Write-G04DCSanitizedLog -Path (Join-Path $evidence 'signtool-sanitized.log') -Text $signToolText -SensitivePaths @($msiItem.FullName, $evidence, $env:USERPROFILE, $env:TEMP)
$digestMatch = [regex]::Match($signToolText, '(?im)^Hash of file \(([a-z0-9-]+)\):')
$signToolAccepted = $signToolExitCode -eq 0 -and
    $signToolText -match '(?im)^Number of signatures successfully Verified:\s*1\s*$' -and
    $signToolText -match '(?im)^Number of warnings:\s*0\s*$' -and
    $signToolText -match '(?im)^Number of errors:\s*0\s*$' -and
    $signToolText -notmatch '(?im)^SignTool (?:Error|Warning):'
if (!$signToolAccepted -or !$digestMatch.Success) {
    throw '[ONLINE_PROVENANCE_SIGNTOOL_FAILED] SignTool did not produce one warning-free accepted embedded-signature result.'
}

$fileTrust = [DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::VerifyOnlineFileTrust($msiItem.FullName)
if (!$fileTrust.passed -or $fileTrust.hashOnly -or $fileTrust.cacheOnlyUrlRetrieval -or $fileTrust.signerCount -ne 1 -or
    $fileTrust.timestampSignerCount -ne 1 -or $fileTrust.providerSignerError -ne 0 -or $fileTrust.providerTimestampError -ne 0 -or
    [string]$fileTrust.signerLeafDerSha256 -cne $signerLeafDerSha256 -or
    [string]$fileTrust.timestampLeafDerSha256 -cne $timestampLeafDerSha256 -or
    [string]::IsNullOrWhiteSpace([string]$fileTrust.timestampUtc)) {
    throw '[ONLINE_PROVENANCE_WINVERIFYTRUST_FAILED] WinVerifyTrust did not prove one exact embedded signer and timestamp with online revocation enabled.'
}

$signerChain = [DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::BuildOnlineChain(
    $signature.SignerCertificate.RawData, 'signer', [string]$fileTrust.timestampUtc, $UrlRetrievalTimeoutMilliseconds
)
$timestampChain = [DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::BuildOnlineChain(
    $signature.TimeStamperCertificate.RawData, 'timestamp', [string]$fileTrust.timestampUtc, $UrlRetrievalTimeoutMilliseconds
)
if (!$signerChain.valid -or !$signerChain.revocationKnownGood -or !$timestampChain.valid -or !$timestampChain.revocationKnownGood) {
    throw '[ONLINE_PROVENANCE_CHAIN_FAILED] Signer or timestamp chain failed complete online revocation and Authenticode policy validation.'
}
if (@($signerChain.certificates).Count -lt 2 -or @($timestampChain.certificates).Count -lt 2) {
    throw '[ONLINE_PROVENANCE_CHAIN_FAILED] Complete signer and timestamp chains are required.'
}
$acceptedSignatureAlgorithms = @(
    '1.2.840.113549.1.1.11', '1.2.840.113549.1.1.12', '1.2.840.113549.1.1.13',
    '1.2.840.10045.4.3.2', '1.2.840.10045.4.3.3', '1.2.840.10045.4.3.4'
)
$weakCertificates = @(@($signerChain.certificates) + @($timestampChain.certificates) | Where-Object {
    [string]$_.signatureAlgorithmOid -notin $acceptedSignatureAlgorithms -or
    ([string]$_.publicKeyAlgorithmOid -ceq '1.2.840.113549.1.1.1' -and [int]$_.publicKeySizeBits -lt 2048) -or
    ([string]$_.publicKeyAlgorithmOid -ceq '1.2.840.10045.2.1' -and [int]$_.publicKeySizeBits -lt 256)
})
if ($weakCertificates.Count -ne 0) { throw '[ONLINE_PROVENANCE_WEAK_ALGORITHM] Certificate chain contains a weak or unsupported signature/public-key algorithm.' }

$identityChecks = [ordered]@{
    regularFile = !$msiItem.PSIsContainer
    nonReparse = ![bool]($msiItem.Attributes -band [IO.FileAttributes]::ReparsePoint)
    sizeBytes = [long]$msiItem.Length -eq [long]$expected.SizeBytes
    sha256 = (Get-G04DCSha256 -Path $msiItem.FullName) -ceq [string]$expected.Sha256
    authenticode = [string]$signature.Status -ceq 'Valid'
    signerThumbprint = $signature.SignerCertificate.Thumbprint.ToUpperInvariant() -ceq [string]$expected.SignerThumbprint
    timestampThumbprint = $signature.TimeStamperCertificate.Thumbprint.ToUpperInvariant() -ceq [string]$expected.TimestampSignerThumbprint
    signerLeafDer = [string]$signerChain.certificates[0].derSha256 -ceq $signerLeafDerSha256
    timestampLeafDer = [string]$timestampChain.certificates[0].derSha256 -ceq $timestampLeafDerSha256
    version = [string]$identity.productVersion -ceq [string]$expected.ProductVersion
    architecture = [string]$identity.architecture -ceq [string]$expected.Architecture
    productCode = [string]$identity.productCode -ceq [string]$expected.ProductCode
    upgradeCode = [string]$identity.upgradeCode -ceq [string]$expected.UpgradeCode
    packageCode = [string]$identity.packageCode -ceq [string]$expected.PackageCode
}
$identityFailures = @($identityChecks.GetEnumerator() | Where-Object { !$_.Value } | ForEach-Object { $_.Key })
if ($identityFailures.Count -ne 0) { throw "[MSI_IDENTITY_MISMATCH] Online provenance MSI identity failed: $($identityFailures -join ', ')" }

$signerCertificateRecords = @($signerChain.certificates | ForEach-Object { Get-G04DCCertificateRecord -Certificate $_ -Purpose 'signer' -CertificateDirectory $certificateDirectory })
$timestampCertificateRecords = @($timestampChain.certificates | ForEach-Object { Get-G04DCCertificateRecord -Certificate $_ -Purpose 'timestamp' -CertificateDirectory $certificateDirectory })
$verifiedAtUtc = [DateTime]::UtcNow.ToString('o')
$record = [pscustomobject][ordered]@{
    schemaVersion = 1
    recordType = 'g04d-c13-online-authenticode-verifier'
    verifier = [ordered]@{
        id = $VerifierId
        instanceId = $VerifierInstanceId.ToLowerInvariant()
        vmUuid = $VmUuid.ToLowerInvariant()
        diskUuid = $DiskUuid.ToLowerInvariant()
        verifiedAtUtc = $verifiedAtUtc
        osCaptionSha256 = Get-G04DCSha256Bytes -Bytes ([Text.Encoding]::UTF8.GetBytes([string]$os.Caption))
        osVersion = [string]$os.Version
        osBuild = [string]$os.BuildNumber
        osArchitecture = [string]$os.OSArchitecture
    }
    subject = [ordered]@{
        filename = $expected.FileName
        sizeBytes = [long]$msiItem.Length
        sha256 = Get-G04DCSha256 -Path $msiItem.FullName
        version = [string]$identity.productVersion
        architecture = [string]$identity.architecture
        productCode = [string]$identity.productCode
        upgradeCode = [string]$identity.upgradeCode
        packageCode = [string]$identity.packageCode
    }
    authenticode = [ordered]@{
        signatureDigestAlgorithm = $digestMatch.Groups[1].Value.ToLowerInvariant()
        signatureDigestAlgorithmOid = '2.16.840.1.101.3.4.2.1'
        timestampType = 'embedded-authenticode-timestamp'
        timestampUtc = [string]$fileTrust.timestampUtc
        signerLeafDerSha256 = $signerLeafDerSha256
        signerLeafThumbprint = $signature.SignerCertificate.Thumbprint.ToUpperInvariant()
        timestampLeafDerSha256 = $timestampLeafDerSha256
        timestampLeafThumbprint = $signature.TimeStamperCertificate.Thumbprint.ToUpperInvariant()
        signerChain = $signerCertificateRecords
        timestampChain = $timestampCertificateRecords
        revocation = [ordered]@{
            signerChainExcludeRoot = 'good'
            timestampChainExcludeRoot = 'good'
            evidenceMode = 'fresh-windows-online-chain-results'
        }
    }
    verification = [ordered]@{
        accepted = $true
        signTool = [ordered]@{ accepted = $true; exitCode = $signToolExitCode; targetOs = '2:10.0.26100.0'; targetOsEnforcement = 'exact-verifier-host-build'; allEmbeddedSignatures = $true; timestampRequired = $true; warningCount = 0 }
        winVerifyTrust = [ordered]@{ accepted = [bool]$fileTrust.passed; statusHex = [string]$fileTrust.statusHex; revocationChecks = 'whole-chain-excluding-root'; cacheOnly = $false }
        signerChain = [ordered]@{ accepted = [bool]$signerChain.valid; errorStatusHex = [string]$signerChain.errorStatusHex; errorStatusNames = @($signerChain.errorStatusNames); policyErrorHex = [string]$signerChain.policyErrorHex; policy = 'AUTHENTICODE'; certificateSignaturesValid = [bool]$signerChain.certificateSignaturesValid; purposeEkuValid = [bool]$signerChain.purposeEkuValid; revocationKnownGood = [bool]$signerChain.revocationKnownGood }
        timestampChain = [ordered]@{ accepted = [bool]$timestampChain.valid; errorStatusHex = [string]$timestampChain.errorStatusHex; errorStatusNames = @($timestampChain.errorStatusNames); policyErrorHex = [string]$timestampChain.policyErrorHex; policy = 'AUTHENTICODE_TS'; certificateSignaturesValid = [bool]$timestampChain.certificateSignaturesValid; purposeEkuValid = [bool]$timestampChain.purposeEkuValid; revocationKnownGood = [bool]$timestampChain.revocationKnownGood }
        urlRetrievalTimeoutMilliseconds = $UrlRetrievalTimeoutMilliseconds
        identityChecks = $identityChecks
    }
    tooling = [ordered]@{
        signTool = Get-G04DCToolIdentity -Path $signToolItem.FullName -PathCategory 'injected-windows-sdk-10.0.26100.0-x64'
        provenanceHelper = [ordered]@{ sourceSha256 = Get-G04DCSha256 -Path (Join-Path $PSScriptRoot 'AuthenticodeProvenance.cs'); runtime = '.NET Framework and Windows cryptographic APIs' }
    }
    privacy = [ordered]@{
        canonicalRecordContainsRawConsoleOutput = $false
        canonicalRecordContainsCertificateSubjectText = $false
        canonicalRecordContainsUsernameOrProfilePath = $false
        canonicalRecordContainsPrivateKeyMaterial = $false
    }
}

$recordPath = Join-Path $evidence "online-verifier-$VerifierId.json"
Assert-G04DCOnlineProvenanceRecordModel -Record $record -ExpectedId $VerifierId -ExpectedMsi $expected | Out-Null
Write-G04DCCanonicalJson -Path $recordPath -Value $record -Depth 30
New-G04DCCanonicalArtifactManifest -EvidenceDirectory $evidence | Out-Null
Assert-G04DCCanonicalArtifactManifest -EvidenceDirectory $evidence | Out-Null
Write-Output $recordPath
