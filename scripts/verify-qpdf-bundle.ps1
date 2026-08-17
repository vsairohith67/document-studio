[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$BundleRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-RelativePath([string]$BasePath, [string]$ChildPath) {
    $normalizedBase = [System.IO.Path]::GetFullPath($BasePath).TrimEnd('\') + '\'
    $normalizedChild = [System.IO.Path]::GetFullPath($ChildPath)
    if (-not $normalizedChild.StartsWith($normalizedBase, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Path is outside the expected root: $normalizedChild"
    }
    return $normalizedChild.Substring($normalizedBase.Length).Replace('\', '/')
}

$ExpectedVersion = '12.3.2'
$ExpectedArchiveSizeBytes = 24555583
$ExpectedArchiveSha256 = '8941870a604e7c87ed24566b038d46c24ce76616254d2383c578f60c0677f202'
$ExpectedFiles = [ordered]@{
    'bin/concrt140.dll' = @{ sizeBytes = 324208; sha256 = '2405355f0a58067b258f8df33c327e3a3d716eaac5a3a5aebb757842d85bd376' }
    'bin/msvcp140.dll' = @{ sizeBytes = 557728; sha256 = '0f885b509a685d2bbfa652fed26b5fb31d88fbdab0a978c641d1c7b8aa460aa9' }
    'bin/msvcp140_1.dll' = @{ sizeBytes = 35952; sha256 = 'bfad5aef4c63a669e3c140655cdfdf395b6c979b400a447bd5dcb65ed8826c3d' }
    'bin/msvcp140_2.dll' = @{ sizeBytes = 280200; sha256 = '3ea06f0ee098b4823cb79599df3780e7f23cce52c19aac31d2a0d47efe33a5e9' }
    'bin/msvcp140_atomic_wait.dll' = @{ sizeBytes = 50304; sha256 = '640b2aefced484d0368eea5bdd06addd0658a3a70a49256e560d6923b404a479' }
    'bin/msvcp140_codecvt_ids.dll' = @{ sizeBytes = 31872; sha256 = 'f2069a52880ec885ee7f0511186100eb7fada0411a2b4948fafea7735b878a18' }
    'bin/qpdf.exe' = @{ sizeBytes = 19968; sha256 = '43f79db620ce09529a67572a5de87aec4065b95f11ba6e5918db557f943a7eac' }
    'bin/qpdf30.dll' = @{ sizeBytes = 7069184; sha256 = '623338ff5a9caab476f9e80ccc40c28c194208f4bc5d8e51eac7fca792e2e969' }
    'bin/vcruntime140.dll' = @{ sizeBytes = 124544; sha256 = 'd5e4d9a3e835fa679450145d6a7d94e36573a509317111904d9b3712c30d9066' }
    'bin/vcruntime140_1.dll' = @{ sizeBytes = 49792; sha256 = '1f2d41c4aa5db0bc33ebf7b66d72943a817d7ce6cbe880502a9403823633093f' }
    'licenses/Apache-2.0.txt' = @{ sizeBytes = 11357; sha256 = 'dc682b79ff16cdd81c0c6f6b149ddf4ad20a0c8489c0b43caa5f0ced24b70011' }
    'licenses/share__doc__qpdf__manual-html___sources__license.rst.txt' = @{ sizeBytes = 458; sha256 = '6bd81aa03c651c1a88b50ec99dc060d458a4c6439f46bf109afa6a7178538a90' }
    'licenses/share__doc__qpdf__manual-html__license.html' = @{ sizeBytes = 6644; sha256 = 'd9c0f00a8e0df2ca3d3a93fe147c12f222f3ee00d1081824cf3f0697c5fe0405' }
    'provenance/qpdf-12.3.2.sha256' = @{ sizeBytes = 2151; sha256 = '2f6a0608086cb6f2ce36432fecdcae1817d8a60e69c5f513e5c5079db80bcd84' }
    'provenance/qpdf-12.3.2.sha256.sigstore' = @{ sizeBytes = 6083; sha256 = '7556aae55ae953398325c220d15b9e05dd2049252e320e780bb9e242e96012b2' }
}
$resolvedRoot = [System.IO.Path]::GetFullPath($BundleRoot)
if (-not (Test-Path -LiteralPath $resolvedRoot -PathType Container)) {
    throw "qpdf bundle is missing: $resolvedRoot"
}

$manifestPath = Join-Path $resolvedRoot 'qpdf-manifest.json'
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    throw 'qpdf-manifest.json is missing.'
}
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
if ($manifest.schemaVersion -ne 1 -or
    $manifest.dependency -ne 'qpdf' -or
    $manifest.version -ne $ExpectedVersion -or
    $manifest.sourceArchiveSizeBytes -ne $ExpectedArchiveSizeBytes -or
    $manifest.sourceArchiveSha256 -ne $ExpectedArchiveSha256 -or
    $manifest.signatureIdentity -ne 'ejb@ql.org' -or
    $manifest.signatureIssuer -ne 'https://github.com/login/oauth' -or
    $manifest.appContainerProfile -ne 'DocumentStudio.PdfEngine.Qpdf.V1' -or
    $manifest.appContainerConfigurationVersion -ne 1 -or
    @($manifest.appContainerCapabilities).Count -ne 0) {
    throw 'The qpdf manifest does not match the approved dependency record.'
}

$manifestPaths = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
if (@($manifest.files).Count -ne $ExpectedFiles.Count) {
    throw 'The qpdf manifest file count does not match the reviewed runtime set.'
}
foreach ($entry in $manifest.files) {
    if ($entry.path -notmatch '^(bin|licenses|provenance)/[^/]+$') {
        throw "Manifest path is outside the approved bundle layout: $($entry.path)"
    }
    $entryPath = Join-Path $resolvedRoot ($entry.path.Replace('/', '\'))
    $resolvedEntryPath = [System.IO.Path]::GetFullPath($entryPath)
    if (-not $resolvedEntryPath.StartsWith($resolvedRoot + [System.IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Manifest path escaped the bundle root: $($entry.path)"
    }
    if (-not (Test-Path -LiteralPath $resolvedEntryPath -PathType Leaf)) {
        throw "Manifest file is missing: $($entry.path)"
    }
    $file = Get-Item -LiteralPath $resolvedEntryPath
    $hash = (Get-FileHash -LiteralPath $resolvedEntryPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($file.Length -ne [int64]$entry.sizeBytes -or $hash -ne $entry.sha256) {
        throw "Manifest verification failed: $($entry.path)"
    }
    if (-not $ExpectedFiles.Contains($entry.path)) {
        throw "The qpdf manifest contains an unapproved runtime file: $($entry.path)"
    }
    $expected = $ExpectedFiles[$entry.path]
    if ([int64]$entry.sizeBytes -ne [int64]$expected.sizeBytes -or $entry.sha256 -ne $expected.sha256) {
        throw "The qpdf manifest differs from the compiled reviewed runtime set: $($entry.path)"
    }
    if (-not $manifestPaths.Add($entry.path)) {
        throw "Manifest path is duplicated: $($entry.path)"
    }
}

$actualFiles = @(
    Get-ChildItem -LiteralPath $resolvedRoot -File -Recurse |
        Where-Object { $_.FullName -ne $manifestPath } |
        ForEach-Object { Get-RelativePath $resolvedRoot $_.FullName }
)
if ($actualFiles.Count -ne $manifestPaths.Count -or @($actualFiles | Where-Object { -not $manifestPaths.Contains($_) }).Count -ne 0) {
    throw 'The qpdf bundle contains unmanifested or missing files.'
}

$qpdfPath = Join-Path $resolvedRoot 'bin\qpdf.exe'
$versionOutput = (& $qpdfPath --version 2>&1 | Out-String).Trim()
$versionLines = @($versionOutput -split '\r?\n' | Where-Object { $_ -ne '' })
if ($LASTEXITCODE -ne 0 -or
    $versionLines.Count -ne 2 -or
    $versionLines[0] -ne 'qpdf version 12.3.2' -or
    $versionLines[1] -ne 'Run qpdf --copyright to see copyright and license information.') {
    throw "Unexpected qpdf version output: $versionOutput"
}

Write-Output "Verified qpdf $ExpectedVersion bundle: $($manifest.files.Count) files"
