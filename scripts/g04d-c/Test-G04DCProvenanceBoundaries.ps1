Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'G04DC.Common.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'G04DC.Provenance.psm1') -Force
if (!('DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier' -as [type])) { Add-Type -Path (Join-Path $PSScriptRoot 'AuthenticodeProvenance.cs') -ErrorAction Stop }

$passed = [System.Collections.Generic.List[string]]::new()
function Assert-G04DCProvenanceThrows {
    param([Parameter(Mandatory = $true)] [string]$Name, [Parameter(Mandatory = $true)] [string]$Code, [Parameter(Mandatory = $true)] [scriptblock]$Action)
    try { & $Action; throw "Expected failure did not occur: $Name" }
    catch {
        if ($_.Exception.Message -notmatch [regex]::Escape("[$Code]")) { throw "Wrong failure for $Name`: $($_.Exception.Message)" }
    }
    $passed.Add($Name)
}
function Add-G04DCProvenancePass {
    param([Parameter(Mandatory = $true)] [string]$Name, [Parameter(Mandatory = $true)] [scriptblock]$Action)
    & $Action
    $passed.Add($Name)
}
function Copy-G04DCProvenanceValue {
    param([Parameter(Mandatory = $true)] $Value)
    return ($Value | ConvertTo-Json -Depth 40 | ConvertFrom-Json)
}
function Get-G04DCTestCertificateStoreDigest {
    $rows = [System.Collections.Generic.List[string]]::new()
    foreach ($location in @([Security.Cryptography.X509Certificates.StoreLocation]::CurrentUser, [Security.Cryptography.X509Certificates.StoreLocation]::LocalMachine)) {
        foreach ($name in @('Root', 'CA', 'AuthRoot', 'TrustedPeople', 'My', 'Disallowed')) {
            $store = [Security.Cryptography.X509Certificates.X509Store]::new($name, $location)
            try {
                $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadOnly -bor [Security.Cryptography.X509Certificates.OpenFlags]::OpenExistingOnly)
                foreach ($certificate in @($store.Certificates | Sort-Object Thumbprint)) {
                    $rows.Add(('{0}|{1}|{2}' -f [string]$location, $name, $certificate.Thumbprint.ToUpperInvariant()))
                }
            }
            finally { $store.Close() }
        }
    }
    return Get-G04DCCanonicalHash -Rows @($rows.ToArray())
}
function Set-G04DCTestAuthenticodeBlobByte {
    param([Parameter(Mandatory = $true)] [string]$Path)
    $bytes = [IO.File]::ReadAllBytes($Path)
    if ($bytes.Length -lt 512 -or [BitConverter]::ToUInt16($bytes, 0) -ne 0x5A4D) { throw 'Synthetic PE image is invalid.' }
    $peOffset = [BitConverter]::ToInt32($bytes, 0x3C)
    $optionalOffset = $peOffset + 24
    $magic = [BitConverter]::ToUInt16($bytes, $optionalOffset)
    $directoryOffset = if ($magic -eq 0x20B) { $optionalOffset + 112 } elseif ($magic -eq 0x10B) { $optionalOffset + 96 } else { throw 'Synthetic PE optional header is invalid.' }
    $certificateDirectoryOffset = $directoryOffset + (8 * 4)
    $certificateOffset = [BitConverter]::ToUInt32($bytes, $certificateDirectoryOffset)
    $certificateSize = [BitConverter]::ToUInt32($bytes, $certificateDirectoryOffset + 4)
    if ($certificateOffset -le 0 -or $certificateSize -le 16 -or $certificateOffset + $certificateSize -gt $bytes.Length) { throw 'Synthetic PE Authenticode directory is invalid.' }
    $target = [int]($certificateOffset + 8)
    $bytes[$target] = $bytes[$target] -bxor 1
    [IO.File]::WriteAllBytes($Path, $bytes)
}
function New-G04DCProvenanceCertificateRecord {
    param([int]$Position, [string]$Prefix, [string]$IssuerPrefix, [string]$Eku)
    $hex = (($Prefix * 64).Substring(0, 64)).ToLowerInvariant()
    $thumb = (($Prefix.ToUpperInvariant() * 40).Substring(0, 40))
    return [pscustomobject][ordered]@{
        chainPosition = $Position; path = "certificates/signer-$('{0:D2}' -f $Position)-$hex.cer"; derSha256 = $hex; thumbprint = $thumb
        serialNumber = '01'; subjectNameSha256 = ($Prefix * 64).Substring(0, 64); issuerNameSha256 = ($IssuerPrefix * 64).Substring(0, 64); publicKeyAlgorithmOid = '1.2.840.113549.1.1.1'
        publicKeySizeBits = 3072; signatureAlgorithmOid = '1.2.840.113549.1.1.11'; ekuOids = @($Eku)
        notBeforeUtc = '2026-01-01T00:00:00.0000000Z'; notAfterUtc = '2030-01-01T00:00:00.0000000Z'; selfSignedName = $Prefix -ceq $IssuerPrefix
    }
}
function New-G04DCSyntheticOnlineRecord {
    param([ValidateSet('A', 'B')] [string]$Id)
    $expected = Get-G04DCExpectedMsi
    $signer = @(
        (New-G04DCProvenanceCertificateRecord -Position 0 -Prefix 'a' -IssuerPrefix 'b' -Eku '1.3.6.1.5.5.7.3.3'),
        (New-G04DCProvenanceCertificateRecord -Position 1 -Prefix 'b' -IssuerPrefix 'c' -Eku '2.5.29.37.0'),
        (New-G04DCProvenanceCertificateRecord -Position 2 -Prefix 'c' -IssuerPrefix 'c' -Eku '2.5.29.37.0')
    )
    $timestamp = @(
        (New-G04DCProvenanceCertificateRecord -Position 0 -Prefix 'd' -IssuerPrefix 'e' -Eku '1.3.6.1.5.5.7.3.8'),
        (New-G04DCProvenanceCertificateRecord -Position 1 -Prefix 'e' -IssuerPrefix 'f' -Eku '2.5.29.37.0'),
        (New-G04DCProvenanceCertificateRecord -Position 2 -Prefix 'f' -IssuerPrefix 'f' -Eku '2.5.29.37.0')
    )
    foreach ($entry in $timestamp) { $entry.path = $entry.path.Replace('signer-', 'timestamp-') }
    $expected = Get-G04DCExpectedMsi
    $signer[0].thumbprint = $expected.SignerThumbprint
    $timestamp[0].thumbprint = $expected.TimestampSignerThumbprint
    $offset = if ($Id -ceq 'A') { 0 } else { 1 }
    return [pscustomobject][ordered]@{
        schemaVersion = 1; recordType = 'g04d-c13-online-authenticode-verifier'
        verifier = [ordered]@{ id = $Id; instanceId = ([guid]::NewGuid().ToString()); vmUuid = ([guid]::NewGuid().ToString()); diskUuid = ([guid]::NewGuid().ToString()); verifiedAtUtc = [DateTime]::UtcNow.AddMinutes($offset).ToString('o'); osCaptionSha256 = ('0' * 64); osVersion = '10.0.26100'; osBuild = '26100'; osArchitecture = '64-bit' }
        subject = [ordered]@{ filename = $expected.FileName; sizeBytes = [long]$expected.SizeBytes; sha256 = $expected.Sha256; version = $expected.ProductVersion; architecture = $expected.Architecture; productCode = $expected.ProductCode; upgradeCode = $expected.UpgradeCode; packageCode = $expected.PackageCode }
        authenticode = [ordered]@{ signatureDigestAlgorithm = 'sha256'; signatureDigestAlgorithmOid = '2.16.840.1.101.3.4.2.1'; timestampType = 'embedded-authenticode-timestamp'; timestampUtc = '2026-07-23T13:18:03.0000000Z'; signerLeafDerSha256 = ('a' * 64); signerLeafThumbprint = $expected.SignerThumbprint; timestampLeafDerSha256 = ('d' * 64); timestampLeafThumbprint = $expected.TimestampSignerThumbprint; signerChain = $signer; timestampChain = $timestamp; revocation = [ordered]@{ signerChainExcludeRoot = 'good'; timestampChainExcludeRoot = 'good'; evidenceMode = 'fresh-windows-online-chain-results' } }
        verification = [ordered]@{
            accepted = $true
            signTool = [ordered]@{ accepted = $true; exitCode = 0; targetOs = '2:10.0.26100.0'; targetOsEnforcement = 'exact-verifier-host-build'; allEmbeddedSignatures = $true; timestampRequired = $true; warningCount = 0 }
            winVerifyTrust = [ordered]@{ accepted = $true; statusHex = '0x00000000'; revocationChecks = 'whole-chain-excluding-root'; cacheOnly = $false }
            signerChain = [ordered]@{ accepted = $true; errorStatusHex = '0x00000000'; errorStatusNames = @(); policyErrorHex = '0x00000000'; policy = 'AUTHENTICODE'; certificateSignaturesValid = $true; purposeEkuValid = $true; revocationKnownGood = $true }
            timestampChain = [ordered]@{ accepted = $true; errorStatusHex = '0x00000000'; errorStatusNames = @(); policyErrorHex = '0x00000000'; policy = 'AUTHENTICODE_TS'; certificateSignaturesValid = $true; purposeEkuValid = $true; revocationKnownGood = $true }
            urlRetrievalTimeoutMilliseconds = 15000
            identityChecks = [ordered]@{ regularFile = $true; nonReparse = $true; sizeBytes = $true; sha256 = $true; authenticode = $true; signerThumbprint = $true; timestampThumbprint = $true; signerLeafDer = $true; timestampLeafDer = $true; version = $true; architecture = $true; productCode = $true; upgradeCode = $true; packageCode = $true }
        }
        tooling = [ordered]@{ signTool = [ordered]@{ pathCategory = 'injected-windows-sdk-10.0.26100.0-x64'; sizeBytes = 1; sha256 = ('3' * 64); fileVersion = '1'; productVersion = '1'; signerLeafThumbprint = ('4' * 40); signerLeafDerSha256 = ('5' * 64) }; provenanceHelper = [ordered]@{ sourceSha256 = ('6' * 64); runtime = '.NET Framework and Windows cryptographic APIs' } }
        privacy = [ordered]@{ canonicalRecordContainsRawConsoleOutput = $false; canonicalRecordContainsCertificateSubjectText = $false; canonicalRecordContainsUsernameOrProfilePath = $false; canonicalRecordContainsPrivateKeyMaterial = $false }
    }
}

$rawRecordA = New-G04DCSyntheticOnlineRecord -Id 'A'
$recordA = Copy-G04DCProvenanceValue $rawRecordA
$recordB = Copy-G04DCProvenanceValue (New-G04DCSyntheticOnlineRecord -Id 'B')
$expectedMsi = Get-G04DCExpectedMsi

Add-G04DCProvenancePass 'online complete good signer chain' { Assert-G04DCOnlineProvenanceRecordModel -Record $rawRecordA -ExpectedId 'A' -ExpectedMsi $expectedMsi | Out-Null }
Add-G04DCProvenancePass 'online complete good timestamp chain' { Assert-G04DCOnlineProvenanceRecordModel -Record $recordB -ExpectedId 'B' -ExpectedMsi $expectedMsi | Out-Null }
$case = Copy-G04DCProvenanceValue $recordA; $case.verification.signerChain.accepted = $false; $case.verification.signerChain.errorStatusNames = @('Revoked')
Assert-G04DCProvenanceThrows 'online revoked signer rejected' 'ONLINE_PROVENANCE_RECORD_INVALID' { Assert-G04DCOnlineProvenanceRecordModel $case 'A' $expectedMsi }
$case = Copy-G04DCProvenanceValue $recordA; $case.verification.timestampChain.accepted = $false; $case.verification.timestampChain.errorStatusNames = @('Revoked')
Assert-G04DCProvenanceThrows 'online revoked timestamp signer rejected' 'ONLINE_PROVENANCE_RECORD_INVALID' { Assert-G04DCOnlineProvenanceRecordModel $case 'A' $expectedMsi }
$case = Copy-G04DCProvenanceValue $recordA; $case.verification.signerChain.revocationKnownGood = $false; $case.verification.signerChain.errorStatusNames = @('RevocationStatusUnknown')
Assert-G04DCProvenanceThrows 'online revocation unknown rejected' 'ONLINE_PROVENANCE_RECORD_INVALID' { Assert-G04DCOnlineProvenanceRecordModel $case 'A' $expectedMsi }
$case = Copy-G04DCProvenanceValue $recordA; $case.verification.signerChain.revocationKnownGood = $false; $case.verification.signerChain.errorStatusNames = @('OfflineRevocation')
Assert-G04DCProvenanceThrows 'online offline revocation rejected' 'ONLINE_PROVENANCE_RECORD_INVALID' { Assert-G04DCOnlineProvenanceRecordModel $case 'A' $expectedMsi }
$case = Copy-G04DCProvenanceValue $recordA; $case.verification.signerChain.accepted = $false; $case.verification.signerChain.errorStatusNames = @('PartialChain')
Assert-G04DCProvenanceThrows 'online partial chain rejected' 'ONLINE_PROVENANCE_RECORD_INVALID' { Assert-G04DCOnlineProvenanceRecordModel $case 'A' $expectedMsi }
$case = Copy-G04DCProvenanceValue $recordA; $case.verification.signerChain.accepted = $false; $case.verification.signerChain.errorStatusNames = @('UntrustedRoot')
Assert-G04DCProvenanceThrows 'online untrusted root rejected' 'ONLINE_PROVENANCE_RECORD_INVALID' { Assert-G04DCOnlineProvenanceRecordModel $case 'A' $expectedMsi }
$expiredNowRecord = Copy-G04DCProvenanceValue $recordA; $expiredNowRecord.authenticode.signerChain[0].notAfterUtc = '2026-08-01T00:00:00.0000000Z'
Add-G04DCProvenancePass 'online expired signer with valid timestamp accepted by policy result' { Assert-G04DCOnlineProvenanceRecordModel $expiredNowRecord 'A' $expectedMsi | Out-Null }
$case = Copy-G04DCProvenanceValue $recordA; $case.verification.signTool.timestampRequired = $false
Assert-G04DCProvenanceThrows 'online expired signer without valid timestamp rejected' 'ONLINE_PROVENANCE_RECORD_INVALID' { Assert-G04DCOnlineProvenanceRecordModel $case 'A' $expectedMsi }
$case = Copy-G04DCProvenanceValue $recordA; $case.authenticode.signerChain[0].ekuOids = @('1.3.6.1.5.5.7.3.1')
Assert-G04DCProvenanceThrows 'online wrong EKU rejected' 'ONLINE_PROVENANCE_RECORD_INVALID' { Assert-G04DCOnlineProvenanceRecordModel $case 'A' $expectedMsi }
$case = Copy-G04DCProvenanceValue $recordA; $case.verification.signTool.targetOs = '2:10.0.22621.0'
Assert-G04DCProvenanceThrows 'online wrong target OS policy rejected' 'ONLINE_PROVENANCE_RECORD_INVALID' { Assert-G04DCOnlineProvenanceRecordModel $case 'A' $expectedMsi }
$case = Copy-G04DCProvenanceValue $recordA; $case.verification.signTool.accepted = $false; $case.verification.signTool.exitCode = 1
Assert-G04DCProvenanceThrows 'online verifier tool failure rejected' 'ONLINE_PROVENANCE_RECORD_INVALID' { Assert-G04DCOnlineProvenanceRecordModel $case 'A' $expectedMsi }
$case = Copy-G04DCProvenanceValue $recordA; $case.verification.urlRetrievalTimeoutMilliseconds = 120001
Assert-G04DCProvenanceThrows 'online retrieval timeout rejected' 'ONLINE_PROVENANCE_RECORD_INVALID' { Assert-G04DCOnlineProvenanceRecordModel $case 'A' $expectedMsi }
$case = Copy-G04DCProvenanceValue $recordA; $case.privacy.canonicalRecordContainsRawConsoleOutput = $true
Assert-G04DCProvenanceThrows 'online canonical output excludes arbitrary logs' 'ONLINE_PROVENANCE_RECORD_INVALID' { Assert-G04DCOnlineProvenanceRecordModel $case 'A' $expectedMsi }

Add-G04DCProvenancePass 'two verifier exact agreement' { Compare-G04DCOnlineProvenanceRecordModels $recordA $recordB | Out-Null }
$case = Copy-G04DCProvenanceValue $recordB; $case.subject.sha256 = ('9' * 64)
Assert-G04DCProvenanceThrows 'two verifier MSI hash disagreement' 'ONLINE_PROVENANCE_VERIFIERS_DISAGREE' { Compare-G04DCOnlineProvenanceRecordModels $recordA $case }
$case = Copy-G04DCProvenanceValue $recordB; $case.authenticode.signerLeafDerSha256 = ('9' * 64)
Assert-G04DCProvenanceThrows 'two verifier signer disagreement' 'ONLINE_PROVENANCE_VERIFIERS_DISAGREE' { Compare-G04DCOnlineProvenanceRecordModels $recordA $case }
$case = Copy-G04DCProvenanceValue $recordB; $case.authenticode.timestampLeafDerSha256 = ('9' * 64)
Assert-G04DCProvenanceThrows 'two verifier timestamp disagreement' 'ONLINE_PROVENANCE_VERIFIERS_DISAGREE' { Compare-G04DCOnlineProvenanceRecordModels $recordA $case }
$case = Copy-G04DCProvenanceValue $recordB; $case.authenticode.signerChain[1].derSha256 = ('9' * 64)
Assert-G04DCProvenanceThrows 'two verifier intermediate disagreement' 'ONLINE_PROVENANCE_VERIFIERS_DISAGREE' { Compare-G04DCOnlineProvenanceRecordModels $recordA $case }
$case = Copy-G04DCProvenanceValue $recordB; $case.authenticode.signerChain[2].derSha256 = ('9' * 64)
Assert-G04DCProvenanceThrows 'two verifier root disagreement' 'ONLINE_PROVENANCE_VERIFIERS_DISAGREE' { Compare-G04DCOnlineProvenanceRecordModels $recordA $case }
$case = Copy-G04DCProvenanceValue $recordB; $case.authenticode.revocation.signerChainExcludeRoot = 'unknown'
Assert-G04DCProvenanceThrows 'two verifier revocation disagreement' 'ONLINE_PROVENANCE_VERIFIERS_DISAGREE' { Compare-G04DCOnlineProvenanceRecordModels $recordA $case }
$case = Copy-G04DCProvenanceValue $recordB; $case.verifier.verifiedAtUtc = [DateTime]::UtcNow.AddMinutes(31).ToString('o')
Assert-G04DCProvenanceThrows 'two verifier time-window violation' 'ONLINE_PROVENANCE_TIME_WINDOW_INVALID' { Compare-G04DCOnlineProvenanceRecordModels $recordA $case }
Assert-G04DCProvenanceThrows 'two verifier one verifier missing' 'ONLINE_PROVENANCE_RECORD_INVALID' { Assert-G04DCProvenanceExactProperties -Value ([pscustomobject]@{}) -Names @('schemaVersion') }

$testRoot = Join-Path $env:TEMP ('g04dc13-provenance-tests-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testRoot | Out-Null
try {
    $rsa = [Security.Cryptography.RSACryptoServiceProvider]::new(3072); $rsa.PersistKeyInCsp = $false
    $payload = [Text.Encoding]::UTF8.GetBytes('{"synthetic":true}')
    $signature = $rsa.SignData($payload, 'SHA256')
    Add-G04DCProvenancePass 'attestation valid detached signature' { if (!$rsa.VerifyData($payload, 'SHA256', $signature)) { throw 'signature invalid' } }
    $altered = [byte[]]$payload.Clone(); $altered[0] = $altered[0] -bxor 1
    Add-G04DCProvenancePass 'attestation altered JSON rejected' { if ($rsa.VerifyData($altered, 'SHA256', $signature)) { throw 'altered JSON accepted' } }
    $alteredSignature = [byte[]]$signature.Clone(); $alteredSignature[0] = $alteredSignature[0] -bxor 1
    Add-G04DCProvenancePass 'attestation altered signature rejected' { if ($rsa.VerifyData($payload, 'SHA256', $alteredSignature)) { throw 'altered signature accepted' } }
    $other = [Security.Cryptography.RSACryptoServiceProvider]::new(3072); $other.PersistKeyInCsp = $false
    try { Add-G04DCProvenancePass 'attestation altered public key rejected' { if ($other.VerifyData($payload, 'SHA256', $signature)) { throw 'altered key accepted' } } }
    finally { $other.Clear(); $other.Dispose() }
    $now = [DateTime]::UtcNow
    Assert-G04DCProvenanceThrows 'attestation expired rejected' 'ATTESTATION_EXPIRED' { Assert-G04DCProvenanceFreshnessModel $now $now.AddHours(-13) $now.AddMinutes(-1) $now.AddHours(-13) $now.AddHours(-12) }
    Assert-G04DCProvenanceThrows 'attestation future dated rejected' 'ATTESTATION_FUTURE_DATED' { Assert-G04DCProvenanceFreshnessModel $now $now.AddMinutes(6) $now.AddHours(1) $now.AddMinutes(6) $now.AddMinutes(7) }
    $missing = [pscustomobject][ordered]@{ schemaVersion = 1 }
    Add-G04DCProvenancePass 'attestation duplicate or missing fields rejected' {
        $missingRejected = $false
        try { Assert-G04DCProvenanceExactProperties $missing @('schemaVersion', 'subject') 'ATTESTATION_SCHEMA_INVALID' | Out-Null }
        catch { $missingRejected = $_.Exception.Message -match '\[ATTESTATION_SCHEMA_INVALID\]' }
        $duplicatePath = Join-Path $testRoot 'duplicate.json'
        [IO.File]::WriteAllText($duplicatePath, '{"schemaVersion":1,"schemaVersion":1}' + "`n", [Text.UTF8Encoding]::new($false, $true))
        $duplicateRejected = $false
        try { Read-G04DCCanonicalJson -Path $duplicatePath -Code 'ATTESTATION_SCHEMA_INVALID' | Out-Null }
        catch { $duplicateRejected = $_.Exception.Message -match '\[ATTESTATION_SCHEMA_INVALID\]' }
        if (!$missingRejected -or !$duplicateRejected) { throw 'missing or duplicate field accepted' }
    }
    $extra = [pscustomobject][ordered]@{ schemaVersion = 1; subject = 'x'; extra = 'x' }
    Assert-G04DCProvenanceThrows 'attestation extra fields rejected' 'ATTESTATION_SCHEMA_INVALID' { Assert-G04DCProvenanceExactProperties $extra @('schemaVersion', 'subject') 'ATTESTATION_SCHEMA_INVALID' }
    $jsonA = Join-Path $testRoot 'a.json'; $jsonB = Join-Path $testRoot 'b.json'; $ordered = [ordered]@{ a = 1; b = @('x', 'y') }
    Write-G04DCCanonicalJson $jsonA $ordered; Write-G04DCCanonicalJson $jsonB $ordered
    Add-G04DCProvenancePass 'attestation canonical JSON determinism' { if ((Get-G04DCSha256 $jsonA) -cne (Get-G04DCSha256 $jsonB)) { throw 'JSON not deterministic' } }
    Add-G04DCProvenancePass 'attestation BOM-free UTF-8' { $bytes = [IO.File]::ReadAllBytes($jsonA); if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) { throw 'BOM present' } }
    $manifestRoot = Join-Path $testRoot 'manifest'; New-Item -ItemType Directory -Path $manifestRoot | Out-Null; [IO.File]::WriteAllText((Join-Path $manifestRoot 'owned.txt'), 'owned')
    New-G04DCCanonicalArtifactManifest $manifestRoot | Out-Null; Assert-G04DCCanonicalArtifactManifest $manifestRoot | Out-Null; [IO.File]::WriteAllText((Join-Path $manifestRoot 'extra.txt'), 'extra')
    Assert-G04DCProvenanceThrows 'attestation manifest completeness' 'ARTIFACT_MANIFEST_INVALID' { Assert-G04DCCanonicalArtifactManifest $manifestRoot }

    $rootKey = [Security.Cryptography.RSA]::Create(3072); $rootRequest = [Security.Cryptography.X509Certificates.CertificateRequest]::new('CN=G04DC13 Synthetic Root', $rootKey, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)
    $rootRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509BasicConstraintsExtension]::new($true, $false, 0, $true))
    $rootRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509KeyUsageExtension]::new([Security.Cryptography.X509Certificates.X509KeyUsageFlags]::KeyCertSign, $true))
    $root = $rootRequest.CreateSelfSigned($now.AddDays(-1), $now.AddDays(30))
    $intermediateKey = [Security.Cryptography.RSA]::Create(3072); $intermediateRequest = [Security.Cryptography.X509Certificates.CertificateRequest]::new('CN=G04DC13 Synthetic Intermediate', $intermediateKey, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)
    $intermediateRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509BasicConstraintsExtension]::new($true, $false, 0, $true))
    $intermediateRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509KeyUsageExtension]::new([Security.Cryptography.X509Certificates.X509KeyUsageFlags]::KeyCertSign, $true))
    $intermediatePublic = $intermediateRequest.Create($root, $now.AddHours(-12), $now.AddDays(20), [byte[]](1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16))
    $intermediate = [Security.Cryptography.X509Certificates.RSACertificateExtensions]::CopyWithPrivateKey($intermediatePublic, $intermediateKey)
    $leafKey = [Security.Cryptography.RSA]::Create(3072); $leafRequest = [Security.Cryptography.X509Certificates.CertificateRequest]::new('CN=G04DC13 Synthetic Signer', $leafKey, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)
    $leafRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509BasicConstraintsExtension]::new($false, $false, 0, $true))
    $leafRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509KeyUsageExtension]::new([Security.Cryptography.X509Certificates.X509KeyUsageFlags]::DigitalSignature, $true))
    $signerEku = [Security.Cryptography.OidCollection]::new(); [void]$signerEku.Add([Security.Cryptography.Oid]::new('1.3.6.1.5.5.7.3.3'))
    $leafRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension]::new($signerEku, $true))
    $leaf = $leafRequest.Create($intermediate, $now.AddHours(-6), $now.AddDays(10), [byte[]](16,15,14,13,12,11,10,9,8,7,6,5,4,3,2,1))
    $chainBytes = [byte[][]]@($leaf.RawData, $intermediate.RawData, $root.RawData)
    $testStoreBefore = Get-G04DCTestCertificateStoreDigest
    $structural = [DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::BuildOfflineExclusiveChain($chainBytes, 'signer', $now.ToString('o'))
    $testStoreAfter = Get-G04DCTestCertificateStoreDigest

    $compilerSource = 'public static class G04DC13SyntheticSignedFile { public static void Main() { } }'
    $syntheticExe = Join-Path $testRoot 'synthetic-signed.exe'
    Add-Type -TypeDefinition $compilerSource -Language CSharp -OutputAssembly $syntheticExe -OutputType ConsoleApplication
    $pfxCertificate = [Security.Cryptography.X509Certificates.RSACertificateExtensions]::CopyWithPrivateKey($leaf, $leafKey)
    $pfxPath = Join-Path $testRoot 'synthetic.pfx'; $pfxPassword = 'G04DC13-Synthetic-Only'
    [IO.File]::WriteAllBytes($pfxPath, $pfxCertificate.Export([Security.Cryptography.X509Certificates.X509ContentType]::Pfx, $pfxPassword))
    $signTool = @(Get-ChildItem -LiteralPath 'C:\Program Files (x86)\Windows Kits\10\bin' -Filter signtool.exe -Recurse -ErrorAction SilentlyContinue | Where-Object { $_.FullName -match '\\x64\\signtool\.exe$' } | Sort-Object FullName -Descending | Select-Object -First 1)
    if ($signTool.Count -ne 1) { throw 'Synthetic Authenticode test requires the installed Windows SDK SignTool.' }
    & $signTool[0].FullName sign /f $pfxPath /p $pfxPassword /fd SHA256 $syntheticExe | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Synthetic Authenticode signing failed.' }
    $fileTrust = [DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::VerifyOfflineFileDigestAndSignature($syntheticExe)
    Add-G04DCProvenancePass 'offline valid embedded file signature' { if (!$fileTrust.passed -or $fileTrust.signerCount -ne 1) { throw 'synthetic signature invalid' } }
    $alteredExe = Join-Path $testRoot 'synthetic-altered.exe'; Copy-Item -LiteralPath $syntheticExe -Destination $alteredExe; $stream = [IO.File]::Open($alteredExe, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None); try { [void]$stream.Seek(1024, [IO.SeekOrigin]::Begin); $originalByte = $stream.ReadByte(); [void]$stream.Seek(1024, [IO.SeekOrigin]::Begin); $stream.WriteByte([byte]($originalByte -bxor 1)) } finally { $stream.Dispose() }
    Add-G04DCProvenancePass 'offline altered signed file rejected' { if ([DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::VerifyOfflineFileDigestAndSignature($alteredExe).passed) { throw 'altered file accepted' } }
    $alteredPkcs7Exe = Join-Path $testRoot 'synthetic-altered-pkcs7.exe'; Copy-Item -LiteralPath $syntheticExe -Destination $alteredPkcs7Exe; Set-G04DCTestAuthenticodeBlobByte -Path $alteredPkcs7Exe
    Add-G04DCProvenancePass 'offline altered PKCS7 rejected' { if ([DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::VerifyOfflineFileDigestAndSignature($alteredPkcs7Exe).passed) { throw 'altered PKCS7 accepted' } }
    Add-G04DCProvenancePass 'offline wrong file digest rejected' { if ([DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::VerifyOfflineFileDigestAndSignature($alteredExe).passed) { throw 'wrong digest accepted' } }
    $embeddedModel = [pscustomobject][ordered]@{ passed = $true; hashOnly = $true; cacheOnlyUrlRetrieval = $true; signerCount = 1; timestampSignerCount = 1; providerSignerError = 0; providerTimestampError = 0; signerLeafDerSha256 = $recordA.authenticode.signerLeafDerSha256; timestampLeafDerSha256 = $recordA.authenticode.timestampLeafDerSha256; timestampUtc = $recordA.authenticode.timestampUtc }
    $wrongEmbedded = Copy-G04DCProvenanceValue $embeddedModel; $wrongEmbedded.signerLeafDerSha256 = ('0' * 64)
    Assert-G04DCProvenanceThrows 'offline wrong signer leaf rejected' 'OFFLINE_PROVENANCE_SIGNATURE_INVALID' { Assert-G04DCOfflineEmbeddedIdentityModel $wrongEmbedded $recordA }
    $wrongEmbedded = Copy-G04DCProvenanceValue $embeddedModel; $wrongEmbedded.timestampLeafDerSha256 = ('0' * 64)
    Assert-G04DCProvenanceThrows 'offline wrong timestamp leaf rejected' 'OFFLINE_PROVENANCE_SIGNATURE_INVALID' { Assert-G04DCOfflineEmbeddedIdentityModel $wrongEmbedded $recordA }
    Add-G04DCProvenancePass 'offline missing intermediate rejected' { $r = [DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::BuildOfflineExclusiveChain([byte[][]]@($leaf.RawData, $root.RawData), 'signer', $now.ToString('o')); if ($r.valid) { throw 'missing intermediate accepted' } }
    $otherIntermediateKey = [Security.Cryptography.RSA]::Create(3072); $otherIntermediateRequest = [Security.Cryptography.X509Certificates.CertificateRequest]::new('CN=Other Intermediate', $otherIntermediateKey, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1); $otherIntermediateRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509BasicConstraintsExtension]::new($true, $false, 0, $true)); $otherIntermediate = $otherIntermediateRequest.Create($root, $now.AddHours(-1), $now.AddDays(5), [byte[]](2,3,4,5,6,7,8,9))
    Add-G04DCProvenancePass 'offline substituted intermediate rejected' { $r = [DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::BuildOfflineExclusiveChain([byte[][]]@($leaf.RawData, $otherIntermediate.RawData, $root.RawData), 'signer', $now.ToString('o')); if ($r.valid) { throw 'substituted intermediate accepted' } }
    $otherRootKey = [Security.Cryptography.RSA]::Create(3072); $otherRootRequest = [Security.Cryptography.X509Certificates.CertificateRequest]::new('CN=Other Root', $otherRootKey, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1); $otherRootRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509BasicConstraintsExtension]::new($true, $false, 0, $true)); $otherRoot = $otherRootRequest.CreateSelfSigned($now.AddHours(-1), $now.AddDays(5))
    Add-G04DCProvenancePass 'offline substituted root rejected' { $r = [DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::BuildOfflineExclusiveChain([byte[][]]@($leaf.RawData, $intermediate.RawData, $otherRoot.RawData), 'signer', $now.ToString('o')); if ($r.valid) { throw 'substituted root accepted' } }
    Add-G04DCProvenancePass 'offline chain order mismatch rejected' { $r = [DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::BuildOfflineExclusiveChain([byte[][]]@($leaf.RawData, $root.RawData, $intermediate.RawData), 'signer', $now.ToString('o')); if ($r.valid) { throw 'wrong order accepted' } }
    Add-G04DCProvenancePass 'offline invalid certificate signature rejected' { $r = [DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::BuildOfflineExclusiveChain([byte[][]]@($leaf.RawData, $otherIntermediate.RawData, $root.RawData), 'signer', $now.ToString('o')); if ($r.valid) { throw 'invalid certificate signature accepted' } }
    Add-G04DCProvenancePass 'offline wrong EKU rejected' { $r = [DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::BuildOfflineExclusiveChain($chainBytes, 'timestamp', $now.ToString('o')); if ($r.valid) { throw 'wrong EKU accepted' } }
    Add-G04DCProvenancePass 'offline certificate-time failure rejected' { $r = [DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::BuildOfflineExclusiveChain($chainBytes, 'signer', $now.AddYears(2).ToString('o')); if ($r.valid) { throw 'expired certificate accepted' } }
    $wrongEmbedded = Copy-G04DCProvenanceValue $embeddedModel; $wrongEmbedded.signerCount = 2
    Assert-G04DCProvenanceThrows 'offline additional unexpected signer rejected' 'OFFLINE_PROVENANCE_SIGNATURE_INVALID' { Assert-G04DCOfflineEmbeddedIdentityModel $wrongEmbedded $recordA }
    Add-G04DCProvenancePass 'offline no network retrieval' {
        $offlineVerifierSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'Test-G04DCOfflineProvenanceAttestation.ps1') -Raw
        if (!$structural.urlRetrievalDisabled -or !$structural.exclusiveRoot -or $offlineVerifierSource -match 'Get-NetRoute\s+-DestinationPrefix') {
            throw 'offline retrieval or disconnected-route handling is invalid'
        }
    }
    Add-G04DCProvenancePass 'offline global store remains unchanged' { if (!$structural.valid -or $testStoreBefore -cne $testStoreAfter) { throw 'exclusive chain changed a global certificate store' } }
    Add-G04DCProvenancePass 'offline diagnostic PartialChain is not trust evidence' { if ((Get-Content -LiteralPath (Join-Path $PSScriptRoot 'Test-G04DCOfflineProvenanceAttestation.ps1') -Raw) -notmatch 'diagnosticDefaultChainAcceptedAsTrust = \$false') { throw 'partial chain boundary missing' } }
    Add-G04DCProvenancePass 'offline diagnostic OfflineRevocation is not trust evidence' { if ((Get-Content -LiteralPath (Join-Path $PSScriptRoot 'Test-G04DCOfflineProvenanceAttestation.ps1') -Raw) -notmatch 'diagnosticOfflineRevocationAcceptedAsTrust = \$false') { throw 'offline revocation boundary missing' } }
    Add-G04DCProvenancePass 'offline online attestation cannot hide structural failure' { $r = [DocumentStudio.G04DC.Provenance.AuthenticodeProvenanceVerifier]::BuildOfflineExclusiveChain([byte[][]]@($leaf.RawData, $otherIntermediate.RawData, $root.RawData), 'signer', $now.ToString('o')); if ($r.valid) { throw 'structural failure hidden' } }

    $onlineSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'New-G04DCOnlineProvenanceRecord.ps1') -Raw
    $combinedSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'New-G04DCCombinedProvenanceAttestation.ps1') -Raw
    $offlineSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'Test-G04DCOfflineProvenanceAttestation.ps1') -Raw
    Add-G04DCProvenancePass 'privacy no raw console logs in canonical record' { if ($onlineSource -notmatch 'canonicalRecordContainsRawConsoleOutput = \$false' -or $combinedSource -match 'signtool-sanitized\.log') { throw 'raw log privacy boundary missing' } }
    Add-G04DCProvenancePass 'privacy no private-key material in guest package' { if ($offlineSource -notmatch 'ATTESTATION_PRIVATE_KEY_LEAK') { throw 'private key boundary missing' } }
    Add-G04DCProvenancePass 'privacy no username or profile path' { if ($onlineSource -notmatch 'canonicalRecordContainsUsernameOrProfilePath = \$false') { throw 'profile privacy boundary missing' } }
    Add-G04DCProvenancePass 'privacy no arbitrary certificate subject text' { if ($onlineSource -notmatch 'canonicalRecordContainsCertificateSubjectText = \$false') { throw 'certificate privacy boundary missing' } }
    Add-G04DCProvenancePass 'privacy no exception stack' { if ($onlineSource -match 'ScriptStackTrace|StackTrace') { throw 'exception stack retained' } }
    Add-G04DCProvenancePass 'privacy no secret or user document' { if ($onlineSource -match '(?i)password|user document|Documents\\') { throw 'secret or user-document path in canonical record' } }
}
finally {
    if ($rsa) { $rsa.Clear(); $rsa.Dispose() }
    if (Test-Path -LiteralPath $testRoot) { Remove-Item -LiteralPath $testRoot -Recurse -Force }
}

if ($passed.Count -ne 60) { throw "Expected 60 G04D-C13 provenance cases; passed $($passed.Count)." }
Write-Output "G04D-C13 provenance boundary tests passed ($($passed.Count) cases): $($passed -join '; ')"
