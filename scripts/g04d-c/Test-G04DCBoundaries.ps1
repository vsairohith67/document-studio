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
if ($comEvidence.properties.ClassId -cne '{11111111-2222-3333-4444-555555555555}' -or !$comEvidence.properties.PSObject.Properties['Data'] -or
    $comEvidence.properties.Data.valueShape -cne 'null' -or $comEvidence.properties.Data.sha256 -notmatch '^[0-9a-f]{64}$') {
    throw 'COM handler ClassId/Data evidence is invalid.'
}
$passed.Add('scheduled task COM ClassId and Data')

$showMessageAction = New-G04DCTestScheduledTaskAction -CimClassName 'MSFT_TaskShowMessageAction' -Properties ([ordered]@{ Id = 'message-1'; Title = 'Synthetic title'; Message = 'Synthetic message' })
$showMessageEvidence = ConvertTo-G04DCScheduledTaskActionEvidence -Action $showMessageAction -Index 0
if ($showMessageEvidence.actionKind -cne 'showMessage' -or $showMessageEvidence.properties.Title.valueShape -cne 'string' -or
    $showMessageEvidence.properties.Message.valueShape -cne 'string' -or ($showMessageEvidence | ConvertTo-Json -Compress -Depth 8) -match 'Synthetic (title|message)') {
    throw 'A valid non-Exec scheduled-task property set was not collected.'
}
$passed.Add('scheduled task different valid property set')

$emailAction = New-G04DCTestScheduledTaskAction -CimClassName 'MSFT_TaskEmailAction' -Properties ([ordered]@{
    Id = 'email-1'; Server = 'smtp.example.invalid'; To = @('one@example.invalid', 'two@example.invalid'); Cc = 'copy@example.invalid'; Subject = ''
})
$emailEvidence = ConvertTo-G04DCScheduledTaskActionEvidence -Action $emailAction -Index 0
if ($emailEvidence.actionKind -cne 'email' -or $emailEvidence.properties.To.valueShape -cne 'array' -or $emailEvidence.properties.To.memberCount -ne 2 -or
    ($emailEvidence.properties.To.memberShapes -join '|') -cne 'string|string' -or $emailEvidence.properties.Cc.valueShape -cne 'string' -or
    $emailEvidence.properties.Subject.valueShape -cne 'emptyString') {
    throw 'Scheduled-task scalar and array property shapes were not preserved.'
}
$passed.Add('scheduled task scalar and array shapes preserved')
$emailJson = $emailEvidence | ConvertTo-Json -Compress -Depth 8
if ($emailJson -match 'smtp\.example\.invalid|one@example\.invalid|two@example\.invalid|copy@example\.invalid') {
    throw 'Sensitive scheduled-task email data was serialized in plaintext.'
}
$passed.Add('scheduled task sensitive email values are hash only')

$nonEmptyComAction = New-G04DCTestScheduledTaskAction -CimClassName 'MSFT_TaskComHandlerAction' -Properties ([ordered]@{
    Id = 'com-sensitive'; ClassId = '{11111111-2222-3333-4444-555555555555}'; Data = 'synthetic secret payload'
})
$nonEmptyComEvidence = ConvertTo-G04DCScheduledTaskActionEvidence -Action $nonEmptyComAction -Index 0
$nonEmptyComJson = $nonEmptyComEvidence | ConvertTo-Json -Compress -Depth 8
if ($nonEmptyComEvidence.properties.Data.valueShape -cne 'string' -or $nonEmptyComEvidence.properties.Data.sha256 -notmatch '^[0-9a-f]{64}$' -or
    $nonEmptyComJson.Contains('synthetic secret payload')) {
    throw 'Sensitive scheduled-task COM data was not retained as hash-only evidence.'
}
$passed.Add('scheduled task sensitive COM data is hash only')

$changedNonEmptyComAction = New-G04DCTestScheduledTaskAction -CimClassName 'MSFT_TaskComHandlerAction' -Properties ([ordered]@{
    Id = 'com-sensitive'; ClassId = '{11111111-2222-3333-4444-555555555555}'; Data = 'changed synthetic secret payload'
})
$changedNonEmptyComEvidence = ConvertTo-G04DCScheduledTaskActionEvidence -Action $changedNonEmptyComAction -Index 0
if ($nonEmptyComEvidence.properties.Data.sha256 -ceq $changedNonEmptyComEvidence.properties.Data.sha256) {
    throw 'Changed sensitive scheduled-task action data did not change its deterministic hash evidence.'
}
$passed.Add('changed sensitive scheduled task value changes hash evidence')

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
if ($runnerShapedEvidence.actionKind -cne 'comHandler' -or $runnerShapedEvidence.properties.Data.valueShape -cne 'emptyString' -or
    $runnerShapedJson.Contains('Execute') -or $runnerShapedJson.Contains('PSComputerName') -or $runnerShapedJson.Contains('RunspaceId') -or $runnerShapedJson.Contains('CimSystemProperties')) {
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

$telemetryRoot = Join-Path ([IO.Path]::GetTempPath()) ('g04dc-telemetry-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $telemetryRoot | Out-Null
$context = $failureContext = $phaseBudgetContext = $overallBudgetContext = $serializationSuccessContext = $serializationContext = $helperSuccessContext = $helperContext = $successGateContext = $hashContext = $null
try {
    $progressPath = Join-Path $telemetryRoot 'progress.ndjson'
    $performancePath = Join-Path $telemetryRoot 'performance.json'
    $context = New-G04DCMachineStateCaptureContext -CaptureLabel 'synthetic-pre' -ProgressPath $progressPath -PerformancePath $performancePath
    if (!(Test-Path -LiteralPath $progressPath -PathType Leaf) -or (Get-Item -LiteralPath $progressPath).Length -ne 0) {
        throw 'Progress evidence was not created before the first phase.'
    }
    $passed.Add('machine-state progress created before first phase')
    Start-G04DCMachineStatePhase -Context $context -Phase 'synthetic-phase'
    if (@(Get-Content -LiteralPath $progressPath).Count -ne 1) { throw 'Phase-start progress was not durably readable before capture completion.' }
    $passed.Add('machine-state progress flush survives in-flight capture')
    Assert-G04DCMachineStateCaptureBudget -Context $context -Phase 'synthetic-phase' -ItemCount 7 -WriteProgress
    Complete-G04DCMachineStatePhase -Context $context -Phase 'synthetic-phase' -ItemCount 9
    Complete-G04DCMachineStateCapture -Context $context -Passed $true -FailureMessage $null
    $records = @(Get-Content -LiteralPath $progressPath | ForEach-Object { $_ | ConvertFrom-Json })
    if (($records.event -join '|') -cne 'phase-start|phase-progress|phase-end|capture-end') { throw 'Phase progress ordering is invalid.' }
    $passed.Add('machine-state phase start and end ordering')
    if (($records.sequence -join '|') -cne '1|2|3|4') { throw 'Progress sequence is not strictly increasing.' }
    $passed.Add('machine-state progress sequence monotonic')
    for ($recordIndex = 1; $recordIndex -lt $records.Count; $recordIndex++) {
        if ([long]$records[$recordIndex].elapsedMilliseconds -lt [long]$records[$recordIndex - 1].elapsedMilliseconds) { throw 'Progress elapsed time moved backwards.' }
    }
    $passed.Add('machine-state progress elapsed monotonic')
    if (@($records | Where-Object { $_.captureLabel -cne 'synthetic-pre' }).Count -ne 0 -or @($records | Select-Object -ExpandProperty captureId -Unique).Count -ne 1) {
        throw 'Capture labels or IDs are inconsistent.'
    }
    $passed.Add('machine-state progress capture labels')
    $progressJson = $records | ConvertTo-Json -Compress -Depth 8
    if ($progressJson -match '(?i)synthetic-secret-value|Registry::|HKEY_|C:\\Users') { throw 'Progress evidence contains captured values or unrelated paths.' }
    $passed.Add('machine-state progress excludes captured values')
    $performance = Get-Content -LiteralPath $performancePath -Raw | ConvertFrom-Json
    if (![bool]$performance.passed -or $performance.captureLabel -cne 'synthetic-pre' -or @($performance.phases).Count -ne 1 -or
        [long]$performance.captureTargetMilliseconds -ne 480000 -or [long]$performance.hardCeilingMilliseconds -ne 720000 -or [long]$performance.phaseCeilingMilliseconds -ne 240000) {
        throw 'Machine-state performance artifact is incomplete.'
    }
    $passed.Add('machine-state performance artifact schema')

    $failureProgress = Join-Path $telemetryRoot 'failure-progress.ndjson'
    $failurePerformance = Join-Path $telemetryRoot 'failure-performance.json'
    $failureContext = New-G04DCMachineStateCaptureContext -CaptureLabel 'synthetic-failure' -ProgressPath $failureProgress -PerformancePath $failurePerformance
    Start-G04DCMachineStatePhase -Context $failureContext -Phase 'synthetic-failure-phase'
    Complete-G04DCMachineStateCapture -Context $failureContext -Passed $false -FailureMessage '[SYNTHETIC_FAILURE] bounded'
    $failureRecords = @(Get-Content -LiteralPath $failureProgress | ForEach-Object { $_ | ConvertFrom-Json })
    if ($failureRecords[-1].event -cne 'capture-failure' -or $failureRecords[-1].status -cne 'failed' -or
        (Get-Content -LiteralPath $failurePerformance -Raw | ConvertFrom-Json).passed) {
        throw 'Capture failure telemetry was not sealed.'
    }
    $passed.Add('machine-state capture failure record')

    $phaseBudgetContext = New-G04DCMachineStateCaptureContext -CaptureLabel 'phase-budget' -ProgressPath (Join-Path $telemetryRoot 'phase-budget.ndjson') -PerformancePath (Join-Path $telemetryRoot 'phase-budget.json') -CaptureTargetMilliseconds 1 -OverallBudgetMilliseconds 1000 -PhaseBudgetMilliseconds 50
    Start-G04DCMachineStatePhase -Context $phaseBudgetContext -Phase 'bounded-phase'
    Start-Sleep -Milliseconds 100
    Assert-Throws 'machine-state phase budget exceeded' 'MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED' {
        Assert-G04DCMachineStateCaptureBudget -Context $phaseBudgetContext -Phase 'bounded-phase' -ItemCount 11
    }
    Complete-G04DCMachineStateCapture -Context $phaseBudgetContext -Passed $false -FailureMessage '[MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED] phase=bounded-phase'

    $overallBudgetContext = New-G04DCMachineStateCaptureContext -CaptureLabel 'overall-budget' -ProgressPath (Join-Path $telemetryRoot 'overall-budget.ndjson') -PerformancePath (Join-Path $telemetryRoot 'overall-budget.json') -CaptureTargetMilliseconds 1 -OverallBudgetMilliseconds 200 -PhaseBudgetMilliseconds 150
    Start-G04DCMachineStatePhase -Context $overallBudgetContext -Phase 'bounded-first'
    Start-Sleep -Milliseconds 100
    Complete-G04DCMachineStatePhase -Context $overallBudgetContext -Phase 'bounded-first' -ItemCount 1
    Start-G04DCMachineStatePhase -Context $overallBudgetContext -Phase 'bounded-overall'
    Start-Sleep -Milliseconds 120
    Assert-Throws 'machine-state overall budget exceeded' 'MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED' {
        Assert-G04DCMachineStateCaptureBudget -Context $overallBudgetContext -Phase 'bounded-overall' -ItemCount 13
    }
    Complete-G04DCMachineStateCapture -Context $overallBudgetContext -Passed $false -FailureMessage '[MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED] phase=bounded-overall'

    $module = Get-Module G04DC.Common
    $serializationSuccessPath = Join-Path $telemetryRoot 'machine-success.json'
    $serializationSuccessContext = New-G04DCMachineStateCaptureContext -CaptureLabel 'serialization-success' -ProgressPath (Join-Path $telemetryRoot 'serialization-success.ndjson') -PerformancePath (Join-Path $telemetryRoot 'serialization-success-performance.json') -CaptureTargetMilliseconds 1000 -OverallBudgetMilliseconds 5000 -PhaseBudgetMilliseconds 3000
    Start-G04DCMachineStatePhase -Context $serializationSuccessContext -Phase 'state-serialization'
    $unicodeSerializationValue = -join @([char]0x0928, [char]0x092E, [char]0x0938, [char]0x094D, [char]0x0924, [char]0x0947)
    & $module { param($Context, [string]$OutputPath, [string]$UnicodeValue) Write-G04DCBoundedMachineStateJson -Path $OutputPath -Value ([pscustomobject][ordered]@{ schemaVersion = 1; bounded = $true; values = @('alpha', '', 7, $true, $null); nested = [pscustomobject][ordered]@{ unicode = $UnicodeValue; map = [ordered]@{ present = $true; value = 'x' } } }) -CaptureContext $Context -CapturePhase 'state-serialization' } $serializationSuccessContext $serializationSuccessPath $unicodeSerializationValue
    Complete-G04DCMachineStatePhase -Context $serializationSuccessContext -Phase 'state-serialization' -ItemCount 1
    Complete-G04DCMachineStateCapture -Context $serializationSuccessContext -Passed $true -FailureMessage $null
    $serializationSuccess = Get-Content -LiteralPath $serializationSuccessPath -Raw -Encoding UTF8 | ConvertFrom-Json
    if (!(Test-Path -LiteralPath $serializationSuccessPath -PathType Leaf) -or ![bool]$serializationSuccess.bounded -or @($serializationSuccess.values).Count -ne 5 -or [string]$serializationSuccess.nested.unicode -cne $unicodeSerializationValue -or ![bool]$serializationSuccess.nested.map.present) {
        throw 'Bounded machine-state serialization did not durably publish its successful JSON output.'
    }
    $passed.Add('machine-state bounded serialization publishes durable JSON')

    $serializationPath = Join-Path $telemetryRoot 'machine-pre.json'
    $serializationProgress = Join-Path $telemetryRoot 'serialization-progress.ndjson'
    $serializationPerformance = Join-Path $telemetryRoot 'serialization-performance.json'
    $serializationContext = New-G04DCMachineStateCaptureContext -CaptureLabel 'serialization-budget' -ProgressPath $serializationProgress -PerformancePath $serializationPerformance -CaptureTargetMilliseconds 1 -OverallBudgetMilliseconds 1000 -PhaseBudgetMilliseconds 50
    Start-G04DCMachineStatePhase -Context $serializationContext -Phase 'state-serialization'
    Assert-Throws 'machine-state serialization helper budget exceeded' 'MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED' {
        & $module { param($Context, [string]$OutputPath) Write-G04DCBoundedMachineStateJson -Path $OutputPath -Value ([pscustomobject]@{ bounded = ('x' * 1000000) }) -CaptureContext $Context -CapturePhase 'state-serialization' } $serializationContext $serializationPath
    }
    Complete-G04DCMachineStateCapture -Context $serializationContext -Passed $false -FailureMessage '[MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED] phase=state-serialization'
    $serializationRecords = @(Get-Content -LiteralPath $serializationProgress | ForEach-Object { $_ | ConvertFrom-Json })
    $serializationEvidence = Get-Content -LiteralPath $serializationPerformance -Raw | ConvertFrom-Json
    if ((Test-Path -LiteralPath $serializationPath) -or [bool]$serializationEvidence.passed -or $serializationEvidence.failurePhase -cne 'state-serialization' -or
        $serializationRecords[-1].event -cne 'capture-failure' -or $serializationRecords[-1].status -cne 'budget-exceeded') {
        throw 'Timed-out machine-state serialization produced success or lost durable failure evidence.'
    }
    $passed.Add('machine-state serialization failure is durable and non-successful')

    $canonicalRows = @(
        [pscustomobject][ordered]@{ path = 'zeta'; value = 3 },
        [pscustomobject][ordered]@{ path = 'Alpha'; value = 1 },
        [pscustomobject][ordered]@{ path = 'middle'; value = @('x', '', $null) }
    )
    $legacyCanonicalHash = Get-G04DCCanonicalHash -Rows $canonicalRows
    $canonicalSuccessContext = New-G04DCMachineStateCaptureContext -CaptureLabel 'canonical-success' -ProgressPath (Join-Path $telemetryRoot 'canonical-success.ndjson') -PerformancePath (Join-Path $telemetryRoot 'canonical-success.json') -CaptureTargetMilliseconds 1000 -OverallBudgetMilliseconds 5000 -PhaseBudgetMilliseconds 3000
    Start-G04DCMachineStatePhase -Context $canonicalSuccessContext -Phase 'canonical-hash'
    $boundedCanonicalHash = Get-G04DCCanonicalHash -Rows $canonicalRows -CaptureContext $canonicalSuccessContext -CapturePhase 'canonical-hash'
    Complete-G04DCMachineStatePhase -Context $canonicalSuccessContext -Phase 'canonical-hash' -ItemCount $canonicalRows.Count
    Complete-G04DCMachineStateCapture -Context $canonicalSuccessContext -Passed $true -FailureMessage $null
    if ($boundedCanonicalHash -cne $legacyCanonicalHash) { throw 'Bounded canonical hashing changed the legacy sorted-row digest.' }
    $passed.Add('machine-state bounded canonical hash preserves legacy digest')

    $canonicalBudgetProgress = Join-Path $telemetryRoot 'canonical-budget.ndjson'
    $canonicalBudgetPerformance = Join-Path $telemetryRoot 'canonical-budget-performance.json'
    $canonicalBudgetSuccess = Join-Path $telemetryRoot 'canonical-budget-success.json'
    $canonicalBudgetRows = @(0..9999 | ForEach-Object { [pscustomobject][ordered]@{ index = $_; value = 'bounded-canonical-row' } })
    $canonicalBudgetContext = New-G04DCMachineStateCaptureContext -CaptureLabel 'canonical-budget' -ProgressPath $canonicalBudgetProgress -PerformancePath $canonicalBudgetPerformance -CaptureTargetMilliseconds 50 -OverallBudgetMilliseconds 1000 -PhaseBudgetMilliseconds 100
    Start-G04DCMachineStatePhase -Context $canonicalBudgetContext -Phase 'canonical-hash'
    Assert-Throws 'machine-state canonical hash budget exceeded' 'MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED' {
        Get-G04DCCanonicalHash -Rows $canonicalBudgetRows -CaptureContext $canonicalBudgetContext -CapturePhase 'canonical-hash' | Out-Null
    }
    Complete-G04DCMachineStateCapture -Context $canonicalBudgetContext -Passed $false -FailureMessage '[MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED] phase=canonical-hash'
    $canonicalBudgetRecords = @(Get-Content -LiteralPath $canonicalBudgetProgress | ForEach-Object { $_ | ConvertFrom-Json })
    $canonicalBudgetEvidence = Get-Content -LiteralPath $canonicalBudgetPerformance -Raw | ConvertFrom-Json
    if ((Test-Path -LiteralPath $canonicalBudgetSuccess) -or [bool]$canonicalBudgetEvidence.passed -or $canonicalBudgetEvidence.failurePhase -cne 'canonical-hash' -or
        $canonicalBudgetRecords[-1].event -cne 'capture-failure' -or $canonicalBudgetRecords[-1].status -cne 'budget-exceeded') {
        throw 'Timed-out canonical hashing produced success or lost durable failure evidence.'
    }
    $passed.Add('machine-state canonical hash budget failure is durable')

    $helperSuccessContext = New-G04DCMachineStateCaptureContext -CaptureLabel 'helper-success' -ProgressPath (Join-Path $telemetryRoot 'helper-success.ndjson') -PerformancePath (Join-Path $telemetryRoot 'helper-success.json') -CaptureTargetMilliseconds 1000 -OverallBudgetMilliseconds 5000 -PhaseBudgetMilliseconds 3000
    Start-G04DCMachineStatePhase -Context $helperSuccessContext -Phase 'provider-inventory'
    $helperSuccess = @(& $module { param($Context) Invoke-G04DCBoundedCaptureProcess -Context $Context -Phase 'provider-inventory' -ScriptBlock { param([string]$Value) [pscustomobject]@{ value = $Value } } -ArgumentList @('bounded-result') } $helperSuccessContext)
    Complete-G04DCMachineStatePhase -Context $helperSuccessContext -Phase 'provider-inventory' -ItemCount $helperSuccess.Count
    Complete-G04DCMachineStateCapture -Context $helperSuccessContext -Passed $true -FailureMessage $null
    if ($helperSuccess.Count -ne 1 -or [string]$helperSuccess[0].value -cne 'bounded-result') { throw 'Bounded helper process did not return its sealed result.' }
    $passed.Add('machine-state helper process returns sealed output')

    $helperContext = New-G04DCMachineStateCaptureContext -CaptureLabel 'helper-budget' -ProgressPath (Join-Path $telemetryRoot 'helper-budget.ndjson') -PerformancePath (Join-Path $telemetryRoot 'helper-budget.json') -CaptureTargetMilliseconds 1 -OverallBudgetMilliseconds 1000 -PhaseBudgetMilliseconds 200
    Start-G04DCMachineStatePhase -Context $helperContext -Phase 'provider-inventory'
    $helperParent = if (![string]::IsNullOrWhiteSpace($env:RUNNER_TEMP) -and (Test-Path -LiteralPath $env:RUNNER_TEMP -PathType Container)) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
    $helperRootsBefore = @(Get-ChildItem -LiteralPath $helperParent -Directory -Filter 'document-studio-g04dc-capture-helper-*' -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
    Assert-Throws 'machine-state helper process timeout is terminally cleaned' 'MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED' {
        & $module { param($Context) Invoke-G04DCBoundedCaptureProcess -Context $Context -Phase 'provider-inventory' -ScriptBlock { Start-Sleep -Seconds 10 } } $helperContext
    }
    $helperRootsAfter = @(Get-ChildItem -LiteralPath $helperParent -Directory -Filter 'document-studio-g04dc-capture-helper-*' -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
    if (@($helperRootsAfter | Where-Object { $helperRootsBefore -cnotcontains $_ }).Count -ne 0) { throw 'Timed-out helper process left an owned helper root behind.' }
    Complete-G04DCMachineStateCapture -Context $helperContext -Passed $false -FailureMessage '[MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED] phase=provider-inventory'

    $successGateContext = New-G04DCMachineStateCaptureContext -CaptureLabel 'success-gate' -ProgressPath (Join-Path $telemetryRoot 'success-gate.ndjson') -PerformancePath (Join-Path $telemetryRoot 'success-gate.json') -CaptureTargetMilliseconds 1 -OverallBudgetMilliseconds 50 -PhaseBudgetMilliseconds 50
    Start-G04DCMachineStatePhase -Context $successGateContext -Phase 'finalized'
    Complete-G04DCMachineStatePhase -Context $successGateContext -Phase 'finalized' -ItemCount 1
    Start-Sleep -Milliseconds 75
    Assert-Throws 'machine-state final success gate enforces overall budget' 'MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED' {
        Complete-G04DCMachineStateCapture -Context $successGateContext -Passed $true -FailureMessage $null
    }
    Complete-G04DCMachineStateCapture -Context $successGateContext -Passed $false -FailureMessage '[MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED] phase=capture-success-gate'

    $hashClosed = [pscustomobject]@{ value = $false; reads = 0 }
    $hashAdapter = @{
        Open = { param([string]$CandidatePath) [pscustomobject]@{ synthetic = $true } }
        Read = { param($Handle, [byte[]]$Buffer) $hashClosed.reads++; if ($hashClosed.reads -eq 1) { Start-Sleep -Milliseconds 75; $Buffer[0] = 97; return 1 }; return 0 }.GetNewClosure()
        Close = { param($Handle) $hashClosed.value = $true }.GetNewClosure()
    }
    $hashContext = New-G04DCMachineStateCaptureContext -CaptureLabel 'hash-budget' -ProgressPath (Join-Path $telemetryRoot 'hash-budget.ndjson') -PerformancePath (Join-Path $telemetryRoot 'hash-budget.json') -CaptureTargetMilliseconds 1 -OverallBudgetMilliseconds 1000 -PhaseBudgetMilliseconds 50
    Start-G04DCMachineStatePhase -Context $hashContext -Phase 'installer-cache-catalog'
    Assert-Throws 'machine-state streaming hash checks deadline per buffer' 'MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED' {
        & $module { param($Context, $Adapter) Get-G04DCBoundedFileSha256 -Path 'C:\synthetic\large.msi' -CaptureContext $Context -CapturePhase 'installer-cache-catalog' -ItemCount 1 -ReadAdapter $Adapter } $hashContext $hashAdapter
    }
    Complete-G04DCMachineStateCapture -Context $hashContext -Passed $false -FailureMessage '[MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED] phase=installer-cache-catalog'
    if (!$hashClosed.value) { throw 'Bounded streaming hash did not dispose its read handle after a deadline failure.' }
    $passed.Add('machine-state streaming hash disposes handle on failure')
}
finally {
    foreach ($telemetryContext in @($context, $failureContext, $phaseBudgetContext, $overallBudgetContext, $serializationSuccessContext, $serializationContext, $canonicalSuccessContext, $canonicalBudgetContext, $helperSuccessContext, $helperContext, $successGateContext, $hashContext)) {
        if ($telemetryContext -and $telemetryContext.writer) { $telemetryContext.writer.Dispose(); $telemetryContext.writer = $null }
    }
    if (Test-Path -LiteralPath $telemetryRoot) { Remove-Item -LiteralPath $telemetryRoot -Recurse -Force }
}

function New-G04DCTestMsiComponentAdapter {
    param(
        [hashtable]$Fixtures = @{},
        [string]$AccessDeniedOperation = '',
        [string]$ReadFailureOperation = ''
    )
    $statistics = [pscustomobject][ordered]@{ baseRootOpenCount = 0; componentOpenCount = 0; closeCount = 0; providerInvocationCount = 0 }
    $handles = [System.Collections.Generic.List[object]]::new()
    $adapter = @{}
    $adapter.OpenBaseRoot = {
        param([string]$Scope)
        $statistics.baseRootOpenCount++
        if ($AccessDeniedOperation -ceq "OpenBaseRoot:$Scope") { throw [System.UnauthorizedAccessException]::new('synthetic base access denied') }
        $handle = [pscustomobject]@{ scope = $Scope; root = $true; disposed = $false }
        $handles.Add($handle)
        [pscustomobject]@{ rootExists = $true; handle = $handle }
    }.GetNewClosure()
    $adapter.OpenComponentKey = {
        param($RootHandle, [string]$PackedComponent)
        $statistics.componentOpenCount++
        if ($AccessDeniedOperation -ceq 'OpenComponentKey') { throw [System.UnauthorizedAccessException]::new('synthetic component access denied') }
        $fixtureKey = "$($RootHandle.scope)|$PackedComponent"
        if (!$Fixtures.ContainsKey($fixtureKey)) { return $null }
        $handle = [pscustomobject]@{ scope = $RootHandle.scope; root = $false; packedComponent = $PackedComponent; fixture = $Fixtures[$fixtureKey]; enumerationCount = 0; disposed = $false }
        $handles.Add($handle)
        return $handle
    }.GetNewClosure()
    $adapter.GetValueNames = {
        param($Handle)
        $Handle.enumerationCount++
        if ([bool]$Handle.fixture.disappear -and $Handle.enumerationCount -gt 1) { return @() }
        return @($Handle.fixture.values.Keys)
    }.GetNewClosure()
    $adapter.GetValueKind = {
        param($Handle, [string]$Name)
        if ($ReadFailureOperation -ceq 'GetValueKind') { throw [System.InvalidOperationException]::new('synthetic value-kind read failure') }
        return [string]$Handle.fixture.values[$Name].kind
    }.GetNewClosure()
    $adapter.GetValue = {
        param($Handle, [string]$Name)
        if ($ReadFailureOperation -ceq 'GetValue') { throw [System.InvalidOperationException]::new('synthetic value read failure') }
        if ([bool]$Handle.fixture.disappear) { return [pscustomobject]@{ present = $false; value = $null } }
        return [pscustomobject]@{ present = $Handle.fixture.values.ContainsKey($Name); value = $Handle.fixture.values[$Name].value }
    }.GetNewClosure()
    $adapter.CloseKey = {
        param($Handle)
        if ([bool]$Handle.disposed) { throw [System.InvalidOperationException]::new('synthetic handle disposed twice') }
        $Handle.disposed = $true
        $statistics.closeCount++
    }.GetNewClosure()
    return [pscustomobject]@{ adapter = $adapter; statistics = $statistics; handles = $handles }
}

function New-G04DCTestComponentFixture([hashtable]$Values, [bool]$Disappear = $false) {
    return [pscustomobject]@{ values = $Values; disappear = $Disappear }
}

$packedProductForComponents = ConvertTo-G04DCPackedGuid -Guid $expected.ProductCode
$componentMissing = '{00000001-0000-0000-0000-000000000001}'
$componentNoValue = '{00000002-0000-0000-0000-000000000002}'
$componentEmpty = '{00000003-0000-0000-0000-000000000003}'
$componentTyped = '{00000004-0000-0000-0000-000000000004}'
$componentBoth = '{00000005-0000-0000-0000-000000000005}'
$fixtureData = @{}
$fixtureData["system|$(ConvertTo-G04DCPackedGuid $componentNoValue)"] = New-G04DCTestComponentFixture -Values @{}
$fixtureData["system|$(ConvertTo-G04DCPackedGuid $componentEmpty)"] = New-G04DCTestComponentFixture -Values @{ $packedProductForComponents = [pscustomobject]@{ kind = 'String'; value = '' } }
$fixtureData["user|$(ConvertTo-G04DCPackedGuid $componentTyped)"] = New-G04DCTestComponentFixture -Values @{ $packedProductForComponents = [pscustomobject]@{ kind = 'DWord'; value = 42 } }
$fixtureData["system|$(ConvertTo-G04DCPackedGuid $componentBoth)"] = New-G04DCTestComponentFixture -Values @{ $packedProductForComponents = [pscustomobject]@{ kind = 'String'; value = 'system' } }
$fixtureData["user|$(ConvertTo-G04DCPackedGuid $componentBoth)"] = New-G04DCTestComponentFixture -Values @{ $packedProductForComponents = [pscustomobject]@{ kind = 'String'; value = 'user' } }
$fixtureAdapterState = New-G04DCTestMsiComponentAdapter -Fixtures $fixtureData
$fixtureRecords = @(Get-G04DCMsiComponentRegistrationState -ComponentCodes @($componentBoth, $componentTyped, $componentMissing, $componentNoValue, $componentEmpty, $componentBoth) -PackedProductCode $packedProductForComponents -AccessAdapter $fixtureAdapterState.adapter)
$fixtureByCode = @{}; foreach ($record in $fixtureRecords) { $fixtureByCode[$record.componentCode] = $record }
if ($fixtureByCode[$componentMissing].systemProductValueState.keyExists -or $fixtureByCode[$componentMissing].userProductValueState.keyExists) { throw 'Missing component key semantics changed.' }
$passed.Add('MSI component key missing equivalence')
if (!$fixtureByCode[$componentNoValue].systemProductValueState.keyExists -or $fixtureByCode[$componentNoValue].systemProductValuePresent) { throw 'Present component key with absent product value semantics changed.' }
$passed.Add('MSI component product value missing equivalence')
if (!$fixtureByCode[$componentEmpty].systemProductValuePresent -or $fixtureByCode[$componentEmpty].systemProductValueState.value -cne '') { throw 'Empty product value was not preserved.' }
$passed.Add('MSI component empty product value equivalence')
if (!$fixtureByCode[$componentTyped].userProductValuePresent -or $fixtureByCode[$componentTyped].userProductValueState.valueType -cne 'DWord' -or [int]$fixtureByCode[$componentTyped].userProductValueState.value -ne 42) { throw 'Typed product value was not preserved.' }
$passed.Add('MSI component value type preservation')
if ($fixtureByCode[$componentTyped].systemProductValuePresent -or !$fixtureByCode[$componentTyped].userProductValuePresent -or
    !$fixtureByCode[$componentBoth].systemProductValuePresent -or !$fixtureByCode[$componentBoth].userProductValuePresent) { throw 'System/user/both scope evidence is invalid.' }
$passed.Add('MSI component system user and both scope differences')
$expectedTypedState = [pscustomobject][ordered]@{ schemaVersion = 1; keyExists = $true; valueName = $packedProductForComponents; valuePresent = $true; valueType = 'DWord'; value = 42 }
if (($fixtureByCode[$componentTyped].userProductValueState | ConvertTo-Json -Compress) -cne ($expectedTypedState | ConvertTo-Json -Compress)) { throw 'Optimized state is not byte-canonical equivalent to the legacy schema.' }
$passed.Add('MSI component canonical semantic equivalence')
if ($fixtureRecords.Count -ne 5 -or ($fixtureRecords.componentCode -join '|') -cne (($fixtureRecords.componentCode | Sort-Object -Unique) -join '|')) { throw 'Duplicate component codes or deterministic ordering changed.' }
$passed.Add('MSI component duplicate input deterministic ordering')
if (@($fixtureAdapterState.handles | Where-Object { ![bool]$_.disposed }).Count -ne 0) { throw 'MSI component registry handles were not all disposed.' }
$passed.Add('MSI component handles disposed')
if ($fixtureAdapterState.statistics.baseRootOpenCount -ne 2) { throw 'MSI component base roots were not opened a bounded number of times.' }
$passed.Add('MSI component bounded base root handles')
Assert-Throws 'malformed MSI component GUID rejection' 'MSI_REGISTRATION_INVALID' {
    Get-G04DCMsiComponentRegistrationState -ComponentCodes @('not-a-guid') -PackedProductCode $packedProductForComponents -AccessAdapter (New-G04DCTestMsiComponentAdapter).adapter
}
$whitespaceAdapterState = New-G04DCTestMsiComponentAdapter
Assert-Throws 'whitespace MSI component GUID rejection' 'MSI_REGISTRATION_INVALID' {
    Get-G04DCMsiComponentRegistrationState -ComponentCodes @($componentMissing, '   ') -PackedProductCode $packedProductForComponents -AccessAdapter $whitespaceAdapterState.adapter
}
if (@($whitespaceAdapterState.handles | Where-Object { ![bool]$_.disposed }).Count -ne 0) { throw 'Whitespace component rejection leaked opened registry roots.' }
$passed.Add('whitespace MSI component rejection disposes roots')
Assert-Throws 'MSI component access denied fail closed' 'MSI_REGISTRATION_CAPTURE_FAILED' {
    Get-G04DCMsiComponentRegistrationState -ComponentCodes @($componentMissing) -PackedProductCode $packedProductForComponents -AccessAdapter (New-G04DCTestMsiComponentAdapter -AccessDeniedOperation 'OpenBaseRoot:system').adapter
}
Assert-Throws 'MSI component read failure fail closed' 'MSI_REGISTRATION_CAPTURE_FAILED' {
    $readFailureData = @{ "system|$(ConvertTo-G04DCPackedGuid $componentTyped)" = (New-G04DCTestComponentFixture -Values @{ $packedProductForComponents = [pscustomobject]@{ kind = 'String'; value = 'present' } }) }
    Get-G04DCMsiComponentRegistrationState -ComponentCodes @($componentTyped) -PackedProductCode $packedProductForComponents -AccessAdapter (New-G04DCTestMsiComponentAdapter -Fixtures $readFailureData -ReadFailureOperation 'GetValueKind').adapter
}
$raceData = @{ "system|$(ConvertTo-G04DCPackedGuid $componentTyped)" = (New-G04DCTestComponentFixture -Values @{ $packedProductForComponents = [pscustomobject]@{ kind = 'String'; value = 'present' } } -Disappear $true) }
$raceRecord = @(Get-G04DCMsiComponentRegistrationState -ComponentCodes @($componentTyped) -PackedProductCode $packedProductForComponents -AccessAdapter (New-G04DCTestMsiComponentAdapter -Fixtures $raceData).adapter)[0]
if (!$raceRecord.systemProductValueState.keyExists -or $raceRecord.systemProductValuePresent) { throw 'Disappearing MSI product value was not reclassified safely.' }
$passed.Add('MSI component disappearing value race')

$viewTestId = [guid]::NewGuid().ToString('N')
$viewNativeRoot = "Software\DocumentStudio-G04DC-Msi-View-$viewTestId"
$viewProviderRoot = "Registry::HKEY_CURRENT_USER\$viewNativeRoot"
$viewComponent = '{00000006-0000-0000-0000-000000000006}'
$viewPackedComponent = ConvertTo-G04DCPackedGuid $viewComponent
$viewBase = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser, [Microsoft.Win32.RegistryView]::Registry64)
try {
    $viewKey = $viewBase.CreateSubKey("$viewNativeRoot\$viewPackedComponent")
    try { $viewKey.SetValue($packedProductForComponents, 'view-equivalent', [Microsoft.Win32.RegistryValueKind]::String) }
    finally { $viewKey.Dispose() }
    $viewAdapter = @{
        OpenBaseRoot = { param([string]$Scope) if ($Scope -ceq 'system') { return [pscustomobject]@{ rootExists = $false; handle = $null } }; $root = $viewBase.OpenSubKey($viewNativeRoot, $false); [pscustomobject]@{ rootExists = $null -ne $root; handle = $root } }.GetNewClosure()
        OpenComponentKey = { param($RootHandle, [string]$PackedComponent) $RootHandle.OpenSubKey($PackedComponent, $false) }
        GetValueNames = { param($Handle) @($Handle.GetValueNames()) }
        GetValueKind = { param($Handle, [string]$Name) [string]$Handle.GetValueKind($Name) }
        GetValue = { param($Handle, [string]$Name) $missing = [object]::new(); $value = $Handle.GetValue($Name, $missing, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames); [pscustomobject]@{ present = ![object]::ReferenceEquals($missing, $value); value = $value } }
        CloseKey = { param($Handle) if ($Handle) { $Handle.Dispose() } }
    }
    $legacyViewState = Get-G04DCRegistryValueState -Path "$viewProviderRoot\$viewPackedComponent" -ValueName $packedProductForComponents
    $optimizedViewState = @(Get-G04DCMsiComponentRegistrationState -ComponentCodes @($viewComponent) -PackedProductCode $packedProductForComponents -AccessAdapter $viewAdapter)[0].userProductValueState
    if (($legacyViewState | ConvertTo-Json -Compress) -cne ($optimizedViewState | ConvertTo-Json -Compress)) { throw 'Registry64 typed access differs from the 64-bit Registry provider view.' }
    $passed.Add('MSI component registry view equivalence')
}
finally {
    $viewBase.Dispose()
    if (Test-Path -LiteralPath $viewProviderRoot) { Remove-Item -LiteralPath $viewProviderRoot -Recurse -Force }
}

$scaleComponents = @(1..6092 | ForEach-Object { '{00000000-0000-0000-0000-' + $_.ToString('X12') + '}' })
$scaleAdapterState = New-G04DCTestMsiComponentAdapter
$scaleRecords = @(Get-G04DCMsiComponentRegistrationState -ComponentCodes $scaleComponents -PackedProductCode $packedProductForComponents -AccessAdapter $scaleAdapterState.adapter)
if ($scaleRecords.Count -ne 6092) { throw 'Scale collector omitted synthetic component records.' }
$passed.Add('MSI component 6092 record scale')
if ($scaleAdapterState.statistics.baseRootOpenCount -ne 2 -or $scaleAdapterState.statistics.componentOpenCount -ne 12184) { throw 'Scale collector did not retain bounded root opens and exact per-component subkey checks.' }
$passed.Add('MSI component scale bounded base roots')
$scaleCodes = @($scaleRecords | ForEach-Object { $_.componentCode })
if (($scaleCodes | Sort-Object -Unique).Count -ne 6092 -or ($scaleCodes -join '|') -cne (($scaleCodes | Sort-Object) -join '|')) { throw 'Scale collector output is incomplete or nondeterministic.' }
$passed.Add('MSI component scale deterministic no omission')
$componentFunctionSource = (Get-Command Get-G04DCMsiComponentRegistrationState).ScriptBlock.ToString()
if ($scaleAdapterState.statistics.providerInvocationCount -ne 0 -or $componentFunctionSource.Contains('Get-G04DCRegistryValueState')) { throw 'Scale collector retained a Registry-provider classification per component.' }
$passed.Add('MSI component scale avoids provider per component')

$g04dcModule = Get-Module G04DC.Common
$utf8Strict = [Text.UTF8Encoding]::new($false, $true)

function Invoke-G04DCTestClassRegistryCollector {
    param(
        [AllowEmptyString()] [string]$Text,
        [AllowEmptyString()] [string]$StderrText = '',
        [long]$MaximumRawBytes = 134217728,
        [int]$MaximumRows = 1000000,
        [int]$MaximumRowCharacters = 16777216,
        [int]$MaximumCanonicalRowBytes = 16777216,
        [long]$MaximumCanonicalBytes = 134217728,
        [int]$ReadBufferBytes = 65536,
        [long]$NormalizeBudgetMilliseconds = 30000,
        [long]$HashBudgetMilliseconds = 30000
    )
    $bytes = $utf8Strict.GetBytes($Text)
    $stream = [IO.MemoryStream]::new($bytes)
    try {
        $collector = [DocumentStudio.G04DC.ClassRegistryDigestCollector]::new(
            $stream,
            $utf8Strict,
            $MaximumRawBytes,
            $MaximumRows,
            $MaximumRowCharacters,
            $MaximumCanonicalRowBytes,
            $MaximumCanonicalBytes,
            $ReadBufferBytes
        )
        $readTask = $collector.BeginRead()
        $readTask.GetAwaiter().GetResult()
        $collector.AppendStderrText($StderrText)
        $collector.Normalize($NormalizeBudgetMilliseconds)
        $digest = $collector.Hash($HashBudgetMilliseconds)
        return [pscustomobject][ordered]@{
            rowCount = [long]$collector.RowCount
            rawByteCount = [long]$collector.RawByteCount
            canonicalByteCount = [long]$collector.CanonicalByteCount
            sha256 = [string]$digest
        }
    }
    finally { $stream.Dispose() }
}

function Get-G04DCTestLegacyNativeDigest {
    param([Parameter(Mandatory = $true)] [AllowEmptyString()] [string]$Text)
    $rows = @(& $g04dcModule { param([string]$NativeText) ConvertFrom-G04DCNativeTextRows -Text $NativeText } $Text)
    return [pscustomobject][ordered]@{ rowCount = $rows.Count; sha256 = Get-G04DCCanonicalHash -Rows $rows }
}

$normalizationText = "alpha  `r`n`r`nbeta`t`r`n"
$legacyNormalization = Get-G04DCTestLegacyNativeDigest -Text $normalizationText
$optimizedNormalization = Invoke-G04DCTestClassRegistryCollector -Text $normalizationText -ReadBufferBytes 3
if ($optimizedNormalization.rowCount -ne $legacyNormalization.rowCount -or $optimizedNormalization.sha256 -cne $legacyNormalization.sha256) {
    throw 'Streaming registry normalization changed the legacy native-row digest.'
}
$passed.Add('current native-row normalization equivalence')
if ($optimizedNormalization.rowCount -ne 3) { throw 'Streaming registry normalization lost the internal blank row or retained the terminal empty row.' }
$passed.Add('terminal empty-row handling')
$trimmedDigest = Invoke-G04DCTestClassRegistryCollector -Text "alpha`r`n"
if ($trimmedDigest.sha256 -cne (Invoke-G04DCTestClassRegistryCollector -Text "alpha  `t`r`n").sha256) { throw 'Streaming registry normalization changed TrimEnd semantics.' }
$passed.Add('trailing-space TrimEnd equivalence')

$canonicalCases = @(
    @(),
    @('single'),
    @('', 'spaces  ', "tab`t", 'quote"', 'backslash\'),
    @(('unicode-' + (-join @([char]0x0928, [char]0x092E))), ([string][char]0x2028), ([string][char]0x0085)),
    @('a', 'A', 'b', 'B')
)
foreach ($canonicalRows in $canonicalCases) {
    $canonicalText = if ($canonicalRows.Count -eq 0) { '' } else { ($canonicalRows -join "`r`n") + "`r`n" }
    $legacyDigest = Get-G04DCCanonicalHash -Rows @($canonicalRows | ForEach-Object { ([string]$_).TrimEnd() })
    $optimizedDigest = Invoke-G04DCTestClassRegistryCollector -Text $canonicalText -ReadBufferBytes 1
    if ($optimizedDigest.sha256 -cne $legacyDigest) { throw 'Incremental canonical string-row hash differs from the legacy digest.' }
}
$passed.Add('incremental hash equality')
if ((Invoke-G04DCTestClassRegistryCollector -Text '').sha256 -cne (Get-G04DCCanonicalHash -Rows @())) { throw 'Zero-row canonical hash changed.' }
$passed.Add('zero-row hash')

$scaleRows = @(0..64235 | ForEach-Object { 'HKEY_CLASSES_ROOT\Synthetic\' + $_.ToString('D6') })
$scaleText = ($scaleRows -join "`r`n") + "`r`n"
$scaleOptimized = Invoke-G04DCTestClassRegistryCollector -Text $scaleText
$scaleLegacy = Get-G04DCCanonicalHash -Rows $scaleRows
if ($scaleOptimized.rowCount -ne 64236 -or $scaleOptimized.sha256 -cne $scaleLegacy) { throw 'Streaming registry collector failed the 64,236-row canonical scale.' }
$passed.Add('64236-row registry digest scale')

$orderA = Invoke-G04DCTestClassRegistryCollector -Text "a`r`nA`r`n"
$orderB = Invoke-G04DCTestClassRegistryCollector -Text "A`r`na`r`n"
$legacyOrderA = Get-G04DCCanonicalHash -Rows @('a', 'A')
$legacyOrderB = Get-G04DCCanonicalHash -Rows @('A', 'a')
if ($orderA.sha256 -cne $legacyOrderA -or $orderB.sha256 -cne $legacyOrderB -or
    (($legacyOrderA -cne $legacyOrderB) -ne ($orderA.sha256 -cne $orderB.sha256))) {
    throw 'Streaming row ordering does not match the legacy case-insensitive Sort-Object model.'
}
$passed.Add('row-order sensitivity matches legacy canonical model')

Assert-Throws 'class registry raw-byte ceiling' 'REGISTRY_DIGEST_RAW_BYTE_CEILING' {
    Invoke-G04DCTestClassRegistryCollector -Text '123456' -MaximumRawBytes 5 | Out-Null
}
Assert-Throws 'class registry row ceiling' 'REGISTRY_DIGEST_ROW_CEILING' {
    Invoke-G04DCTestClassRegistryCollector -Text "a`nb`nc`n" -MaximumRows 2 | Out-Null
}
Assert-Throws 'class registry long-row ceiling' 'REGISTRY_DIGEST_ROW_LENGTH_CEILING' {
    Invoke-G04DCTestClassRegistryCollector -Text '12345' -MaximumRowCharacters 4 | Out-Null
}
$maximumConfiguredRow = Invoke-G04DCTestClassRegistryCollector -Text ('x' * 4096) -MaximumRowCharacters 4096 -MaximumCanonicalRowBytes 4098
if ($maximumConfiguredRow.rowCount -ne 1) { throw 'Exact configured maximum registry row was rejected.' }
$passed.Add('maximum configured registry row accepted')
$aggregateBoundary = Invoke-G04DCTestClassRegistryCollector -Text 'a' -MaximumCanonicalBytes 5
if ($aggregateBoundary.canonicalByteCount -ne 5) { throw 'Exact canonical aggregate-byte boundary changed.' }
$passed.Add('maximum aggregate-byte boundary')
Assert-Throws 'class registry aggregate-byte ceiling' 'REGISTRY_DIGEST_CANONICAL_BYTE_CEILING' {
    Invoke-G04DCTestClassRegistryCollector -Text 'a' -MaximumCanonicalBytes 4 | Out-Null
}

$unicodeSplit = -join @([char]0x0928, [char]0x092E, [char]0x0938, [char]0x094D, [char]0x0924, [char]0x0947)
$unicodeSplitDigest = Invoke-G04DCTestClassRegistryCollector -Text ($unicodeSplit + "`r`n") -ReadBufferBytes 1
if ($unicodeSplitDigest.sha256 -cne (Get-G04DCCanonicalHash -Rows @($unicodeSplit))) { throw 'UTF-8 multibyte decoding changed across one-byte buffers.' }
$passed.Add('explicit UTF-8 native output encoding')
$passed.Add('split multibyte character across stream buffers')
$splitCrlf = Invoke-G04DCTestClassRegistryCollector -Text "alpha`r`nbeta`r`n" -ReadBufferBytes 1
if ($splitCrlf.rowCount -ne 2 -or $splitCrlf.sha256 -cne (Get-G04DCCanonicalHash -Rows @('alpha', 'beta'))) { throw 'CRLF split across buffers changed row framing.' }
$passed.Add('CRLF across stream buffers')
$finalWithoutNewline = Invoke-G04DCTestClassRegistryCollector -Text 'final-row' -ReadBufferBytes 1
if ($finalWithoutNewline.rowCount -ne 1 -or $finalWithoutNewline.sha256 -cne (Get-G04DCCanonicalHash -Rows @('final-row'))) { throw 'Final native row without a newline was lost.' }
$passed.Add('final row without newline')
Assert-Throws 'malformed native UTF-8 output' 'REGISTRY_DIGEST_DECODING_INVALID' {
    $invalidStream = [IO.MemoryStream]::new([byte[]]@(0xC3))
    try {
        $invalidCollector = [DocumentStudio.G04DC.ClassRegistryDigestCollector]::new($invalidStream, $utf8Strict, 16, 16, 16, 16, 64, 1)
        $invalidCollector.BeginRead().GetAwaiter().GetResult()
    }
    finally { $invalidStream.Dispose() }
}
$passed.Add('truncated multibyte native output rejected')

Assert-Throws 'bounded registry stderr capture' 'REGISTRY_DIGEST_STDERR_CEILING' {
    $stderrStream = [IO.MemoryStream]::new($utf8Strict.GetBytes('123456'))
    try {
        $stderrReader = [DocumentStudio.G04DC.BoundedTextCapture]::new($stderrStream, $utf8Strict, 5, 1)
        $stderrReader.BeginRead().GetAwaiter().GetResult()
    }
    finally { $stderrStream.Dispose() }
}

$normalizationTimeoutText = ((0..49999 | ForEach-Object { 'normalization-timeout-row-' + $_ }) -join "`n")
Assert-Throws 'class registry normalization timeout' 'REGISTRY_DIGEST_TIMEOUT' {
    Invoke-G04DCTestClassRegistryCollector -Text $normalizationTimeoutText -NormalizeBudgetMilliseconds 0 | Out-Null
}
Assert-Throws 'class registry hashing timeout' 'REGISTRY_DIGEST_TIMEOUT' {
    Invoke-G04DCTestClassRegistryCollector -Text $normalizationTimeoutText -HashBudgetMilliseconds 0 | Out-Null
}

function New-G04DCTestNativeArguments {
    param([Parameter(Mandatory = $true)] [string]$Code)
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($Code))
    return "-NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $encoded"
}

function Invoke-G04DCTestNativeDigest {
    param(
        [Parameter(Mandatory = $true)] [string]$Code,
        [AllowNull()] $CaptureContext,
        [long]$MaximumStderrBytes = 1048576
    )
    $powershellPath = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $arguments = @{
        NativePath = 'HKEY_CURRENT_USER\Software\DocumentStudio-G04DC-C7-Synthetic'
        NativeExecutablePath = $powershellPath
        NativeArguments = New-G04DCTestNativeArguments -Code $Code
        AllowTestNativePath = $true
        MaximumStderrBytes = $MaximumStderrBytes
    }
    if ($CaptureContext) { $arguments.CaptureContext = $CaptureContext; $arguments.CapturePhase = 'class-registry-digest' }
    return & $g04dcModule { param($Arguments) Get-G04DCRegistryTreeDigest @Arguments } $arguments
}

$syntheticOutputCode = '$bytes=[Text.UTF8Encoding]::new($false).GetBytes("alpha  `r`n`r`nbeta`t`r`n");$stream=[Console]::OpenStandardOutput();$stream.Write($bytes,0,$bytes.Length);$stream.Flush()'
$syntheticProcessDigest = Invoke-G04DCTestNativeDigest -Code $syntheticOutputCode
if ($syntheticProcessDigest.sha256 -cne $legacyNormalization.sha256 -or $syntheticProcessDigest.rowCount -ne $legacyNormalization.rowCount) {
    throw 'Direct owned native helper process changed the streaming digest.'
}
$passed.Add('direct native process streaming digest')
Assert-Throws 'class registry native process nonzero exit' 'MACHINE_STATE_CAPTURE_FAILED' {
    Invoke-G04DCTestNativeDigest -Code 'exit 7' | Out-Null
}
$stderrCode = '$bytes=[Text.UTF8Encoding]::new($false).GetBytes("123456789");$stream=[Console]::OpenStandardError();$stream.Write($bytes,0,$bytes.Length);$stream.Flush()'
Assert-Throws 'class registry native stderr bound' 'MACHINE_STATE_CAPTURE_FAILED' {
    Invoke-G04DCTestNativeDigest -Code $stderrCode -MaximumStderrBytes 8 | Out-Null
}

$c7TelemetryRoot = Join-Path ([IO.Path]::GetTempPath()) ('g04dc-c7-telemetry-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $c7TelemetryRoot | Out-Null
$c7SuccessContext = $c7TimeoutContext = $null
try {
    $c7SuccessProgress = Join-Path $c7TelemetryRoot 'success-progress.ndjson'
    $c7SuccessPerformance = Join-Path $c7TelemetryRoot 'success-performance.json'
    $c7SuccessContext = New-G04DCMachineStateCaptureContext -CaptureLabel 'c7-substages' -ProgressPath $c7SuccessProgress -PerformancePath $c7SuccessPerformance -CaptureTargetMilliseconds 1000 -OverallBudgetMilliseconds 5000 -PhaseBudgetMilliseconds 3000
    Start-G04DCMachineStatePhase -Context $c7SuccessContext -Phase 'class-registry-digest'
    $phaseStartBeforeSubstages = [long]$c7SuccessContext.activePhaseStartMilliseconds
    $c7SyntheticDigest = Invoke-G04DCTestNativeDigest -Code $syntheticOutputCode -CaptureContext $c7SuccessContext
    if ([long]$c7SuccessContext.activePhaseStartMilliseconds -ne $phaseStartBeforeSubstages) { throw 'Registry digest substages reset the active phase timer.' }
    Complete-G04DCMachineStatePhase -Context $c7SuccessContext -Phase 'class-registry-digest' -ItemCount $c7SyntheticDigest.rowCount
    Complete-G04DCMachineStateCapture -Context $c7SuccessContext -Passed $true -FailureMessage $null
    $c7ProgressRows = @(Get-Content -LiteralPath $c7SuccessProgress | ForEach-Object { $_ | ConvertFrom-Json })
    $c7Performance = Get-Content -LiteralPath $c7SuccessPerformance -Raw | ConvertFrom-Json
    $c7Substages = @($c7Performance.phases[0].substages)
    $expectedSubstages = 'native-query-startup|native-query-read|row-normalization|canonical-hash|helper-cleanup'
    if (($c7Substages.substage -join '|') -cne $expectedSubstages -or @($c7Substages | Where-Object { $_.status -cne 'success' }).Count -ne 0) {
        throw 'Class registry digest substage ordering or status is invalid.'
    }
    $passed.Add('class registry substage ordering')
    if (@($c7Substages | Where-Object { [long]$_.elapsedMilliseconds -lt 0 -or [long]$_.rowCount -lt 0 }).Count -ne 0 -or
        @($c7ProgressRows | Where-Object { $_.event -ceq 'substage-end' }).Count -ne 5) {
        throw 'Class registry digest substage timing/count evidence is incomplete.'
    }
    $passed.Add('class registry substage elapsed and count evidence')
    $passed.Add('class registry substages retain one phase timer')
    $c7ProgressJson = $c7ProgressRows | ConvertTo-Json -Compress -Depth 8
    if ($c7ProgressJson.Contains('alpha  ') -or $c7ProgressJson.Contains('beta') -or $c7ProgressJson -match 'HKEY_CURRENT_USER') {
        throw 'Class registry digest progress exposed raw registry content or paths.'
    }
    $passed.Add('no raw registry output in telemetry')

    $timeoutPidPath = Join-Path $c7TelemetryRoot 'timeout-pid.txt'
    $escapedPidPath = $timeoutPidPath.Replace("'", "''")
    $timeoutCode = "[IO.File]::WriteAllText('$escapedPidPath',[Diagnostics.Process]::GetCurrentProcess().Id.ToString());Start-Sleep -Seconds 30"
    $timeoutProgress = Join-Path $c7TelemetryRoot 'timeout-progress.ndjson'
    $timeoutPerformance = Join-Path $c7TelemetryRoot 'timeout-performance.json'
    $c7TimeoutContext = New-G04DCMachineStateCaptureContext -CaptureLabel 'c7-timeout' -ProgressPath $timeoutProgress -PerformancePath $timeoutPerformance -CaptureTargetMilliseconds 1 -OverallBudgetMilliseconds 4000 -PhaseBudgetMilliseconds 2000
    Start-G04DCMachineStatePhase -Context $c7TimeoutContext -Phase 'class-registry-digest'
    Assert-Throws 'class registry native-query timeout' 'MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED' {
        Invoke-G04DCTestNativeDigest -Code $timeoutCode -CaptureContext $c7TimeoutContext | Out-Null
    }
    Complete-G04DCMachineStateCapture -Context $c7TimeoutContext -Passed $false -FailureMessage '[MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED] phase=class-registry-digest'
    if (!(Test-Path -LiteralPath $timeoutPidPath -PathType Leaf)) { throw 'Timed registry helper did not publish its test-owned PID evidence.' }
    $timedProcessId = [int](Get-Content -LiteralPath $timeoutPidPath -Raw)
    if (Get-Process -Id $timedProcessId -ErrorAction SilentlyContinue) { throw 'Timed registry helper remained alive after owned Job Object termination.' }
    $passed.Add('class registry helper termination')
    $passed.Add('class registry helper Job Object cleanup')
    $timeoutRows = @(Get-Content -LiteralPath $timeoutProgress | ForEach-Object { $_ | ConvertFrom-Json })
    if ($timeoutRows[-1].status -cne 'budget-exceeded' -or @($timeoutRows | Where-Object {
        $_.PSObject.Properties['substage'] -and [string]$_.substage -ceq 'helper-cleanup' -and [string]$_.status -ceq 'success'
    }).Count -ne 1) {
        throw 'Registry timeout did not durably record failure and owned helper cleanup.'
    }
    $passed.Add('class registry timeout evidence is durable')
}
finally {
    foreach ($c7Context in @($c7SuccessContext, $c7TimeoutContext)) {
        if ($c7Context -and $c7Context.writer) { $c7Context.writer.Dispose(); $c7Context.writer = $null }
    }
    if (Test-Path -LiteralPath $c7TelemetryRoot) { Remove-Item -LiteralPath $c7TelemetryRoot -Recurse -Force }
}

function Get-G04DCTestLegacyRegistryDigest {
    param([Parameter(Mandatory = $true)] [string]$NativePath)
    $reg = Join-Path $env:SystemRoot 'System32\reg.exe'
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $reg
    $startInfo.Arguments = "query `"$NativePath`" /s"
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $startInfo.StandardOutputEncoding = $utf8Strict
    $startInfo.StandardErrorEncoding = $utf8Strict
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    try {
        if (!$process.Start()) { throw 'Legacy registry fixture query did not start.' }
        $stdout = $process.StandardOutput.ReadToEnd()
        $stderr = $process.StandardError.ReadToEnd()
        $process.WaitForExit()
        if ($process.ExitCode -ne 0) { throw "Legacy registry fixture query exited $($process.ExitCode)." }
        $combined = $stdout + $(if ([string]::IsNullOrEmpty($stderr)) { '' } else { "`r`n$stderr" })
        return Get-G04DCTestLegacyNativeDigest -Text $combined
    }
    finally { $process.Dispose() }
}

function Get-G04DCTestOptimizedRegistryDigest {
    param([Parameter(Mandatory = $true)] [string]$NativePath)
    return & $g04dcModule { param([string]$Path) Get-G04DCRegistryTreeDigest -NativePath $Path -AllowTestNativePath } $NativePath
}

function Assert-G04DCTestRegistryDigestEquivalence {
    param([Parameter(Mandatory = $true)] [string]$NativePath)
    $legacy = Get-G04DCTestLegacyRegistryDigest -NativePath $NativePath
    $optimized = Get-G04DCTestOptimizedRegistryDigest -NativePath $NativePath
    if ($legacy.rowCount -ne $optimized.rowCount -or $legacy.sha256 -cne $optimized.sha256) {
        throw 'Optimized registry query differs from the legacy query on the GUID-owned fixture.'
    }
    return [string]$optimized.sha256
}

$c7RegistryTestId = [guid]::NewGuid().ToString('N')
$c7RegistryNativeRoot = "Software\DocumentStudio-G04DC-C7-Registry-$c7RegistryTestId"
$c7RegistryPath = "HKEY_CURRENT_USER\$c7RegistryNativeRoot"
$c7RegistryBase = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser, [Microsoft.Win32.RegistryView]::Registry64)
$c7RegistryRoot = $c7RegistryBase.CreateSubKey($c7RegistryNativeRoot)
if (!$c7RegistryRoot) { throw 'Could not create the C7 GUID-owned registry fixture.' }
$c7RegistryRoot.Dispose()
try {
    $baselineDigest = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
    $key = $c7RegistryBase.CreateSubKey("$c7RegistryNativeRoot\Created")
    $key.Dispose()
    $createdDigest = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
    if ($createdDigest -ceq $baselineDigest) { throw 'Registry key creation did not change the full digest.' }
    $passed.Add('registry key creation mutation equivalence')
    $c7RegistryBase.DeleteSubKeyTree("$c7RegistryNativeRoot\Created", $false)
    $deletedDigest = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
    if ($deletedDigest -ceq $createdDigest) { throw 'Registry key deletion did not change the full digest.' }
    $passed.Add('registry key deletion mutation equivalence')

    $nested = $c7RegistryBase.CreateSubKey("$c7RegistryNativeRoot\Outer\Inner")
    $nested.Dispose()
    $nestedDigest = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
    if ($nestedDigest -ceq $deletedDigest) { throw 'Nested registry key creation did not change the full digest.' }
    $passed.Add('nested registry key mutation equivalence')
    $c7RegistryBase.DeleteSubKeyTree("$c7RegistryNativeRoot\Outer\Inner", $false)
    $renamed = $c7RegistryBase.CreateSubKey("$c7RegistryNativeRoot\Outer\Renamed")
    $renamed.Dispose()
    $renamedDigest = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
    if ($renamedDigest -ceq $nestedDigest) { throw 'Registry key rename shape did not change the full digest.' }
    $passed.Add('registry key rename mutation equivalence')

    $values = $c7RegistryBase.CreateSubKey("$c7RegistryNativeRoot\Values")
    try {
        $defaultAbsent = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
        $values.SetValue('', '', [Microsoft.Win32.RegistryValueKind]::String)
        $defaultEmpty = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
        if ($defaultAbsent -ceq $defaultEmpty) { throw 'Absent and present-empty defaults share one registry digest.' }
        $passed.Add('registry default absent-empty distinction')
        $values.SetValue('', 'normal', [Microsoft.Win32.RegistryValueKind]::String)
        $normalString = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
        if ($normalString -ceq $defaultEmpty) { throw 'Normal default string change was not detected.' }
        $passed.Add('registry normal string mutation equivalence')
        $values.SetValue('Expand', '%TEMP%\DocumentStudio', [Microsoft.Win32.RegistryValueKind]::ExpandString)
        $expandDigest = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
        if ($expandDigest -ceq $normalString) { throw 'Stored REG_EXPAND_SZ change was not detected.' }
        $passed.Add('registry expand-string stored-value equivalence')
        $values.SetValue('Multi', [string[]]@('alpha', '', 'beta'), [Microsoft.Win32.RegistryValueKind]::MultiString)
        $multiDigest = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
        if ($multiDigest -ceq $expandDigest) { throw 'REG_MULTI_SZ change was not detected.' }
        $passed.Add('registry multi-string mutation equivalence')
        $values.SetValue('Binary', [byte[]]@(0, 1, 127, 128, 255), [Microsoft.Win32.RegistryValueKind]::Binary)
        $binaryDigest = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
        if ($binaryDigest -ceq $multiDigest) { throw 'REG_BINARY change was not detected.' }
        $passed.Add('registry binary mutation equivalence')
        $values.SetValue('Dword', 42, [Microsoft.Win32.RegistryValueKind]::DWord)
        $dwordDigest = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
        if ($dwordDigest -ceq $binaryDigest) { throw 'DWORD change was not detected.' }
        $passed.Add('registry DWORD mutation equivalence')
        $values.SetValue('Qword', [long]4294967297, [Microsoft.Win32.RegistryValueKind]::QWord)
        $qwordDigest = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
        if ($qwordDigest -ceq $dwordDigest) { throw 'QWORD change was not detected.' }
        $passed.Add('registry QWORD mutation equivalence')
        $values.SetValue('Changing', 'text', [Microsoft.Win32.RegistryValueKind]::String)
        $kindBefore = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
        $values.SetValue('Changing', [byte[]]@(1, 2, 3), [Microsoft.Win32.RegistryValueKind]::Binary)
        $kindAfter = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
        if ($kindBefore -ceq $kindAfter) { throw 'Registry value kind change was not detected.' }
        $passed.Add('registry value-kind mutation equivalence')
        $values.SetValue('Changing', [byte[]]@(1, 2, 4), [Microsoft.Win32.RegistryValueKind]::Binary)
        $dataAfter = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
        if ($kindAfter -ceq $dataAfter) { throw 'Registry value data change was not detected.' }
        $passed.Add('registry value-data mutation equivalence')
        $values.DeleteValue('Changing', $false)
        $valueDeleted = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
        if ($dataAfter -ceq $valueDeleted) { throw 'Registry value deletion was not detected.' }
        $passed.Add('registry value deletion mutation equivalence')
        $unicodeName = -join @([char]0x0928, [char]0x092E)
        $unicodeValue = -join @([char]0x0924, [char]0x0947, [char]0x0932, [char]0x0941, [char]0x0917, [char]0x0941)
        $values.SetValue($unicodeName, $unicodeValue, [Microsoft.Win32.RegistryValueKind]::String)
        $unicodeDigest = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
        if ($unicodeDigest -ceq $valueDeleted) { throw 'Unicode registry value change was not detected.' }
        $passed.Add('registry Unicode mutation equivalence')
        $values.SetValue('Long', ('x' * 65536), [Microsoft.Win32.RegistryValueKind]::String)
        $longDigest = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
        if ($longDigest -ceq $unicodeDigest) { throw 'Long bounded registry value change was not detected.' }
        $passed.Add('registry long bounded value equivalence')
    }
    finally { $values.Dispose() }

    $repeatA = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
    $repeatB = Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath
    if ($repeatA -cne $repeatB) { throw 'Repeated class registry digest is nondeterministic.' }
    $passed.Add('class registry repeated deterministic digest')
    $rawBeforeC7Read = Get-TestRegistryRawSnapshot -NativeSubKey $c7RegistryNativeRoot
    Assert-G04DCTestRegistryDigestEquivalence -NativePath $c7RegistryPath | Out-Null
    $rawAfterC7Read = Get-TestRegistryRawSnapshot -NativeSubKey $c7RegistryNativeRoot
    if ($rawBeforeC7Read -cne $rawAfterC7Read) { throw 'Optimized class registry collector mutated the GUID-owned fixture.' }
    $passed.Add('optimized class registry collector performs no mutation')
    $passed.Add('class registry disappearing key-value transition detected')
}
finally {
    $c7RegistryBase.DeleteSubKeyTree($c7RegistryNativeRoot, $false)
    $c7RegistryBase.Dispose()
}

$shortcutRetryRoot = Join-Path ([IO.Path]::GetTempPath()) ('g04dc-shortcut-retry-' + [guid]::NewGuid().ToString('N'))
$shortcutRetryMarker = $shortcutRetryRoot + '.ready'
$shortcutRetryProcess = $null
try {
    [IO.Directory]::CreateDirectory($shortcutRetryRoot) | Out-Null
    $shortcutRetryFile = Join-Path $shortcutRetryRoot 'locked.lnk'
    [IO.File]::WriteAllBytes($shortcutRetryFile, [Text.Encoding]::ASCII.GetBytes('shortcut-evidence'))
    $escapedRetryFile = $shortcutRetryFile.Replace("'", "''")
    $escapedRetryMarker = $shortcutRetryMarker.Replace("'", "''")
    $shortcutRetryCode = "`$stream=[IO.FileStream]::new('$escapedRetryFile',[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::None);try{[IO.File]::WriteAllText('$escapedRetryMarker','ready');Start-Sleep -Milliseconds 500}finally{`$stream.Dispose()}"
    $encodedRetryCode = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($shortcutRetryCode))
    $shortcutRetryStartInfo = [Diagnostics.ProcessStartInfo]::new()
    $shortcutRetryStartInfo.FileName = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $shortcutRetryStartInfo.Arguments = "-NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $encodedRetryCode"
    $shortcutRetryStartInfo.UseShellExecute = $false
    $shortcutRetryStartInfo.CreateNoWindow = $true
    $shortcutRetryProcess = [Diagnostics.Process]::Start($shortcutRetryStartInfo)
    $retryWait = [Diagnostics.Stopwatch]::StartNew()
    while (!(Test-Path -LiteralPath $shortcutRetryMarker -PathType Leaf) -and $retryWait.ElapsedMilliseconds -lt 5000) { Start-Sleep -Milliseconds 25 }
    if (!(Test-Path -LiteralPath $shortcutRetryMarker -PathType Leaf)) { throw 'Synthetic shortcut lock did not become ready.' }
    $retriedShortcutDigest = & $module { param([string]$Root) Get-G04DCDirectoryTreeDigest -Roots @($Root) } $shortcutRetryRoot
    if ($retriedShortcutDigest.rowCount -ne 1 -or [string]$retriedShortcutDigest.sha256 -notmatch '^[0-9a-f]{64}$') { throw 'Transient shortcut sharing retry lost the complete entry.' }
    $passed.Add('shortcut catalog transient sharing retry preserves full entry')
    $shortcutRetryProcess.WaitForExit()

    $exclusiveLock = [IO.FileStream]::new($shortcutRetryFile, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
    try {
        Assert-Throws 'shortcut catalog persistent sharing failure remains fail closed' 'DIRECTORY_TREE_CAPTURE_UNSTABLE' {
            & $module { param([string]$Root) Get-G04DCDirectoryTreeDigest -Roots @($Root) } $shortcutRetryRoot | Out-Null
        }
    }
    finally { $exclusiveLock.Dispose() }
}
finally {
    if ($shortcutRetryProcess -and !$shortcutRetryProcess.HasExited) { $shortcutRetryProcess.Kill(); $shortcutRetryProcess.WaitForExit() }
    if ($shortcutRetryProcess) { $shortcutRetryProcess.Dispose() }
    if (Test-Path -LiteralPath $shortcutRetryRoot) { Remove-Item -LiteralPath $shortcutRetryRoot -Recurse -Force }
    if (Test-Path -LiteralPath $shortcutRetryMarker) { Remove-Item -LiteralPath $shortcutRetryMarker -Force }
}

$classRegistryHelperSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'ClassRegistryDigest.cs') -Raw
if ($commonSource -match '(?i)UseShellExecute\s*=\s*\$true|cmd\.exe' -or $classRegistryHelperSource -match '(?i)Microsoft\.Win32|CreateSubKey|SetValue|DeleteValue|DeleteSubKey') {
    throw 'C7 streaming registry collector introduced a shell or registry mutation API.'
}
$passed.Add('class registry collector uses no shell')
if ($commonSource -match '(?i)Win32_Product' -or $classRegistryHelperSource -match '(?i)Win32_Product') { throw 'C7 registry digest introduced Win32_Product.' }
$passed.Add('class registry collector uses no Win32_Product')
if ($classRegistryHelperSource -match '(?i)WriteAllText|WriteAllBytes|FileMode\.Create|FileMode\.CreateNew') { throw 'C7 class registry helper writes raw output to disk.' }
$passed.Add('class registry helper creates no raw temporary artifact')
if (!$commonSource.Contains('PhaseBudgetMilliseconds = 240000') -or !$commonSource.Contains('MaximumRawBytes = 134217728')) { throw 'C7 changed the hard phase ceiling or omitted the raw-byte ceiling.' }
$passed.Add('hard class registry phase ceiling remains 240000 ms')

$workflowSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot '..\..\.github\workflows\g04d-c-libreoffice-runtime-proof.yml') -Raw
$precheckSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'Invoke-G04DCMachineStatePrecheck.ps1') -Raw
$adminSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'Invoke-G04DCAdminImageProof.ps1') -Raw
$minimalSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'Invoke-G04DCMinimalMsiProof.ps1') -Raw
$nativeRows = @(& (Get-Module G04DC.Common) { ConvertFrom-G04DCNativeTextRows -Text "alpha  `r`n`r`nbeta`t`r`n" })
if (($nativeRows -join '|') -cne 'alpha||beta') { throw 'Native registry digest row normalization lost internal blank rows or trailing-space semantics.' }
$passed.Add('registry digest preserves legacy blank row semantics')
if (!$commonSource.Contains('KillOnCloseJob') -or !$commonSource.Contains('TerminateAndVerify') -or !$commonSource.Contains('MACHINE_STATE_CAPTURE_HELPER_CLEANUP_FAILED')) {
    throw 'Native registry helper is not assigned to verified kill-on-close ownership.'
}
$passed.Add('registry digest helper cleanup is terminally verified')
if (!$precheckSource.Contains('classRegistryDigestTargetMilliseconds = 180000L') -or !$precheckSource.Contains('CLASS_REGISTRY_DIGEST_TARGET_EXCEEDED')) { throw 'C7 PRECHECK does not enforce the 180-second class registry target.' }
$passed.Add('C7 class registry target is 180000 ms')
if (!$workflowSource.Contains('proofMode:') -or !$workflowSource.Contains('- precheck') -or $workflowSource -match '(?m)^\s+(push|pull_request|schedule|workflow_run|repository_dispatch):') { throw 'PRECHECK workflow trigger/input boundary is invalid.' }
$passed.Add('PRECHECK workflow dispatch only')
if ($precheckSource -match '(?i)msiexec|/a|administrative-extraction') { throw 'PRECHECK contains an administrative extraction path.' }
$passed.Add('PRECHECK never invokes administrative extraction')
if ($precheckSource -match '(?i)soffice|libreoffice\.exe|Invoke-G04DCSandboxSmoke|AppContainer') { throw 'PRECHECK contains a runtime launch or sandbox path.' }
$passed.Add('PRECHECK never launches runtime')
if (!$adminSource.Contains("@('/a'") -or !$workflowSource.Contains("inputs.proofMode == 'full'")) { throw 'FULL proof path was not retained behind the full mode.' }
$passed.Add('FULL workflow retains frozen proof path')
if ($adminSource.IndexOf('Assert-G04DCMachineStatePerformanceEvidence', [StringComparison]::Ordinal) -lt 0 -or
    $adminSource.IndexOf('Assert-G04DCMachineStatePerformanceEvidence', [StringComparison]::Ordinal) -gt $adminSource.IndexOf("@('/a'", [StringComparison]::Ordinal) -or
    $minimalSource.IndexOf('Assert-G04DCMachineStatePerformanceEvidence', [StringComparison]::Ordinal) -lt 0 -or
    $minimalSource.IndexOf('Assert-G04DCMachineStatePerformanceEvidence', [StringComparison]::Ordinal) -gt $minimalSource.IndexOf("@('/i'", [StringComparison]::Ordinal) -or
    !$precheckSource.Contains("-StateOutputPath (Join-Path `$evidence 'machine-pre.json')")) {
    throw 'FULL/PRECHECK Windows Installer gates do not require bounded durable state serialization first.'
}
$passed.Add('machine-state performance gate precedes every Windows Installer command')
if ([regex]::Matches($workflowSource, '(?m)^\s+timeout-minutes:\s*90\s*$').Count -ne 2) { throw 'FULL proof timeout is no longer 90 minutes for both proof jobs.' }
$passed.Add('FULL workflow retains 90 minute timeout')
if (!$workflowSource.Contains('g04d-c-machine-state-precheck-evidence') -or [regex]::Matches($workflowSource, 'if: \$\{\{ always\(\) \}\}').Count -lt 6) { throw 'Always-run progress/performance artifact preservation is incomplete.' }
$passed.Add('machine-state failure evidence always uploaded')
$changedPaths = @(git -C (Join-Path $PSScriptRoot '..\..') diff --name-only origin/main --)
if (@($changedPaths | Where-Object { $_ -match '^(apps|packages|src|migrations)/' }).Count -ne 0) { throw 'C5 modified a production path.' }
$passed.Add('C5 has no production impact path')

$sourcePolicyTestRoot = Join-Path ([IO.Path]::GetTempPath()) ('g04dc-source-policy-test-' + [guid]::NewGuid().ToString('N'))
$sourceGateScript = Join-Path $PSScriptRoot 'Test-G04DCPowerShell51Source.ps1'
$sourcePolicyScript = Join-Path $PSScriptRoot '..\g04d_c_powershell_source_policy.py'
New-Item -ItemType Directory -Path $sourcePolicyTestRoot | Out-Null
function New-G04DCSourcePolicyCase([string]$Name) {
    $caseRoot = Join-Path $sourcePolicyTestRoot $Name
    $caseSource = Join-Path $caseRoot 'source'
    New-Item -ItemType Directory -Path $caseSource -Force | Out-Null
    return [pscustomobject][ordered]@{ root = $caseRoot; source = $caseSource }
}
function Write-G04DCSourcePolicyBytes([string]$Path, [byte[]]$Bytes) {
    $parent = [IO.Path]::GetDirectoryName($Path)
    if (!(Test-Path -LiteralPath $parent -PathType Container)) { New-Item -ItemType Directory -Path $parent -Force | Out-Null }
    [IO.File]::WriteAllBytes($Path, $Bytes)
}
function Invoke-G04DCSourcePolicyCase($Case) {
    return & $sourceGateScript `
        -RepositoryRoot $Case.root `
        -SourceRoot $Case.source `
        -PythonPolicyPath $sourcePolicyScript `
        -Quiet `
        -PassThru
}

try {
    $validCase = New-G04DCSourcePolicyCase 'valid-sources'
    Write-G04DCSourcePolicyBytes -Path (Join-Path $validCase.source 'valid.ps1') -Bytes ([Text.Encoding]::ASCII.GetBytes("Write-Output 'valid'`r`n"))
    Write-G04DCSourcePolicyBytes -Path (Join-Path $validCase.source 'valid.psm1') -Bytes ([Text.Encoding]::ASCII.GetBytes("function Get-Valid { return 1 }`r`n"))
    $validReport = Invoke-G04DCSourcePolicyCase $validCase
    if ($validReport.sourceFileCount -ne 2 -or $validReport.asciiGateStatus -cne 'PASS' -or $validReport.parserGateStatus -cne 'PASS') { throw 'Valid ASCII sources did not pass both source gates.' }
    $passed.Add('ASCII-only .ps1 source passes')
    $passed.Add('ASCII-only .psm1 source passes')

    $emDashCase = New-G04DCSourcePolicyCase 'utf8-em-dash'
    $emDashBytes = [byte[]](@([Text.Encoding]::ASCII.GetBytes('Write-Output "blocked ')) + @(0xE2, 0x80, 0x94) + @([Text.Encoding]::ASCII.GetBytes('"')))
    Write-G04DCSourcePolicyBytes -Path (Join-Path $emDashCase.source 'em-dash.ps1') -Bytes $emDashBytes
    Assert-Throws 'UTF-8-no-BOM em dash source rejected' 'G04DC_SOURCE_ASCII_INVALID' { Invoke-G04DCSourcePolicyCase $emDashCase | Out-Null }

    $smartQuoteCase = New-G04DCSourcePolicyCase 'utf8-smart-quote'
    $smartQuoteBytes = [byte[]](@([Text.Encoding]::ASCII.GetBytes('# quote ')) + @(0xE2, 0x80, 0x9C))
    Write-G04DCSourcePolicyBytes -Path (Join-Path $smartQuoteCase.source 'smart-quote.ps1') -Bytes $smartQuoteBytes
    Assert-Throws 'UTF-8-no-BOM smart quote source rejected' 'G04DC_SOURCE_ASCII_INVALID' { Invoke-G04DCSourcePolicyCase $smartQuoteCase | Out-Null }

    $utf8BomCase = New-G04DCSourcePolicyCase 'utf8-bom'
    Write-G04DCSourcePolicyBytes -Path (Join-Path $utf8BomCase.source 'utf8-bom.ps1') -Bytes ([byte[]](0xEF, 0xBB, 0xBF, 0x23, 0x20, 0x78))
    Assert-Throws 'UTF-8 BOM source rejected' 'G04DC_SOURCE_ASCII_INVALID' { Invoke-G04DCSourcePolicyCase $utf8BomCase | Out-Null }

    $utf16LeCase = New-G04DCSourcePolicyCase 'utf16-le-bom'
    Write-G04DCSourcePolicyBytes -Path (Join-Path $utf16LeCase.source 'utf16-le.ps1') -Bytes ([byte[]](0xFF, 0xFE, 0x57, 0x00))
    Assert-Throws 'UTF-16 LE BOM source rejected' 'G04DC_SOURCE_ASCII_INVALID' { Invoke-G04DCSourcePolicyCase $utf16LeCase | Out-Null }

    $utf16BeCase = New-G04DCSourcePolicyCase 'utf16-be-bom'
    Write-G04DCSourcePolicyBytes -Path (Join-Path $utf16BeCase.source 'utf16-be.ps1') -Bytes ([byte[]](0xFE, 0xFF, 0x00, 0x57))
    Assert-Throws 'UTF-16 BE BOM source rejected' 'G04DC_SOURCE_ASCII_INVALID' { Invoke-G04DCSourcePolicyCase $utf16BeCase | Out-Null }

    $utf32LeCase = New-G04DCSourcePolicyCase 'utf32-le-bom'
    Write-G04DCSourcePolicyBytes -Path (Join-Path $utf32LeCase.source 'utf32-le.ps1') -Bytes ([byte[]](0xFF, 0xFE, 0x00, 0x00, 0x57, 0x00, 0x00, 0x00))
    Assert-Throws 'UTF-32 LE BOM source rejected' 'G04DC_SOURCE_ASCII_INVALID' { Invoke-G04DCSourcePolicyCase $utf32LeCase | Out-Null }

    $utf32BeCase = New-G04DCSourcePolicyCase 'utf32-be-bom'
    Write-G04DCSourcePolicyBytes -Path (Join-Path $utf32BeCase.source 'utf32-be.ps1') -Bytes ([byte[]](0x00, 0x00, 0xFE, 0xFF, 0x00, 0x00, 0x00, 0x57))
    Assert-Throws 'UTF-32 BE BOM source rejected' 'G04DC_SOURCE_ASCII_INVALID' { Invoke-G04DCSourcePolicyCase $utf32BeCase | Out-Null }

    $commentCase = New-G04DCSourcePolicyCase 'non-ascii-comment'
    $commentBytes = [byte[]](@([Text.Encoding]::ASCII.GetBytes('# comment ')) + @(0xC3, 0xA9))
    Write-G04DCSourcePolicyBytes -Path (Join-Path $commentCase.source 'comment.ps1') -Bytes $commentBytes
    Assert-Throws 'non-ASCII source comment rejected' 'G04DC_SOURCE_ASCII_INVALID' { Invoke-G04DCSourcePolicyCase $commentCase | Out-Null }

    $stringCase = New-G04DCSourcePolicyCase 'non-ascii-string'
    $stringBytes = [byte[]](@([Text.Encoding]::ASCII.GetBytes('Write-Output "')) + @(0xC3, 0xA9) + @([Text.Encoding]::ASCII.GetBytes('"')))
    Write-G04DCSourcePolicyBytes -Path (Join-Path $stringCase.source 'string.ps1') -Bytes $stringBytes
    Assert-Throws 'non-ASCII source string rejected' 'G04DC_SOURCE_ASCII_INVALID' { Invoke-G04DCSourcePolicyCase $stringCase | Out-Null }

    $invalidParserCase = New-G04DCSourcePolicyCase 'invalid-ascii-parser'
    Write-G04DCSourcePolicyBytes -Path (Join-Path $invalidParserCase.source 'invalid.ps1') -Bytes ([Text.Encoding]::ASCII.GetBytes('Write-Output "unterminated'))
    Assert-Throws 'invalid ASCII PowerShell source rejected by parser' 'G04DC_SOURCE_PARSE_INVALID' { Invoke-G04DCSourcePolicyCase $invalidParserCase | Out-Null }
    if ($PSVersionTable.PSEdition -cne 'Desktop' -or $PSVersionTable.PSVersion.Major -ne 5 -or $PSVersionTable.PSVersion.Minor -ne 1 -or
        $validReport.parserErrorCount -ne 0 -or $validReport.unknownTokenCount -ne 0) {
        throw 'Valid source was not accepted by the actual Windows PowerShell 5.1 parser.'
    }
    $passed.Add('valid ASCII PowerShell script accepted by Windows PowerShell 5.1')

    $documentationCase = New-G04DCSourcePolicyCase 'documentation-unicode'
    Write-G04DCSourcePolicyBytes -Path (Join-Path $documentationCase.source 'valid.ps1') -Bytes ([Text.Encoding]::ASCII.GetBytes("Write-Output 'valid'"))
    Write-G04DCSourcePolicyBytes -Path (Join-Path $documentationCase.source 'guide.md') -Bytes ([byte[]](0x23, 0x20, 0xE2, 0x80, 0x94))
    $documentationReport = Invoke-G04DCSourcePolicyCase $documentationCase
    if ($documentationReport.sourceFileCount -ne 1 -or $documentationReport.parserGateStatus -cne 'PASS') { throw 'Unicode documentation crossed the executable source boundary.' }
    $passed.Add('documentation Unicode remains allowed')

    $fixtureCase = New-G04DCSourcePolicyCase 'non-executable-fixture'
    Write-G04DCSourcePolicyBytes -Path (Join-Path $fixtureCase.source 'valid.ps1') -Bytes ([Text.Encoding]::ASCII.GetBytes("Write-Output 'valid'"))
    Write-G04DCSourcePolicyBytes -Path (Join-Path $fixtureCase.source 'explicit-encoding.ps1.fixture') -Bytes ([byte[]](0xE2, 0x80, 0x94))
    $fixtureReport = Invoke-G04DCSourcePolicyCase $fixtureCase
    if ($fixtureReport.sourceFileCount -ne 1 -or $fixtureReport.parserGateStatus -cne 'PASS') { throw 'Explicit non-executable encoding fixture crossed the executable source boundary.' }
    $passed.Add('non-executable explicit encoding fixtures remain allowed')

    $offsetCase = New-G04DCSourcePolicyCase 'exact-offset'
    $offsetPrefix = [Text.Encoding]::ASCII.GetBytes('# exact-offset ')
    $offsetBytes = [byte[]](@($offsetPrefix) + @(0xE2, 0x80, 0x94))
    Write-G04DCSourcePolicyBytes -Path (Join-Path $offsetCase.source 'exact.ps1') -Bytes $offsetBytes
    try { Invoke-G04DCSourcePolicyCase $offsetCase | Out-Null; throw 'Exact-offset source policy case did not fail.' }
    catch {
        $expectedOffsetEvidence = "source/exact.ps1 byte offset $($offsetPrefix.Length)"
        if ($_.Exception.Message -notmatch [regex]::Escape($expectedOffsetEvidence)) { throw "Exact offending path/offset was not reported: $($_.Exception.Message)" }
    }
    $passed.Add('repository validation reports exact source path and byte offset')

    $deterministicCase = New-G04DCSourcePolicyCase 'deterministic-discovery'
    Write-G04DCSourcePolicyBytes -Path (Join-Path $deterministicCase.source 'z.ps1') -Bytes ([Text.Encoding]::ASCII.GetBytes("'z'"))
    Write-G04DCSourcePolicyBytes -Path (Join-Path $deterministicCase.source 'A.psm1') -Bytes ([Text.Encoding]::ASCII.GetBytes("'a'"))
    Write-G04DCSourcePolicyBytes -Path (Join-Path $deterministicCase.source 'nested\b.ps1') -Bytes ([Text.Encoding]::ASCII.GetBytes("'b'"))
    $deterministicReport = Invoke-G04DCSourcePolicyCase $deterministicCase
    if (($deterministicReport.sourceFiles -join '|') -cne 'source/A.psm1|source/nested/b.ps1|source/z.ps1') { throw 'Executable source discovery is not deterministic.' }
    $passed.Add('G04D-C executable source discovery is deterministic')

    $repositoryRootForSourceGate = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
    $repositoryReport = & $sourceGateScript -RepositoryRoot $repositoryRootForSourceGate -SourceRoot $PSScriptRoot -PythonPolicyPath $sourcePolicyScript -Quiet -PassThru
    if ($repositoryReport.asciiGateStatus -cne 'PASS' -or $repositoryReport.parserGateStatus -cne 'PASS' -or
        $repositoryReport.sourceFiles -cnotcontains 'scripts/g04d-c/Invoke-G04DCMachineStatePrecheck.ps1') {
        throw 'Current PRECHECK script did not pass both source compatibility gates.'
    }
    $passed.Add('current PRECHECK source passes ASCII and parser gates')

    $workflowInvokedSources = @([regex]::Matches($workflowSource, '(?i)[.]\\scripts\\g04d-c\\([A-Za-z0-9.-]+[.]ps(?:1|m1))') | ForEach-Object {
        'scripts/g04d-c/' + $_.Groups[1].Value
    } | Sort-Object -Unique)
    $missingWorkflowSources = @($workflowInvokedSources | Where-Object { $repositoryReport.sourceFiles -cnotcontains $_ })
    if ($workflowInvokedSources.Count -eq 0 -or $missingWorkflowSources.Count -ne 0 -or $repositoryReport.parserGateStatus -cne 'PASS') {
        throw "Workflow-invoked source compatibility is incomplete: $($missingWorkflowSources -join ', ')"
    }
    $passed.Add('every workflow-invoked G04D-C source passes both gates')
    if ($repositoryReport.incompleteTokenCount -ne 0 -or $repositoryReport.malformedStringTokenCount -ne 0 -or $repositoryReport.unknownTokenCount -ne 0) {
        throw 'Repository parser report contains incomplete or malformed tokens.'
    }
    $passed.Add('repository parser report has zero incomplete or malformed tokens')
}
finally {
    if (Test-Path -LiteralPath $sourcePolicyTestRoot) { Remove-Item -LiteralPath $sourcePolicyTestRoot -Recurse -Force }
}

if ($passed.Count -ne 265) { throw "Expected 265 fail-closed cases; passed $($passed.Count)." }
Write-Output "G04D-C fail-closed boundary tests passed ($($passed.Count) cases): $($passed -join '; ')"
