param(
    [Parameter(Mandatory = $true)] [string]$RuntimeRoot,
    [Parameter(Mandatory = $true)] [string]$WorkRoot,
    [Parameter(Mandatory = $true)] [string]$EvidenceDirectory,
    [Parameter(Mandatory = $true)] [string]$RepositoryRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'G04DC.Common.psm1') -Force

function Quote-G04DCArgument([string]$Value) {
    if ($Value.IndexOf([char]0) -ge 0) { throw '[ARGUMENT_VECTOR_INVALID] Embedded NUL.' }
    if ($Value.Contains('"')) { throw '[ARGUMENT_VECTOR_INVALID] Embedded quote.' }
    if ($Value -notmatch '[\s"]') { return $Value }
    return '"' + $Value + '"'
}

function Get-G04DCFileInventory([string]$Root) {
    $canonicalRoot = [IO.Path]::GetFullPath($Root).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
    return @(Get-ChildItem -LiteralPath $canonicalRoot -File -Recurse -Force | Sort-Object FullName | ForEach-Object {
        [pscustomobject][ordered]@{
            path = $_.FullName.Substring($canonicalRoot.Length + 1).Replace('\', '/')
            sizeBytes = [long]$_.Length
            sha256 = Get-G04DCSha256 -Path $_.FullName
        }
    })
}

$runtime = (Resolve-Path -LiteralPath $RuntimeRoot).Path
$work = (Resolve-Path -LiteralPath $WorkRoot).Path
$evidence = (Resolve-Path -LiteralPath $EvidenceDirectory).Path
$sofficeCandidates = @(Get-ChildItem -LiteralPath $runtime -Filter 'soffice.com' -File -Recurse -Force)
if ($sofficeCandidates.Count -ne 1) { throw '[RUNTIME_IDENTITY_INVALID] Sandbox requires exactly one soffice.com.' }
$soffice = $sofficeCandidates[0].FullName
$sofficeBinCandidates = @(Get-ChildItem -LiteralPath $runtime -Filter 'soffice.bin' -File -Recurse -Force)
if ($sofficeBinCandidates.Count -ne 1) { throw '[RUNTIME_IDENTITY_INVALID] Sandbox requires exactly one soffice.bin.' }
$expected = Get-G04DCExpectedMsi
$observedVersionMatch = [regex]::Match(
    ([string]$sofficeBinCandidates[0].VersionInfo.ProductVersion + ' ' + [string]$sofficeBinCandidates[0].VersionInfo.FileVersion),
    '\d+\.\d+\.\d+\.\d+'
)
if (!$observedVersionMatch.Success -or $observedVersionMatch.Value -cne $expected.ProductVersion) {
    throw "[RUNTIME_IDENTITY_INVALID] soffice.bin version is '$($observedVersionMatch.Value)', expected '$($expected.ProductVersion)'."
}

$versionRoot = Join-Path $work 'sandbox-version'
$conversionRoot = Join-Path $work 'sandbox-conversion'
foreach ($definition in @(
    [pscustomobject]@{ root = $versionRoot; children = @('', 'profile', 'temp', 'appdata', 'localappdata') },
    [pscustomobject]@{ root = $conversionRoot; children = @('', 'fixture', 'profile', 'temp', 'appdata', 'localappdata', 'staging') }
)) {
    foreach ($child in $definition.children) {
        New-Item -ItemType Directory -Path (Join-Path $definition.root $child) -Force | Out-Null
    }
}
$fixture = Join-Path $conversionRoot 'fixture\g04d-c-sandbox-smoke.odt'
& (Join-Path $PSScriptRoot 'New-G04DCSyntheticOdt.ps1') -Destination $fixture | Out-Null
$runtimeInventoryBefore = @(Get-G04DCFileInventory -Root $runtime)
$runtimeDigestBefore = Get-G04DCCanonicalHash -Rows $runtimeInventoryBefore
$fixtureSha256Before = Get-G04DCSha256 -Path $fixture
$ordinaryProfile = Join-Path $env:APPDATA 'LibreOffice'
$ordinaryBefore = Test-Path -LiteralPath $ordinaryProfile

$profileName = 'DocumentStudio.OfficeEngine.LibreOffice.G04DC.Proof'
Add-Type -Path (Join-Path $PSScriptRoot 'G04DC.Sandbox.cs')

# Create and immediately delete a temporary profile only to derive the exact SID used for owned ACLs.
$userenv = @'
using System;
using System.Runtime.InteropServices;
public static class G04DCSidDeriver {
  public sealed class Result { public string Sid { get; set; } public string Folder { get; set; } }
  [DllImport("userenv.dll", CharSet=CharSet.Unicode)] public static extern int CreateAppContainerProfile(string n,string d,string x,IntPtr c,uint z,out IntPtr s);
  [DllImport("userenv.dll", CharSet=CharSet.Unicode)] public static extern int DeleteAppContainerProfile(string n);
  [DllImport("userenv.dll", CharSet=CharSet.Unicode)] public static extern int GetAppContainerFolderPath(string s,out IntPtr p);
  [DllImport("advapi32.dll", SetLastError=true)] public static extern bool ConvertSidToStringSid(IntPtr s,out IntPtr t);
  [DllImport("kernel32.dll")] public static extern IntPtr LocalFree(IntPtr p);
  [DllImport("advapi32.dll")] public static extern IntPtr FreeSid(IntPtr p);
  public static Result Derive(string n) { IntPtr s=IntPtr.Zero; IntPtr t=IntPtr.Zero; IntPtr p=IntPtr.Zero; bool created=false; try { int h=CreateAppContainerProfile(n,"Document Studio LibreOffice proof","Proof ACL derivation",IntPtr.Zero,0,out s); if(h<0) throw new Exception("Create profile HRESULT 0x"+h.ToString("X8")); created=true; if(!ConvertSidToStringSid(s,out t)) throw new Exception("SID conversion failed"); string sid=Marshal.PtrToStringUni(t); int f=GetAppContainerFolderPath(sid,out p); if(f<0) throw new Exception("Get folder HRESULT 0x"+f.ToString("X8")); return new Result { Sid=sid, Folder=Marshal.PtrToStringUni(p) }; } finally { if(p!=IntPtr.Zero) Marshal.FreeCoTaskMem(p); if(t!=IntPtr.Zero) LocalFree(t); if(s!=IntPtr.Zero) FreeSid(s); if(created && DeleteAppContainerProfile(n)<0) throw new Exception("Profile cleanup failed"); } }
}
'@
Add-Type -TypeDefinition $userenv -Language CSharp
$profileIdentity = [G04DCSidDeriver]::Derive($profileName)
$sid = $profileIdentity.Sid
$appContainerFolder = [IO.Path]::GetFullPath($profileIdentity.Folder)
$appContainerRegistryPaths = @(
    "Registry::HKEY_CURRENT_USER\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppContainer\Mappings\$sid",
    "Registry::HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\AppContainer\Storage\$sid"
)
function Get-G04DCAppContainerResidue {
    return [pscustomobject][ordered]@{
        storagePath = $appContainerFolder
        storagePresent = Test-Path -LiteralPath $appContainerFolder
        registryPaths = @($appContainerRegistryPaths)
        registryPathsPresent = @($appContainerRegistryPaths | Where-Object { Test-Path -LiteralPath $_ })
    }
}
function Get-G04DCLoopbackExemptionState {
    $output = & (Join-Path $env:SystemRoot 'System32\CheckNetIsolation.exe') 'LoopbackExempt' '-s' 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0) { throw '[NETWORK_ATTEMPT] CheckNetIsolation loopback-exemption query failed.' }
    return [pscustomobject][ordered]@{
        appContainerSid = $sid
        sidExempt = $output.IndexOf($sid, [StringComparison]::OrdinalIgnoreCase) -ge 0
        boundedOutputSha256 = Get-G04DCCanonicalHash -Rows @($output)
        rawOutputRetained = $false
    }
}
$appContainerResidueBaseline = Get-G04DCAppContainerResidue
if ($appContainerResidueBaseline.storagePresent -or @($appContainerResidueBaseline.registryPathsPresent).Count -ne 0) {
    throw '[APPCONTAINER_FAILURE] Proof-profile SID derivation left package storage or registry residue.'
}
$loopbackBaseline = Get-G04DCLoopbackExemptionState
if ($loopbackBaseline.sidExempt) { throw '[NETWORK_ATTEMPT] Proof AppContainer SID has a pre-existing loopback exemption.' }
$aclGrants = [System.Collections.Generic.List[object]]::new()

function Grant-G04DCAcl([string]$Path, [string]$Permission, [bool]$Recursive) {
    $argument = "*$sid`:$Permission"
    $args = @($Path, '/grant', $argument, '/q')
    if ($Recursive) { $args += @('/t', '/c') }
    & (Join-Path $env:SystemRoot 'System32\icacls.exe') @args | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "[APPCONTAINER_FAILURE] icacls failed for $Path" }
    $aclGrants.Add([pscustomobject][ordered]@{ path = [IO.Path]::GetFullPath($Path); permission = $Permission; recursive = $Recursive })
}
Grant-G04DCAcl -Path (Split-Path -Parent $runtime) -Permission '(RX)' -Recursive $false
Grant-G04DCAcl -Path $runtime -Permission '(OI)(CI)(RX)' -Recursive $true
Grant-G04DCAcl -Path $work -Permission '(RX)' -Recursive $false
foreach ($probeRoot in @($versionRoot, $conversionRoot)) {
    Grant-G04DCAcl -Path $probeRoot -Permission '(RX)' -Recursive $false
    foreach ($mutable in @('profile', 'temp', 'appdata', 'localappdata')) {
        Grant-G04DCAcl -Path (Join-Path $probeRoot $mutable) -Permission '(OI)(CI)(M)' -Recursive $true
    }
}
Grant-G04DCAcl -Path (Join-Path $conversionRoot 'fixture') -Permission '(OI)(CI)(RX)' -Recursive $true
Grant-G04DCAcl -Path (Join-Path $conversionRoot 'staging') -Permission '(OI)(CI)(M)' -Recursive $true

function Invoke-G04DCZeroCapabilityProbe {
    param(
        [Parameter(Mandatory = $true)] [string]$ProbeName,
        [Parameter(Mandatory = $true)] [string]$ProbeRoot,
        [Parameter(Mandatory = $true)] [string[]]$Arguments
    )
    $commandLine = ((@((Quote-G04DCArgument $soffice)) + @($Arguments | ForEach-Object { Quote-G04DCArgument $_ })) -join ' ')
    $savedEnvironment = @{}
    foreach ($name in @('TEMP', 'TMP', 'APPDATA', 'LOCALAPPDATA')) {
        $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
    }
    [Environment]::SetEnvironmentVariable('TEMP', (Join-Path $ProbeRoot 'temp'), 'Process')
    [Environment]::SetEnvironmentVariable('TMP', (Join-Path $ProbeRoot 'temp'), 'Process')
    [Environment]::SetEnvironmentVariable('APPDATA', (Join-Path $ProbeRoot 'appdata'), 'Process')
    [Environment]::SetEnvironmentVariable('LOCALAPPDATA', (Join-Path $ProbeRoot 'localappdata'), 'Process')
    $loopbackBefore = Get-G04DCLoopbackExemptionState
    if ($loopbackBefore.sidExempt) { throw '[NETWORK_ATTEMPT] Proof AppContainer SID gained a loopback exemption before launch.' }
    $unrelated = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\notepad.exe') -PassThru -WindowStyle Hidden
    $result = $null
    try {
        $result = [DocumentStudio.G04DC.Proof.OfficeSandbox]::Run($profileName, $soffice, $commandLine, $ProbeRoot, 60000, 16, 2147483648L)
        $result | Add-Member -NotePropertyName unrelatedProcessSurvived -NotePropertyValue (!$unrelated.HasExited)
        $residueAfterProbe = Get-G04DCAppContainerResidue
        $loopbackAfter = Get-G04DCLoopbackExemptionState
        $result | Add-Member -NotePropertyName loopbackExemptBefore -NotePropertyValue ([bool]$loopbackBefore.sidExempt)
        $result | Add-Member -NotePropertyName loopbackExemptAfter -NotePropertyValue ([bool]$loopbackAfter.sidExempt)
        $result | Add-Member -NotePropertyName fileAccessObservation -NotePropertyValue ([pscustomobject][ordered]@{
            probeRoot = [IO.Path]::GetFullPath($ProbeRoot)
            ownedWritablePathInventory = @(Get-G04DCFileInventory -Root $ProbeRoot)
            appContainerStoragePath = $appContainerFolder
            appContainerStorageAbsentAfterProfileDeletion = ![bool]$residueAfterProbe.storagePresent
            appContainerRegistryResidueAbsentAfterProfileDeletion = @($residueAfterProbe.registryPathsPresent).Count -eq 0
            captured = $true
        })
        Write-G04DCJson -Path (Join-Path $evidence "$ProbeName-process-evidence.json") -Value $result
        Assert-G04DCProcessEvidence -Evidence $result -RuntimeRoot $runtime | Out-Null
        if ($result.timedOut -or $result.exitCode -ne 0) {
            throw "[APPCONTAINER_FAILURE] LibreOffice $ProbeName probe failed inside the zero-capability AppContainer."
        }
        return $result
    }
    catch {
        $processEvidence = $result
        if (!$processEvidence -and $_.Exception.PSObject.Properties['Evidence']) { $processEvidence = $_.Exception.Evidence }
        if ($processEvidence -and !$processEvidence.PSObject.Properties['unrelatedProcessSurvived']) {
            $processEvidence | Add-Member -NotePropertyName unrelatedProcessSurvived -NotePropertyValue (!$unrelated.HasExited)
        }
        Write-G04DCJson -Path (Join-Path $evidence "$ProbeName-failure.json") -Value ([ordered]@{
            type = $_.Exception.GetType().FullName
            message = $_.Exception.Message
            processEvidence = $processEvidence
            appContainerProfileDeleted = if ($processEvidence) { [bool]$processEvidence.profileDeleted } else { $false }
            unrelatedProcessSurvived = !$unrelated.HasExited
            isolatedProfilePath = Join-Path $ProbeRoot 'profile'
            isolatedProfilePathPresent = Test-Path -LiteralPath (Join-Path $ProbeRoot 'profile')
            ownedWritablePathInventory = @(Get-G04DCFileInventory -Root $ProbeRoot)
            appContainerResidueAfterFailure = Get-G04DCAppContainerResidue
            loopbackExemptionBefore = $loopbackBefore
            loopbackExemptionAfterFailure = Get-G04DCLoopbackExemptionState
            ordinaryProfilePresent = Test-Path -LiteralPath $ordinaryProfile
            recordedAtUtc = [DateTime]::UtcNow.ToString('o')
        })
        throw
    }
    finally {
        foreach ($name in $savedEnvironment.Keys) {
            [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name], 'Process')
        }
        if (!$unrelated.HasExited) { $unrelated.Kill(); $unrelated.WaitForExit() }
        $unrelated.Dispose()
    }
}

$versionProfileUri = ([uri](Join-Path $versionRoot 'profile')).AbsoluteUri
$versionArguments = @(
    '--headless', '--nologo', '--nodefault', '--nolockcheck', '--nofirststartwizard', '--norestore',
    "-env:UserInstallation=$versionProfileUri", '--version'
)
$versionResult = Invoke-G04DCZeroCapabilityProbe -ProbeName 'sandbox-version' -ProbeRoot $versionRoot -Arguments $versionArguments
Write-G04DCJson -Path (Join-Path $evidence 'sandbox-version-result.json') -Value ([ordered]@{
    expectedVersion = $expected.ProductVersion
    observedRuntimeFileVersion = $observedVersionMatch.Value
    exitCode = $versionResult.exitCode
    timedOut = $versionResult.timedOut
    networkConnections = @($versionResult.networkConnections)
    zeroCapabilities = @($versionResult.capabilities).Count -eq 0
})

$conversionProfileUri = ([uri](Join-Path $conversionRoot 'profile')).AbsoluteUri
$conversionArguments = @(
    '--headless', '--nologo', '--nodefault', '--nolockcheck', '--nofirststartwizard', '--norestore',
    "-env:UserInstallation=$conversionProfileUri", '--convert-to', 'pdf:writer_pdf_Export',
    '--outdir', (Join-Path $conversionRoot 'staging'), $fixture
)
$conversionResult = Invoke-G04DCZeroCapabilityProbe -ProbeName 'sandbox-conversion' -ProbeRoot $conversionRoot -Arguments $conversionArguments

$loadBearingModuleEvidence = Get-G04DCLoadBearingModuleEvidence `
    -ProcessEvidence @($versionResult, $conversionResult) `
    -RuntimeRoot $runtime `
    -WindowsRoot $env:SystemRoot
Write-G04DCJson -Path (Join-Path $evidence 'sandbox-load-bearing-modules.json') -Value $loadBearingModuleEvidence
Assert-G04DCLoadBearingModuleEvidence -Evidence $loadBearingModuleEvidence | Out-Null

$outputs = @(Get-ChildItem -LiteralPath (Join-Path $conversionRoot 'staging') -File)
if ($outputs.Count -ne 1 -or $outputs[0].Extension -cne '.pdf') { throw '[OUTPUT_MISSING_OR_CORRUPT] Sandboxed conversion did not create exactly one PDF.' }
$output = $outputs[0]
$qpdfRoot = Join-Path $RepositoryRoot 'apps\desktop\src-tauri\resources\qpdf\12.3.2'
& (Join-Path $RepositoryRoot 'scripts\verify-qpdf-bundle.ps1') -BundleRoot $qpdfRoot | Out-Null
$qpdf = Join-Path $qpdfRoot 'bin\qpdf.exe'
& $qpdf $output.FullName '--suppress-recovery' '--check' | Out-Null
$qpdfStrict = $LASTEXITCODE -eq 0
& $qpdf $output.FullName '--is-encrypted' | Out-Null
$encrypted = $LASTEXITCODE -ne 2
$pdfjsPath = Join-Path $evidence 'sandbox-pdfjs-output.json'
& node (Join-Path $PSScriptRoot 'verify-pdfjs.mjs') $output.FullName $pdfjsPath
if ($LASTEXITCODE -ne 0) { throw '[OUTPUT_MISSING_OR_CORRUPT] Sandboxed PDF failed PDF.js.' }
$pdfjs = Get-Content -LiteralPath $pdfjsPath -Raw | ConvertFrom-Json
$bytes = [IO.File]::ReadAllBytes($output.FullName)
$outputEvidence = [pscustomobject][ordered]@{
    path = $output.FullName
    regularFile = $true
    reparsePoint = [bool]($output.Attributes -band [IO.FileAttributes]::ReparsePoint)
    sizeBytes = [long]$output.Length
    sha256 = Get-G04DCSha256 -Path $output.FullName
    magic = [Text.Encoding]::ASCII.GetString($bytes, 0, [Math]::Min(5, $bytes.Length))
    qpdfStrict = $qpdfStrict
    encrypted = $encrypted
    pdfjsOpened = [bool]$pdfjs.pdfjsOpened
    pageCount = [int]$pdfjs.pageCount
}
Assert-G04DCOutputEvidence -Evidence $outputEvidence | Out-Null
Write-G04DCJson -Path (Join-Path $evidence 'sandbox-output-verification.json') -Value $outputEvidence
if (!$ordinaryBefore -and (Test-Path -LiteralPath $ordinaryProfile)) { throw '[ORDINARY_PROFILE_CREATED] Sandbox created the ordinary profile.' }
$runtimeInventoryAfter = @(Get-G04DCFileInventory -Root $runtime)
$runtimeDigestAfter = Get-G04DCCanonicalHash -Rows $runtimeInventoryAfter
$workPrefix = [IO.Path]::GetFullPath($work).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
$probeRootsOwned = @(@($versionRoot, $conversionRoot) | Where-Object { [IO.Path]::GetFullPath($_).StartsWith($workPrefix, [StringComparison]::OrdinalIgnoreCase) }).Count -eq 2
$fileAccessEvidence = [pscustomobject][ordered]@{
    aclGrantSetExact = $aclGrants.Count -eq 15
    explicitAclGrants = @($aclGrants.ToArray())
    noNetworkCapability = @($versionResult.capabilities).Count -eq 0 -and @($conversionResult.capabilities).Count -eq 0
    loopbackExemptionAbsent = !$versionResult.loopbackExemptBefore -and !$versionResult.loopbackExemptAfter -and !$conversionResult.loopbackExemptBefore -and !$conversionResult.loopbackExemptAfter
    networkObservation = 'TCP and UDP owned sockets/listeners sampled; denied-attempt telemetry is not claimed'
    ownedWritablePathInventoriesCaptured = $probeRootsOwned
    actualAccessTelemetryCaptured = $false
    effectiveDenialOutsideAllowedRootsProven = $false
    telemetryDisposition = 'REJECTED: bounded path inventories and ACL grants are evidence, but Windows file-access telemetry was not captured; inherited AppContainer-readable paths cannot be ruled out.'
    appContainerExternalStorageAbsent = ![bool](Get-G04DCAppContainerResidue).storagePresent
    appContainerRegistryResidueAbsent = @((Get-G04DCAppContainerResidue).registryPathsPresent).Count -eq 0
    runtimeTreeSha256Before = $runtimeDigestBefore
    runtimeTreeSha256After = $runtimeDigestAfter
    runtimeTreeUnchanged = $runtimeDigestBefore -ceq $runtimeDigestAfter
    fixtureSha256Before = $fixtureSha256Before
    fixtureSha256After = Get-G04DCSha256 -Path $fixture
    fixtureUnchanged = $fixtureSha256Before -ceq (Get-G04DCSha256 -Path $fixture)
    probes = @(
        [pscustomobject]@{ name = 'version'; captured = [bool]$versionResult.fileAccessObservation.captured; observation = $versionResult.fileAccessObservation },
        [pscustomobject]@{ name = 'conversion'; captured = [bool]$conversionResult.fileAccessObservation.captured; observation = $conversionResult.fileAccessObservation }
    )
}
Write-G04DCJson -Path (Join-Path $evidence 'sandbox-file-access-evidence.json') -Value $fileAccessEvidence
Assert-G04DCFileAccessEvidence -Evidence $fileAccessEvidence | Out-Null
Write-Output $output.FullName
