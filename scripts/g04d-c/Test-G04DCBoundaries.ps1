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

function New-G04DCTestScheduledTaskAction {
    param(
        [Parameter(Mandatory = $true)] [string]$CimClassName,
        [System.Collections.IDictionary]$Properties = ([ordered]@{})
    )
    $action = [ordered]@{ CimClass = [pscustomobject][ordered]@{ CimClassName = $CimClassName } }
    foreach ($name in $Properties.Keys) { $action[$name] = $Properties[$name] }
    return [pscustomobject]$action
}

function New-G04DCTestScheduledTask {
    param(
        [Parameter(Mandatory = $true)] [string]$TaskPath,
        [Parameter(Mandatory = $true)] [string]$TaskName,
        [Parameter(Mandatory = $true)] [object[]]$Actions,
        [Parameter(Mandatory = $true)] [string]$DefinitionXml
    )
    return [pscustomobject][ordered]@{
        TaskPath = $TaskPath
        TaskName = $TaskName
        State = 'Ready'
        Actions = $Actions
        DefinitionXml = $DefinitionXml
    }
}

$testTaskExportAdapter = {
    param($Task, [string]$TaskName, [string]$TaskPath)
    return $Task.DefinitionXml
}
$execAction = New-G04DCTestScheduledTaskAction -CimClassName 'MSFT_TaskExecAction' -Properties ([ordered]@{
    Id = 'exec-1'; Execute = 'C:\Synthetic\exec-one.exe'; Arguments = '--synthetic'; WorkingDirectory = 'C:\Synthetic'
})
$execEvidence = ConvertTo-G04DCScheduledTaskActionEvidence -Action $execAction -Index 0
if ($execEvidence.index -ne 0 -or $execEvidence.cimClass -cne 'MSFT_TaskExecAction' -or $execEvidence.actionKind -cne 'exec' -or
    $execEvidence.properties.Execute -cne 'C:\Synthetic\exec-one.exe' -or $execEvidence.properties.Arguments -cne '--synthetic') {
    throw 'Normal executable scheduled-task action evidence is invalid.'
}
$passed.Add('normal scheduled task Exec action')

$emptyArgumentsAction = New-G04DCTestScheduledTaskAction -CimClassName 'MSFT_TaskExecAction' -Properties ([ordered]@{ Execute = 'synthetic.exe'; Arguments = ''; WorkingDirectory = 'C:\' })
$emptyArgumentsEvidence = ConvertTo-G04DCScheduledTaskActionEvidence -Action $emptyArgumentsAction -Index 0
if (!$emptyArgumentsEvidence.properties.PSObject.Properties['Arguments'] -or $emptyArgumentsEvidence.properties.Arguments -cne '') { throw 'Empty Exec Arguments were not preserved.' }
$passed.Add('scheduled task Exec empty Arguments')

$absentWorkingDirectoryAction = New-G04DCTestScheduledTaskAction -CimClassName 'MSFT_TaskExecAction' -Properties ([ordered]@{ Execute = 'synthetic.exe'; Arguments = '--synthetic' })
$absentWorkingDirectoryEvidence = ConvertTo-G04DCScheduledTaskActionEvidence -Action $absentWorkingDirectoryAction -Index 0
if ($absentWorkingDirectoryEvidence.properties.PSObject.Properties['WorkingDirectory']) { throw 'Absent Exec WorkingDirectory was serialized.' }
$passed.Add('scheduled task Exec absent WorkingDirectory')

$comAction = New-G04DCTestScheduledTaskAction -CimClassName 'MSFT_TaskComHandlerAction' -Properties ([ordered]@{ Id = 'com-1'; ClassId = '{11111111-2222-3333-4444-555555555555}'; Data = $null })
$comEvidence = ConvertTo-G04DCScheduledTaskActionEvidence -Action $comAction -Index 0
if ($comEvidence.actionKind -cne 'comHandler' -or $comEvidence.properties.PSObject.Properties['Execute']) { throw 'COM handler action incorrectly required Execute.' }
$passed.Add('scheduled task COM handler without Execute')
if ($comEvidence.properties.ClassId -cne '{11111111-2222-3333-4444-555555555555}' -or !$comEvidence.properties.PSObject.Properties['Data'] -or $null -ne $comEvidence.properties.Data) {
    throw 'COM handler ClassId/Data evidence is invalid.'
}
$passed.Add('scheduled task COM ClassId and Data')

$showMessageAction = New-G04DCTestScheduledTaskAction -CimClassName 'MSFT_TaskShowMessageAction' -Properties ([ordered]@{ Id = 'message-1'; Title = 'Synthetic title'; Message = 'Synthetic message' })
$showMessageEvidence = ConvertTo-G04DCScheduledTaskActionEvidence -Action $showMessageAction -Index 0
if ($showMessageEvidence.actionKind -cne 'showMessage' -or $showMessageEvidence.properties.Title -cne 'Synthetic title' -or $showMessageEvidence.properties.Message -cne 'Synthetic message') {
    throw 'A valid non-Exec scheduled-task property set was not collected.'
}
$passed.Add('scheduled task different valid property set')

$emailAction = New-G04DCTestScheduledTaskAction -CimClassName 'MSFT_TaskEmailAction' -Properties ([ordered]@{
    Id = 'email-1'; Server = 'smtp.example.invalid'; To = @('one@example.invalid', 'two@example.invalid'); Cc = 'copy@example.invalid'; Subject = ''
})
$emailEvidence = ConvertTo-G04DCScheduledTaskActionEvidence -Action $emailAction -Index 0
if ($emailEvidence.actionKind -cne 'email' -or $emailEvidence.properties.To -isnot [array] -or $emailEvidence.properties.To.Count -ne 2 -or
    $emailEvidence.properties.To[0] -cne 'one@example.invalid' -or $emailEvidence.properties.Cc -isnot [string] -or $emailEvidence.properties.Subject -cne '') {
    throw 'Scheduled-task scalar and array property shapes were not preserved.'
}
$passed.Add('scheduled task scalar and array shapes preserved')

$mixedTaskXml = '<Task xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task"><Actions><Exec><Command>exec-one.exe</Command></Exec><ComHandler><ClassId>{11111111-2222-3333-4444-555555555555}</ClassId></ComHandler></Actions></Task>'
$mixedTask = New-G04DCTestScheduledTask -TaskPath '\Synthetic\' -TaskName 'Mixed' -Actions @($execAction, $comAction) -DefinitionXml $mixedTaskXml
$mixedEvidence = ConvertTo-G04DCScheduledTaskEvidence -Task $mixedTask -ExportTaskAdapter $testTaskExportAdapter
if ($mixedEvidence.actions.Count -ne 2 -or $mixedEvidence.actions[0].actionKind -cne 'exec' -or $mixedEvidence.actions[1].actionKind -cne 'comHandler') {
    throw 'Mixed Exec/COM scheduled-task evidence is invalid.'
}
$passed.Add('mixed scheduled task Exec and COM actions')
if ($mixedEvidence.actions[0].index -ne 0 -or $mixedEvidence.actions[1].index -ne 1 -or $mixedEvidence.actions[0].properties.Id -cne 'exec-1' -or $mixedEvidence.actions[1].properties.Id -cne 'com-1') {
    throw 'Original scheduled-task action order was not preserved.'
}
$passed.Add('scheduled task original action order preserved')

$presentNullWorkingDirectoryAction = New-G04DCTestScheduledTaskAction -CimClassName 'MSFT_TaskExecAction' -Properties ([ordered]@{ Execute = 'synthetic.exe'; Arguments = '--synthetic'; WorkingDirectory = $null })
$presentNullWorkingDirectoryEvidence = ConvertTo-G04DCScheduledTaskActionEvidence -Action $presentNullWorkingDirectoryAction -Index 0
$absentJson = $absentWorkingDirectoryEvidence | ConvertTo-Json -Compress -Depth 8
$presentNullJson = $presentNullWorkingDirectoryEvidence | ConvertTo-Json -Compress -Depth 8
if ($absentJson.Contains('"WorkingDirectory"') -or !$presentNullJson.Contains('"WorkingDirectory":null')) { throw 'Absent and present-null scheduled-task properties were conflated.' }
$passed.Add('scheduled task property absent versus present null')
if (!$emptyArgumentsEvidence.properties.PSObject.Properties['Arguments'] -or ($emptyArgumentsEvidence | ConvertTo-Json -Compress -Depth 8) -notmatch '"Arguments":""') {
    throw 'Present empty-string scheduled-task property was not preserved.'
}
$passed.Add('scheduled task property present empty string')

$unknownAction = New-G04DCTestScheduledTaskAction -CimClassName 'Contoso_TaskOpaqueAction' -Properties ([ordered]@{ Id = 'opaque-1'; Name = 'synthetic'; OpaqueConfig = 'not-structured' })
$unknownEvidence = ConvertTo-G04DCScheduledTaskActionEvidence -Action $unknownAction -Index 0
if ($unknownEvidence.actionKind -cne 'other' -or $unknownEvidence.cimClass -cne 'Contoso_TaskOpaqueAction' -or $unknownEvidence.properties.Id -cne 'opaque-1' -or
    $unknownEvidence.properties.PSObject.Properties['OpaqueConfig']) { throw 'Unknown identifiable scheduled-task action was not safely bounded.' }
$passed.Add('unknown identifiable scheduled task CIM class')

$unknownXmlA = '<Task xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task"><Actions><Opaque><Config>A</Config></Opaque></Actions></Task>'
$unknownXmlB = '<Task xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task"><Actions><Opaque><Config>B</Config></Opaque></Actions></Task>'
$unknownTaskA = New-G04DCTestScheduledTask -TaskPath '\Synthetic\' -TaskName 'Opaque' -Actions @($unknownAction) -DefinitionXml $unknownXmlA
$unknownTaskB = New-G04DCTestScheduledTask -TaskPath '\Synthetic\' -TaskName 'Opaque' -Actions @($unknownAction) -DefinitionXml $unknownXmlB
$unknownTaskEvidenceA = ConvertTo-G04DCScheduledTaskEvidence -Task $unknownTaskA -ExportTaskAdapter $testTaskExportAdapter
$unknownTaskEvidenceB = ConvertTo-G04DCScheduledTaskEvidence -Task $unknownTaskB -ExportTaskAdapter $testTaskExportAdapter
if (($unknownTaskEvidenceA.actions | ConvertTo-Json -Compress -Depth 8) -cne ($unknownTaskEvidenceB.actions | ConvertTo-Json -Compress -Depth 8) -or
    $unknownTaskEvidenceA.definitionSha256 -ceq $unknownTaskEvidenceB.definitionSha256) { throw 'Task XML hash did not cover an unstructured unknown-action change.' }
$passed.Add('unknown scheduled task action retains XML hash coverage')

Assert-Throws 'scheduled task action class cannot be identified' 'SCHEDULED_TASK_ACTION_CAPTURE_FAILED' {
    ConvertTo-G04DCScheduledTaskActionEvidence -Action ([pscustomobject]@{ Id = 'missing-class' }) -Index 0
}
Assert-Throws 'scheduled task Export-ScheduledTask failure' 'SCHEDULED_TASK_DEFINITION_CAPTURE_FAILED' {
    ConvertTo-G04DCScheduledTaskEvidence -Task $mixedTask -ExportTaskAdapter { param($Task, $TaskName, $TaskPath) throw [System.InvalidOperationException]::new('synthetic export failure') }
}

$getterFailureAction = New-G04DCTestScheduledTaskAction -CimClassName 'MSFT_TaskExecAction' -Properties ([ordered]@{ Execute = 'synthetic.exe'; Arguments = '--synthetic' })
$getterFailureAdapter = @{
    GetProperty = { param($CandidateObject, [string]$PropertyName) $CandidateObject.PSObject.Properties[$PropertyName] }
    GetValue = {
        param($Property, [string]$PropertyName)
        if ($PropertyName -ceq 'Execute') { throw [System.InvalidOperationException]::new('synthetic getter failure') }
        return $Property.Value
    }
}
Assert-Throws 'scheduled task property getter failure' 'SCHEDULED_TASK_ACTION_CAPTURE_FAILED' {
    ConvertTo-G04DCScheduledTaskActionEvidence -Action $getterFailureAction -Index 0 -PropertyAccessAdapter $getterFailureAdapter
}

$oversizedStringAction = New-G04DCTestScheduledTaskAction -CimClassName 'MSFT_TaskExecAction' -Properties ([ordered]@{ Execute = ('x' * 16385) })
Assert-Throws 'scheduled task bounded string overflow' 'SCHEDULED_TASK_ACTION_CAPTURE_FAILED' {
    ConvertTo-G04DCScheduledTaskActionEvidence -Action $oversizedStringAction -Index 0
}
$oversizedArrayAction = New-G04DCTestScheduledTaskAction -CimClassName 'MSFT_TaskEmailAction' -Properties ([ordered]@{ To = (@('recipient@example.invalid') * 129) })
Assert-Throws 'scheduled task bounded array overflow' 'SCHEDULED_TASK_ACTION_CAPTURE_FAILED' {
    ConvertTo-G04DCScheduledTaskActionEvidence -Action $oversizedArrayAction -Index 0
}
$recursiveAction = New-G04DCTestScheduledTaskAction -CimClassName 'MSFT_TaskComHandlerAction' -Properties ([ordered]@{ ClassId = '{11111111-2222-3333-4444-555555555555}'; Data = [pscustomobject]@{ nested = 'prohibited' } })
Assert-Throws 'scheduled task recursive object serialization' 'SCHEDULED_TASK_ACTION_CAPTURE_FAILED' {
    ConvertTo-G04DCScheduledTaskActionEvidence -Action $recursiveAction -Index 0
}
$nonFiniteAction = New-G04DCTestScheduledTaskAction -CimClassName 'MSFT_TaskEmailAction' -Properties ([ordered]@{ Server = [double]::NaN })
Assert-Throws 'scheduled task non-finite primitive serialization' 'SCHEDULED_TASK_ACTION_CAPTURE_FAILED' {
    ConvertTo-G04DCScheduledTaskActionEvidence -Action $nonFiniteAction -Index 0
}

$deterministicJsonA = ConvertTo-G04DCScheduledTaskActionEvidence -Action $comAction -Index 3 | ConvertTo-Json -Compress -Depth 8
$deterministicJsonB = ConvertTo-G04DCScheduledTaskActionEvidence -Action $comAction -Index 3 | ConvertTo-Json -Compress -Depth 8
if ($deterministicJsonA -cne $deterministicJsonB -or (Get-G04DCCanonicalHash -Rows @($deterministicJsonA)) -cne (Get-G04DCCanonicalHash -Rows @($deterministicJsonB))) {
    throw 'Repeated scheduled-task action serialization is not deterministic.'
}
$passed.Add('scheduled task deterministic repeated serialization')

$changedExecAction = New-G04DCTestScheduledTaskAction -CimClassName 'MSFT_TaskExecAction' -Properties ([ordered]@{ Id = 'exec-1'; Execute = 'C:\Synthetic\exec-two.exe'; Arguments = '--synthetic'; WorkingDirectory = 'C:\Synthetic' })
$changedTaskXml = $mixedTaskXml.Replace('exec-one.exe', 'exec-two.exe')
$changedTask = New-G04DCTestScheduledTask -TaskPath '\Synthetic\' -TaskName 'Mixed' -Actions @($changedExecAction, $comAction) -DefinitionXml $changedTaskXml
$changedEvidence = ConvertTo-G04DCScheduledTaskEvidence -Task $changedTask -ExportTaskAdapter $testTaskExportAdapter
if ($mixedEvidence.definitionSha256 -ceq $changedEvidence.definitionSha256 -or
    ($mixedEvidence.actions | ConvertTo-Json -Compress -Depth 8) -ceq ($changedEvidence.actions | ConvertTo-Json -Compress -Depth 8)) {
    throw 'Changed scheduled-task action did not change both definition hash and structured evidence.'
}
$passed.Add('changed scheduled task action changes definition evidence')

$comActionClone = New-G04DCTestScheduledTaskAction -CimClassName 'MSFT_TaskComHandlerAction' -Properties ([ordered]@{ Id = 'com-1'; ClassId = '{11111111-2222-3333-4444-555555555555}'; Data = $null })
$comCloneJson = ConvertTo-G04DCScheduledTaskActionEvidence -Action $comActionClone -Index 0 | ConvertTo-Json -Compress -Depth 8
if (($comEvidence | ConvertTo-Json -Compress -Depth 8) -cne $comCloneJson) { throw 'Unchanged heterogeneous scheduled-task actions compared unequal.' }
$passed.Add('unchanged heterogeneous scheduled task action compares equal')

$sourceBefore = $unknownAction | ConvertTo-Json -Compress -Depth 8
ConvertTo-G04DCScheduledTaskActionEvidence -Action $unknownAction -Index 0 | Out-Null
$sourceAfter = $unknownAction | ConvertTo-Json -Compress -Depth 8
$commonSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'G04DC.Common.psm1') -Raw
if ($sourceBefore -cne $sourceAfter -or $commonSource -match '(?i)(Register|Set|Unregister|Disable|Enable)-ScheduledTask') {
    throw 'Scheduled-task evidence collection mutated input or contains a task mutation command.'
}
$passed.Add('scheduled task collection performs no mutation')

$runnerShapedComAction = New-G04DCTestScheduledTaskAction -CimClassName 'MSFT_TaskComHandlerAction' -Properties ([ordered]@{
    Id = $null
    ClassId = '{7D096C5F-AC08-4F1F-BEB7-5C22C517CE39}'
    Data = ''
    PSComputerName = 'fv-azrunner-win'
    RunspaceId = [guid]'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee'
    CimSystemProperties = [pscustomobject]@{ Namespace = 'Root/Microsoft/Windows/TaskScheduler' }
})
$runnerShapedEvidence = ConvertTo-G04DCScheduledTaskActionEvidence -Action $runnerShapedComAction -Index 0
$runnerShapedJson = $runnerShapedEvidence | ConvertTo-Json -Compress -Depth 8
if ($runnerShapedEvidence.actionKind -cne 'comHandler' -or $runnerShapedJson.Contains('Execute') -or $runnerShapedJson.Contains('PSComputerName') -or $runnerShapedJson.Contains('RunspaceId') -or $runnerShapedJson.Contains('CimSystemProperties')) {
    throw 'GitHub-hosted runner-shaped non-Exec action was not safely serialized.'
}
$passed.Add('GitHub runner shaped non-Exec scheduled task action')

if ($commonSource -match '\$_\.Execute|\$action\.Execute|\$Action\.Execute') { throw 'Direct .Execute assumption remains in scheduled-task catalog collection.' }
$passed.Add('no direct Execute assumption in scheduled task catalog')

$taskA = New-G04DCTestScheduledTask -TaskPath '\Zeta\' -TaskName 'Alpha' -Actions @($execAction) -DefinitionXml '<Task xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task"><Actions><Exec /></Actions></Task>'
$taskB = New-G04DCTestScheduledTask -TaskPath '\Alpha\' -TaskName 'Zulu' -Actions @($execAction) -DefinitionXml '<Task xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task"><Actions><Exec /></Actions></Task>'
$taskC = New-G04DCTestScheduledTask -TaskPath '\Alpha\' -TaskName 'Alpha' -Actions @($execAction) -DefinitionXml '<Task xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task"><Actions><Exec /></Actions></Task>'
$orderedTasks = @(Get-G04DCScheduledTaskCatalogEvidence -Tasks @($taskA, $taskB, $taskC) -ExportTaskAdapter $testTaskExportAdapter)
if (($orderedTasks | ForEach-Object { "$($_.taskPath)$($_.taskName)" }) -join '|' -cne '\Alpha\Alpha|\Alpha\Zulu|\Zeta\Alpha') {
    throw 'Scheduled-task catalog ordering is not TaskPath plus TaskName.'
}
$passed.Add('scheduled task TaskPath and TaskName ordering')

function New-State(
    [string]$FontValue = 'baseline',
    [string]$Service = '',
    [string]$Association = 'Word.OpenDocumentText.12',
    [AllowNull()] $AssociationState,
    [bool]$Profile = $false
) {
    if ($null -eq $AssociationState) {
        $AssociationState = [pscustomobject][ordered]@{
            schemaVersion = 1
            keyExists = $true
            defaultValuePresent = $true
            defaultValueType = 'String'
            defaultValue = $Association
        }
    }
    return [pscustomobject][ordered]@{
        fontCatalogCount = 1; fontCatalogSha256 = $FontValue; msiFontTargets = @(); externalRuntimeTargets = @()
        associations = @([pscustomobject]@{
            extension = '.odt'
            classDefaultState = $AssociationState
            userChoiceProgIdState = [pscustomobject][ordered]@{ schemaVersion = 1; keyExists = $false; valueName = 'ProgId'; valuePresent = $false }
            userChoicePresent = $false
        })
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

function Get-TestRegistryRawSnapshot([string]$NativeSubKey) {
    $root = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($NativeSubKey, $false)
    if (!$root) { return 'missing' }
    $queue = [System.Collections.Generic.Queue[object]]::new()
    $queue.Enqueue([pscustomobject]@{ key = $root; path = '' })
    $rows = [System.Collections.Generic.List[object]]::new()
    while ($queue.Count -ne 0) {
        $entry = $queue.Dequeue()
        try {
            $rows.Add([pscustomobject][ordered]@{ path = $entry.path; keyPresent = $true })
            foreach ($name in @($entry.key.GetValueNames() | Sort-Object)) {
                $kind = [string]$entry.key.GetValueKind($name)
                $value = $entry.key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
                if ($value -is [byte[]]) { $value = ([BitConverter]::ToString($value)).Replace('-', '').ToLowerInvariant() }
                $rows.Add([pscustomobject][ordered]@{ path = $entry.path; name = $name; kind = $kind; value = $value })
            }
            foreach ($subKeyName in @($entry.key.GetSubKeyNames() | Sort-Object)) {
                $child = $entry.key.OpenSubKey($subKeyName, $false)
                if (!$child) { throw 'Test-owned registry key disappeared during raw non-mutation snapshot.' }
                $childPath = if ([string]::IsNullOrEmpty([string]$entry.path)) { $subKeyName } else { "$($entry.path)\$subKeyName" }
                $queue.Enqueue([pscustomobject]@{ key = $child; path = $childPath })
            }
        }
        finally { $entry.key.Dispose() }
    }
    return Get-G04DCCanonicalHash -Rows @($rows.ToArray())
}

$registryTestId = [guid]::NewGuid().ToString('N')
$registryTestNativeRoot = "Software\DocumentStudio-G04DC-Registry-State-$registryTestId"
$registryTestProviderRoot = "Registry::HKEY_CURRENT_USER\$registryTestNativeRoot"
$registryRootHandle = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($registryTestNativeRoot)
if (!$registryRootHandle) { throw 'Could not create the GUID-owned registry test root.' }
$registryRootHandle.Dispose()
try {
    foreach ($keyName in @('no-default', 'empty-default', 'normal-default', 'dword-default', '.odt', '.ods', '.odp')) {
        $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey("$registryTestNativeRoot\$keyName")
        if (!$key) { throw "Could not create test-owned registry key $keyName." }
        try {
            switch ($keyName) {
                'empty-default' { $key.SetValue('', '', [Microsoft.Win32.RegistryValueKind]::String) }
                'normal-default' { $key.SetValue('', 'DocumentStudio.G04DC.Test', [Microsoft.Win32.RegistryValueKind]::String) }
                'dword-default' { $key.SetValue('', 42, [Microsoft.Win32.RegistryValueKind]::DWord) }
            }
        }
        finally { $key.Dispose() }
    }

    $missingDefaultState = Get-G04DCRegistryDefaultValueState -Path "$registryTestProviderRoot\missing"
    if ($missingDefaultState.keyExists -or $missingDefaultState.defaultValuePresent) { throw 'Missing-key registry state was not classified as state A.' }
    $passed.Add('registry key missing state')

    $noDefaultState = Get-G04DCRegistryDefaultValueState -Path "$registryTestProviderRoot\no-default"
    if (!$noDefaultState.keyExists -or $noDefaultState.defaultValuePresent -or $noDefaultState.PSObject.Properties['defaultValue']) { throw 'Existing key without a default was not classified as state B.' }
    $passed.Add('registry key exists without default state')

    $emptyDefaultState = Get-G04DCRegistryDefaultValueState -Path "$registryTestProviderRoot\empty-default"
    if (!$emptyDefaultState.keyExists -or !$emptyDefaultState.defaultValuePresent -or $emptyDefaultState.defaultValueType -cne 'String' -or $emptyDefaultState.defaultValue -cne '') { throw 'Empty REG_SZ default was not preserved as present.' }
    $passed.Add('empty-string registry default present')

    $normalDefaultState = Get-G04DCRegistryDefaultValueState -Path "$registryTestProviderRoot\normal-default"
    if (!$normalDefaultState.defaultValuePresent -or $normalDefaultState.defaultValueType -cne 'String' -or $normalDefaultState.defaultValue -cne 'DocumentStudio.G04DC.Test') { throw 'Normal REG_SZ default was not preserved.' }
    $passed.Add('normal REG_SZ registry default')

    $dwordDefaultState = Get-G04DCRegistryDefaultValueState -Path "$registryTestProviderRoot\dword-default"
    if (!$dwordDefaultState.defaultValuePresent -or $dwordDefaultState.defaultValueType -cne 'DWord' -or [int]$dwordDefaultState.defaultValue -ne 42) { throw 'Legitimate non-string registry kind was not preserved.' }
    $passed.Add('typed non-string registry default')

    $raceHandle = [pscustomobject]@{ enumerationCount = 0 }
    $raceAdapter = @{
        OpenKey = { param($Path) [pscustomobject]@{ keyExists = $true; handle = $raceHandle } }
        GetValueNames = {
            param($Handle)
            $Handle.enumerationCount++
            if ($Handle.enumerationCount -eq 1) { @('') } else { @() }
        }
        GetValueKind = { param($Handle, $Name) throw [System.IO.IOException]::new('simulated disappearance') }
        GetValue = { param($Handle, $Name) [pscustomobject]@{ present = $false; value = $null } }
        CloseKey = { param($Handle) }
    }
    $raceState = Get-G04DCRegistryDefaultValueState -Path "$registryTestProviderRoot\race" -AccessAdapter $raceAdapter
    if (!$raceState.keyExists -or $raceState.defaultValuePresent) { throw 'Value disappearance was not reclassified as an existing key without a default.' }
    $passed.Add('registry value disappears during read')

    $accessDeniedAdapter = @{
        OpenKey = { param($Path) throw [System.UnauthorizedAccessException]::new('simulated access denied') }
        GetValueNames = { param($Handle) @() }
        GetValueKind = { param($Handle, $Name) 'String' }
        GetValue = { param($Handle, $Name) [pscustomobject]@{ present = $false; value = $null } }
        CloseKey = { param($Handle) }
    }
    Assert-Throws 'registry access denied simulation' 'REGISTRY_STATE_CAPTURE_FAILED' {
        Get-G04DCRegistryDefaultValueState -Path "$registryTestProviderRoot\access-denied" -AccessAdapter $accessDeniedAdapter
    }

    $providerFailureAdapter = @{
        OpenKey = { param($Path) throw [System.InvalidOperationException]::new('simulated provider failure') }
        GetValueNames = { param($Handle) @() }
        GetValueKind = { param($Handle, $Name) 'String' }
        GetValue = { param($Handle, $Name) [pscustomobject]@{ present = $false; value = $null } }
        CloseKey = { param($Handle) }
    }
    Assert-Throws 'unexpected registry provider failure' 'REGISTRY_STATE_CAPTURE_FAILED' {
        Get-G04DCRegistryDefaultValueState -Path "$registryTestProviderRoot\provider-failure" -AccessAdapter $providerFailureAdapter
    }

    $odtAbsentState = Get-G04DCRegistryDefaultValueState -Path "$registryTestProviderRoot\.odt"
    if (!$odtAbsentState.keyExists -or $odtAbsentState.defaultValuePresent) { throw '.odt runner-equivalent missing-default state was not preserved.' }
    $passed.Add('.odt exact missing-default runner condition')

    $odsAbsentState = Get-G04DCRegistryDefaultValueState -Path "$registryTestProviderRoot\.ods"
    $odpAbsentState = Get-G04DCRegistryDefaultValueState -Path "$registryTestProviderRoot\.odp"
    if (!$odsAbsentState.keyExists -or $odsAbsentState.defaultValuePresent -or !$odpAbsentState.keyExists -or $odpAbsentState.defaultValuePresent) { throw '.ods/.odp missing-default states were not preserved.' }
    $passed.Add('.ods and .odp missing-default handling')

    $missingJson = $missingDefaultState | ConvertTo-Json -Compress
    $absentJson = $noDefaultState | ConvertTo-Json -Compress
    $emptyJson = $emptyDefaultState | ConvertTo-Json -Compress
    if ($missingJson -ceq $absentJson -or $absentJson -ceq $emptyJson -or $missingJson -ceq $emptyJson -or
        $absentJson.Contains('"defaultValue":') -or !$emptyJson.Contains('"defaultValue":""')) {
        throw 'Serialized registry evidence does not distinguish missing key, absent default, and present empty default.'
    }
    $passed.Add('registry state serialization distinction')

    $identicalAbsentComparison = Compare-G04DCMachineState -Before (New-State -AssociationState $odtAbsentState) -After (New-State -AssociationState (Get-G04DCRegistryDefaultValueState -Path "$registryTestProviderRoot\.odt"))
    if ($identicalAbsentComparison.protectedMutation) { throw 'Identical absent-default states compared unequal.' }
    $passed.Add('identical absent registry defaults compare equal')

    $odtKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("$registryTestNativeRoot\.odt", $true)
    try { $odtKey.SetValue('', 'LibreOffice.WriterDocument.1', [Microsoft.Win32.RegistryValueKind]::String) }
    finally { $odtKey.Dispose() }
    $odtValueState = Get-G04DCRegistryDefaultValueState -Path "$registryTestProviderRoot\.odt"
    $noDefaultToValue = Compare-G04DCMachineState -Before (New-State -AssociationState $odtAbsentState) -After (New-State -AssociationState $odtValueState)
    if (!$noDefaultToValue.protectedMutation -or @($noDefaultToValue.changes | Where-Object { $_.boundary -ceq 'associations' }).Count -ne 1) { throw 'No-default to value transition was not detected.' }
    $passed.Add('registry no-default to value transition detected')

    $odtKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("$registryTestNativeRoot\.odt", $true)
    try { $odtKey.DeleteValue('', $false) }
    finally { $odtKey.Dispose() }
    $odtAbsentAgain = Get-G04DCRegistryDefaultValueState -Path "$registryTestProviderRoot\.odt"
    $valueToNoDefault = Compare-G04DCMachineState -Before (New-State -AssociationState $odtValueState) -After (New-State -AssociationState $odtAbsentAgain)
    if (!$valueToNoDefault.protectedMutation -or @($valueToNoDefault.changes | Where-Object { $_.boundary -ceq 'associations' }).Count -ne 1) { throw 'Value to no-default transition was not detected.' }
    $passed.Add('registry value to no-default transition detected')

    $rawRegistryBefore = Get-TestRegistryRawSnapshot -NativeSubKey $registryTestNativeRoot
    foreach ($path in @('no-default', 'empty-default', 'normal-default', 'dword-default', '.odt', '.ods', '.odp')) {
        Get-G04DCRegistryDefaultValueState -Path "$registryTestProviderRoot\$path" | Out-Null
    }
    $rawRegistryAfter = Get-TestRegistryRawSnapshot -NativeSubKey $registryTestNativeRoot
    if ($rawRegistryBefore -cne $rawRegistryAfter) { throw 'Read-only registry helper mutated the GUID-owned test tree.' }
    $passed.Add('registry collector performs no mutation')
}
finally {
    if (Test-Path -LiteralPath $registryTestProviderRoot) {
        Remove-Item -LiteralPath $registryTestProviderRoot -Recurse -Force -ErrorAction Stop
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

if ($passed.Count -ne 131) { throw "Expected 131 fail-closed cases; passed $($passed.Count)." }
Write-Output "G04D-C fail-closed boundary tests passed ($($passed.Count) cases): $($passed -join '; ')"
