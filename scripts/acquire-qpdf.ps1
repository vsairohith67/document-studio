[CmdletBinding(DefaultParameterSetName = 'Acquire')]
param(
    [Parameter(Mandatory = $true, ParameterSetName = 'Acquire')]
    [string]$Destination,

    [Parameter(Mandatory = $true, ParameterSetName = 'Manifest')]
    [string]$ManifestBundleRoot,

    [Parameter(Mandatory = $true, ParameterSetName = 'Manifest')]
    [string]$ManifestOutput
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$ExpectedVersion = '12.3.2'
$ExpectedArchiveName = 'qpdf-12.3.2-msvc64.zip'
$ExpectedArchiveSizeBytes = 24555583
$ExpectedArchiveSha256 = '8941870a604e7c87ed24566b038d46c24ce76616254d2383c578f60c0677f202'
$ExpectedApacheLicenseSha256 = 'dc682b79ff16cdd81c0c6f6b149ddf4ad20a0c8489c0b43caa5f0ced24b70011'
$ReleaseBase = 'https://github.com/qpdf/qpdf/releases/download/v12.3.2'
$AllowedDownloads = @(
    "$ReleaseBase/$ExpectedArchiveName",
    "$ReleaseBase/qpdf-12.3.2.sha256",
    "$ReleaseBase/qpdf-12.3.2.sha256.sigstore"
)

function Resolve-CosignExecutable {
    $command = Get-Command cosign -CommandType Application -ErrorAction SilentlyContinue
    if ($null -eq $command) {
        $wingetAlias = Join-Path $env:LOCALAPPDATA 'Microsoft\WinGet\Links\cosign-windows-amd64.exe'
        if (Test-Path -LiteralPath $wingetAlias -PathType Leaf) {
            return [System.IO.Path]::GetFullPath($wingetAlias)
        }
        throw 'Cosign is not installed. No download was started.'
    }
    return [System.IO.Path]::GetFullPath($command.Source)
}

function Assert-CosignVersion([string]$CosignExecutable) {
    $versionOutput = (& $CosignExecutable version 2>&1 | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or $versionOutput -notmatch '(?m)GitVersion:\s+v?3\.0\.6\b') {
        throw 'Cosign 3.0.6 is required. No download was started.'
    }
}

function Get-RelativePath([string]$BasePath, [string]$ChildPath) {
    $normalizedBase = [System.IO.Path]::GetFullPath($BasePath).TrimEnd('\') + '\'
    $normalizedChild = [System.IO.Path]::GetFullPath($ChildPath)
    if (-not $normalizedChild.StartsWith($normalizedBase, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Path is outside the expected root: $normalizedChild"
    }
    return $normalizedChild.Substring($normalizedBase.Length).Replace('\', '/')
}

function Write-CanonicalQpdfManifest([string]$BundleRoot, [string]$OutputPath) {
    $resolvedBundleRoot = [System.IO.Path]::GetFullPath($BundleRoot)
    if (-not (Test-Path -LiteralPath $resolvedBundleRoot -PathType Container)) {
        throw "Manifest bundle root does not exist: $resolvedBundleRoot"
    }

    $resolvedOutputPath = [System.IO.Path]::GetFullPath($OutputPath)
    $payloadFiles = [System.Collections.Generic.SortedDictionary[string, string]]::new(
        [System.StringComparer]::Ordinal
    )
    foreach ($file in Get-ChildItem -LiteralPath $resolvedBundleRoot -File -Recurse) {
        $relativePath = Get-RelativePath $resolvedBundleRoot $file.FullName
        if ($relativePath -ceq 'qpdf-manifest.json') {
            continue
        }
        if ($payloadFiles.ContainsKey($relativePath)) {
            throw "Manifest payload path is duplicated: $relativePath"
        }
        $payloadFiles.Add($relativePath, $file.FullName)
    }

    $manifestFiles = @(
        foreach ($payloadFile in $payloadFiles.GetEnumerator()) {
            $file = Get-Item -LiteralPath $payloadFile.Value
            [ordered]@{
                path = $payloadFile.Key
                sizeBytes = [int64]$file.Length
                sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
            }
        }
    )
    $manifest = [ordered]@{
        schemaVersion = 1
        dependency = 'qpdf'
        version = $ExpectedVersion
        sourceArchive = $ExpectedArchiveName
        sourceArchiveSizeBytes = [int64]$ExpectedArchiveSizeBytes
        sourceArchiveSha256 = $ExpectedArchiveSha256
        signatureIdentity = 'ejb@ql.org'
        signatureIssuer = 'https://github.com/login/oauth'
        appContainerProfile = 'DocumentStudio.PdfEngine.Qpdf.V1'
        appContainerConfigurationVersion = 1
        appContainerCapabilities = @()
        files = $manifestFiles
    }

    $manifestJson = ConvertTo-Json -InputObject $manifest -Depth 6 -Compress
    if ($manifestJson.Contains("`r") -or $manifestJson.Contains("`n")) {
        throw 'Canonical manifest serialization unexpectedly contained a newline.'
    }
    $utf8 = [System.Text.UTF8Encoding]::new($false, $true)
    $jsonBytes = $utf8.GetBytes($manifestJson)
    $outputBytes = [byte[]]::new($jsonBytes.Length + 1)
    [System.Buffer]::BlockCopy($jsonBytes, 0, $outputBytes, 0, $jsonBytes.Length)
    $outputBytes[$outputBytes.Length - 1] = 0x0a

    $outputParent = Split-Path -Parent $resolvedOutputPath
    if ($outputParent -and -not (Test-Path -LiteralPath $outputParent -PathType Container)) {
        [System.IO.Directory]::CreateDirectory($outputParent) | Out-Null
    }
    [System.IO.File]::WriteAllBytes($resolvedOutputPath, $outputBytes)
}

if ($PSCmdlet.ParameterSetName -eq 'Manifest') {
    Write-CanonicalQpdfManifest -BundleRoot $ManifestBundleRoot -OutputPath $ManifestOutput
    Write-Output "Canonical qpdf manifest written to $([System.IO.Path]::GetFullPath($ManifestOutput))"
    return
}

$resolvedDestination = [System.IO.Path]::GetFullPath($Destination)
if (Test-Path -LiteralPath $resolvedDestination) {
    throw "Destination already exists and will not be overwritten: $resolvedDestination"
}

$destinationParent = Split-Path -Parent $resolvedDestination
$apacheLicenseSource = Join-Path $PSScriptRoot 'licenses\Apache-2.0.txt'
if (-not (Test-Path -LiteralPath $apacheLicenseSource -PathType Leaf) -or
    (Get-FileHash -LiteralPath $apacheLicenseSource -Algorithm SHA256).Hash.ToLowerInvariant() -ne $ExpectedApacheLicenseSha256) {
    throw 'The repository Apache-2.0 redistribution license is missing or altered. No download was started.'
}

$cosignExecutable = Resolve-CosignExecutable
Assert-CosignVersion $cosignExecutable

$temporaryRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$temporaryPath = Join-Path $temporaryRoot ("document-studio-qpdf-acquire-" + [guid]::NewGuid().ToString('N'))
$temporaryPath = [System.IO.Path]::GetFullPath($temporaryPath)
if (-not $temporaryPath.StartsWith($temporaryRoot, [StringComparison]::OrdinalIgnoreCase) -or
    -not (Split-Path -Leaf $temporaryPath).StartsWith('document-studio-qpdf-acquire-', [StringComparison]::Ordinal)) {
    throw 'The private acquisition directory could not be proven safe.'
}

New-Item -ItemType Directory -Path $temporaryPath | Out-Null
try {
    foreach ($download in $AllowedDownloads) {
        $fileName = [System.IO.Path]::GetFileName(([uri]$download).AbsolutePath)
        Invoke-WebRequest -Uri $download -OutFile (Join-Path $temporaryPath $fileName) -UseBasicParsing
    }

    Push-Location $temporaryPath
    try {
        & $cosignExecutable verify-blob 'qpdf-12.3.2.sha256' `
            --bundle 'qpdf-12.3.2.sha256.sigstore' `
            --certificate-identity 'ejb@ql.org' `
            --certificate-oidc-issuer 'https://github.com/login/oauth'
        if ($LASTEXITCODE -ne 0) {
            throw 'The qpdf checksum provenance signature could not be verified.'
        }
    }
    finally {
        Pop-Location
    }

    $archivePath = Join-Path $temporaryPath $ExpectedArchiveName
    $archiveSize = (Get-Item -LiteralPath $archivePath).Length
    if ($archiveSize -ne $ExpectedArchiveSizeBytes) {
        throw "The qpdf archive size did not match the approved value: $archiveSize"
    }
    $archiveHash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($archiveHash -ne $ExpectedArchiveSha256) {
        throw "The qpdf archive SHA-256 did not match the approved value: $archiveHash"
    }

    $checksumLines = @(
        Get-Content -LiteralPath (Join-Path $temporaryPath 'qpdf-12.3.2.sha256') |
            Where-Object { $_ -match [regex]::Escape($ExpectedArchiveName) }
    )
    if ($checksumLines.Count -ne 1 -or $checksumLines[0] -notmatch "^$ExpectedArchiveSha256\s+\*?$([regex]::Escape($ExpectedArchiveName))$") {
        throw 'The signed upstream checksum file did not contain the exact approved archive hash.'
    }

    $expandedPath = Join-Path $temporaryPath 'expanded'
    Expand-Archive -LiteralPath $archivePath -DestinationPath $expandedPath
    $qpdfExecutables = @(Get-ChildItem -LiteralPath $expandedPath -Filter 'qpdf.exe' -File -Recurse)
    if ($qpdfExecutables.Count -ne 1 -or $qpdfExecutables[0].Directory.Name -ne 'bin') {
        throw 'The signed archive did not contain exactly one bin\qpdf.exe.'
    }

    $archiveRoot = Split-Path -Parent $qpdfExecutables[0].Directory.FullName
    $licenseFiles = @(
        Get-ChildItem -LiteralPath $archiveRoot -File -Recurse |
            Where-Object { $_.Name -match '(?i)(license|notice|copying|copyright)' } |
            Sort-Object FullName
    )
    if ($licenseFiles.Count -eq 0) {
        throw 'The signed archive did not contain any identifiable license or notice file.'
    }

    $bundleRoot = Join-Path $temporaryPath 'bundle'
    $bundleBin = Join-Path $bundleRoot 'bin'
    $bundleLicenses = Join-Path $bundleRoot 'licenses'
    $bundleProvenance = Join-Path $bundleRoot 'provenance'
    New-Item -ItemType Directory -Path $bundleBin, $bundleLicenses, $bundleProvenance | Out-Null

    Copy-Item -LiteralPath $qpdfExecutables[0].FullName -Destination $bundleBin
    $runtimeDlls = @(Get-ChildItem -LiteralPath $qpdfExecutables[0].Directory.FullName -Filter '*.dll' -File)
    if ($runtimeDlls.Count -eq 0) {
        throw 'The signed Windows archive did not contain qpdf runtime DLLs.'
    }
    foreach ($runtimeDll in $runtimeDlls) {
        Copy-Item -LiteralPath $runtimeDll.FullName -Destination $bundleBin
    }

    foreach ($licenseFile in $licenseFiles) {
        $licenseRelativePath = Get-RelativePath $archiveRoot $licenseFile.FullName
        $licenseBundleName = $licenseRelativePath.Replace('/', '__')
        $licenseDestination = Join-Path $bundleLicenses $licenseBundleName
        if (Test-Path -LiteralPath $licenseDestination) {
            throw "License destination name collided: $licenseBundleName"
        }
        Copy-Item -LiteralPath $licenseFile.FullName -Destination $licenseDestination
    }
    Copy-Item -LiteralPath $apacheLicenseSource -Destination (Join-Path $bundleLicenses 'Apache-2.0.txt')
    Copy-Item -LiteralPath (Join-Path $temporaryPath 'qpdf-12.3.2.sha256') -Destination $bundleProvenance
    Copy-Item -LiteralPath (Join-Path $temporaryPath 'qpdf-12.3.2.sha256.sigstore') -Destination $bundleProvenance

    $bundledVersion = (& (Join-Path $bundleBin 'qpdf.exe') --version 2>&1 | Out-String).Trim()
    $bundledVersionLines = @($bundledVersion -split '\r?\n' | Where-Object { $_ -ne '' })
    if ($LASTEXITCODE -ne 0 -or
        $bundledVersionLines.Count -ne 2 -or
        $bundledVersionLines[0] -ne 'qpdf version 12.3.2' -or
        $bundledVersionLines[1] -ne 'Run qpdf --copyright to see copyright and license information.') {
        throw "The selected runtime subset could not execute the approved qpdf version: $bundledVersion"
    }

    $manifestPath = Join-Path $bundleRoot 'qpdf-manifest.json'
    Write-CanonicalQpdfManifest -BundleRoot $bundleRoot -OutputPath $manifestPath

    [System.IO.Directory]::CreateDirectory($destinationParent) | Out-Null
    if (Test-Path -LiteralPath $resolvedDestination) {
        throw "Destination appeared during acquisition and will not be overwritten: $resolvedDestination"
    }
    Move-Item -LiteralPath $bundleRoot -Destination $resolvedDestination
    Write-Output "Verified qpdf bundle created at $resolvedDestination"
}
finally {
    if (Test-Path -LiteralPath $temporaryPath) {
        $verifiedTemporaryPath = [System.IO.Path]::GetFullPath($temporaryPath)
        if ($verifiedTemporaryPath.StartsWith($temporaryRoot, [StringComparison]::OrdinalIgnoreCase) -and
            (Split-Path -Leaf $verifiedTemporaryPath).StartsWith('document-studio-qpdf-acquire-', [StringComparison]::Ordinal)) {
            Remove-Item -LiteralPath $verifiedTemporaryPath -Recurse -Force
        }
    }
}
