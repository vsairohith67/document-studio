param(
    [Parameter(Mandatory = $true)] [string]$RuntimeRoot,
    [Parameter(Mandatory = $true)] [string]$OutputPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'G04DC.Common.psm1') -Force

$rootItem = Get-Item -LiteralPath $RuntimeRoot -Force
if (!$rootItem.PSIsContainer -or [bool]($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw '[RUNTIME_IDENTITY_INVALID] Runtime root must be a regular non-reparse directory.'
}
$root = $rootItem.FullName
$reparseEntries = @(Get-ChildItem -LiteralPath $root -Recurse -Force | Where-Object { [bool]($_.Attributes -band [IO.FileAttributes]::ReparsePoint) })
if ($reparseEntries.Count -ne 0) { throw "[RUNTIME_IDENTITY_INVALID] Candidate runtime contains $($reparseEntries.Count) reparse entries." }

$files = [System.Collections.Generic.List[object]]::new()
foreach ($file in @(Get-ChildItem -LiteralPath $root -File -Recurse -Force | Sort-Object FullName)) {
    $relative = $file.FullName.Substring($root.Length + 1).Replace('\', '/')
    $potentialExecutable = $file.Extension -match '^(?i:\.exe|\.dll|\.com|\.bin|\.pyd|\.ocx|\.cpl)$'
    $signature = if ($potentialExecutable) { Get-G04DCAuthenticodeEvidence -Path $file.FullName } else { $null }
    $files.Add([pscustomobject][ordered]@{
        path = $relative
        sizeBytes = [long]$file.Length
        sha256 = Get-G04DCSha256 -Path $file.FullName
        potentialExecutable = $potentialExecutable
        staticallyAdmittedEntryPoint = $false
        authenticode = $signature
        fileVersion = if ($potentialExecutable) { [string]$file.VersionInfo.FileVersion } else { $null }
        productVersion = if ($potentialExecutable) { [string]$file.VersionInfo.ProductVersion } else { $null }
    })
}

$expected = Get-G04DCExpectedMsi
$canonicalExecutables = [ordered]@{}
foreach ($name in @('soffice.com', 'soffice.exe', 'soffice.bin')) {
    $matches = @($files | Where-Object { [IO.Path]::GetFileName($_.path) -ceq $name })
    if ($matches.Count -ne 1) { throw "[RUNTIME_IDENTITY_INVALID] Candidate runtime requires exactly one $name; found $($matches.Count)." }
    $entry = $matches[0]
    $entry.staticallyAdmittedEntryPoint = $true
    if (!$entry.authenticode -or [string]$entry.authenticode.status -cne 'Valid' -or ![bool]$entry.authenticode.chainValid -or
        @($entry.authenticode.chain).Count -lt 2 -or [string]$entry.authenticode.signerThumbprint -cne $expected.SignerThumbprint) {
        throw "[RUNTIME_IDENTITY_INVALID] Canonical entry point $name failed the exact signer-thumbprint and chain policy."
    }
    $canonicalExecutables[$name] = $entry.path
}

$sofficeBin = @($files | Where-Object { [IO.Path]::GetFileName($_.path) -ceq 'soffice.bin' })[0]
$versionMatch = [regex]::Match(([string]$sofficeBin.productVersion + ' ' + [string]$sofficeBin.fileVersion), '\d+\.\d+\.\d+\.\d+')
if (!$versionMatch.Success -or $versionMatch.Value -cne $expected.ProductVersion) {
    throw "[RUNTIME_IDENTITY_INVALID] Installed runtime version is '$($versionMatch.Value)', expected '$($expected.ProductVersion)'."
}
$notices = @($files | Where-Object { $_.path -match '(?i)(license|licence|notice|readme|copying)' } | ForEach-Object { $_.path })
if ($notices.Count -eq 0) { throw '[RUNTIME_IDENTITY_INVALID] Candidate runtime contains no licence or notice file.' }

$manifest = [pscustomobject][ordered]@{
    schemaVersion = 2
    runtimeRoot = $root
    fileCount = $files.Count
    files = @($files.ToArray())
    noticePaths = $notices
    canonicalExecutables = $canonicalExecutables
    installedFourPartVersion = $versionMatch.Value
    staticEntryPointSignerPolicy = "Authenticode Valid, chain length at least two, and exact TDF leaf thumbprint $($expected.SignerThumbprint)"
    loadBearingPolicy = 'This whole-tree manifest hashes every file and records signatures for PE-like files. It does not label unused bundled PE files load-bearing. The sandbox probe separately seals every dynamically loaded module and applies the exact runtime/Windows root and signer-chain policy.'
}
Write-G04DCJson -Path $OutputPath -Value $manifest
Write-Output $OutputPath
