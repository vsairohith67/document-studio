Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'G04DC.Common.psm1') -Force

$passed = [System.Collections.Generic.List[string]]::new()
function Assert-Throws([string]$Name, [string]$Code, [scriptblock]$Action) {
    try { & $Action; throw "Test $Name did not throw." }
    catch {
        if ($_.Exception.Message -notmatch [regex]::Escape("[$Code]")) { throw "Test $Name threw unexpected error: $($_.Exception.Message)" }
        $passed.Add($Name)
    }
}

$expected = Get-G04DCExpectedMsi
function New-ValidIdentity {
    return [pscustomobject][ordered]@{
        regularFile = $true; reparsePoint = $false; sizeBytes = $expected.SizeBytes; sha256 = $expected.Sha256
        authenticodeStatus = 'Valid'; signerSubject = 'CN=The Document Foundation'; signerThumbprint = $expected.SignerThumbprint
        signerChainValid = $true; signerChain = @([pscustomobject]@{ thumbprint = $expected.SignerThumbprint }, [pscustomobject]@{ thumbprint = 'ROOT' })
        timestampSignerThumbprint = $expected.TimestampSignerThumbprint; timestampChainValid = $true
        timestampChain = @([pscustomobject]@{ thumbprint = $expected.TimestampSignerThumbprint }, [pscustomobject]@{ thumbprint = 'ROOT' })
        productVersion = $expected.ProductVersion; architecture = $expected.Architecture; productCode = $expected.ProductCode
        upgradeCode = $expected.UpgradeCode; packageCode = $expected.PackageCode
    }
}

$case = New-ValidIdentity; $case.sha256 = '0' * 64
Assert-Throws 'wrong MSI hash' 'MSI_IDENTITY_MISMATCH' { Assert-G04DCMsiIdentity -Identity $case }
$case = New-ValidIdentity; $case.sizeBytes = $expected.SizeBytes - 1
Assert-Throws 'wrong MSI byte count' 'MSI_IDENTITY_MISMATCH' { Assert-G04DCMsiIdentity -Identity $case }
$case = New-ValidIdentity; $case.authenticodeStatus = 'HashMismatch'
Assert-Throws 'invalid Authenticode' 'MSI_IDENTITY_MISMATCH' { Assert-G04DCMsiIdentity -Identity $case }
$case = New-ValidIdentity; $case.signerSubject = 'CN=Substituted Publisher'
Assert-Throws 'wrong signer' 'MSI_IDENTITY_MISMATCH' { Assert-G04DCMsiIdentity -Identity $case }
$case = New-ValidIdentity; $case.signerThumbprint = '0' * 40
Assert-Throws 'wrong signer thumbprint' 'MSI_IDENTITY_MISMATCH' { Assert-G04DCMsiIdentity -Identity $case }
$case = New-ValidIdentity; $case.signerChainValid = $false
Assert-Throws 'invalid signer chain' 'MSI_IDENTITY_MISMATCH' { Assert-G04DCMsiIdentity -Identity $case }
$case = New-ValidIdentity; $case.timestampSignerThumbprint = '0' * 40
Assert-Throws 'wrong timestamp signer' 'MSI_IDENTITY_MISMATCH' { Assert-G04DCMsiIdentity -Identity $case }
$case = New-ValidIdentity; $case.timestampChainValid = $false
Assert-Throws 'invalid timestamp chain' 'MSI_IDENTITY_MISMATCH' { Assert-G04DCMsiIdentity -Identity $case }
$case = New-ValidIdentity; $case.productCode = '{00000000-0000-0000-0000-000000000000}'
Assert-Throws 'wrong ProductCode' 'MSI_IDENTITY_MISMATCH' { Assert-G04DCMsiIdentity -Identity $case }
$case = New-ValidIdentity; $case.upgradeCode = '{00000000-0000-0000-0000-000000000000}'
Assert-Throws 'wrong UpgradeCode' 'MSI_IDENTITY_MISMATCH' { Assert-G04DCMsiIdentity -Identity $case }
$case = New-ValidIdentity; $case.packageCode = '{00000000-0000-0000-0000-000000000000}'
Assert-Throws 'wrong PackageCode' 'MSI_IDENTITY_MISMATCH' { Assert-G04DCMsiIdentity -Identity $case }
$case = New-ValidIdentity; $case.productVersion = '26.2.5.3'
Assert-Throws 'wrong version' 'MSI_IDENTITY_MISMATCH' { Assert-G04DCMsiIdentity -Identity $case }
$case = New-ValidIdentity; $case.architecture = 'Intel'
Assert-Throws 'wrong architecture' 'MSI_IDENTITY_MISMATCH' { Assert-G04DCMsiIdentity -Identity $case }
$case = New-ValidIdentity; $case.signerChain = @()
Assert-Throws 'missing signer chain elements' 'MSI_IDENTITY_MISMATCH' { Assert-G04DCMsiIdentity -Identity $case }

$publicAddress = @('93.184.216.34')
Assert-G04DCAcquisitionUri -Uri ([uri]$expected.Url) -CanonicalFirstRequest -ResolvedAddresses $publicAddress | Out-Null
$passed.Add('exact canonical first request')
$mirrorUri = [uri]'https://mirror.example.org/tdf/libreoffice/stable/26.2.5/win/x86_64/LibreOffice_26.2.5_Win_x86-64.msi'
Assert-G04DCAcquisitionUri -Uri $mirrorUri -ResolvedAddresses $publicAddress | Out-Null
$passed.Add('accepted HTTPS cross-origin MirrorBrain redirect')
Assert-G04DCPinnedRemoteEndpoint -ApprovedAddresses $publicAddress -ConnectedAddress ([System.Net.IPAddress]::Parse($publicAddress[0])) | Out-Null
$passed.Add('pinned HTTPS remote endpoint')
Assert-Throws 'DNS rebinding remote endpoint' 'MSI_ACQUISITION_SOURCE_REJECTED' {
    Assert-G04DCPinnedRemoteEndpoint -ApprovedAddresses $publicAddress -ConnectedAddress ([System.Net.IPAddress]::Parse('127.0.0.1'))
}
$firstTransition = Resolve-G04DCRedirectTransition -CurrentUri ([uri]$expected.Url) -StatusCode 302 -Location $mirrorUri.AbsoluteUri -RedirectCount 0 -SeenUris @($expected.Url)
$secondUri = [uri]'https://download2.example.org/tdf/libreoffice/stable/26.2.5/win/x86_64/LibreOffice_26.2.5_Win_x86-64.msi'
$secondTransition = Resolve-G04DCRedirectTransition -CurrentUri $firstTransition.nextUri -StatusCode 307 -Location $secondUri.AbsoluteUri -RedirectCount $firstTransition.redirectCount -SeenUris @($expected.Url, $mirrorUri.AbsoluteUri)
$finalTransition = Resolve-G04DCRedirectTransition -CurrentUri $secondTransition.nextUri -StatusCode 200 -Location $null -RedirectCount $secondTransition.redirectCount -SeenUris @($expected.Url, $mirrorUri.AbsoluteUri, $secondUri.AbsoluteUri)
if (!$firstTransition.redirect -or !$secondTransition.redirect -or !$finalTransition.final -or $finalTransition.redirectCount -ne 2) { throw 'Multi-hop HTTPS redirect model failed.' }
$passed.Add('multi-hop HTTPS redirect')
$current = [uri]$expected.Url
$seen = [System.Collections.Generic.List[string]]::new()
$seen.Add($current.AbsoluteUri)
$count = 0
for ($index = 1; $index -le 8; $index++) {
    $next = "https://mirror$index.example.org/tdf/libreoffice/stable/26.2.5/win/x86_64/LibreOffice_26.2.5_Win_x86-64.msi"
    $step = Resolve-G04DCRedirectTransition -CurrentUri $current -StatusCode 302 -Location $next -RedirectCount $count -SeenUris @($seen.ToArray())
    $count = $step.redirectCount
    $current = $step.nextUri
    $seen.Add($current.AbsoluteUri)
}
$eightHopFinal = Resolve-G04DCRedirectTransition -CurrentUri $current -StatusCode 200 -Location $null -RedirectCount $count -SeenUris @($seen.ToArray())
if (!$eightHopFinal.final -or $eightHopFinal.redirectCount -ne 8) { throw 'Exact eight-hop redirect boundary failed.' }
$passed.Add('exact eight-hop redirect boundary')
Assert-Throws 'ninth-hop rejection' 'MSI_ACQUISITION_SOURCE_REJECTED' {
    Resolve-G04DCRedirectTransition -CurrentUri $current -StatusCode 302 -Location 'https://ninth.example.org/LibreOffice_26.2.5_Win_x86-64.msi' -RedirectCount 8 -SeenUris @($seen.ToArray())
}
Assert-Throws 'redirect loop' 'MSI_ACQUISITION_SOURCE_REJECTED' {
    Resolve-G04DCRedirectTransition -CurrentUri $mirrorUri -StatusCode 302 -Location $expected.Url -RedirectCount 1 -SeenUris @($expected.Url, $mirrorUri.AbsoluteUri)
}
Assert-Throws 'redirect missing Location' 'MSI_ACQUISITION_SOURCE_REJECTED' {
    Resolve-G04DCRedirectTransition -CurrentUri ([uri]$expected.Url) -StatusCode 302 -Location $null -RedirectCount 0 -SeenUris @($expected.Url)
}
Assert-Throws 'HTTP downgrade' 'MSI_ACQUISITION_SOURCE_REJECTED' { Assert-G04DCAcquisitionUri -Uri ([uri]'http://mirror.example.org/LibreOffice_26.2.5_Win_x86-64.msi') -ResolvedAddresses $publicAddress }
Assert-Throws 'non-default port' 'MSI_ACQUISITION_SOURCE_REJECTED' { Assert-G04DCAcquisitionUri -Uri ([uri]'https://mirror.example.org:8443/LibreOffice_26.2.5_Win_x86-64.msi') -ResolvedAddresses $publicAddress }
Assert-Throws 'acquisition URI userinfo' 'MSI_ACQUISITION_SOURCE_REJECTED' { Assert-G04DCAcquisitionUri -Uri ([uri]'https://user@mirror.example.org/LibreOffice_26.2.5_Win_x86-64.msi') -ResolvedAddresses $publicAddress }
Assert-Throws 'acquisition URI empty userinfo' 'MSI_ACQUISITION_SOURCE_REJECTED' { Assert-G04DCAcquisitionUri -Uri ([uri]'https://@mirror.example.org/LibreOffice_26.2.5_Win_x86-64.msi') -ResolvedAddresses $publicAddress }
Assert-Throws 'acquisition URI fragment' 'MSI_ACQUISITION_SOURCE_REJECTED' { Assert-G04DCAcquisitionUri -Uri ([uri]'https://mirror.example.org/LibreOffice_26.2.5_Win_x86-64.msi#fragment') -ResolvedAddresses $publicAddress }
Assert-Throws 'localhost target' 'MSI_ACQUISITION_SOURCE_REJECTED' { Assert-G04DCAcquisitionUri -Uri ([uri]'https://localhost/LibreOffice_26.2.5_Win_x86-64.msi') -ResolvedAddresses @('93.184.216.34') }
Assert-Throws 'IPv4 loopback target' 'MSI_ACQUISITION_SOURCE_REJECTED' { Assert-G04DCAcquisitionUri -Uri $mirrorUri -ResolvedAddresses @('127.0.0.1') }
Assert-Throws 'IPv6 loopback target' 'MSI_ACQUISITION_SOURCE_REJECTED' { Assert-G04DCAcquisitionUri -Uri $mirrorUri -ResolvedAddresses @('::1') }
Assert-Throws 'RFC1918 private target' 'MSI_ACQUISITION_SOURCE_REJECTED' { Assert-G04DCAcquisitionUri -Uri $mirrorUri -ResolvedAddresses @('10.12.0.1') }
Assert-Throws 'link-local target' 'MSI_ACQUISITION_SOURCE_REJECTED' { Assert-G04DCAcquisitionUri -Uri $mirrorUri -ResolvedAddresses @('169.254.2.3') }
Assert-Throws 'multicast target' 'MSI_ACQUISITION_SOURCE_REJECTED' { Assert-G04DCAcquisitionUri -Uri $mirrorUri -ResolvedAddresses @('224.0.0.1') }
Assert-Throws 'reserved target' 'MSI_ACQUISITION_SOURCE_REJECTED' { Assert-G04DCAcquisitionUri -Uri $mirrorUri -ResolvedAddresses @('198.51.100.5') }
Assert-Throws 'unspecified target' 'MSI_ACQUISITION_SOURCE_REJECTED' { Assert-G04DCAcquisitionUri -Uri $mirrorUri -ResolvedAddresses @('0.0.0.0') }
Assert-Throws 'raw IP literal target' 'MSI_ACQUISITION_SOURCE_REJECTED' { Assert-G04DCAcquisitionUri -Uri ([uri]'https://93.184.216.34/LibreOffice_26.2.5_Win_x86-64.msi') -ResolvedAddresses $publicAddress }
Assert-Throws 'unexpected final filename or path' 'MSI_ACQUISITION_SOURCE_REJECTED' {
    Resolve-G04DCRedirectTransition -CurrentUri ([uri]'https://mirror.example.org/LibreOffice-substituted.msi') -StatusCode 200 -Location $null -RedirectCount 1 -SeenUris @()
}
Assert-Throws 'truncated acquisition body' 'MSI_ACQUISITION_SIZE_INVALID' {
    $input = [IO.MemoryStream]::new([Text.Encoding]::ASCII.GetBytes('ab'))
    $output = [IO.MemoryStream]::new()
    try { Copy-G04DCBoundedHttpsBody -Response ([pscustomobject]@{ contentLength = 3L; transferEncoding = $null; stream = $input }) -Output $output -ExpectedBytes 3L }
    finally { $output.Dispose(); $input.Dispose() }
}
Assert-Throws 'oversized acquisition body' 'MSI_ACQUISITION_SIZE_INVALID' {
    $input = [IO.MemoryStream]::new([Text.Encoding]::ASCII.GetBytes('abcd'))
    $output = [IO.MemoryStream]::new()
    try { Copy-G04DCBoundedHttpsBody -Response ([pscustomobject]@{ contentLength = -1L; transferEncoding = $null; stream = $input }) -Output $output -ExpectedBytes 3L }
    finally { $output.Dispose(); $input.Dispose() }
}
$chunkedInput = [IO.MemoryStream]::new([Text.Encoding]::ASCII.GetBytes("3`r`nabc`r`n0`r`n`r`n"))
$chunkedOutput = [IO.MemoryStream]::new()
try {
    Copy-G04DCBoundedHttpsBody -Response ([pscustomobject]@{ contentLength = -1L; transferEncoding = 'chunked'; stream = $chunkedInput }) -Output $chunkedOutput -ExpectedBytes 3L | Out-Null
    if ([Text.Encoding]::ASCII.GetString($chunkedOutput.ToArray()) -cne 'abc') { throw 'Chunked HTTPS body decode mismatch.' }
}
finally { $chunkedOutput.Dispose(); $chunkedInput.Dispose() }
$passed.Add('bounded chunked acquisition body')
Assert-Throws 'failed-download cleanup ownership' 'CLEANUP_OWNERSHIP_MISMATCH' {
    Assert-G04DCFailedDownloadCleanup -Evidence ([pscustomobject]@{ exactFailedDownload = 'C:\owned\candidate.msi'; markerOwned = $false; removed = $true })
}
$redirectEvidence = [pscustomobject][ordered]@{
    initialUri = $expected.Url
    redirectCount = 1
    finalUri = $mirrorUri.AbsoluteUri
    hops = @(
        [pscustomobject][ordered]@{ requestedUri = $expected.Url; statusCode = 302; location = $mirrorUri.AbsoluteUri; resolvedEffectiveUri = $mirrorUri.AbsoluteUri; hostname = 'download.documentfoundation.org'; resolvedAddresses = $publicAddress; connectedAddress = $publicAddress[0]; remoteEndpoint = "$($publicAddress[0]):443" },
        [pscustomobject][ordered]@{ requestedUri = $mirrorUri.AbsoluteUri; statusCode = 200; location = $null; resolvedEffectiveUri = $mirrorUri.AbsoluteUri; hostname = 'mirror.example.org'; resolvedAddresses = $publicAddress; connectedAddress = $publicAddress[0]; remoteEndpoint = "$($publicAddress[0]):443" }
    )
}
Assert-G04DCRedirectChainEvidence -Evidence $redirectEvidence | Out-Null
$passed.Add('complete redirect-chain evidence')
$productionAcquisition = @(Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot '..\..\apps'), (Join-Path $PSScriptRoot '..\..\packages') -File -Recurse | Where-Object {
    (Get-Content -LiteralPath $_.FullName -Raw -ErrorAction SilentlyContinue) -match 'Invoke-G04DCAcquireMsi|download\.documentfoundation\.org'
})
if ($productionAcquisition.Count -ne 0) { throw 'Production application contains a G04D-C acquisition path.' }
$passed.Add('no production acquisition path')

Assert-Throws 'ambiguous MSI feature ownership' 'AMBIGUOUS_FEATURE_OWNERSHIP' {
    Assert-G04DCFeatureAnalysis -Analysis ([pscustomobject]@{ ambiguous = $true; ambiguityReasons = @('shared protected component'); candidateMinimumFeatureSet = @('gm_Root') })
}
Assert-Throws 'unexpected installed feature state' 'MINIMAL_FEATURE_STATE_INVALID' {
    Assert-G04DCInstalledFeatureStates -States @(
        [pscustomobject]@{ feature = 'gm_Root'; state = 3 },
        [pscustomobject]@{ feature = 'gm_o_Onlineupdate'; state = 3 }
    ) -SelectedFeatures @('gm_Root')
}
Assert-Throws 'unexpected installed component state' 'MINIMAL_COMPONENT_STATE_INVALID' {
    Assert-G04DCInstalledComponentStates -States @(
        [pscustomobject]@{ componentCode = '{11111111-1111-1111-1111-111111111111}'; state = 3 },
        [pscustomobject]@{ componentCode = '{22222222-2222-2222-2222-222222222222}'; state = 3 }
    ) -ComponentOwnership @(
        [pscustomobject]@{ componentId = '{11111111-1111-1111-1111-111111111111}'; selectedOwners = @('gm_Root'); expectedInstallState = 3 },
        [pscustomobject]@{ componentId = '{22222222-2222-2222-2222-222222222222}'; selectedOwners = @(); expectedInstallState = 2 }
    )
}
Assert-Throws 'excluded component file installed' 'MINIMAL_FILE_OWNERSHIP_INVALID' {
    Assert-G04DCInstalledFileOwnership -RuntimeManifest ([pscustomobject]@{ files = @([pscustomobject]@{ path = 'program/update_service.exe' }) }) -FileComponentOwnership @(
        [pscustomobject]@{ component = 'core'; targetRelativePath = 'program/soffice.bin'; underInstallLocation = $true; selectedOwners = @('gm_Root') },
        [pscustomobject]@{ component = 'update'; targetRelativePath = 'program/update_service.exe'; underInstallLocation = $true; selectedOwners = @() }
    ) -ComponentOwnership @(
        [pscustomobject]@{ component = 'core'; expectedInstallState = 3 },
        [pscustomobject]@{ component = 'update'; expectedInstallState = 2 }
    )
}
Assert-Throws 'MSI condition evaluation error' 'AMBIGUOUS_FEATURE_OWNERSHIP' {
    Resolve-G04DCExpectedComponentStates -ComponentOwnership @(
        [pscustomobject]@{ component = 'conditioned'; selectedOwners = @('gm_Root'); condition = 'WRITE_REGISTRY=1' }
    ) -ConditionEvaluations @([pscustomobject]@{ condition = 'WRITE_REGISTRY=1'; result = 3 })
}
$conditionOwnership = @(Resolve-G04DCExpectedComponentStates -ComponentOwnership @(
    [pscustomobject]@{ component = 'false'; selectedOwners = @('gm_Root'); condition = 'WRITE_REGISTRY=1' },
    [pscustomobject]@{ component = 'true'; selectedOwners = @('gm_Root'); condition = 'MsiNetAssemblySupport >= "4.0.0.0"' },
    [pscustomobject]@{ component = 'none'; selectedOwners = @('gm_Root'); condition = '' }
) -ConditionEvaluations @(
    [pscustomobject]@{ condition = 'WRITE_REGISTRY=1'; result = 0 },
    [pscustomobject]@{ condition = 'MsiNetAssemblySupport >= "4.0.0.0"'; result = 1 }
))
if ([int]$conditionOwnership[0].expectedInstallState -ne 2 -or [int]$conditionOwnership[1].expectedInstallState -ne 3 -or [int]$conditionOwnership[2].expectedInstallState -ne 3) {
    throw 'MSI condition state resolution did not produce FALSE=Absent and TRUE/blank=Local.'
}
$passed.Add('MSI conditioned component state resolution')
Assert-Throws 'ambiguous MSI effect ownership' 'AMBIGUOUS_MSI_EFFECT_OWNERSHIP' {
    Assert-G04DCMinimalMutationClosure -Closure ([pscustomobject]@{ ambiguousRows = @([pscustomobject]@{ table = 'ProgId' }); enabledMutationRows = @(); unboundedInstallCustomActions = @() })
}
Assert-Throws 'enabled MSI shortcut mutation' 'PROTECTED_MSI_EFFECT_UNAVOIDABLE' {
    Assert-G04DCMinimalMutationClosure -Closure ([pscustomobject]@{ ambiguousRows = @(); enabledMutationRows = @([pscustomobject]@{ table = 'Shortcut' }); unboundedInstallCustomActions = @() })
}
Assert-Throws 'unbounded install custom action' 'UNBOUNDED_MSI_CUSTOM_ACTION' {
    Assert-G04DCMinimalMutationClosure -Closure ([pscustomobject]@{ ambiguousRows = @(); enabledMutationRows = @(); unboundedInstallCustomActions = @([pscustomobject]@{ action = 'arbitrary' }) })
}
Assert-Throws 'unbounded administrative custom action' 'UNBOUNDED_MSI_CUSTOM_ACTION' {
    Assert-G04DCAdminMutationClosure -Closure ([pscustomobject]@{ unboundedAdminCustomActions = @([pscustomobject]@{ action = 'arbitrary' }) })
}

function New-State([string]$FontValue = 'baseline', [string]$Service = '', [string]$Association = 'Word.OpenDocumentText.12', [bool]$Profile = $false) {
    return [pscustomobject][ordered]@{
        fontCatalogCount = 1; fontCatalogSha256 = $FontValue; msiFontTargets = @(); externalRuntimeTargets = @()
        associations = @([pscustomobject]@{ extension = '.odt'; classDefault = $Association; userChoiceProgId = $null; userChoicePresent = $false })
        libreOfficeServices = if ($Service) { @([pscustomobject]@{ name = $Service; startMode = 'Manual'; pathName = 'owned' }) } else { @() }
        appPaths = @(); appPathCatalogCount = 0; appPathCatalogSha256 = 'app-path-catalog'
        libreOfficeProgIds = @(); classKeyCatalogCount = 0; classKeyCatalogSha256 = 'class-key-catalog'
        classRegistryCatalogCount = 0; classRegistryCatalogSha256 = 'class-registry-catalog'
        msiProtectedRegistryTargets = @(); scheduledTasks = @(); scheduledTaskCatalogCount = 0; scheduledTaskCatalogSha256 = 'task-catalog'
        startup = @(); startupCatalogCount = 0; startupCatalogSha256 = 'startup-catalog'
        shortcutCatalogCount = 0; shortcutCatalogSha256 = 'shortcut-catalog'
        firewallRules = @(); firewallCatalogCount = 0; firewallCatalogSha256 = 'firewall-catalog'
        firewallFilterCatalogCount = 0; firewallFilterCatalogSha256 = 'firewall-filter-catalog'
        serviceCatalogCount = 0; serviceCatalogSha256 = 'service-catalog'
        serviceRegistryCatalogCount = 0; serviceRegistryCatalogSha256 = 'service-registry-catalog'
        machinePath = 'machine'; userPath = 'user'; environmentCatalogCount = 0; environmentCatalogSha256 = 'environment-catalog'; pendingReboot = [pscustomobject]@{ reboot = $false }
        otherInstalledProductCatalogCount = 0; otherInstalledProductCatalogSha256 = 'other-product-catalog'
        installedProductCatalogCount = 0; installedProductCatalogSha256 = 'product-catalog'
        installerCacheCatalogCount = 0; installerCacheCatalogSha256 = 'installer-cache-catalog'
        ordinaryProfile = [pscustomobject]@{ roaming = 'profile'; roamingPresent = $Profile; local = 'local'; localPresent = $false }
        libreOfficeProcesses = @()
    }
}
$baseline = New-State
$comparison = Compare-G04DCMachineState -Before $baseline -After (New-State -Service 'pre-existing-service')
Assert-Throws 'pre-existing service change' 'PROTECTED_HOST_MUTATION' { Assert-G04DCNonMutation -Comparison $comparison }
$caseState = New-State; $caseState.serviceCatalogCount = 1; $caseState.serviceCatalogSha256 = 'changed-service-catalog'
$comparison = Compare-G04DCMachineState -Before $baseline -After $caseState
Assert-Throws 'unexpected service catalog change' 'PROTECTED_HOST_MUTATION' { Assert-G04DCNonMutation -Comparison $comparison }
$caseState = New-State; $caseState.serviceRegistryCatalogSha256 = 'changed-service-registry'
$comparison = Compare-G04DCMachineState -Before $baseline -After $caseState
Assert-Throws 'service registry configuration change' 'PROTECTED_HOST_MUTATION' { Assert-G04DCNonMutation -Comparison $comparison }
$comparison = Compare-G04DCMachineState -Before $baseline -After (New-State -FontValue 'added-font')
Assert-Throws 'pre-existing font change' 'PROTECTED_HOST_MUTATION' { Assert-G04DCNonMutation -Comparison $comparison }
$comparison = Compare-G04DCMachineState -Before $baseline -After (New-State -Association 'LibreOffice.WriterDocument.1')
Assert-Throws 'pre-existing association change' 'PROTECTED_HOST_MUTATION' { Assert-G04DCNonMutation -Comparison $comparison }
$caseState = New-State; $caseState.otherInstalledProductCatalogCount = 1; $caseState.otherInstalledProductCatalogSha256 = 'unrelated-product'
$comparison = Compare-G04DCMachineState -Before $baseline -After $caseState
Assert-Throws 'unrelated installed product change' 'PROTECTED_HOST_MUTATION' { Assert-G04DCNonMutation -Comparison $comparison }
$caseState = New-State; $caseState.classRegistryCatalogSha256 = 'changed-class-registry'
$comparison = Compare-G04DCMachineState -Before $baseline -After $caseState
Assert-Throws 'full classes registry change' 'PROTECTED_HOST_MUTATION' { Assert-G04DCNonMutation -Comparison $comparison }
$caseState = New-State; $caseState.shortcutCatalogSha256 = 'changed-shortcut-catalog'
$comparison = Compare-G04DCMachineState -Before $baseline -After $caseState
Assert-Throws 'desktop or start-menu shortcut change' 'PROTECTED_HOST_MUTATION' { Assert-G04DCNonMutation -Comparison $comparison }
$caseState = New-State; $caseState.environmentCatalogSha256 = 'changed-environment-catalog'
$comparison = Compare-G04DCMachineState -Before $baseline -After $caseState
Assert-Throws 'environment registry change' 'PROTECTED_HOST_MUTATION' { Assert-G04DCNonMutation -Comparison $comparison }
$caseState = New-State; $caseState.installerCacheCatalogSha256 = 'changed-installer-cache'
$comparison = Compare-G04DCMachineState -Before $baseline -After $caseState -IncludeInstallerCacheCatalog
Assert-Throws 'Windows Installer cache residue' 'PROTECTED_HOST_MUTATION' { Assert-G04DCNonMutation -Comparison $comparison }
Assert-Throws 'administrative extraction mutation' 'ADMINISTRATIVE_EXTRACTION_MUTATION' { Assert-G04DCNonMutation -Comparison $comparison -Code 'ADMINISTRATIVE_EXTRACTION_MUTATION' }
Assert-Throws 'minimal-install mutation' 'MINIMAL_INSTALL_MUTATION' { Assert-G04DCNonMutation -Comparison $comparison -Code 'MINIMAL_INSTALL_MUTATION' }
$comparison = Compare-G04DCMachineState -Before $baseline -After (New-State -Profile $true)
Assert-Throws 'ordinary-profile creation' 'PROTECTED_HOST_MUTATION' { Assert-G04DCNonMutation -Comparison $comparison }
$caseState = New-State -Profile $true
Assert-Throws 'pre-existing ordinary profile' 'PREEXISTING_RUNTIME_STATE' { Assert-G04DCRunnerIsolation -State $caseState }
$caseState = New-State; $caseState.libreOfficeProcesses = @([pscustomobject]@{ pid = 100; name = 'soffice.bin' })
Assert-Throws 'pre-existing LibreOffice process' 'PREEXISTING_RUNTIME_STATE' { Assert-G04DCRunnerIsolation -State $caseState }
Assert-Throws 'external VC runtime dependency missing' 'EXTERNAL_RUNTIME_DEPENDENCY_INVALID' { Assert-G04DCExternalRuntimeDependencies -State (New-State) }
Assert-Throws 'Windows Installer registration residue' 'MSI_REGISTRATION_RESIDUE' {
    Assert-G04DCMsiRegistrationAbsent -State ([pscustomobject]@{
        productState = 5; localPackage = [pscustomobject]@{ present = $false }; productRegistryTargets = @(); upgradeProductValuePresent = $false; componentRegistrations = @()
    })
}
Assert-Throws 'Windows Installer cached package invalid' 'MSI_REGISTRATION_INVALID' {
    Assert-G04DCMsiRegistrationInstalled -State ([pscustomobject]@{
        productState = 5; productRegistryTargets = @(); upgradeProductValuePresent = $false; componentRegistrations = @(); localPackage = [pscustomobject]@{ present = $false }
    }) -ExpectedComponents @([pscustomobject]@{ componentId = '{11111111-1111-1111-1111-111111111111}'; expectedInstallState = 3 })
}
$packedProduct = ConvertTo-G04DCPackedGuid -Guid $expected.ProductCode
if ($packedProduct -cne '917764B3B52CC874F8C4E8E2ADE00239') { throw "Packed MSI ProductCode is invalid: $packedProduct" }
$passed.Add('packed MSI GUID mapping')

$validProcess = [pscustomobject][ordered]@{
    appContainer = $true; capabilities = @(); assignedBeforeResume = $true; profileDeleted = $true; breakawayAllowed = $false
    totalAssignedProcesses = 1; peakAssignedProcessCount = 3; activeProcessLimit = 16; peakJobMemoryBytes = 1048576; aggregateMemoryLimitBytes = 2147483648
    networkConnections = @(); loopbackExemptBefore = $false; loopbackExemptAfter = $false
    moduleInventoryComplete = $true; loadedModules = @([pscustomobject]@{ pid = 1; path = 'C:\owned-runtime\program\soffice.com' })
    processes = @([pscustomobject]@{ path = 'C:\owned-runtime\program\soffice.com' }); unrelatedProcessSurvived = $true
}
$case = $validProcess.PSObject.Copy(); $case.processes = @([pscustomobject]@{ path = 'C:\Windows\unexpected.exe' }); $case.loadedModules = @([pscustomobject]@{ pid = 1; path = 'C:\Windows\unexpected.exe' })
Assert-Throws 'unexpected process descendant' 'UNEXPECTED_PROCESS_DESCENDANT' { Assert-G04DCProcessEvidence -Evidence $case -RuntimeRoot 'C:\owned-runtime' }
$case = $validProcess.PSObject.Copy(); $case.processes = @([pscustomobject]@{ path = 'C:\owned-runtime-escape\soffice.bin' }); $case.loadedModules = @([pscustomobject]@{ pid = 1; path = 'C:\owned-runtime-escape\soffice.bin' })
Assert-Throws 'sibling-prefix process escape' 'UNEXPECTED_PROCESS_DESCENDANT' { Assert-G04DCProcessEvidence -Evidence $case -RuntimeRoot 'C:\owned-runtime' }
$case = $validProcess.PSObject.Copy(); $case.processes = @([pscustomobject]@{ path = $null })
Assert-Throws 'unresolved process identity' 'UNEXPECTED_PROCESS_DESCENDANT' { Assert-G04DCProcessEvidence -Evidence $case -RuntimeRoot 'C:\owned-runtime' }
$case = $validProcess.PSObject.Copy(); $case.totalAssignedProcesses = 2
Assert-Throws 'missed short-lived descendant' 'UNEXPECTED_PROCESS_DESCENDANT' { Assert-G04DCProcessEvidence -Evidence $case -RuntimeRoot 'C:\owned-runtime' }
$case = $validProcess.PSObject.Copy(); $case.moduleInventoryComplete = $false
Assert-Throws 'incomplete module inventory' 'RUNTIME_IDENTITY_INVALID' { Assert-G04DCProcessEvidence -Evidence $case -RuntimeRoot 'C:\owned-runtime' }
$case = $validProcess.PSObject.Copy(); $case.networkConnections = @('TCP 127.0.0.1:1234')
Assert-Throws 'network attempt' 'NETWORK_ATTEMPT' { Assert-G04DCProcessEvidence -Evidence $case -RuntimeRoot 'C:\owned-runtime' }
$case = $validProcess.PSObject.Copy(); $case.loopbackExemptAfter = $true
Assert-Throws 'loopback exemption' 'NETWORK_ATTEMPT' { Assert-G04DCProcessEvidence -Evidence $case -RuntimeRoot 'C:\owned-runtime' }
$case = $validProcess.PSObject.Copy(); $case.appContainer = $false
Assert-Throws 'AppContainer failure' 'APPCONTAINER_FAILURE' { Assert-G04DCProcessEvidence -Evidence $case -RuntimeRoot 'C:\owned-runtime' }
$case = $validProcess.PSObject.Copy(); $case.breakawayAllowed = $true
Assert-Throws 'no-breakaway failure' 'NO_BREAKAWAY_FAILURE' { Assert-G04DCProcessEvidence -Evidence $case -RuntimeRoot 'C:\owned-runtime' }
$case = $validProcess.PSObject.Copy(); $case.unrelatedProcessSurvived = $false
Assert-Throws 'unrelated process survival' 'UNRELATED_PROCESS_TERMINATED' { Assert-G04DCProcessEvidence -Evidence $case -RuntimeRoot 'C:\owned-runtime' }
Assert-Throws 'loaded module root or signature rejected' 'RUNTIME_IDENTITY_INVALID' {
    Assert-G04DCLoadBearingModuleEvidence -Evidence ([pscustomobject]@{ passed = $false; modules = @([pscustomobject]@{ accepted = $false; rootClass = 'rejected' }) })
}

Assert-Throws 'output missing or corrupt' 'OUTPUT_MISSING_OR_CORRUPT' {
    Assert-G04DCOutputEvidence -Evidence ([pscustomobject]@{ regularFile = $true; reparsePoint = $false; sizeBytes = 0; magic = ''; qpdfStrict = $false; encrypted = $false; pdfjsOpened = $false; pageCount = 0 })
}
Assert-Throws 'file-access observation incomplete' 'FILE_ACCESS_BOUNDARY_INVALID' {
    Assert-G04DCFileAccessEvidence -Evidence ([pscustomobject]@{
        aclGrantSetExact = $true; ownedWritablePathInventoriesCaptured = $true; appContainerExternalStorageAbsent = $true; appContainerRegistryResidueAbsent = $true
        actualAccessTelemetryCaptured = $false; effectiveDenialOutsideAllowedRootsProven = $false
        runtimeTreeUnchanged = $true; fixtureUnchanged = $true
        probes = @([pscustomobject]@{ captured = $true })
    })
}
Assert-Throws 'cleanup ownership mismatch' 'CLEANUP_OWNERSHIP_MISMATCH' {
    Assert-G04DCCleanupEvidence -Evidence ([pscustomobject]@{ markerOwnedPathsOnly = $false; unrelatedProcessSurvived = $true })
}
$cleanupParent = [IO.Path]::GetTempPath().TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
$cleanupRoot = Join-Path $cleanupParent ('g04dc-cleanup-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $cleanupRoot | Out-Null
$cleanupMarker = Join-Path $cleanupRoot '.g04d-c-owned-root'
$cleanupMarkerContent = "G04DC-TEST-OWNED`n"
[IO.File]::WriteAllText($cleanupMarker, $cleanupMarkerContent, [Text.UTF8Encoding]::new($false))
$cleanupEvidence = Remove-G04DCOwnedRoot -OwnedRoot $cleanupRoot -MarkerPath $cleanupMarker -MarkerContent $cleanupMarkerContent -RequiredParent $cleanupParent
if (!$cleanupEvidence.markerOwnedPathsOnly -or !$cleanupEvidence.removed -or (Test-Path -LiteralPath $cleanupRoot)) { throw 'Exact marker-owned cleanup helper failed.' }
$passed.Add('marker-owned cleanup helper')

$artifactRoot = Join-Path ([IO.Path]::GetTempPath()) ('g04dc-artifact-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $artifactRoot | Out-Null
try {
    [IO.File]::WriteAllText((Join-Path $artifactRoot 'candidate-result.json'), '{}', [Text.UTF8Encoding]::new($false))
    New-G04DCArtifactManifest -EvidenceDirectory $artifactRoot | Out-Null
    [IO.File]::AppendAllText((Join-Path $artifactRoot 'candidate-result.json'), 'tamper', [Text.UTF8Encoding]::new($false))
    Assert-Throws 'artifact manifest hash mismatch' 'ARTIFACT_MANIFEST_INVALID' { Assert-G04DCArtifactManifest -EvidenceDirectory $artifactRoot }
}
finally { Remove-Item -LiteralPath $artifactRoot -Recurse -Force }

if ($passed.Count -ne 89) { throw "Expected 89 fail-closed cases; passed $($passed.Count)." }
Write-Output "G04D-C fail-closed boundary tests passed ($($passed.Count) cases): $($passed -join '; ')"
