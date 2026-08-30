Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:G04DCExpectedMsi = [ordered]@{
    Url = 'https://download.documentfoundation.org/libreoffice/stable/26.2.5/win/x86_64/LibreOffice_26.2.5_Win_x86-64.msi'
    FileName = 'LibreOffice_26.2.5_Win_x86-64.msi'
    SizeBytes = 372948992L
    Sha256 = 'f15ba07bfcb0186986cf3171063506f5d207c11f8cc051ba0d135209e9e915f9'
    ProductVersion = '26.2.5.2'
    Architecture = 'x64'
    ProductCode = '{3B467719-C25B-478C-8F4C-8E2EDA0E2093}'
    UpgradeCode = '{4B17E523-5D91-4E69-BD96-7FD81CFA81BB}'
    PackageCode = '{5D7F0329-EE50-4638-9909-70F6CEB181D0}'
    Signer = 'The Document Foundation'
    SignerThumbprint = '6480532A562B36D1BFFFC5B5EACF7C31E74E9B28'
    TimestampSignerThumbprint = '571468410CA85AF3424EF9164A513610F4D38D98'
}

$script:G04DCTables = @(
    'Feature', 'FeatureComponents', 'Component', 'File', 'Registry',
    'RemoveRegistry', 'ServiceInstall', 'ServiceControl', 'Font', 'Extension',
    'ProgId', 'MIME', 'Verb', 'Class', 'AppId', 'Shortcut', 'Environment',
    'CustomAction', 'InstallExecuteSequence', 'InstallUISequence', 'Property', 'Directory',
    'AdminExecuteSequence', 'AdminUISequence', 'Media'
)

function Get-G04DCExpectedMsi {
    [CmdletBinding()]
    param()
    return [pscustomobject]$script:G04DCExpectedMsi
}

function Write-G04DCJson {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [string]$Path,
        [Parameter(Mandatory = $true)] [AllowNull()] $Value,
        [int]$Depth = 20
    )
    $parent = Split-Path -Parent $Path
    if ($parent -and !(Test-Path -LiteralPath $parent)) {
        New-Item -ItemType Directory -Path $parent | Out-Null
    }
    $json = $Value | ConvertTo-Json -Depth $Depth
    [System.IO.File]::WriteAllText($Path, $json + [Environment]::NewLine, [System.Text.UTF8Encoding]::new($false))
}

function Get-G04DCSha256 {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] [string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-G04DCAuthenticodeEvidence {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] [string]$Path)
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    $chain = [System.Security.Cryptography.X509Certificates.X509Chain]::new()
    try {
        $chainValid = [bool]($signature.SignerCertificate -and $chain.Build($signature.SignerCertificate))
        return [pscustomobject][ordered]@{
            status = [string]$signature.Status
            signerSubject = if ($signature.SignerCertificate) { $signature.SignerCertificate.Subject } else { $null }
            signerThumbprint = if ($signature.SignerCertificate) { $signature.SignerCertificate.Thumbprint.ToUpperInvariant() } else { $null }
            chainValid = $chainValid
            chainStatus = @($chain.ChainStatus | ForEach-Object { [string]$_.Status })
            chain = @($chain.ChainElements | ForEach-Object {
                [pscustomobject][ordered]@{
                    subject = $_.Certificate.Subject
                    issuer = $_.Certificate.Issuer
                    thumbprint = $_.Certificate.Thumbprint.ToUpperInvariant()
                    notBeforeUtc = $_.Certificate.NotBefore.ToUniversalTime().ToString('o')
                    notAfterUtc = $_.Certificate.NotAfter.ToUniversalTime().ToString('o')
                }
            })
        }
    }
    finally { $chain.Dispose() }
}

function Test-G04DCRestrictedIpAddress {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] [System.Net.IPAddress]$Address)

    if ($Address.IsIPv4MappedToIPv6) { $Address = $Address.MapToIPv4() }
    $bytes = $Address.GetAddressBytes()
    if ($Address.AddressFamily -eq [System.Net.Sockets.AddressFamily]::InterNetwork) {
        return (
            $bytes[0] -eq 0 -or
            $bytes[0] -eq 10 -or
            $bytes[0] -eq 127 -or
            ($bytes[0] -eq 100 -and $bytes[1] -ge 64 -and $bytes[1] -le 127) -or
            ($bytes[0] -eq 169 -and $bytes[1] -eq 254) -or
            ($bytes[0] -eq 172 -and $bytes[1] -ge 16 -and $bytes[1] -le 31) -or
            ($bytes[0] -eq 192 -and $bytes[1] -eq 0 -and $bytes[2] -in @(0, 2)) -or
            ($bytes[0] -eq 192 -and $bytes[1] -in @(31, 52, 88, 175)) -or
            ($bytes[0] -eq 192 -and $bytes[1] -eq 168) -or
            ($bytes[0] -eq 198 -and $bytes[1] -in @(18, 19, 51)) -or
            ($bytes[0] -eq 203 -and $bytes[1] -eq 0 -and $bytes[2] -eq 113) -or
            $bytes[0] -ge 224
        )
    }
    if ($Address.AddressFamily -eq [System.Net.Sockets.AddressFamily]::InterNetworkV6) {
        $globalUnicast = ($bytes[0] -band 0xE0) -eq 0x20
        $special2001 = $bytes[0] -eq 0x20 -and $bytes[1] -eq 0x01 -and ($bytes[2] -band 0xFE) -eq 0
        $documentation = ($bytes[0] -eq 0x20 -and $bytes[1] -eq 0x01 -and $bytes[2] -eq 0x0D -and $bytes[3] -eq 0xB8) -or
            ($bytes[0] -eq 0x3F -and $bytes[1] -eq 0xFF)
        return $Address.Equals([System.Net.IPAddress]::IPv6None) -or
            $Address.Equals([System.Net.IPAddress]::IPv6Loopback) -or
            $Address.IsIPv6LinkLocal -or $Address.IsIPv6Multicast -or $Address.IsIPv6SiteLocal -or
            (($bytes[0] -band 0xFE) -eq 0xFC) -or !$globalUnicast -or $special2001 -or $documentation
    }
    return $true
}

function Assert-G04DCAcquisitionUri {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [uri]$Uri,
        [switch]$CanonicalFirstRequest,
        [AllowEmptyCollection()] [string[]]$ResolvedAddresses
    )
    $expected = Get-G04DCExpectedMsi
    $strongAuthority = $Uri.GetComponents([UriComponents]::StrongAuthority, [UriFormat]::UriEscaped)
    if (!$Uri.IsAbsoluteUri -or $Uri.AbsoluteUri.Length -gt 4096 -or $Uri.Scheme -cne 'https' -or $Uri.Port -ne 443 -or
        $strongAuthority.Contains('@') -or ![string]::IsNullOrEmpty($Uri.Fragment)) {
        throw '[MSI_ACQUISITION_SOURCE_REJECTED] Acquisition URI must be absolute HTTPS on the default port with no userinfo or fragment.'
    }
    if ($CanonicalFirstRequest -and $Uri.AbsoluteUri -cne $expected.Url) {
        throw '[MSI_ACQUISITION_SOURCE_REJECTED] First request URI does not exactly match the canonical TDF URI.'
    }
    $literalHost = $Uri.DnsSafeHost.Trim('[', ']')
    $literal = $null
    if ([System.Net.IPAddress]::TryParse($literalHost, [ref]$literal)) {
        throw '[MSI_ACQUISITION_SOURCE_REJECTED] Raw IP-literal acquisition hosts are prohibited.'
    }
    $hostname = $Uri.IdnHost.TrimEnd('.')
    if ([string]::IsNullOrWhiteSpace($hostname) -or $hostname.Length -gt 253 -or $hostname -ieq 'localhost' -or $hostname.EndsWith('.localhost', [StringComparison]::OrdinalIgnoreCase)) {
        throw '[MSI_ACQUISITION_SOURCE_REJECTED] Localhost acquisition targets are prohibited.'
    }
    $addressText = @(if ($PSBoundParameters.ContainsKey('ResolvedAddresses')) {
        @($ResolvedAddresses)
    }
    else {
        try { @([System.Net.Dns]::GetHostAddresses($hostname) | ForEach-Object { $_.ToString() }) }
        catch { throw '[MSI_ACQUISITION_SOURCE_REJECTED] Acquisition hostname DNS resolution failed.' }
    })
    if ($addressText.Count -eq 0) { throw '[MSI_ACQUISITION_SOURCE_REJECTED] Acquisition hostname resolved to no addresses.' }
    $addresses = [System.Collections.Generic.List[System.Net.IPAddress]]::new()
    foreach ($text in @($addressText | Sort-Object -Unique)) {
        $address = $null
        if (![System.Net.IPAddress]::TryParse([string]$text, [ref]$address) -or (Test-G04DCRestrictedIpAddress -Address $address)) {
            throw '[MSI_ACQUISITION_SOURCE_REJECTED] Acquisition hostname resolves to loopback, link-local, private, multicast, reserved, or unspecified address space.'
        }
        $addresses.Add($address)
    }
    return [pscustomobject][ordered]@{
        uri = $Uri.AbsoluteUri
        hostname = $hostname
        resolvedAddresses = @($addresses.ToArray() | ForEach-Object { $_.ToString() } | Sort-Object -Unique)
    }
}

function Assert-G04DCPinnedRemoteEndpoint {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [string[]]$ApprovedAddresses,
        [Parameter(Mandatory = $true)] [System.Net.IPAddress]$ConnectedAddress
    )
    if (Test-G04DCRestrictedIpAddress -Address $ConnectedAddress) {
        throw '[MSI_ACQUISITION_SOURCE_REJECTED] Connected remote endpoint is in prohibited address space.'
    }
    $matched = $false
    foreach ($text in $ApprovedAddresses) {
        $approved = $null
        if ([System.Net.IPAddress]::TryParse($text, [ref]$approved)) {
            if ($approved.IsIPv4MappedToIPv6) { $approved = $approved.MapToIPv4() }
            $actual = $ConnectedAddress
            if ($actual.IsIPv4MappedToIPv6) { $actual = $actual.MapToIPv4() }
            if ($approved.Equals($actual)) { $matched = $true; break }
        }
    }
    if (!$matched) {
        throw '[MSI_ACQUISITION_SOURCE_REJECTED] Connected remote endpoint was not one of the prevalidated public addresses.'
    }
    return $true
}

function Read-G04DCHttpsHeaderBlock {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [System.IO.Stream]$Stream,
        [int]$MaximumBytes = 16384
    )
    $bytes = [System.Collections.Generic.List[byte]]::new()
    while ($true) {
        $value = $Stream.ReadByte()
        if ($value -lt 0) { throw '[MSI_ACQUISITION_FAILED] HTTPS response ended before its header block completed.' }
        if ($bytes.Count -ge $MaximumBytes) { throw '[MSI_ACQUISITION_FAILED] HTTPS response header block exceeded the bounded ceiling.' }
        $bytes.Add([byte]$value)
        $count = $bytes.Count
        if ($count -ge 4 -and $bytes[$count - 4] -eq 13 -and $bytes[$count - 3] -eq 10 -and $bytes[$count - 2] -eq 13 -and $bytes[$count - 1] -eq 10) { break }
    }
    return [Text.Encoding]::ASCII.GetString($bytes.ToArray())
}

function Read-G04DCHttpsLine {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [System.IO.Stream]$Stream,
        [int]$MaximumBytes = 1024
    )
    $bytes = [System.Collections.Generic.List[byte]]::new()
    while ($true) {
        $value = $Stream.ReadByte()
        if ($value -lt 0) { throw '[MSI_ACQUISITION_SIZE_INVALID] HTTPS body ended before its framing completed.' }
        if ($bytes.Count -ge $MaximumBytes) { throw '[MSI_ACQUISITION_SIZE_INVALID] HTTPS body framing line exceeded the bounded ceiling.' }
        $bytes.Add([byte]$value)
        $count = $bytes.Count
        if ($count -ge 2 -and $bytes[$count - 2] -eq 13 -and $bytes[$count - 1] -eq 10) {
            return [Text.Encoding]::ASCII.GetString($bytes.ToArray(), 0, $count - 2)
        }
    }
}

function Invoke-G04DCPinnedHttpsRequest {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [uri]$Uri,
        [Parameter(Mandatory = $true)] [string[]]$ApprovedAddresses
    )
    $selectedText = @($ApprovedAddresses | Sort-Object -Unique)[0]
    $selected = $null
    if (![System.Net.IPAddress]::TryParse($selectedText, [ref]$selected) -or (Test-G04DCRestrictedIpAddress -Address $selected)) {
        throw '[MSI_ACQUISITION_SOURCE_REJECTED] Pinned HTTPS transport received an invalid or prohibited address.'
    }
    $tcp = [System.Net.Sockets.TcpClient]::new($selected.AddressFamily)
    $tcp.ReceiveTimeout = 120000
    $tcp.SendTimeout = 60000
    $ssl = $null
    try {
        $tcp.Connect($selected, 443)
        $remote = [System.Net.IPEndPoint]$tcp.Client.RemoteEndPoint
        Assert-G04DCPinnedRemoteEndpoint -ApprovedAddresses $ApprovedAddresses -ConnectedAddress $remote.Address | Out-Null
        $network = $tcp.GetStream()
        $ssl = [System.Net.Security.SslStream]::new($network, $false)
        $certificates = [System.Security.Cryptography.X509Certificates.X509CertificateCollection]::new()
        $ssl.AuthenticateAsClient($Uri.IdnHost, $certificates, [System.Security.Authentication.SslProtocols]::Tls12, $true)
        $requestTarget = $Uri.PathAndQuery
        if ([string]::IsNullOrEmpty($requestTarget)) { $requestTarget = '/' }
        $requestText = "GET $requestTarget HTTP/1.1`r`nHost: $($Uri.IdnHost)`r`nUser-Agent: DocumentStudio-G04D-C-Proof/2.0`r`nAccept: */*`r`nAccept-Encoding: identity`r`nConnection: close`r`n`r`n"
        $requestBytes = [Text.Encoding]::ASCII.GetBytes($requestText)
        $ssl.Write($requestBytes, 0, $requestBytes.Length)
        $ssl.Flush()

        $headerBlock = Read-G04DCHttpsHeaderBlock -Stream $ssl
        $lines = @($headerBlock -split "`r`n")
        if ($lines.Count -lt 3 -or $lines[0] -notmatch '^HTTP/1\.[01] ([0-9]{3})(?: |$)') {
            throw '[MSI_ACQUISITION_FAILED] HTTPS response status line is invalid.'
        }
        $statusCode = [int]$Matches[1]
        $headers = @{}
        for ($lineIndex = 1; $lineIndex -lt $lines.Count; $lineIndex++) {
            $line = $lines[$lineIndex]
            if ([string]::IsNullOrEmpty($line)) { break }
            if ($line[0] -eq ' ' -or $line[0] -eq "`t" -or $line.IndexOf(':') -le 0) {
                throw '[MSI_ACQUISITION_FAILED] HTTPS response contains invalid or folded headers.'
            }
            $separator = $line.IndexOf(':')
            $name = $line.Substring(0, $separator).Trim()
            $value = $line.Substring($separator + 1).Trim()
            if ($name -notmatch '^[!#$%&''*+.^_`|~0-9A-Za-z-]+$') { throw '[MSI_ACQUISITION_FAILED] HTTPS response contains an invalid header name.' }
            if ($headers.ContainsKey($name)) { $headers[$name] = @($headers[$name]) + $value }
            else { $headers[$name] = [string[]]@($value) }
        }
        $location = $null
        if ($headers.ContainsKey('Location')) {
            if (@($headers['Location']).Count -ne 1) { throw '[MSI_ACQUISITION_SOURCE_REJECTED] Redirect response contains multiple Location values.' }
            $location = [string]$headers['Location'][0]
        }
        $contentLength = -1L
        if ($headers.ContainsKey('Content-Length')) {
            $lengthValues = @($headers['Content-Length'] | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Sort-Object -Unique)
            if ($lengthValues.Count -ne 1 -or $lengthValues[0] -notmatch '^[0-9]+$' -or ![long]::TryParse($lengthValues[0], [ref]$contentLength)) {
                throw '[MSI_ACQUISITION_SIZE_INVALID] HTTPS response Content-Length is invalid or ambiguous.'
            }
        }
        $transferEncoding = if ($headers.ContainsKey('Transfer-Encoding')) { (@($headers['Transfer-Encoding']) -join ',').Trim() } else { $null }
        if ($contentLength -ge 0 -and ![string]::IsNullOrEmpty($transferEncoding)) {
            throw '[MSI_ACQUISITION_SIZE_INVALID] HTTPS response contains ambiguous Content-Length and Transfer-Encoding framing.'
        }
        $contentEncoding = if ($headers.ContainsKey('Content-Encoding')) { (@($headers['Content-Encoding']) -join ',').Trim() } else { $null }
        if (![string]::IsNullOrEmpty($contentEncoding) -and $contentEncoding -ine 'identity') {
            throw '[MSI_ACQUISITION_SIZE_INVALID] Encoded HTTPS response bodies are prohibited.'
        }
        return [pscustomobject][ordered]@{
            statusCode = $statusCode
            location = $location
            contentLength = $contentLength
            transferEncoding = $transferEncoding
            stream = $ssl
            tcpClient = $tcp
            connectedAddress = $remote.Address.ToString()
            remoteEndpoint = $remote.ToString()
        }
    }
    catch {
        if ($ssl) { $ssl.Dispose() }
        $tcp.Dispose()
        throw
    }
}

function Close-G04DCPinnedHttpsResponse {
    [CmdletBinding()]
    param([AllowNull()] $Response)
    if ($Response) {
        if ($Response.stream) { $Response.stream.Dispose() }
        if ($Response.tcpClient) { $Response.tcpClient.Dispose() }
    }
}

function Copy-G04DCBoundedHttpsBody {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] $Response,
        [Parameter(Mandatory = $true)] [System.IO.Stream]$Output,
        [Parameter(Mandatory = $true)] [long]$ExpectedBytes
    )
    if ($Response.contentLength -ge 0 -and [long]$Response.contentLength -ne $ExpectedBytes) {
        throw '[MSI_ACQUISITION_SIZE_INVALID] Response Content-Length does not equal the frozen package size.'
    }
    $buffer = New-Object byte[] 1048576
    $totalBytes = 0L
    if (![string]::IsNullOrEmpty([string]$Response.transferEncoding)) {
            if ([string]$Response.transferEncoding -ine 'chunked') { throw '[MSI_ACQUISITION_SIZE_INVALID] Unsupported HTTPS body transfer encoding.' }
            while ($true) {
                $chunkLine = Read-G04DCHttpsLine -Stream $Response.stream
                $chunkToken = ($chunkLine -split ';', 2)[0].Trim()
                if ($chunkToken -notmatch '^[0-9A-Fa-f]+$' -or $chunkToken.Length -gt 16) { throw '[MSI_ACQUISITION_SIZE_INVALID] Invalid chunked HTTPS body framing.' }
                $chunkLength = [Convert]::ToInt64($chunkToken, 16)
                if ($chunkLength -eq 0) {
                    $trailerBytes = 0
                    while ($true) {
                        $trailer = Read-G04DCHttpsLine -Stream $Response.stream -MaximumBytes 16384
                        $trailerBytes += $trailer.Length + 2
                        if ($trailerBytes -gt 16384) { throw '[MSI_ACQUISITION_SIZE_INVALID] HTTPS trailer block exceeded the bounded ceiling.' }
                        if ($trailer.Length -eq 0) { break }
                    }
                    break
                }
                $remaining = $chunkLength
                while ($remaining -gt 0) {
                    $wanted = [int][Math]::Min([long]$buffer.Length, $remaining)
                    $read = $Response.stream.Read($buffer, 0, $wanted)
                    if ($read -le 0) { throw '[MSI_ACQUISITION_SIZE_INVALID] HTTPS body was truncated.' }
                    if ($totalBytes + $read -gt $ExpectedBytes) { throw '[MSI_ACQUISITION_SIZE_INVALID] Stream exceeded the exact byte ceiling.' }
                    $Output.Write($buffer, 0, $read)
                    $totalBytes += $read
                    $remaining -= $read
                }
                if ($Response.stream.ReadByte() -ne 13 -or $Response.stream.ReadByte() -ne 10) { throw '[MSI_ACQUISITION_SIZE_INVALID] Invalid chunk terminator.' }
            }
    }
    elseif ($Response.contentLength -ge 0) {
            $remaining = [long]$Response.contentLength
            while ($remaining -gt 0) {
                $wanted = [int][Math]::Min([long]$buffer.Length, $remaining)
                $read = $Response.stream.Read($buffer, 0, $wanted)
                if ($read -le 0) { throw '[MSI_ACQUISITION_SIZE_INVALID] HTTPS body was truncated.' }
                if ($totalBytes + $read -gt $ExpectedBytes) { throw '[MSI_ACQUISITION_SIZE_INVALID] Stream exceeded the exact byte ceiling.' }
                $Output.Write($buffer, 0, $read)
                $totalBytes += $read
                $remaining -= $read
            }
    }
    else {
            while (($read = $Response.stream.Read($buffer, 0, $buffer.Length)) -gt 0) {
                if ($totalBytes + $read -gt $ExpectedBytes) { throw '[MSI_ACQUISITION_SIZE_INVALID] Stream exceeded the exact byte ceiling.' }
                $Output.Write($buffer, 0, $read)
                $totalBytes += $read
            }
    }
    Assert-G04DCBoundedDownloadLength -ObservedBytes $totalBytes -ExpectedBytes $ExpectedBytes -StreamComplete $true | Out-Null
    if ($Output -is [System.IO.FileStream]) { ([System.IO.FileStream]$Output).Flush($true) }
    else { $Output.Flush() }
    return $totalBytes
}

function Resolve-G04DCRedirectTransition {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [uri]$CurrentUri,
        [Parameter(Mandatory = $true)] [int]$StatusCode,
        [AllowNull()] [string]$Location,
        [Parameter(Mandatory = $true)] [int]$RedirectCount,
        [AllowEmptyCollection()] [string[]]$SeenUris = @()
    )
    $expected = Get-G04DCExpectedMsi
    if ($StatusCode -ge 300 -and $StatusCode -lt 400) {
        if ([string]::IsNullOrWhiteSpace($Location)) { throw '[MSI_ACQUISITION_SOURCE_REJECTED] Redirect response omitted Location.' }
        if ($RedirectCount -ge 8) { throw '[MSI_ACQUISITION_SOURCE_REJECTED] Ninth redirect exceeds the eight-redirect ceiling.' }
        try { $nextUri = [uri]::new($CurrentUri, $Location) }
        catch { throw '[MSI_ACQUISITION_SOURCE_REJECTED] Redirect Location is not a valid URI reference.' }
        if ($SeenUris -contains $nextUri.AbsoluteUri) { throw '[MSI_ACQUISITION_SOURCE_REJECTED] Redirect loop detected.' }
        return [pscustomobject][ordered]@{ redirect = $true; nextUri = $nextUri; redirectCount = $RedirectCount + 1; final = $false }
    }
    if ($StatusCode -ne 200) { throw "[MSI_ACQUISITION_FAILED] Acquisition source returned HTTP $StatusCode." }
    $fileName = [uri]::UnescapeDataString([IO.Path]::GetFileName($CurrentUri.AbsolutePath))
    if ($fileName -cne $expected.FileName) { throw '[MSI_ACQUISITION_SOURCE_REJECTED] Final acquisition path does not identify the exact expected MSI filename.' }
    return [pscustomobject][ordered]@{ redirect = $false; nextUri = $null; redirectCount = $RedirectCount; final = $true }
}

function Assert-G04DCRedirectChainEvidence {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] $Evidence)
    $expected = Get-G04DCExpectedMsi
    $hops = @($Evidence.hops)
    if ([string]$Evidence.initialUri -cne $expected.Url -or $hops.Count -lt 1 -or $hops.Count -gt 9 -or
        [int]$Evidence.redirectCount -gt 8 -or [string]$Evidence.finalUri -cne [string]$hops[-1].resolvedEffectiveUri) {
        throw '[MSI_ACQUISITION_EVIDENCE_INVALID] Redirect-chain bounds or terminal URI evidence is invalid.'
    }
    foreach ($hop in $hops) {
        foreach ($property in @('requestedUri', 'statusCode', 'location', 'resolvedEffectiveUri', 'hostname', 'resolvedAddresses', 'connectedAddress', 'remoteEndpoint')) {
            if (!$hop.PSObject.Properties[$property]) { throw "[MSI_ACQUISITION_EVIDENCE_INVALID] Redirect hop omitted $property." }
        }
    }
    return $true
}

function Assert-G04DCBoundedDownloadLength {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [long]$ObservedBytes,
        [Parameter(Mandatory = $true)] [long]$ExpectedBytes,
        [Parameter(Mandatory = $true)] [bool]$StreamComplete
    )
    if ($ObservedBytes -gt $ExpectedBytes) { throw '[MSI_ACQUISITION_SIZE_INVALID] Stream exceeded the exact byte ceiling.' }
    if (!$StreamComplete -or $ObservedBytes -ne $ExpectedBytes) { throw '[MSI_ACQUISITION_SIZE_INVALID] Stream was truncated before the frozen package size.' }
    return $true
}

function Assert-G04DCFailedDownloadCleanup {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] $Evidence)
    if (![bool]$Evidence.markerOwned -or ![bool]$Evidence.removed -or [string]::IsNullOrWhiteSpace([string]$Evidence.exactFailedDownload)) {
        throw '[CLEANUP_OWNERSHIP_MISMATCH] Exact failed-download cleanup ownership was not proven.'
    }
    return $true
}

function Get-G04DCCanonicalHash {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] [AllowEmptyCollection()] [object[]]$Rows)
    $canonical = (($Rows | ForEach-Object { $_ | ConvertTo-Json -Compress -Depth 12 }) | Sort-Object) -join "`n"
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($canonical)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        return ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
    }
    finally {
        $sha.Dispose()
    }
}

function Get-G04DCMsiQueryRows {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] $Database,
        [Parameter(Mandatory = $true)] [string]$Sql,
        [Parameter(Mandatory = $true)] [string[]]$Columns
    )
    $view = $Database.OpenView($Sql)
    try {
        [void]$view.Execute()
        $result = [System.Collections.Generic.List[object]]::new()
        while ($record = $view.Fetch()) {
            $row = [ordered]@{}
            for ($index = 0; $index -lt $Columns.Count; $index++) {
                $row[$Columns[$index]] = [string]$record.StringData($index + 1)
            }
            $result.Add([pscustomobject]$row)
        }
        return @($result.ToArray())
    }
    finally {
        [void]$view.Close()
    }
}

function Get-G04DCMsiIdentity {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] [string]$MsiPath)

    $item = Get-Item -LiteralPath $MsiPath -Force
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $database = $installer.OpenDatabase($item.FullName, 0)
    $properties = @{}
    foreach ($name in @('ProductName', 'ProductVersion', 'ProductCode', 'UpgradeCode', 'Manufacturer')) {
        $escaped = $name.Replace("'", "''")
        $rows = @(Get-G04DCMsiQueryRows -Database $database -Sql "SELECT ``Value`` FROM ``Property`` WHERE ``Property``='$escaped'" -Columns @('Value'))
        $properties[$name] = if ($rows.Count -eq 1) { $rows[0].Value } else { $null }
    }
    $summary = $database.SummaryInformation(0)
    $template = [string]$summary.Property(7)
    $packageCode = ([string]$summary.Property(9)).ToUpperInvariant()
    $signature = Get-AuthenticodeSignature -LiteralPath $item.FullName
    $signerChain = [System.Security.Cryptography.X509Certificates.X509Chain]::new()
    $timestampChain = [System.Security.Cryptography.X509Certificates.X509Chain]::new()
    try {
        $signerChainOk = $signature.SignerCertificate -and $signerChain.Build($signature.SignerCertificate)
        $timestampChainOk = $signature.TimeStamperCertificate -and $timestampChain.Build($signature.TimeStamperCertificate)
        $signerChainElements = @($signerChain.ChainElements | ForEach-Object {
            [pscustomobject][ordered]@{
                subject = $_.Certificate.Subject
                issuer = $_.Certificate.Issuer
                thumbprint = $_.Certificate.Thumbprint.ToUpperInvariant()
                notBeforeUtc = $_.Certificate.NotBefore.ToUniversalTime().ToString('o')
                notAfterUtc = $_.Certificate.NotAfter.ToUniversalTime().ToString('o')
            }
        })
        $timestampChainElements = @($timestampChain.ChainElements | ForEach-Object {
            [pscustomobject][ordered]@{
                subject = $_.Certificate.Subject
                issuer = $_.Certificate.Issuer
                thumbprint = $_.Certificate.Thumbprint.ToUpperInvariant()
                notBeforeUtc = $_.Certificate.NotBefore.ToUniversalTime().ToString('o')
                notAfterUtc = $_.Certificate.NotAfter.ToUniversalTime().ToString('o')
            }
        })
        return [pscustomobject][ordered]@{
            path = $item.FullName
            regularFile = !$item.PSIsContainer
            reparsePoint = [bool]($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint)
            sizeBytes = [long]$item.Length
            sha256 = Get-G04DCSha256 -Path $item.FullName
            authenticodeStatus = [string]$signature.Status
            signerSubject = if ($signature.SignerCertificate) { $signature.SignerCertificate.Subject } else { $null }
            signerThumbprint = if ($signature.SignerCertificate) { $signature.SignerCertificate.Thumbprint.ToUpperInvariant() } else { $null }
            signerChainValid = [bool]$signerChainOk
            signerChainStatus = @($signerChain.ChainStatus | ForEach-Object { [string]$_.Status })
            signerChain = $signerChainElements
            timestampSignerSubject = if ($signature.TimeStamperCertificate) { $signature.TimeStamperCertificate.Subject } else { $null }
            timestampSignerThumbprint = if ($signature.TimeStamperCertificate) { $signature.TimeStamperCertificate.Thumbprint.ToUpperInvariant() } else { $null }
            timestampChainValid = [bool]$timestampChainOk
            timestampChainStatus = @($timestampChain.ChainStatus | ForEach-Object { [string]$_.Status })
            timestampChain = $timestampChainElements
            productName = $properties.ProductName
            productVersion = $properties.ProductVersion
            productCode = ([string]$properties.ProductCode).ToUpperInvariant()
            upgradeCode = ([string]$properties.UpgradeCode).ToUpperInvariant()
            packageCode = $packageCode
            manufacturer = $properties.Manufacturer
            summaryTemplate = $template
            architecture = if ($template -match '(^|;)x64($|;)') { 'x64' } else { $template }
        }
    }
    finally {
        $signerChain.Dispose()
        $timestampChain.Dispose()
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($summary)
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($database)
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer)
    }
}

function Assert-G04DCMsiIdentity {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] $Identity)
    $expected = Get-G04DCExpectedMsi
    $checks = [ordered]@{
        regularFile = [bool]$Identity.regularFile
        nonReparse = ![bool]$Identity.reparsePoint
        sizeBytes = [long]$Identity.sizeBytes -eq [long]$expected.SizeBytes
        sha256 = [string]$Identity.sha256 -ceq [string]$expected.Sha256
        authenticode = [string]$Identity.authenticodeStatus -ceq 'Valid'
        signer = [string]$Identity.signerSubject -match [regex]::Escape($expected.Signer)
        signerThumbprint = [string]$Identity.signerThumbprint -ceq $expected.SignerThumbprint
        signerChain = [bool]$Identity.signerChainValid -and $Identity.PSObject.Properties['signerChain'] -and @($Identity.signerChain).Count -ge 2
        timestampThumbprint = [string]$Identity.timestampSignerThumbprint -ceq $expected.TimestampSignerThumbprint
        timestampChain = [bool]$Identity.timestampChainValid -and $Identity.PSObject.Properties['timestampChain'] -and @($Identity.timestampChain).Count -ge 2
        version = [string]$Identity.productVersion -ceq $expected.ProductVersion
        architecture = [string]$Identity.architecture -ceq $expected.Architecture
        productCode = [string]$Identity.productCode -ceq $expected.ProductCode
        upgradeCode = [string]$Identity.upgradeCode -ceq $expected.UpgradeCode
        packageCode = [string]$Identity.packageCode -ceq $expected.PackageCode
    }
    $failed = @($checks.GetEnumerator() | Where-Object { !$_.Value } | ForEach-Object { $_.Key })
    if ($failed.Count -ne 0) {
        throw "[MSI_IDENTITY_MISMATCH] LibreOffice MSI identity failed: $($failed -join ', ')"
    }
    return [pscustomobject]$checks
}

function Invoke-G04DCAcquireMsi {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [string]$Destination,
        [Parameter(Mandatory = $true)] [string]$EvidenceDirectory
    )
    $expected = Get-G04DCExpectedMsi
    $destinationCanonical = [IO.Path]::GetFullPath($Destination)
    if (Test-Path -LiteralPath $destinationCanonical) {
        throw "[CLEANUP_OWNERSHIP_MISMATCH] Refusing to overwrite MSI path: $destinationCanonical"
    }
    $parent = Split-Path -Parent $destinationCanonical
    if (!(Test-Path -LiteralPath $parent)) { New-Item -ItemType Directory -Path $parent | Out-Null }
    $parentItem = Get-Item -LiteralPath $parent -Force
    if (!$parentItem.PSIsContainer -or [bool]($parentItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw '[CLEANUP_OWNERSHIP_MISMATCH] Download parent is not a canonical non-reparse directory.'
    }
    $redirectChain = [System.Collections.Generic.List[object]]::new()
    $seenUris = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $currentUri = [uri]$expected.Url
    $response = $null
    $redirectCount = 0
    $downloadCreated = $false
    $cleanupEvidence = $null
    $finalUri = $null
    try {
        while ($true) {
            if (!$seenUris.Add($currentUri.AbsoluteUri)) { throw '[MSI_ACQUISITION_SOURCE_REJECTED] Redirect loop detected.' }
            $uriEvidence = Assert-G04DCAcquisitionUri -Uri $currentUri -CanonicalFirstRequest:($seenUris.Count -eq 1)
            $response = Invoke-G04DCPinnedHttpsRequest -Uri $currentUri -ApprovedAddresses @($uriEvidence.resolvedAddresses)
            $statusCode = [int]$response.statusCode
            $location = [string]$response.location
            $resolvedEffectiveUri = if ($statusCode -ge 300 -and $statusCode -lt 400 -and ![string]::IsNullOrWhiteSpace($location)) {
                try { ([uri]::new($currentUri, $location)).AbsoluteUri } catch { $null }
            }
            else { $currentUri.AbsoluteUri }
            $redirectChain.Add([pscustomobject][ordered]@{
                requestedUri = $currentUri.AbsoluteUri
                statusCode = $statusCode
                location = if ([string]::IsNullOrEmpty($location)) { $null } else { $location }
                resolvedEffectiveUri = $resolvedEffectiveUri
                hostname = $uriEvidence.hostname
                resolvedAddresses = @($uriEvidence.resolvedAddresses)
                connectedAddress = $response.connectedAddress
                remoteEndpoint = $response.remoteEndpoint
            })
            $transition = Resolve-G04DCRedirectTransition -CurrentUri $currentUri -StatusCode $statusCode -Location $location -RedirectCount $redirectCount -SeenUris @($seenUris)
            if ($transition.redirect) {
                $redirectCount = [int]$transition.redirectCount
                $currentUri = $transition.nextUri
                Close-G04DCPinnedHttpsResponse -Response $response
                $response = $null
                continue
            }
            $finalUri = $currentUri.AbsoluteUri
            $output = [IO.File]::Open($destinationCanonical, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
            $downloadCreated = $true
            try { Copy-G04DCBoundedHttpsBody -Response $response -Output $output -ExpectedBytes ([long]$expected.SizeBytes) | Out-Null }
            finally { $output.Dispose() }
            break
        }
        $sourceEvidence = [pscustomobject][ordered]@{
            schemaVersion = 2
            initialUri = $expected.Url
            maximumRedirects = 8
            redirectCount = $redirectCount
            hops = @($redirectChain.ToArray())
            finalUri = $finalUri
            mirrorHostnameIsTrustAnchor = $false
            accepted = $true
            failedDownloadCleanup = $null
        }
        Assert-G04DCRedirectChainEvidence -Evidence $sourceEvidence | Out-Null
        Write-G04DCJson -Path (Join-Path $EvidenceDirectory 'msi-acquisition-source.json') -Value $sourceEvidence

        $item = Get-Item -LiteralPath $destinationCanonical -Force
        $fileEnvelope = [pscustomobject][ordered]@{
            path = $item.FullName
            regularFile = !$item.PSIsContainer
            reparsePoint = [bool]($item.Attributes -band [IO.FileAttributes]::ReparsePoint)
            sizeBytes = [long]$item.Length
            sha256 = Get-G04DCSha256 -Path $item.FullName
        }
        Write-G04DCJson -Path (Join-Path $EvidenceDirectory 'msi-file-envelope.json') -Value $fileEnvelope
        if (!$fileEnvelope.regularFile -or $fileEnvelope.reparsePoint -or $fileEnvelope.sizeBytes -ne [long]$expected.SizeBytes -or $fileEnvelope.sha256 -cne $expected.Sha256) {
            throw '[MSI_IDENTITY_MISMATCH] Downloaded bytes failed the frozen regular-file, size, or SHA-256 envelope before MSI database inspection.'
        }
        $identity = Get-G04DCMsiIdentity -MsiPath $destinationCanonical
        Write-G04DCJson -Path (Join-Path $EvidenceDirectory 'msi-identity-observed.json') -Value ([ordered]@{
            expected = $expected
            observed = $identity
            acquiredAtUtc = [DateTime]::UtcNow.ToString('o')
        })
        $checks = Assert-G04DCMsiIdentity -Identity $identity
        Write-G04DCJson -Path (Join-Path $EvidenceDirectory 'msi-identity.json') -Value ([ordered]@{
            expected = $expected
            observed = $identity
            checks = $checks
            acquiredAtUtc = [DateTime]::UtcNow.ToString('o')
        })
        return $identity
    }
    catch {
        $original = $_
        if ($downloadCreated -and (Test-Path -LiteralPath $destinationCanonical)) {
            $item = Get-Item -LiteralPath $destinationCanonical -Force -ErrorAction SilentlyContinue
            $ownedRoot = Split-Path -Parent $parent
            $ownedMarker = Join-Path $ownedRoot '.g04d-c-owned-root'
            $markerItem = Get-Item -LiteralPath $ownedMarker -Force -ErrorAction SilentlyContinue
            $markerText = if ($markerItem -and !$markerItem.PSIsContainer -and ![bool]($markerItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { [IO.File]::ReadAllText($markerItem.FullName, [Text.Encoding]::UTF8) } else { $null }
            $owned = $item -and !$item.PSIsContainer -and ![bool]($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -and
                $item.FullName -ceq $destinationCanonical -and $markerText -clike "DOCUMENT-STUDIO-G04D-C-*-OWNED`n"
            if ($owned) { Remove-Item -LiteralPath $destinationCanonical -Force -ErrorAction Stop }
            $cleanupEvidence = [pscustomobject][ordered]@{
                exactFailedDownload = $destinationCanonical
                markerPath = $ownedMarker
                markerOwned = [bool]$owned
                removed = !(Test-Path -LiteralPath $destinationCanonical)
            }
        }
        $failureEvidence = [pscustomobject][ordered]@{
            schemaVersion = 2
            initialUri = $expected.Url
            maximumRedirects = 8
            redirectCount = $redirectCount
            hops = @($redirectChain.ToArray())
            finalUri = $finalUri
            mirrorHostnameIsTrustAnchor = $false
            accepted = $false
            failure = $original.Exception.Message
            failedDownloadCleanup = $cleanupEvidence
        }
        Write-G04DCJson -Path (Join-Path $EvidenceDirectory 'msi-acquisition-source.json') -Value $failureEvidence
        if ($downloadCreated -and (!$cleanupEvidence -or !$cleanupEvidence.markerOwned -or !$cleanupEvidence.removed)) {
            throw "[CLEANUP_OWNERSHIP_MISMATCH] Exact failed download cleanup did not complete. Original failure: $($original.Exception.Message)"
        }
        if ($downloadCreated) { Assert-G04DCFailedDownloadCleanup -Evidence $cleanupEvidence | Out-Null }
        throw $original
    }
    finally { if ($response) { Close-G04DCPinnedHttpsResponse -Response $response } }
}

function Export-G04DCMsiDatabase {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [string]$MsiPath,
        [Parameter(Mandatory = $true)] [string]$OutputDirectory
    )
    if (Test-Path -LiteralPath $OutputDirectory) {
        throw "[CLEANUP_OWNERSHIP_MISMATCH] Refusing to overwrite MSI table directory: $OutputDirectory"
    }
    New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $database = $installer.OpenDatabase((Resolve-Path -LiteralPath $MsiPath).Path, 0)
    try {
        $available = @(Get-G04DCMsiQueryRows -Database $database -Sql 'SELECT `Name` FROM `_Tables`' -Columns @('Name') | ForEach-Object { $_.Name })
        $manifest = [System.Collections.Generic.List[object]]::new()
        $exports = @{}
        foreach ($table in $script:G04DCTables) {
            if ($available -notcontains $table) {
                $manifest.Add([pscustomobject][ordered]@{ table = $table; present = $false; rowCount = 0; path = $null; sha256 = $null })
                continue
            }
            $escaped = $table.Replace("'", "''")
            $columnRows = @(Get-G04DCMsiQueryRows -Database $database -Sql "SELECT ``Name``,``Number`` FROM ``_Columns`` WHERE ``Table``='$escaped'" -Columns @('Name', 'Number') | Sort-Object { [int]$_.Number })
            $columns = @($columnRows | ForEach-Object { $_.Name })
            $quoted = @($columns | ForEach-Object { "``$_``" }) -join ','
            $rows = @(Get-G04DCMsiQueryRows -Database $database -Sql "SELECT $quoted FROM ``$table``" -Columns $columns)
            $rows = @($rows | Sort-Object { $_ | ConvertTo-Json -Compress -Depth 8 })
            $fileName = $table + '.json'
            $path = Join-Path $OutputDirectory $fileName
            Write-G04DCJson -Path $path -Value ([ordered]@{ table = $table; columns = $columns; rows = $rows })
            $manifest.Add([pscustomobject][ordered]@{
                table = $table
                present = $true
                rowCount = $rows.Count
                path = $fileName
                sha256 = Get-G04DCSha256 -Path $path
            })
            $exports[$table] = $rows
        }
        $manifestPath = Join-Path $OutputDirectory 'table-manifest.json'
        Write-G04DCJson -Path $manifestPath -Value ([ordered]@{
            schemaVersion = 1
            msiSha256 = Get-G04DCSha256 -Path $MsiPath
            tables = @($manifest.ToArray())
        })
        $analysis = Get-G04DCFeatureAnalysis -Tables $exports
        Write-G04DCJson -Path (Join-Path $OutputDirectory 'feature-analysis.json') -Value $analysis
        return $analysis
    }
    finally {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($database)
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer)
    }
}

function Get-G04DCFeatureAnalysis {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] [hashtable]$Tables)
    foreach ($requiredTable in @('Feature', 'FeatureComponents', 'Component', 'File', 'Registry', 'Font', 'Directory')) {
        if (!$Tables.ContainsKey($requiredTable)) {
            throw "[AMBIGUOUS_FEATURE_OWNERSHIP] Required MSI table is absent: $requiredTable"
        }
    }
    $features = @($Tables.Feature)
    $featureNames = @($features | ForEach-Object { $_.Feature })
    $requiredLeaves = @(
        'gm_Root', 'gm_Prg',
        'gm_p_Wrt', 'gm_p_Wrt_Bin', 'gm_Brand_p_Wrt',
        'gm_p_Calc', 'gm_p_Calc_Bin', 'gm_Brand_p_Calc',
        'gm_p_Impress', 'gm_p_Impress_Bin', 'gm_Brand_p_Impress',
        'gm_r_Brand', 'gm_r_Files_Images', 'gm_r_Ure_Hidden', 'gm_Oo_Linguistic',
        'gm_Pdfimport', 'gm_Langpack_Languageroot', 'gm_Langpack_r_en_US',
        'gm_Langpack_Basis_en_US', 'gm_Langpack_Writer_en_US',
        'gm_Langpack_Calc_en_US', 'gm_Langpack_Impress_en_US', 'gm_Langpack_Brand_en_US'
    )
    $missing = @($requiredLeaves | Where-Object { $featureNames -notcontains $_ })
    if ($missing.Count -ne 0) {
        throw "[AMBIGUOUS_FEATURE_OWNERSHIP] Candidate runtime feature is absent: $($missing -join ', ')"
    }
    $parentByFeature = @{}
    foreach ($feature in $features) { $parentByFeature[$feature.Feature] = $feature.Feature_Parent }
    $selected = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($leaf in $requiredLeaves) {
        $cursor = $leaf
        while ($cursor) {
            [void]$selected.Add($cursor)
            $cursor = [string]$parentByFeature[$cursor]
        }
    }
    $protectedFeaturePatterns = @(
        '^gm_r_Fonts_OOo_Hidden$', '^gm_o_Systemintegration$', '^gm_o_Onlineupdate$',
        '^gm_o_Winexplorerext$', '^gm_o_Quickstart$', '^gm_o_Extensions',
        '^gm_Script_Provider_For_Python$', '^gm_Pyuno$', '^gm_o_Firebird$',
        '^gm_p_Base', '^gm_Reportbuilder$', '^gm_Dictionaries$', '^gm_r_ex_Dictionary_',
        '^gm_Helppack_', '^gm_o_Xsltfiltersamples$', '^gm_o_Pyuno_LibreLogo$'
    )
    $excluded = @($featureNames | Where-Object {
        $name = $_
        @($protectedFeaturePatterns | Where-Object { $name -match $_ }).Count -ne 0
    } | Sort-Object -Unique)
    $selectedArray = @($selected | Sort-Object)
    $featureComponents = @($Tables.FeatureComponents)
    $selectedComponents = @($featureComponents | Where-Object { $selected.Contains([string]$_.Feature_) } | ForEach-Object { $_.Component_ } | Sort-Object -Unique)
    $excludedComponents = @($featureComponents | Where-Object { $excluded -contains $_.Feature_ } | ForEach-Object { $_.Component_ } | Sort-Object -Unique)
    $ambiguousComponents = @(Compare-Object -ReferenceObject $selectedComponents -DifferenceObject $excludedComponents -IncludeEqual -ExcludeDifferent | ForEach-Object { $_.InputObject })

    $fileByKey = @{}
    foreach ($file in @($Tables.File)) { $fileByKey[[string]$file.File] = $file }
    $fontComponents = @($Tables.Font | ForEach-Object {
        $file = $fileByKey[[string]$_.File_]
        if ($file) { $file.Component_ }
    } | Sort-Object -Unique)
    $fontFiles = @($Tables.Font | ForEach-Object {
        $file = $fileByKey[[string]$_.File_]
        if ($file) {
            [pscustomobject][ordered]@{
                fileKey = $file.File
                component = $file.Component_
                installedFileName = ([string]$file.FileName -split '\|')[-1]
                fontTitle = $_.FontTitle
            }
        }
    })
    $selectedFontComponents = @($fontComponents | Where-Object { $selectedComponents -contains $_ })
    $registryRows = @($Tables.Registry)
    $removeRegistryRows = if ($Tables.ContainsKey('RemoveRegistry')) { @($Tables.RemoveRegistry) } else { @() }
    $allRegistryMutationRows = @($registryRows) + @($removeRegistryRows)
    $selectedRegistry = @($registryRows | Where-Object { $selectedComponents -contains $_.Component_ })
    $protectedRegistry = @($selectedRegistry | Where-Object {
        ([string]$_.Key -match '(?i)App Paths|\\Classes\\\.(odt|ods|odp|docx|xlsx|pptx|pdf)|\\Shell Extensions') -or
        ([string]$_.Name -match '(?i)odt|ods|odp|soffice') -or
        ([string]$_.Value -match '(?i)soffice|LibreOfficeMaintenance')
    })
    $maintenanceFiles = @($Tables.File | Where-Object { [string]$_.FileName -match '(?i)maintenance|update' })
    $selectedMaintenance = @($maintenanceFiles | Where-Object { $selectedComponents -contains $_.Component_ })
    $componentFeatureOwners = @{}
    foreach ($mapping in $featureComponents) {
        $component = [string]$mapping.Component_
        if (!$componentFeatureOwners.ContainsKey($component)) { $componentFeatureOwners[$component] = [System.Collections.Generic.List[string]]::new() }
        $componentFeatureOwners[$component].Add([string]$mapping.Feature_)
    }
    $directoryById = @{}
    foreach ($directory in @($Tables.Directory)) { $directoryById[[string]$directory.Directory] = $directory }
    function Get-G04DCDirectoryTargetSegment([string]$DefaultDir) {
        if ([string]::IsNullOrWhiteSpace($DefaultDir)) { return '' }
        $target = ($DefaultDir -split ':', 2)[0]
        $target = ($target -split '\|')[-1]
        if ($target -ceq '.') { return '' }
        return $target
    }
    function Resolve-G04DCDirectoryRelativePath([string]$DirectoryId) {
        $segments = [System.Collections.Generic.List[string]]::new()
        $visited = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
        $cursor = $DirectoryId
        while ($cursor -and $cursor -cne 'INSTALLLOCATION') {
            if (!$visited.Add($cursor) -or !$directoryById.ContainsKey($cursor)) { return $null }
            $row = $directoryById[$cursor]
            $segment = Get-G04DCDirectoryTargetSegment -DefaultDir ([string]$row.DefaultDir)
            if ($segment) { $segments.Insert(0, $segment) }
            $cursor = [string]$row.Directory_Parent
        }
        if ($cursor -cne 'INSTALLLOCATION') { return $null }
        return ($segments -join '/')
    }
    $componentInstallOwnership = @($Tables.Component | ForEach-Object {
        $owners = if ($componentFeatureOwners.ContainsKey([string]$_.Component)) { @($componentFeatureOwners[[string]$_.Component].ToArray() | Sort-Object -Unique) } else { @() }
        $relativeDirectory = Resolve-G04DCDirectoryRelativePath -DirectoryId ([string]$_.Directory_)
        [pscustomobject][ordered]@{
            component = $_.Component
            componentId = $_.ComponentId
            directory = $_.Directory_
            condition = $_.Condition
            relativeDirectory = $relativeDirectory
            underInstallLocation = $null -ne $relativeDirectory
            featureOwners = $owners
            selectedOwners = @($owners | Where-Object { $selected.Contains($_) })
            excludedOwners = @($owners | Where-Object { $excluded -contains $_ })
        }
    })
    $vcRedistPropertyRows = @($Tables.Property | Where-Object { $_.Property -ceq 'VC_REDIST' })
    $vcRedistDefault = if ($vcRedistPropertyRows.Count -eq 1) { [string]$vcRedistPropertyRows[0].Value } else { $null }
    $desktopLinkDefault = [string](@($Tables.Property | Where-Object { $_.Property -ceq 'CREATEDESKTOPLINK' } | Select-Object -First 1).Value)
    $writeRegistryDefault = [string](@($Tables.Property | Where-Object { $_.Property -ceq 'WRITE_REGISTRY' } | Select-Object -First 1).Value)
    $selectedExternalComponents = @($componentInstallOwnership | Where-Object { @($_.selectedOwners).Count -ne 0 -and ![bool]$_.underInstallLocation })
    $vcRedistSuppressionSafe = $selectedExternalComponents.Count -ne 0 -and $vcRedistDefault -ceq '1' -and
        @($selectedExternalComponents | Where-Object {
            $normalizedCondition = ([string]$_.condition).Replace(' ', '')
            $normalizedCondition -notmatch '(^|\()VC_REDIST=1(\)|$)' -or $normalizedCondition -match '(?i)OR' -or
            [string]$_.directory -notmatch '^System(64)?Folder(\.|$)'
        }).Count -eq 0
    foreach ($componentOwnership in $componentInstallOwnership) {
        $disabledByCandidateProperty = @($componentOwnership.selectedOwners).Count -ne 0 -and ![bool]$componentOwnership.underInstallLocation -and $vcRedistSuppressionSafe
        $componentOwnership | Add-Member -NotePropertyName disabledByCandidateProperty -NotePropertyValue $disabledByCandidateProperty
        $componentOwnership | Add-Member -NotePropertyName expectedInstallState -NotePropertyValue $(if (@($componentOwnership.selectedOwners).Count -ne 0 -and !$disabledByCandidateProperty) { 3 } else { 2 })
    }
    $componentInstallByKey = @{}
    foreach ($row in $componentInstallOwnership) { $componentInstallByKey[[string]$row.component] = $row }
    $fileComponentOwnership = @($Tables.File | ForEach-Object {
        $owners = if ($componentFeatureOwners.ContainsKey([string]$_.Component_)) { @($componentFeatureOwners[[string]$_.Component_].ToArray() | Sort-Object -Unique) } else { @() }
        $longName = ([string]$_.FileName -split '\|')[-1]
        $componentInstall = $componentInstallByKey[[string]$_.Component_]
        $relativePath = if ($componentInstall -and $componentInstall.underInstallLocation) {
            (@([string]$componentInstall.relativeDirectory, $longName) | Where-Object { $_ }) -join '/'
        } else { $null }
        [pscustomobject][ordered]@{
            fileKey = $_.File
            msiFileName = $_.FileName
            installedFileName = $longName
            component = $_.Component_
            targetRelativePath = $relativePath
            underInstallLocation = [bool]($componentInstall -and $componentInstall.underInstallLocation)
            disabledByCandidateProperty = [bool]($componentInstall -and $componentInstall.disabledByCandidateProperty)
            componentDirectory = if ($componentInstall) { $componentInstall.directory } else { $null }
            featureOwners = $owners
            selectedOwners = @($owners | Where-Object { $selected.Contains($_) })
            excludedOwners = @($owners | Where-Object { $excluded -contains $_ })
        }
    })
    $customActions = if ($Tables.ContainsKey('CustomAction')) { @($Tables.CustomAction) } else { @() }
    $mutationTables = @('Registry', 'RemoveRegistry', 'ServiceInstall', 'ServiceControl', 'Font', 'Extension', 'ProgId', 'MIME', 'Verb', 'Class', 'AppId', 'Shortcut', 'Environment')
    $mutationTableOwnership = @($mutationTables | ForEach-Object {
        $tableName = $_
        $rows = if ($Tables.ContainsKey($tableName)) { @($Tables[$tableName]) } else { @() }
        foreach ($row in $rows) {
            $componentKey = if ($row.PSObject.Properties['Component_']) {
                [string]$row.Component_
            }
            elseif ($tableName -ceq 'Font' -and $row.PSObject.Properties['File_'] -and $fileByKey.ContainsKey([string]$row.File_)) {
                [string]$fileByKey[[string]$row.File_].Component_
            }
            else { $null }
            $owners = if ($componentKey -and $componentFeatureOwners.ContainsKey($componentKey)) { @($componentFeatureOwners[$componentKey].ToArray() | Sort-Object -Unique) } else { @() }
            [pscustomobject][ordered]@{
                table = $tableName
                component = $componentKey
                directComponentOwnership = ![string]::IsNullOrWhiteSpace($componentKey)
                featureOwners = $owners
                selectedOwners = @($owners | Where-Object { $selected.Contains($_) })
                row = $row
            }
        }
    })
    $sequenceTables = @('InstallExecuteSequence', 'InstallUISequence', 'AdminExecuteSequence', 'AdminUISequence')
    $sequencedCustomActions = @($sequenceTables | ForEach-Object {
        $sequenceTable = $_
        if ($Tables.ContainsKey($sequenceTable)) {
            foreach ($sequenceRow in @($Tables[$sequenceTable])) {
                $custom = @($customActions | Where-Object { [string]$_.Action -ceq [string]$sequenceRow.Action })
                foreach ($customRow in $custom) {
                    $type = [int]$customRow.Type
                    [pscustomobject][ordered]@{
                        sequenceTable = $sequenceTable
                        action = $customRow.Action
                        condition = $sequenceRow.Condition
                        sequence = $sequenceRow.Sequence
                        type = $type
                        source = $customRow.Source
                        target = $customRow.Target
                        propertyOnly = $type -eq 51
                    }
                }
            }
        }
    })
    $unboundedInstallCustomActions = @($sequencedCustomActions | Where-Object { $_.sequenceTable -in @('InstallExecuteSequence', 'InstallUISequence') -and ![bool]$_.propertyOnly })
    $unboundedAdminCustomActions = @($sequencedCustomActions | Where-Object { $_.sequenceTable -in @('AdminExecuteSequence', 'AdminUISequence') -and ![bool]$_.propertyOnly })
    $maintenanceActions = @($customActions | Where-Object {
        ([string]$_.Action -match '(?i)maint|update') -or ([string]$_.Target -match '(?i)maint|update')
    })
    function Get-CategoryOwnership([string]$FeaturePattern, [string]$FilePattern, [string]$RegistryPattern) {
        $categoryFeatures = @($features | Where-Object { ([string]$_.Feature + '|' + [string]$_.Title) -match $FeaturePattern } | ForEach-Object { $_.Feature } | Sort-Object -Unique)
        $categoryComponents = @($featureComponents | Where-Object { $categoryFeatures -contains $_.Feature_ } | ForEach-Object { $_.Component_ } | Sort-Object -Unique)
        $matchingFiles = if ($FilePattern) { @($Tables.File | Where-Object { ([string]$_.File + '|' + [string]$_.FileName + '|' + [string]$_.Component_) -match $FilePattern }) } else { @() }
        $matchingRegistry = if ($RegistryPattern) { @($registryRows | Where-Object { ([string]$_.Key + '|' + [string]$_.Name + '|' + [string]$_.Value + '|' + [string]$_.Component_) -match $RegistryPattern }) } else { @() }
        return [pscustomobject][ordered]@{
            features = $categoryFeatures
            components = $categoryComponents
            files = $matchingFiles
            registryRows = $matchingRegistry
            selectedFeatures = @($categoryFeatures | Where-Object { $selected.Contains($_) })
            selectedComponents = @($categoryComponents | Where-Object { $selectedComponents -contains $_ })
        }
    }
    $ownershipByCategory = [pscustomobject][ordered]@{
        systemFonts = [pscustomobject][ordered]@{
            features = @($featureComponents | Where-Object { $fontComponents -contains $_.Component_ } | ForEach-Object { $_.Feature_ } | Sort-Object -Unique)
            components = $fontComponents
            selectedComponents = $selectedFontComponents
            fontRows = @($Tables.Font)
            fontFiles = $fontFiles
        }
        odfAssociations = Get-CategoryOwnership '(?i)Reg_(Odt|Ods|Odp)|Wrt|Calc|Impress' '' '(?i)(^|\\|\.)odt|ods|odp|OpenDocument'
        maintenanceService = [pscustomobject][ordered]@{
            serviceInstallRows = if ($Tables.ContainsKey('ServiceInstall')) { @($Tables.ServiceInstall) } else { @() }
            serviceControlRows = if ($Tables.ContainsKey('ServiceControl')) { @($Tables.ServiceControl) } else { @() }
            customActions = $maintenanceActions
            files = @($Tables.File | Where-Object { ([string]$_.FileName + '|' + [string]$_.File) -match '(?i)update_service|maintenance' })
        }
        updater = Get-CategoryOwnership '(?i)Onlineupdate|update' '(?i)updater|update_service|update-settings' '(?i)update'
        appPaths = Get-CategoryOwnership '(?i)Root|Wrt|Calc|Impress' '' '(?i)App Paths'
        shellIntegration = Get-CategoryOwnership '(?i)Systemintegration|Winexplorerext' '(?i)shlxthdl|explorer' '(?i)Shell Extensions|\\shell\\|CLSID'
        python = Get-CategoryOwnership '(?i)Python|Pyuno' '(?i)python|pyuno' '(?i)python|pyuno'
        java = Get-CategoryOwnership '(?i)Java|BeanShell|Script_Provider_For_JS' '(?i)java|jvm|beanshell' '(?i)java|jvm|beanshell'
        firebird = Get-CategoryOwnership '(?i)Firebird|Base' '(?i)firebird|sdbc' '(?i)firebird|sdbc'
        sharedExtensions = Get-CategoryOwnership '(?i)Extensions|MEDIAWIKI|NLPSolver|Reportbuilder' '(?i)extension|mediawiki|nlpsolver|reportbuilder' '(?i)extension|mediawiki|nlpsolver|reportbuilder'
        dictionaries = Get-CategoryOwnership '(?i)Dictionaries|Dictionary_' '(?i)dictionary|dict_' '(?i)dictionary'
        writerCalcImpressCore = [pscustomobject][ordered]@{
            features = @($selectedArray | Where-Object { $_ -match '^gm_(p|Brand_p)_(Wrt|Calc|Impress)' })
            components = @($featureComponents | Where-Object { $selected.Contains([string]$_.Feature_) -and $_.Feature_ -match '^gm_(p|Brand_p)_(Wrt|Calc|Impress)' } | ForEach-Object { $_.Component_ } | Sort-Object -Unique)
        }
    }

    $ambiguities = [System.Collections.Generic.List[string]]::new()
    if ($ambiguousComponents.Count -ne 0) { $ambiguities.Add('selected components overlap protected-feature components') }
    if ($selectedFontComponents.Count -ne 0) { $ambiguities.Add('selected feature closure contains MSI Font-table components') }
    if ($selectedExternalComponents.Count -ne 0 -and !$vcRedistSuppressionSafe) { $ambiguities.Add('selected out-of-INSTALLLOCATION components cannot be safely disabled by the exact VC_REDIST table condition') }
    return [pscustomobject][ordered]@{
        schemaVersion = 1
        featureTree = @($features | Sort-Object Feature | ForEach-Object {
            [pscustomobject][ordered]@{
                feature = $_.Feature
                parent = $_.Feature_Parent
                title = $_.Title
                level = $_.Level
                attributes = $_.Attributes
            }
        })
        componentToFeature = @($featureComponents | Sort-Object Component_, Feature_)
        componentInstallOwnership = $componentInstallOwnership
        fileComponentOwnership = $fileComponentOwnership
        ownershipByCategory = $ownershipByCategory
        candidateMinimumFeatureSet = $selectedArray
        candidatePublicProperties = [pscustomobject][ordered]@{
            VC_REDIST = if ($vcRedistSuppressionSafe) { '0' } else { $null }
            CREATEDESKTOPLINK = if ($desktopLinkDefault -ceq '1') { '0' } else { $null }
            WRITE_REGISTRY = if ($writeRegistryDefault -ceq '1') { '0' } else { $null }
            msiDefaultVC_REDIST = $vcRedistDefault
            msiDefaultCREATEDESKTOPLINK = $desktopLinkDefault
            msiDefaultWRITE_REGISTRY = $writeRegistryDefault
            selectedExternalComponentCount = $selectedExternalComponents.Count
            suppressionCondition = 'VC_REDIST=1'
            suppressionSafe = $vcRedistSuppressionSafe
        }
        explicitlyExcludedFeatures = $excluded
        selectedComponentCount = $selectedComponents.Count
        protectedOwnership = [pscustomobject][ordered]@{
            allRegistryRows = $allRegistryMutationRows
            fontComponents = $fontComponents
            selectedFontComponents = $selectedFontComponents
            overlappingProtectedComponents = $ambiguousComponents
            selectedProtectedRegistryRows = $protectedRegistry
            selectedMaintenanceFiles = $selectedMaintenance
            maintenanceCustomActions = $maintenanceActions
            maintenanceCustomActionDynamicProofRequired = ($maintenanceActions.Count -ne 0 -and !$Tables.ContainsKey('ServiceInstall'))
            mutationTableOwnership = $mutationTableOwnership
            sequencedCustomActions = $sequencedCustomActions
            unboundedInstallCustomActions = $unboundedInstallCustomActions
            unboundedAdminCustomActions = $unboundedAdminCustomActions
        }
        mandatoryProtectedMutationEvidence = [pscustomobject][ordered]@{
            selectedProtectedRegistryRowCount = $protectedRegistry.Count
            selectedMaintenanceNamedFileCount = $selectedMaintenance.Count
            dynamicInstallProofRequired = $true
        }
        unavailableTables = @($script:G04DCTables | Where-Object { !$Tables.ContainsKey($_) })
        ambiguous = $ambiguities.Count -ne 0
        ambiguityReasons = @($ambiguities.ToArray())
    }
}

function Assert-G04DCFeatureAnalysis {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] $Analysis)
    if ([bool]$Analysis.ambiguous) {
        throw "[AMBIGUOUS_FEATURE_OWNERSHIP] $($Analysis.ambiguityReasons -join '; ')"
    }
    if (@($Analysis.candidateMinimumFeatureSet).Count -eq 0) {
        throw '[AMBIGUOUS_FEATURE_OWNERSHIP] Candidate minimum feature set is empty.'
    }
    return $true
}

function Get-G04DCInstalledFeatureStates {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [string]$ProductCode,
        [Parameter(Mandatory = $true)] [string[]]$FeatureNames
    )
    $installer = New-Object -ComObject WindowsInstaller.Installer
    try {
        return @($FeatureNames | Sort-Object -Unique | ForEach-Object {
            [pscustomobject][ordered]@{ feature = $_; state = [int]$installer.FeatureState($ProductCode, $_) }
        })
    }
    finally {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer)
    }
}

function Assert-G04DCInstalledFeatureStates {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [object[]]$States,
        [Parameter(Mandatory = $true)] [string[]]$SelectedFeatures
    )
    $selected = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($feature in $SelectedFeatures) { [void]$selected.Add($feature) }
    $invalid = @($States | Where-Object {
        if ($selected.Contains([string]$_.feature)) { return [int]$_.state -ne 3 }
        return [int]$_.state -ne 2
    })
    if ($invalid.Count -ne 0) {
        throw "[MINIMAL_FEATURE_STATE_INVALID] $($invalid.Count) installed feature states differ from the exact selected-local/unselected-absent model."
    }
    return $true
}

function Get-G04DCInstalledComponentStates {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [string]$ProductCode,
        [Parameter(Mandatory = $true)] [string[]]$ComponentCodes
    )
    $installer = New-Object -ComObject WindowsInstaller.Installer
    try {
        return @($ComponentCodes | Sort-Object -Unique | ForEach-Object {
            [pscustomobject][ordered]@{ componentCode = $_; state = [int]$installer.ComponentState($ProductCode, $_) }
        })
    }
    finally {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer)
    }
}

function Assert-G04DCInstalledComponentStates {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [object[]]$States,
        [Parameter(Mandatory = $true)] [object[]]$ComponentOwnership
    )
    $ownershipByCode = @{}
    foreach ($row in $ComponentOwnership) { $ownershipByCode[[string]$row.componentId] = $row }
    $invalid = @($States | Where-Object {
        $ownership = $ownershipByCode[[string]$_.componentCode]
        if (!$ownership) { return $true }
        return [int]$_.state -ne [int]$ownership.expectedInstallState
    })
    if ($invalid.Count -ne 0 -or $States.Count -ne $ComponentOwnership.Count) {
        throw "[MINIMAL_COMPONENT_STATE_INVALID] Installed component states differ from the exact selected-local/unselected-absent model."
    }
    return $true
}

function Resolve-G04DCExpectedComponentStates {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [object[]]$ComponentOwnership,
        [Parameter(Mandatory = $true)] [object[]]$ConditionEvaluations
    )
    $evaluationByCondition = @{}
    foreach ($evaluation in $ConditionEvaluations) { $evaluationByCondition[[string]$evaluation.condition] = [int]$evaluation.result }
    foreach ($ownership in $ComponentOwnership) {
        $expectedState = 2
        if (@($ownership.selectedOwners).Count -ne 0) {
            if ([string]::IsNullOrWhiteSpace([string]$ownership.condition)) {
                $expectedState = 3
            }
            elseif (!$evaluationByCondition.ContainsKey([string]$ownership.condition) -or [int]$evaluationByCondition[[string]$ownership.condition] -notin @(0, 1)) {
                throw "[AMBIGUOUS_FEATURE_OWNERSHIP] MSI condition was not deterministically evaluated: $($ownership.condition)"
            }
            elseif ([int]$evaluationByCondition[[string]$ownership.condition] -eq 1) {
                $expectedState = 3
            }
        }
        $ownership | Add-Member -NotePropertyName expectedInstallState -NotePropertyValue $expectedState -Force
    }
    return $ComponentOwnership
}

function Get-G04DCMutationClosure {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] $Analysis,
        [Parameter(Mandatory = $true)] [object[]]$ComponentOwnership
    )
    $stateByComponent = @{}
    foreach ($component in $ComponentOwnership) { $stateByComponent[[string]$component.component] = [int]$component.expectedInstallState }
    $rows = @($Analysis.protectedOwnership.mutationTableOwnership | ForEach-Object {
        $component = [string]$_.component
        $stateKnown = ![string]::IsNullOrWhiteSpace($component) -and $stateByComponent.ContainsKey($component)
        [pscustomobject][ordered]@{
            table = $_.table
            component = $component
            directComponentOwnership = [bool]$_.directComponentOwnership
            expectedInstallState = if ($stateKnown) { [int]$stateByComponent[$component] } else { $null }
            enabledForCandidate = $stateKnown -and [int]$stateByComponent[$component] -eq 3
            ownershipResolved = $stateKnown
            featureOwners = @($_.featureOwners)
            selectedOwners = @($_.selectedOwners)
            row = $_.row
        }
    })
    $ambiguousRows = @($rows | Where-Object { ![bool]$_.ownershipResolved })
    $enabledRows = @($rows | Where-Object { [bool]$_.enabledForCandidate })
    return [pscustomobject][ordered]@{
        evaluatedRowCount = $rows.Count
        rows = $rows
        ambiguousRows = $ambiguousRows
        enabledMutationRows = $enabledRows
        unboundedInstallCustomActions = @($Analysis.protectedOwnership.unboundedInstallCustomActions)
        unboundedAdminCustomActions = @($Analysis.protectedOwnership.unboundedAdminCustomActions)
        minimalInstallModelClosed = $ambiguousRows.Count -eq 0 -and $enabledRows.Count -eq 0 -and @($Analysis.protectedOwnership.unboundedInstallCustomActions).Count -eq 0
        administrativeActionModelClosed = @($Analysis.protectedOwnership.unboundedAdminCustomActions).Count -eq 0
    }
}

function Assert-G04DCMinimalMutationClosure {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] $Closure)
    if (@($Closure.ambiguousRows).Count -ne 0) {
        throw '[AMBIGUOUS_MSI_EFFECT_OWNERSHIP] At least one MSI mutation-table row lacks exact component ownership.'
    }
    if (@($Closure.enabledMutationRows).Count -ne 0) {
        throw '[PROTECTED_MSI_EFFECT_UNAVOIDABLE] The selected component closure enables MSI-managed Registry/Font/service/association/shortcut/environment mutation.'
    }
    if (@($Closure.unboundedInstallCustomActions).Count -ne 0) {
        throw '[UNBOUNDED_MSI_CUSTOM_ACTION] The install sequence contains a non-property custom action whose effects are not statically bounded.'
    }
    return $true
}

function Assert-G04DCAdminMutationClosure {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] $Closure)
    if (@($Closure.unboundedAdminCustomActions).Count -ne 0) {
        throw '[UNBOUNDED_MSI_CUSTOM_ACTION] The administrative sequence contains a non-property custom action whose effects are not statically bounded.'
    }
    return $true
}

function Assert-G04DCInstalledFileOwnership {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] $RuntimeManifest,
        [Parameter(Mandatory = $true)] [object[]]$FileComponentOwnership,
        [Parameter(Mandatory = $true)] [object[]]$ComponentOwnership
    )
    $componentByKey = @{}
    foreach ($component in $ComponentOwnership) { $componentByKey[[string]$component.component] = $component }
    $selectedPaths = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $selectedOutsideRoot = @($FileComponentOwnership | Where-Object {
        $component = $componentByKey[[string]$_.component]
        $component -and [int]$component.expectedInstallState -eq 3 -and ![bool]$_.underInstallLocation
    })
    if ($selectedOutsideRoot.Count -ne 0) {
        throw "[MINIMAL_FILE_OWNERSHIP_INVALID] $($selectedOutsideRoot.Count) selected-component files target outside INSTALLLOCATION."
    }
    foreach ($row in @($FileComponentOwnership | Where-Object {
        $component = $componentByKey[[string]$_.component]
        $component -and [int]$component.expectedInstallState -eq 3
    })) {
        if ([string]::IsNullOrWhiteSpace([string]$row.targetRelativePath)) {
            throw '[MINIMAL_FILE_OWNERSHIP_INVALID] Selected MSI file has no canonical INSTALLLOCATION-relative path.'
        }
        [void]$selectedPaths.Add(([string]$row.targetRelativePath).Replace('\', '/'))
    }
    $actualPaths = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($file in @($RuntimeManifest.files)) { [void]$actualPaths.Add(([string]$file.path).Replace('\', '/')) }
    $unexpected = @($actualPaths | Where-Object { !$selectedPaths.Contains($_) })
    $missing = @($selectedPaths | Where-Object { !$actualPaths.Contains($_) })
    if ($unexpected.Count -ne 0 -or $missing.Count -ne 0) {
        throw "[MINIMAL_FILE_OWNERSHIP_INVALID] Runtime differs from exact selected MSI file targets (unexpected=$($unexpected.Count), missing=$($missing.Count))."
    }
    return $true
}

function Get-G04DCExternalRuntimeTargetPaths {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [object[]]$FileComponentOwnership,
        [Parameter(Mandatory = $true)] [string]$WindowsRoot
    )
    $targets = [System.Collections.Generic.List[string]]::new()
    foreach ($row in @($FileComponentOwnership | Where-Object { [bool]$_.disabledByCandidateProperty })) {
        $directory = switch ([string]$row.componentDirectory) {
            { $_ -match '^System64Folder(\.|$)' } { Join-Path $WindowsRoot 'System32' }
            { $_ -match '^SystemFolder(\.|$)' } { Join-Path $WindowsRoot 'SysWOW64' }
            default { throw "[AMBIGUOUS_FEATURE_OWNERSHIP] Unsupported external component directory: $($row.componentDirectory)" }
        }
        $targets.Add((Join-Path $directory ([string]$row.installedFileName)))
    }
    $result = @($targets.ToArray() | Sort-Object -Unique)
    if ($result.Count -eq 0) { throw '[AMBIGUOUS_FEATURE_OWNERSHIP] No external VC runtime target files were derived.' }
    return $result
}

function New-G04DCRegistryValueState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [bool]$KeyExists,
        [Parameter(Mandatory = $true)] [AllowEmptyString()] [string]$ValueName,
        [Parameter(Mandatory = $true)] [bool]$ValuePresent,
        [AllowNull()] [string]$ValueType,
        [AllowNull()] $Value
    )
    $state = [ordered]@{
        schemaVersion = 1
        keyExists = $KeyExists
        valueName = if ([string]::IsNullOrEmpty($ValueName)) { '(default)' } else { $ValueName }
        valuePresent = $ValuePresent
    }
    if ($ValuePresent) {
        $state.valueType = $ValueType
        $state.value = $Value
    }
    return [pscustomobject]$state
}

function ConvertTo-G04DCBoundedRegistryValue {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [string]$ValueType,
        [Parameter(Mandatory = $true)] [AllowEmptyString()] $Value
    )
    $maximumBytes = 1048576
    switch ($ValueType) {
        'String' {
            if ($null -eq $Value -or $Value -isnot [string] -or ([Text.Encoding]::Unicode.GetByteCount([string]$Value) -gt $maximumBytes)) {
                throw '[REGISTRY_STATE_CAPTURE_FAILED] REG_SZ evidence is null, non-string, or exceeds the bounded value ceiling.'
            }
            return [string]$Value
        }
        'ExpandString' {
            if ($null -eq $Value -or $Value -isnot [string] -or ([Text.Encoding]::Unicode.GetByteCount([string]$Value) -gt $maximumBytes)) {
                throw '[REGISTRY_STATE_CAPTURE_FAILED] REG_EXPAND_SZ evidence is null, non-string, or exceeds the bounded value ceiling.'
            }
            return [string]$Value
        }
        'DWord' { return [int]$Value }
        'QWord' { return [long]$Value }
        'MultiString' {
            if ($null -eq $Value -or $Value -isnot [string[]] -or @($Value).Count -gt 4096) {
                throw '[REGISTRY_STATE_CAPTURE_FAILED] REG_MULTI_SZ evidence is null, malformed, or exceeds the bounded entry ceiling.'
            }
            $totalBytes = 0
            $result = @($Value | ForEach-Object {
                if ($null -eq $_) { throw '[REGISTRY_STATE_CAPTURE_FAILED] REG_MULTI_SZ evidence contains a null entry.' }
                $totalBytes += [Text.Encoding]::Unicode.GetByteCount([string]$_) + 2
                [string]$_
            })
            if ($totalBytes -gt $maximumBytes) { throw '[REGISTRY_STATE_CAPTURE_FAILED] REG_MULTI_SZ evidence exceeds the bounded value ceiling.' }
            return ,$result
        }
        { $_ -in @('Binary', 'None') } {
            if ($null -eq $Value -or $Value -isnot [byte[]] -or $Value.Length -gt $maximumBytes) {
                throw "[REGISTRY_STATE_CAPTURE_FAILED] Registry $ValueType evidence is null, malformed, or exceeds the bounded value ceiling."
            }
            return ([BitConverter]::ToString($Value)).Replace('-', '').ToLowerInvariant()
        }
        default { throw "[REGISTRY_STATE_CAPTURE_FAILED] Unsupported registry value kind: $ValueType" }
    }
}

function Test-G04DCRegistryValueNamePresent {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [AllowEmptyCollection()] [AllowEmptyString()] [string[]]$Names,
        [Parameter(Mandatory = $true)] [AllowEmptyString()] [string]$ValueName
    )
    foreach ($name in $Names) {
        if ([string]::Equals([string]$name, $ValueName, [StringComparison]::OrdinalIgnoreCase)) { return $true }
    }
    return $false
}

function Get-G04DCRegistryValueState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [string]$Path,
        [Parameter(Mandatory = $true)] [AllowEmptyString()] [string]$ValueName,
        [Parameter(DontShow = $true)] [AllowNull()] [hashtable]$AccessAdapter
    )
    if ($Path.Length -gt 2048 -or $Path -notmatch '^Registry::HKEY_(CLASSES_ROOT|CURRENT_USER|LOCAL_MACHINE|USERS)(\\|$)' -or $ValueName.Length -gt 16383) {
        throw '[REGISTRY_STATE_CAPTURE_FAILED] Registry path or value name is outside the bounded provider model.'
    }
    if (!$AccessAdapter) {
        $AccessAdapter = @{
            OpenKey = {
                param([string]$CandidatePath)
                try {
                    [pscustomobject][ordered]@{ keyExists = $true; handle = Get-Item -LiteralPath $CandidatePath -ErrorAction Stop }
                }
                catch [System.Management.Automation.ItemNotFoundException] {
                    [pscustomobject][ordered]@{ keyExists = $false; handle = $null }
                }
            }
            GetValueNames = { param($Handle) @($Handle.GetValueNames()) }
            GetValueKind = { param($Handle, [string]$Name) $Handle.GetValueKind($Name) }
            GetValue = {
                param($Handle, [string]$Name)
                $missing = [object]::new()
                $observed = $Handle.GetValue($Name, $missing, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
                [pscustomobject][ordered]@{ present = ![object]::ReferenceEquals($missing, $observed); value = $observed }
            }
            CloseKey = { param($Handle) if ($Handle -is [IDisposable]) { $Handle.Dispose() } }
        }
    }
    foreach ($operation in @('OpenKey', 'GetValueNames', 'GetValueKind', 'GetValue', 'CloseKey')) {
        if (!$AccessAdapter.ContainsKey($operation) -or $AccessAdapter[$operation] -isnot [scriptblock]) {
            throw '[REGISTRY_STATE_CAPTURE_FAILED] Registry read adapter is incomplete.'
        }
    }

    $handle = $null
    try {
        $opened = & $AccessAdapter.OpenKey $Path
        if (!$opened -or !$opened.PSObject.Properties['keyExists']) {
            throw '[REGISTRY_STATE_CAPTURE_FAILED] Registry provider returned an invalid key state.'
        }
        if (![bool]$opened.keyExists) {
            return New-G04DCRegistryValueState -KeyExists $false -ValueName $ValueName -ValuePresent $false
        }
        $handle = $opened.handle
        if ($null -eq $handle) { throw '[REGISTRY_STATE_CAPTURE_FAILED] Registry provider returned an existing key without a read handle.' }
        $names = @(& $AccessAdapter.GetValueNames $handle)
        if (!(Test-G04DCRegistryValueNamePresent -Names $names -ValueName $ValueName)) {
            return New-G04DCRegistryValueState -KeyExists $true -ValueName $ValueName -ValuePresent $false
        }

        try {
            $valueType = [string](& $AccessAdapter.GetValueKind $handle $ValueName)
            $valueResult = & $AccessAdapter.GetValue $handle $ValueName
        }
        catch [System.IO.IOException] {
            $namesAfterRace = @(& $AccessAdapter.GetValueNames $handle)
            if (!(Test-G04DCRegistryValueNamePresent -Names $namesAfterRace -ValueName $ValueName)) {
                return New-G04DCRegistryValueState -KeyExists $true -ValueName $ValueName -ValuePresent $false
            }
            throw '[REGISTRY_STATE_CAPTURE_FAILED] Registry value changed during classification and could not be sealed.'
        }
        if (!$valueResult -or !$valueResult.PSObject.Properties['present']) {
            throw '[REGISTRY_STATE_CAPTURE_FAILED] Registry provider returned an invalid value-read state.'
        }
        if (![bool]$valueResult.present) {
            $namesAfterRace = @(& $AccessAdapter.GetValueNames $handle)
            if (!(Test-G04DCRegistryValueNamePresent -Names $namesAfterRace -ValueName $ValueName)) {
                return New-G04DCRegistryValueState -KeyExists $true -ValueName $ValueName -ValuePresent $false
            }
            throw '[REGISTRY_STATE_CAPTURE_FAILED] Registry value changed during classification and could not be sealed.'
        }
        $boundedValue = ConvertTo-G04DCBoundedRegistryValue -ValueType $valueType -Value $valueResult.value
        return New-G04DCRegistryValueState -KeyExists $true -ValueName $ValueName -ValuePresent $true -ValueType $valueType -Value $boundedValue
    }
    catch {
        if ($_.Exception.Message.StartsWith('[REGISTRY_STATE_CAPTURE_FAILED]', [StringComparison]::Ordinal)) { throw }
        throw "[REGISTRY_STATE_CAPTURE_FAILED] Read-only registry classification failed ($($_.Exception.GetType().FullName))."
    }
    finally {
        if ($null -ne $handle) {
            try { & $AccessAdapter.CloseKey $handle }
            catch { throw "[REGISTRY_STATE_CAPTURE_FAILED] Registry read-handle cleanup failed ($($_.Exception.GetType().FullName))." }
        }
    }
}

function Get-G04DCRegistryDefaultValueState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [string]$Path,
        [Parameter(DontShow = $true)] [AllowNull()] [hashtable]$AccessAdapter
    )
    $arguments = @{ Path = $Path; ValueName = '' }
    if ($AccessAdapter) { $arguments.AccessAdapter = $AccessAdapter }
    $state = Get-G04DCRegistryValueState @arguments
    $defaultState = [ordered]@{
        schemaVersion = [int]$state.schemaVersion
        keyExists = [bool]$state.keyExists
        defaultValuePresent = [bool]$state.valuePresent
    }
    if ([bool]$state.valuePresent) {
        $defaultState.defaultValueType = [string]$state.valueType
        $defaultState.defaultValue = $state.value
    }
    return [pscustomobject]$defaultState
}

function ConvertTo-G04DCPackedGuid {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] [string]$Guid)
    $parts = $Guid.Trim('{}').ToUpperInvariant().Split('-')
    if ($parts.Count -ne 5 -or ($parts -join '') -notmatch '^[0-9A-F]{32}$') { throw "[MSI_REGISTRATION_INVALID] Invalid MSI GUID: $Guid" }
    function Reverse-G04DCText([string]$Value) { return -join $Value.ToCharArray()[($Value.Length - 1)..0] }
    $tail = $parts[3] + $parts[4]
    $swappedTailCharacters = for ($index = 0; $index -lt $tail.Length; $index += 2) { $tail[$index + 1]; $tail[$index] }
    $swappedTail = -join $swappedTailCharacters
    return (Reverse-G04DCText $parts[0]) + (Reverse-G04DCText $parts[1]) + (Reverse-G04DCText $parts[2]) + $swappedTail
}

function Get-G04DCMsiRegistrationState {
    [CmdletBinding()]
    param([AllowEmptyCollection()] [string[]]$ComponentCodes = @())
    $expected = Get-G04DCExpectedMsi
    $packedProduct = ConvertTo-G04DCPackedGuid -Guid $expected.ProductCode
    $packedUpgrade = ConvertTo-G04DCPackedGuid -Guid $expected.UpgradeCode
    $installer = New-Object -ComObject WindowsInstaller.Installer
    try {
        $productState = try { [int]$installer.ProductState($expected.ProductCode) } catch { -1 }
        $localPackage = if ($productState -ne -1) { try { [string]$installer.ProductInfo($expected.ProductCode, 'LocalPackage') } catch { $null } } else { $null }
    }
    finally { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer) }
    $localPackageItem = if ($localPackage) { Get-Item -LiteralPath $localPackage -Force -ErrorAction SilentlyContinue } else { $null }
    $localPackageSignature = if ($localPackageItem -and !$localPackageItem.PSIsContainer) { Get-G04DCAuthenticodeEvidence -Path $localPackageItem.FullName } else { $null }
    $productRegistryPaths = @(
        "Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Classes\Installer\Products\$packedProduct",
        "Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Classes\Installer\Features\$packedProduct",
        "Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\UserData\S-1-5-18\Products\$packedProduct",
        "Registry::HKEY_CURRENT_USER\SOFTWARE\Microsoft\Installer\Products\$packedProduct",
        "Registry::HKEY_CURRENT_USER\SOFTWARE\Microsoft\Installer\Features\$packedProduct"
    )
    $productRegistryTargets = @($productRegistryPaths | ForEach-Object {
        [pscustomobject][ordered]@{ path = $_; pathPresent = Test-Path -LiteralPath $_; values = @(Get-G04DCRegistryValues -Path $_) }
    })
    $upgradePath = "Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Classes\Installer\UpgradeCodes\$packedUpgrade"
    $upgradeValueState = Get-G04DCRegistryValueState -Path $upgradePath -ValueName $packedProduct
    $componentRegistrations = @($ComponentCodes | Where-Object { $_ } | Sort-Object -Unique | ForEach-Object {
        $componentCode = $_
        $packedComponent = ConvertTo-G04DCPackedGuid -Guid $componentCode
        $systemPath = "Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\UserData\S-1-5-18\Components\$packedComponent"
        $userPath = "Registry::HKEY_CURRENT_USER\SOFTWARE\Microsoft\Installer\Components\$packedComponent"
        $systemValueState = Get-G04DCRegistryValueState -Path $systemPath -ValueName $packedProduct
        $userValueState = Get-G04DCRegistryValueState -Path $userPath -ValueName $packedProduct
        [pscustomobject][ordered]@{
            componentCode = $componentCode
            packedComponent = $packedComponent
            systemPath = $systemPath
            systemProductValuePresent = [bool]$systemValueState.valuePresent
            systemProductValueState = $systemValueState
            userPath = $userPath
            userProductValuePresent = [bool]$userValueState.valuePresent
            userProductValueState = $userValueState
        }
    })
    return [pscustomobject][ordered]@{
        productCode = $expected.ProductCode
        packedProductCode = $packedProduct
        productState = $productState
        productStateUnknown = -1
        productStateDefault = 5
        productRegistryTargets = $productRegistryTargets
        upgradeCodePath = $upgradePath
        upgradeProductValuePresent = [bool]$upgradeValueState.valuePresent
        upgradeProductValueState = $upgradeValueState
        componentRegistrations = $componentRegistrations
        localPackage = [pscustomobject][ordered]@{
            path = $localPackage
            present = [bool]$localPackageItem
            regularFile = [bool]($localPackageItem -and !$localPackageItem.PSIsContainer)
            reparsePoint = if ($localPackageItem) { [bool]($localPackageItem.Attributes -band [IO.FileAttributes]::ReparsePoint) } else { $false }
            sizeBytes = if ($localPackageItem -and !$localPackageItem.PSIsContainer) { [long]$localPackageItem.Length } else { 0 }
            sha256 = if ($localPackageItem -and !$localPackageItem.PSIsContainer) { Get-G04DCSha256 -Path $localPackageItem.FullName } else { $null }
            authenticode = $localPackageSignature
        }
    }
}

function Assert-G04DCMsiRegistrationAbsent {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] $State)
    if ([int]$State.productState -ne -1 -or [bool]$State.localPackage.present -or
        @($State.productRegistryTargets | Where-Object { [bool]$_.pathPresent }).Count -ne 0 -or [bool]$State.upgradeProductValuePresent -or
        @($State.componentRegistrations | Where-Object { [bool]$_.systemProductValuePresent -or [bool]$_.userProductValuePresent }).Count -ne 0) {
        throw '[MSI_REGISTRATION_RESIDUE] Authoritative Windows Installer ProductState/cache/Products/Features/Components state is not absent.'
    }
    return $true
}

function Assert-G04DCMsiRegistrationInstalled {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] $State, [Parameter(Mandatory = $true)] [object[]]$ExpectedComponents)
    if ([int]$State.productState -ne 5) { throw '[MSI_REGISTRATION_INVALID] Windows Installer ProductState is not INSTALLSTATE_DEFAULT.' }
    $requiredProductTargets = @($State.productRegistryTargets | Where-Object { $_.path -match 'HKEY_LOCAL_MACHINE' })
    if ($requiredProductTargets.Count -lt 3 -or @($requiredProductTargets | Where-Object { ![bool]$_.pathPresent }).Count -ne 0 -or ![bool]$State.upgradeProductValuePresent) {
        throw '[MSI_REGISTRATION_INVALID] Authoritative Installer Products/Features/UserData/UpgradeCode registration is incomplete.'
    }
    $package = $State.localPackage
    $expected = Get-G04DCExpectedMsi
    if (![bool]$package.present -or ![bool]$package.regularFile -or [bool]$package.reparsePoint -or !$package.authenticode -or
        [string]$package.authenticode.status -cne 'Valid' -or ![bool]$package.authenticode.chainValid -or @($package.authenticode.chain).Count -lt 2 -or
        [string]$package.authenticode.signerThumbprint -cne $expected.SignerThumbprint) {
        throw '[MSI_REGISTRATION_INVALID] Windows Installer cached package failed the exact identity/signature-chain boundary.'
    }
    $expectedByCode = @{}
    foreach ($component in $ExpectedComponents) { $expectedByCode[[string]$component.componentId] = [int]$component.expectedInstallState }
    $invalid = @($State.componentRegistrations | Where-Object {
        $expectedState = $expectedByCode[[string]$_.componentCode]
        $present = [bool]$_.systemProductValuePresent -or [bool]$_.userProductValuePresent
        return ($expectedState -eq 3 -and !$present) -or ($expectedState -ne 3 -and $present)
    })
    if ($invalid.Count -ne 0 -or @($State.componentRegistrations).Count -ne $ExpectedComponents.Count) {
        throw '[MSI_REGISTRATION_INVALID] Installer component registration differs from the exact evaluated component-state model.'
    }
    return $true
}

function Get-G04DCRegistryValues {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] [string]$Path)
    if (!(Test-Path -LiteralPath $Path)) { return @() }
    $item = Get-ItemProperty -LiteralPath $Path
    return @($item.PSObject.Properties | Where-Object { $_.Name -notmatch '^PS(Path|ParentPath|ChildName|Drive|Provider)$' } | Sort-Object Name | ForEach-Object {
        [pscustomobject][ordered]@{ name = $_.Name; value = [string]$_.Value }
    })
}

function Get-G04DCRegistryTreeDigest {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] [string]$NativePath)
    $reg = Join-Path $env:SystemRoot 'System32\reg.exe'
    $rows = @(& $reg query $NativePath /s 2>&1 | ForEach-Object { ([string]$_).TrimEnd() })
    if ($LASTEXITCODE -ne 0) { throw "[MACHINE_STATE_CAPTURE_FAILED] reg.exe could not seal $NativePath." }
    return [pscustomobject][ordered]@{
        path = $NativePath
        rowCount = $rows.Count
        sha256 = Get-G04DCCanonicalHash -Rows $rows
    }
}

function Get-G04DCRegistryValueDigest {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] [string[]]$Paths)
    $rows = @($Paths | Sort-Object -Unique | ForEach-Object {
        $path = $_
        if (Test-Path -LiteralPath $path) {
            $key = Get-Item -LiteralPath $path -ErrorAction Stop
            foreach ($name in @($key.GetValueNames() | Sort-Object)) {
                [pscustomobject][ordered]@{
                    path = $path
                    name = $name
                    kind = [string]$key.GetValueKind($name)
                    value = [string]$key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
                }
            }
        }
    })
    return [pscustomobject][ordered]@{ rowCount = $rows.Count; sha256 = Get-G04DCCanonicalHash -Rows $rows }
}

function Get-G04DCDirectoryTreeDigest {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] [string[]]$Roots)
    $rows = [System.Collections.Generic.List[object]]::new()
    foreach ($candidateRoot in @($Roots | Sort-Object -Unique)) {
        if ([string]::IsNullOrWhiteSpace($candidateRoot)) { throw '[MACHINE_STATE_CAPTURE_FAILED] A shortcut boundary root was unresolved.' }
        $root = [IO.Path]::GetFullPath($candidateRoot).TrimEnd('\')
        $rootItem = Get-Item -LiteralPath $root -Force -ErrorAction SilentlyContinue
        if (!$rootItem) {
            $rows.Add([pscustomobject][ordered]@{ root = $root; path = ''; present = $false; directory = $true; reparsePoint = $false; sizeBytes = 0; sha256 = $null; lastWriteUtc = $null })
            continue
        }
        if (!$rootItem.PSIsContainer -or [bool]($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "[MACHINE_STATE_CAPTURE_FAILED] Shortcut boundary root is not a canonical non-reparse directory: $root"
        }
        $queue = [System.Collections.Generic.Queue[string]]::new()
        $queue.Enqueue($root)
        while ($queue.Count -ne 0) {
            $directory = $queue.Dequeue()
            foreach ($item in @(Get-ChildItem -LiteralPath $directory -Force -ErrorAction Stop | Sort-Object FullName)) {
                $relative = $item.FullName.Substring($root.Length).TrimStart('\')
                $isReparse = [bool]($item.Attributes -band [IO.FileAttributes]::ReparsePoint)
                $rows.Add([pscustomobject][ordered]@{
                    root = $root
                    path = $relative
                    present = $true
                    directory = [bool]$item.PSIsContainer
                    reparsePoint = $isReparse
                    sizeBytes = if ($item.PSIsContainer) { 0 } else { [long]$item.Length }
                    sha256 = if (!$item.PSIsContainer -and !$isReparse) { Get-G04DCSha256 -Path $item.FullName } else { $null }
                    lastWriteUtc = $item.LastWriteTimeUtc.ToString('o')
                })
                if ($item.PSIsContainer -and !$isReparse) { $queue.Enqueue($item.FullName) }
                if ($rows.Count -gt 100000) { throw '[MACHINE_STATE_CAPTURE_FAILED] Shortcut boundary exceeded 100000 entries.' }
            }
        }
    }
    $canonicalRows = @($rows.ToArray() | Sort-Object root, path)
    return [pscustomobject][ordered]@{ rowCount = $canonicalRows.Count; sha256 = Get-G04DCCanonicalHash -Rows $canonicalRows }
}

function Get-G04DCMachineState {
    [CmdletBinding()]
    param(
        [AllowEmptyCollection()] [object[]]$ProtectedRegistryRows = @(),
        [AllowEmptyCollection()] [string[]]$ProtectedFontFileNames = @(),
        [AllowEmptyCollection()] [string[]]$ProtectedExternalFilePaths = @(),
        [AllowEmptyCollection()] [string[]]$ProtectedMsiComponentCodes = @()
    )
    $extensions = @('.odt', '.ods', '.odp', '.doc', '.docx', '.xls', '.xlsx', '.ppt', '.pptx', '.pdf')
    $associations = @($extensions | ForEach-Object {
        $extension = $_
        $classKey = "Registry::HKEY_CLASSES_ROOT\$extension"
        $choiceKey = "Registry::HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\$extension\UserChoice"
        $classDefaultState = Get-G04DCRegistryDefaultValueState -Path $classKey
        $userChoiceProgIdState = Get-G04DCRegistryValueState -Path $choiceKey -ValueName 'ProgId'
        [pscustomobject][ordered]@{
            extension = $extension
            classDefaultState = $classDefaultState
            userChoiceProgIdState = $userChoiceProgIdState
            userChoicePresent = [bool]$userChoiceProgIdState.keyExists
        }
    })
    $fontRoots = @(
        'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts',
        'Registry::HKEY_CURRENT_USER\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts'
    )
    $fontCatalogRows = @($fontRoots | ForEach-Object {
        $fontRoot = $_
        @(Get-G04DCRegistryValues -Path $fontRoot) | ForEach-Object { [pscustomobject][ordered]@{ path = $fontRoot; name = $_.name; value = $_.value } }
    })
    $msiFontTargets = @($ProtectedFontFileNames | Sort-Object -Unique | ForEach-Object {
        $fontFileName = $_
        $fontPath = Join-Path (Join-Path $env:SystemRoot 'Fonts') $fontFileName
        $fontItem = Get-Item -LiteralPath $fontPath -Force -ErrorAction SilentlyContinue
        [pscustomobject][ordered]@{
            fileName = $fontFileName
            path = $fontPath
            filePresent = [bool]$fontItem
            fileReparsePoint = if ($fontItem) { [bool]($fontItem.Attributes -band [IO.FileAttributes]::ReparsePoint) } else { $false }
            fileSizeBytes = if ($fontItem) { [long]$fontItem.Length } else { 0 }
            fileSha256 = if ($fontItem -and !$fontItem.PSIsContainer) { Get-G04DCSha256 -Path $fontItem.FullName } else { $null }
            registryMatches = @($fontCatalogRows | Where-Object {
                ([IO.Path]::GetFileName([string]$_.value) -ieq $fontFileName) -or ([string]$_.name -imatch [regex]::Escape([IO.Path]::GetFileNameWithoutExtension($fontFileName)))
            })
        }
    })
    $externalRuntimeTargets = @($ProtectedExternalFilePaths | Sort-Object -Unique | ForEach-Object {
        $targetPath = [IO.Path]::GetFullPath($_)
        $targetItem = Get-Item -LiteralPath $targetPath -Force -ErrorAction SilentlyContinue
        $signature = if ($targetItem -and !$targetItem.PSIsContainer) { Get-G04DCAuthenticodeEvidence -Path $targetItem.FullName } else { $null }
        [pscustomobject][ordered]@{
            path = $targetPath
            present = [bool]$targetItem
            regularFile = [bool]($targetItem -and !$targetItem.PSIsContainer)
            reparsePoint = if ($targetItem) { [bool]($targetItem.Attributes -band [IO.FileAttributes]::ReparsePoint) } else { $false }
            sizeBytes = if ($targetItem -and !$targetItem.PSIsContainer) { [long]$targetItem.Length } else { 0 }
            sha256 = if ($targetItem -and !$targetItem.PSIsContainer) { Get-G04DCSha256 -Path $targetItem.FullName } else { $null }
            authenticodeStatus = if ($signature) { [string]$signature.status } else { $null }
            signer = if ($signature) { [string]$signature.signerSubject } else { $null }
            signerThumbprint = if ($signature) { [string]$signature.signerThumbprint } else { $null }
            signerChainValid = if ($signature) { [bool]$signature.chainValid } else { $false }
            signerChain = if ($signature) { @($signature.chain) } else { @() }
            fileVersion = if ($targetItem -and !$targetItem.PSIsContainer) { [string]$targetItem.VersionInfo.FileVersion } else { $null }
        }
    })
    $services = @(Get-CimInstance Win32_Service | Sort-Object Name | ForEach-Object {
        [pscustomobject][ordered]@{
            name = $_.Name
            displayName = $_.DisplayName
            state = $_.State
            status = $_.Status
            startMode = $_.StartMode
            pathName = $_.PathName
            serviceType = $_.ServiceType
            startName = $_.StartName
            errorControl = $_.ErrorControl
            desktopInteract = [bool]$_.DesktopInteract
            processId = [int]$_.ProcessId
        }
    })
    $serviceRegistryCatalog = Get-G04DCRegistryTreeDigest -NativePath 'HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Services'
    $libreOfficeServices = @($services | Where-Object { $_.name -match '(?i)libreoffice|soffice' -or $_.pathName -match '(?i)libreoffice|soffice' })
    $appPathKeys = @(
        foreach ($scope in @(
            'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths',
            'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths',
            'Registry::HKEY_CURRENT_USER\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths'
        )) {
            foreach ($executable in @('soffice.exe', 'swriter.exe', 'scalc.exe', 'simpress.exe', 'sdraw.exe', 'unopkg.exe')) {
                "$scope\$executable"
            }
        }
    )
    $appPaths = @($appPathKeys | ForEach-Object { [pscustomobject][ordered]@{ path = $_; values = @(Get-G04DCRegistryValues -Path $_) } })
    $appPathScopes = @(
        'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths',
        'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths',
        'Registry::HKEY_CURRENT_USER\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths'
    )
    $appPathCatalogRows = @($appPathScopes | ForEach-Object {
        $scope = $_
        if (Test-Path -LiteralPath $scope) {
            Get-ChildItem -LiteralPath $scope -ErrorAction SilentlyContinue | Sort-Object PSChildName | ForEach-Object {
                [pscustomobject][ordered]@{ path = "$scope\$($_.PSChildName)"; values = @(Get-G04DCRegistryValues -Path $_.PSPath) }
            }
        }
    })
    $classKeyNames = @(Get-ChildItem -LiteralPath 'Registry::HKEY_CLASSES_ROOT' -ErrorAction SilentlyContinue | ForEach-Object { $_.PSChildName } | Sort-Object)
    $classRegistryCatalog = Get-G04DCRegistryTreeDigest -NativePath 'HKEY_CLASSES_ROOT'
    $progIds = @(Get-ChildItem -LiteralPath 'Registry::HKEY_CLASSES_ROOT' -ErrorAction SilentlyContinue | Where-Object {
        $_.PSChildName -match '^(?i:LibreOffice\.|soffice\.)'
    } | Sort-Object PSChildName | ForEach-Object {
        [pscustomobject][ordered]@{
            key = $_.PSChildName
            defaultValueState = Get-G04DCRegistryDefaultValueState -Path "Registry::HKEY_CLASSES_ROOT\$($_.PSChildName)"
        }
    })
    $startupKeys = @(
        'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Run',
        'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce',
        'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run',
        'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\RunOnce',
        'Registry::HKEY_CURRENT_USER\SOFTWARE\Microsoft\Windows\CurrentVersion\Run',
        'Registry::HKEY_CURRENT_USER\SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce'
    )
    $startupRegistryRows = @($startupKeys | ForEach-Object {
        [pscustomobject][ordered]@{ type = 'registry'; path = $_; values = @(Get-G04DCRegistryValues -Path $_) }
    })
    $startupFolderRoots = @(
        [pscustomobject]@{ name = 'machine'; path = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\Startup' },
        [pscustomobject]@{ name = 'user'; path = [Environment]::GetFolderPath([Environment+SpecialFolder]::Startup) }
    )
    $startupFileRows = @($startupFolderRoots | ForEach-Object {
        $startupRoot = $_
        if (Test-Path -LiteralPath $startupRoot.path) {
            Get-ChildItem -LiteralPath $startupRoot.path -Force -ErrorAction Stop | Sort-Object Name | ForEach-Object {
                [pscustomobject][ordered]@{
                    type = 'file'
                    root = $startupRoot.name
                    name = $_.Name
                    directory = $_.PSIsContainer
                    reparsePoint = [bool]($_.Attributes -band [IO.FileAttributes]::ReparsePoint)
                    sizeBytes = if ($_.PSIsContainer) { 0 } else { [long]$_.Length }
                    sha256 = if (!$_.PSIsContainer -and ![bool]($_.Attributes -band [IO.FileAttributes]::ReparsePoint)) { Get-G04DCSha256 -Path $_.FullName } else { $null }
                    lastWriteUtc = $_.LastWriteTimeUtc.ToString('o')
                }
            }
        }
    })
    $startupCatalogRows = @($startupRegistryRows) + @($startupFileRows)
    $startup = @(
        @($startupRegistryRows | ForEach-Object {
            [pscustomobject][ordered]@{ type = 'registry'; path = $_.path; values = @($_.values | Where-Object { $_.name -match '(?i)libreoffice|soffice' -or $_.value -match '(?i)libreoffice|soffice' }) }
        })
        @($startupFileRows | Where-Object { $_.name -match '(?i)libreoffice|soffice' })
    )
    $allTaskRows = @(Get-ScheduledTask -ErrorAction SilentlyContinue | Sort-Object TaskPath, TaskName | ForEach-Object {
        $taskXml = Export-ScheduledTask -TaskName $_.TaskName -TaskPath $_.TaskPath -ErrorAction Stop
        [pscustomobject][ordered]@{
            taskPath = $_.TaskPath
            taskName = $_.TaskName
            state = [string]$_.State
            actions = @($_.Actions | ForEach-Object { [pscustomobject][ordered]@{ execute = $_.Execute; arguments = $_.Arguments; workingDirectory = $_.WorkingDirectory } })
            definitionSha256 = Get-G04DCCanonicalHash -Rows @($taskXml)
        }
    })
    $shortcutCatalog = Get-G04DCDirectoryTreeDigest -Roots @(
        (Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs'),
        ([Environment]::GetFolderPath([Environment+SpecialFolder]::Programs)),
        ([Environment]::GetFolderPath([Environment+SpecialFolder]::CommonDesktopDirectory)),
        ([Environment]::GetFolderPath([Environment+SpecialFolder]::DesktopDirectory))
    )
    $environmentCatalog = Get-G04DCRegistryValueDigest -Paths @(
        'Registry::HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\Session Manager\Environment',
        'Registry::HKEY_CURRENT_USER\Environment'
    )
    $tasks = @($allTaskRows | Where-Object {
        $_.taskName -match '(?i)libreoffice|soffice' -or $_.taskPath -match '(?i)libreoffice|soffice' -or
        (@($_.actions | Where-Object { $_.execute -match '(?i)libreoffice|soffice' -or $_.arguments -match '(?i)libreoffice|soffice' }).Count -ne 0)
    })
    $allFirewallApplicationPrograms = @{}
    foreach ($filter in @(Get-NetFirewallApplicationFilter -ErrorAction SilentlyContinue)) {
        $allFirewallApplicationPrograms[[string]$filter.InstanceID] = [string]$filter.Program
    }
    $allFirewallRows = @(Get-NetFirewallRule -ErrorAction SilentlyContinue | Sort-Object Name | ForEach-Object {
        [pscustomobject][ordered]@{
            name = $_.Name
            displayName = $_.DisplayName
            enabled = [string]$_.Enabled
            direction = [string]$_.Direction
            action = [string]$_.Action
            profile = [string]$_.Profile
            edgeTraversalPolicy = [string]$_.EdgeTraversalPolicy
            program = $allFirewallApplicationPrograms[[string]$_.InstanceID]
        }
    })
    $allFirewallFilterRows = @(
        @(Get-NetFirewallApplicationFilter -ErrorAction SilentlyContinue | ForEach-Object { [pscustomobject][ordered]@{ type = 'application'; instanceId = $_.InstanceID; program = $_.Program; package = $_.Package } })
        @(Get-NetFirewallPortFilter -ErrorAction SilentlyContinue | ForEach-Object { [pscustomobject][ordered]@{ type = 'port'; instanceId = $_.InstanceID; protocol = [string]$_.Protocol; localPort = [string]$_.LocalPort; remotePort = [string]$_.RemotePort; icmpType = [string]$_.IcmpType } })
        @(Get-NetFirewallAddressFilter -ErrorAction SilentlyContinue | ForEach-Object { [pscustomobject][ordered]@{ type = 'address'; instanceId = $_.InstanceID; localAddress = [string]$_.LocalAddress; remoteAddress = [string]$_.RemoteAddress } })
        @(Get-NetFirewallServiceFilter -ErrorAction SilentlyContinue | ForEach-Object { [pscustomobject][ordered]@{ type = 'service'; instanceId = $_.InstanceID; service = $_.Service } })
        @(Get-NetFirewallInterfaceFilter -ErrorAction SilentlyContinue | ForEach-Object { [pscustomobject][ordered]@{ type = 'interface'; instanceId = $_.InstanceID; interfaceAlias = [string]$_.InterfaceAlias } })
        @(Get-NetFirewallSecurityFilter -ErrorAction SilentlyContinue | ForEach-Object { [pscustomobject][ordered]@{ type = 'security'; instanceId = $_.InstanceID; authentication = [string]$_.Authentication; encryption = [string]$_.Encryption; localUser = [string]$_.LocalUser; remoteUser = [string]$_.RemoteUser; remoteMachine = [string]$_.RemoteMachine; overrideBlockRules = [string]$_.OverrideBlockRules } })
    )
    $firewall = @($allFirewallRows | Where-Object { $_.displayName -match '(?i)libreoffice|soffice' -or $_.name -match '(?i)libreoffice|soffice' -or $_.program -match '(?i)libreoffice|soffice' })
    $protectedTargets = [System.Collections.Generic.List[object]]::new()
    foreach ($row in $ProtectedRegistryRows) {
        $roots = switch ([string]$row.Root) {
            '0' { @('Registry::HKEY_CLASSES_ROOT') }
            '1' { @('Registry::HKEY_CURRENT_USER') }
            '2' { @('Registry::HKEY_LOCAL_MACHINE') }
            '3' { @('Registry::HKEY_USERS') }
            '-1' { @('Registry::HKEY_CURRENT_USER', 'Registry::HKEY_LOCAL_MACHINE') }
            default { @() }
        }
        foreach ($registryRoot in $roots) {
            $path = "$registryRoot\$([string]$row.Key)"
            $name = if ([string]::IsNullOrEmpty([string]$row.Name)) { '(default)' } else { [string]$row.Name }
            $valueState = if ($name -ceq '(default)') {
                Get-G04DCRegistryDefaultValueState -Path $path
            }
            else {
                Get-G04DCRegistryValueState -Path $path -ValueName $name
            }
            $protectedTargets.Add([pscustomobject][ordered]@{ path = $path; name = $name; pathPresent = [bool]$valueState.keyExists; valueState = $valueState })
        }
    }
    $protectedTargets = @($protectedTargets.ToArray() | Sort-Object path, name -Unique)
    $processes = @(Get-CimInstance Win32_Process | Where-Object { $_.Name -match '^(?i:soffice|libreoffice).*' -or $_.ExecutablePath -match '(?i)libreoffice' } | Sort-Object ProcessId | ForEach-Object {
        [pscustomobject][ordered]@{ pid = [int]$_.ProcessId; parentPid = [int]$_.ParentProcessId; name = $_.Name; executablePath = $_.ExecutablePath }
    })
    $productCode = $script:G04DCExpectedMsi.ProductCode
    $productKeys = @(
        "Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$productCode",
        "Registry::HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\$productCode"
    )
    $products = @($productKeys | Where-Object { Test-Path -LiteralPath $_ } | ForEach-Object { [pscustomobject][ordered]@{ path = $_; values = @(Get-G04DCRegistryValues -Path $_) } })
    $uninstallRoots = @(
        'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
        'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
    )
    $productCatalogRows = @($uninstallRoots | ForEach-Object {
        $scope = $_
        if (Test-Path -LiteralPath $scope) {
            Get-ChildItem -LiteralPath $scope -ErrorAction SilentlyContinue | Sort-Object PSChildName | ForEach-Object {
                [pscustomobject][ordered]@{ path = "$scope\$($_.PSChildName)"; values = @(Get-G04DCRegistryValues -Path $_.PSPath) }
            }
        }
    })
    $otherProductCatalogRows = @($productCatalogRows | Where-Object { !([string]$_.path).EndsWith($productCode, [StringComparison]::OrdinalIgnoreCase) })
    $installerCacheCatalog = Get-G04DCDirectoryTreeDigest -Roots @((Join-Path $env:SystemRoot 'Installer'))
    $pendingFileRenameKey = 'Registry::HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\Session Manager'
    $pendingReboot = [pscustomobject][ordered]@{
        componentBasedServicing = Test-Path -LiteralPath 'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending'
        windowsUpdate = Test-Path -LiteralPath 'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired'
        pendingFileRenameOperationsState = Get-G04DCRegistryValueState -Path $pendingFileRenameKey -ValueName 'PendingFileRenameOperations'
    }
    $serviceDigestRows = @($services)
    $msiRegistration = Get-G04DCMsiRegistrationState -ComponentCodes $ProtectedMsiComponentCodes
    return [pscustomobject][ordered]@{
        capturedAtUtc = [DateTime]::UtcNow.ToString('o')
        fontCatalogCount = $fontCatalogRows.Count
        fontCatalogSha256 = Get-G04DCCanonicalHash -Rows $fontCatalogRows
        msiFontTargets = $msiFontTargets
        externalRuntimeTargets = $externalRuntimeTargets
        associations = $associations
        libreOfficeServices = $libreOfficeServices
        serviceCatalogCount = $services.Count
        serviceCatalogSha256 = Get-G04DCCanonicalHash -Rows $serviceDigestRows
        serviceRegistryCatalogCount = $serviceRegistryCatalog.rowCount
        serviceRegistryCatalogSha256 = $serviceRegistryCatalog.sha256
        appPaths = $appPaths
        appPathCatalogCount = $appPathCatalogRows.Count
        appPathCatalogSha256 = Get-G04DCCanonicalHash -Rows $appPathCatalogRows
        libreOfficeProgIds = $progIds
        classKeyCatalogCount = $classKeyNames.Count
        classKeyCatalogSha256 = Get-G04DCCanonicalHash -Rows $classKeyNames
        classRegistryCatalogCount = $classRegistryCatalog.rowCount
        classRegistryCatalogSha256 = $classRegistryCatalog.sha256
        msiProtectedRegistryTargets = $protectedTargets
        scheduledTasks = $tasks
        scheduledTaskCatalogCount = $allTaskRows.Count
        scheduledTaskCatalogSha256 = Get-G04DCCanonicalHash -Rows $allTaskRows
        startup = $startup
        startupCatalogCount = $startupCatalogRows.Count
        startupCatalogSha256 = Get-G04DCCanonicalHash -Rows $startupCatalogRows
        shortcutCatalogCount = $shortcutCatalog.rowCount
        shortcutCatalogSha256 = $shortcutCatalog.sha256
        firewallRules = $firewall
        firewallCatalogCount = $allFirewallRows.Count
        firewallCatalogSha256 = Get-G04DCCanonicalHash -Rows $allFirewallRows
        firewallFilterCatalogCount = $allFirewallFilterRows.Count
        firewallFilterCatalogSha256 = Get-G04DCCanonicalHash -Rows $allFirewallFilterRows
        machinePath = [Environment]::GetEnvironmentVariable('PATH', 'Machine')
        userPath = [Environment]::GetEnvironmentVariable('PATH', 'User')
        environmentCatalogCount = $environmentCatalog.rowCount
        environmentCatalogSha256 = $environmentCatalog.sha256
        installedProduct = $products
        msiRegistration = $msiRegistration
        otherInstalledProductCatalogCount = $otherProductCatalogRows.Count
        otherInstalledProductCatalogSha256 = Get-G04DCCanonicalHash -Rows $otherProductCatalogRows
        installedProductCatalogCount = $productCatalogRows.Count
        installedProductCatalogSha256 = Get-G04DCCanonicalHash -Rows $productCatalogRows
        installerCacheCatalogCount = $installerCacheCatalog.rowCount
        installerCacheCatalogSha256 = $installerCacheCatalog.sha256
        pendingReboot = $pendingReboot
        ordinaryProfile = [pscustomobject][ordered]@{
            roaming = Join-Path $env:APPDATA 'LibreOffice'
            roamingPresent = Test-Path -LiteralPath (Join-Path $env:APPDATA 'LibreOffice')
            local = Join-Path $env:LOCALAPPDATA 'LibreOffice'
            localPresent = Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA 'LibreOffice')
        }
        libreOfficeProcesses = $processes
    }
}

function Compare-G04DCMachineState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] $Before,
        [Parameter(Mandatory = $true)] $After,
        [switch]$IncludeInstalledProductCatalog,
        [switch]$IncludeAcceptedMsiRegistration,
        [switch]$IncludeInstallerCacheCatalog
    )
    $boundaries = @(
        'fontCatalogCount', 'fontCatalogSha256', 'msiFontTargets', 'externalRuntimeTargets', 'associations', 'libreOfficeServices', 'serviceCatalogCount', 'serviceCatalogSha256', 'serviceRegistryCatalogCount', 'serviceRegistryCatalogSha256',
        'appPaths', 'appPathCatalogCount', 'appPathCatalogSha256', 'libreOfficeProgIds', 'classKeyCatalogCount', 'classKeyCatalogSha256',
        'classRegistryCatalogCount', 'classRegistryCatalogSha256',
        'msiProtectedRegistryTargets', 'scheduledTasks', 'scheduledTaskCatalogCount', 'scheduledTaskCatalogSha256',
        'startup', 'startupCatalogCount', 'startupCatalogSha256', 'shortcutCatalogCount', 'shortcutCatalogSha256', 'firewallRules', 'firewallCatalogCount', 'firewallCatalogSha256',
        'firewallFilterCatalogCount', 'firewallFilterCatalogSha256',
        'machinePath', 'userPath', 'environmentCatalogCount', 'environmentCatalogSha256', 'pendingReboot', 'ordinaryProfile', 'otherInstalledProductCatalogCount', 'otherInstalledProductCatalogSha256'
    )
    if ($IncludeInstalledProductCatalog) {
        $boundaries += @('installedProductCatalogCount', 'installedProductCatalogSha256')
    }
    if ($IncludeAcceptedMsiRegistration) { $boundaries += 'msiRegistration' }
    if ($IncludeInstallerCacheCatalog) { $boundaries += @('installerCacheCatalogCount', 'installerCacheCatalogSha256') }
    $changes = [System.Collections.Generic.List[object]]::new()
    foreach ($boundary in $boundaries) {
        $beforeValue = $Before.$boundary | ConvertTo-Json -Compress -Depth 12
        $afterValue = $After.$boundary | ConvertTo-Json -Compress -Depth 12
        if ($beforeValue -cne $afterValue) {
            $changes.Add([pscustomobject][ordered]@{ boundary = $boundary; before = $Before.$boundary; after = $After.$boundary })
        }
    }
    return [pscustomobject][ordered]@{
        protectedMutation = $changes.Count -ne 0
        changes = @($changes.ToArray())
    }
}

function Assert-G04DCNonMutation {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] $Comparison, [string]$Code = 'PROTECTED_HOST_MUTATION')
    if ([bool]$Comparison.protectedMutation) {
        throw "[$Code] Protected machine boundaries changed: $(@($Comparison.changes | ForEach-Object { $_.boundary }) -join ', ')"
    }
    return $true
}

function Assert-G04DCRunnerIsolation {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] $State, [string]$Code = 'PREEXISTING_RUNTIME_STATE')
    if ([bool]$State.ordinaryProfile.roamingPresent -or [bool]$State.ordinaryProfile.localPresent) {
        throw "[$Code] Disposable runner has or created an ordinary LibreOffice profile."
    }
    if (@($State.libreOfficeProcesses).Count -ne 0) {
        throw "[$Code] Disposable runner has a LibreOffice/soffice process."
    }
    return $true
}

function Assert-G04DCExternalRuntimeDependencies {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] $State)
    $targets = @($State.externalRuntimeTargets)
    if ($targets.Count -eq 0) { throw '[EXTERNAL_RUNTIME_DEPENDENCY_INVALID] No exact external VC runtime targets were derived.' }
    $invalid = @($targets | Where-Object {
        ![bool]$_.present -or ![bool]$_.regularFile -or [bool]$_.reparsePoint -or
        [string]$_.authenticodeStatus -cne 'Valid' -or ![bool]$_.signerChainValid -or @($_.signerChain).Count -lt 2 -or
        [string]$_.signer -notmatch '(^|,\s*)O=Microsoft Corporation(,|$)'
    })
    if ($invalid.Count -ne 0) {
        throw "[EXTERNAL_RUNTIME_DEPENDENCY_INVALID] $($invalid.Count) pre-existing VC runtime targets are absent, noncanonical or not Microsoft-signed."
    }
    return $true
}

function Assert-G04DCProcessEvidence {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] $Evidence, [Parameter(Mandatory = $true)] [string]$RuntimeRoot)
    if (![bool]$Evidence.appContainer) { throw '[APPCONTAINER_FAILURE] Process token is not AppContainer.' }
    if (@($Evidence.capabilities).Count -ne 0) { throw '[APPCONTAINER_FAILURE] Office proof token has capabilities.' }
    if (![bool]$Evidence.assignedBeforeResume) { throw '[APPCONTAINER_FAILURE] Process was not assigned to the Job Object before resume.' }
    if (![bool]$Evidence.profileDeleted) { throw '[APPCONTAINER_FAILURE] Proof-only AppContainer profile was not deleted.' }
    if ([bool]$Evidence.breakawayAllowed) { throw '[NO_BREAKAWAY_FAILURE] Job Object permits breakaway.' }
    if ([int]$Evidence.peakAssignedProcessCount -gt [int]$Evidence.activeProcessLimit) { throw '[APPCONTAINER_FAILURE] Owned process count exceeded the Job Object limit.' }
    if ([long]$Evidence.peakJobMemoryBytes -gt [long]$Evidence.aggregateMemoryLimitBytes) { throw '[APPCONTAINER_FAILURE] Owned process tree exceeded aggregate memory limit.' }
    if (@($Evidence.networkConnections).Count -ne 0) { throw '[NETWORK_ATTEMPT] Owned process tree attempted a network connection.' }
    if ([bool]$Evidence.loopbackExemptBefore -or [bool]$Evidence.loopbackExemptAfter) { throw '[NETWORK_ATTEMPT] Proof AppContainer SID has a loopback exemption.' }
    $rootPath = [System.IO.Path]::GetFullPath($RuntimeRoot).TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
    $rootPrefix = $rootPath + [System.IO.Path]::DirectorySeparatorChar
    $ownedProcesses = @($Evidence.processes)
    if ($ownedProcesses.Count -eq 0) { throw '[UNEXPECTED_PROCESS_DESCENDANT] Owned process tree evidence is empty.' }
    if ([int]$Evidence.totalAssignedProcesses -ne $ownedProcesses.Count) {
        throw "[UNEXPECTED_PROCESS_DESCENDANT] Job assigned $([int]$Evidence.totalAssignedProcesses) processes but only $($ownedProcesses.Count) identities were resolved."
    }
    $unexpected = @($ownedProcesses | Where-Object {
        if ([string]::IsNullOrWhiteSpace([string]$_.path)) { return $true }
        try {
            $processPath = [System.IO.Path]::GetFullPath([string]$_.path)
            return !$processPath.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase)
        }
        catch { return $true }
    })
    if ($unexpected.Count -ne 0) { throw '[UNEXPECTED_PROCESS_DESCENDANT] Owned tree contains an unresolved process or a process outside the candidate runtime root.' }
    if (![bool]$Evidence.moduleInventoryComplete) { throw '[RUNTIME_IDENTITY_INVALID] Owned process module inventory is incomplete.' }
    $loadedModulePaths = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($module in @($Evidence.loadedModules)) {
        if ([string]::IsNullOrWhiteSpace([string]$module.path)) { throw '[RUNTIME_IDENTITY_INVALID] Loaded module has no resolved path.' }
        [void]$loadedModulePaths.Add([IO.Path]::GetFullPath([string]$module.path))
    }
    if (@($ownedProcesses | Where-Object {
        if ([string]::IsNullOrWhiteSpace([string]$_.path)) { return $true }
        try { return !$loadedModulePaths.Contains([IO.Path]::GetFullPath([string]$_.path)) }
        catch { return $true }
    }).Count -ne 0) {
        throw '[RUNTIME_IDENTITY_INVALID] One or more owned process entry modules were not observed.'
    }
    if (![bool]$Evidence.unrelatedProcessSurvived) { throw '[UNRELATED_PROCESS_TERMINATED] Owned-tree cleanup terminated the unrelated canary.' }
    return $true
}

function Get-G04DCLoadBearingModuleEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [object[]]$ProcessEvidence,
        [Parameter(Mandatory = $true)] [string]$RuntimeRoot,
        [Parameter(Mandatory = $true)] [string]$WindowsRoot
    )
    $runtimeCanonical = [IO.Path]::GetFullPath($RuntimeRoot).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
    $runtimePrefix = $runtimeCanonical + [IO.Path]::DirectorySeparatorChar
    $windowsCanonical = [IO.Path]::GetFullPath($WindowsRoot).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
    $systemRoots = @('System32', 'SysWOW64', 'WinSxS') | ForEach-Object {
        [IO.Path]::GetFullPath((Join-Path $windowsCanonical $_)).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    }
    $modulesByPath = @{}
    foreach ($probe in $ProcessEvidence) {
        if (![bool]$probe.moduleInventoryComplete) { throw '[RUNTIME_IDENTITY_INVALID] A sandbox probe has incomplete loaded-module evidence.' }
        foreach ($module in @($probe.loadedModules)) {
            if ([string]::IsNullOrWhiteSpace([string]$module.path)) { throw '[RUNTIME_IDENTITY_INVALID] A loaded module has no resolved path.' }
            $canonical = [IO.Path]::GetFullPath([string]$module.path)
            if (!$modulesByPath.ContainsKey($canonical)) { $modulesByPath[$canonical] = [System.Collections.Generic.List[int]]::new() }
            $modulesByPath[$canonical].Add([int]$module.pid)
        }
    }
    $records = [System.Collections.Generic.List[object]]::new()
    foreach ($path in @($modulesByPath.Keys | Sort-Object)) {
        $item = Get-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
        $rootClass = if ($path.StartsWith($runtimePrefix, [StringComparison]::OrdinalIgnoreCase)) {
            'candidate-runtime'
        } elseif (@($systemRoots | Where-Object { $path.StartsWith($_, [StringComparison]::OrdinalIgnoreCase) }).Count -ne 0) {
            'windows-system'
        } else { 'rejected' }
        $signature = if ($item -and !$item.PSIsContainer) { Get-G04DCAuthenticodeEvidence -Path $item.FullName } else { $null }
        $acceptedSigner = if ($rootClass -ceq 'candidate-runtime') {
            $signature -and [string]$signature.signerThumbprint -ceq $script:G04DCExpectedMsi.SignerThumbprint
        } elseif ($rootClass -ceq 'windows-system') {
            $signature -and [string]$signature.signerSubject -match '(^|,\s*)O=Microsoft Corporation(,|$)'
        } else { $false }
        $records.Add([pscustomobject][ordered]@{
            path = $path
            pids = @($modulesByPath[$path].ToArray() | Sort-Object -Unique)
            rootClass = $rootClass
            present = [bool]$item
            regularFile = [bool]($item -and !$item.PSIsContainer)
            reparsePoint = if ($item) { [bool]($item.Attributes -band [IO.FileAttributes]::ReparsePoint) } else { $false }
            sizeBytes = if ($item -and !$item.PSIsContainer) { [long]$item.Length } else { 0 }
            sha256 = if ($item -and !$item.PSIsContainer) { Get-G04DCSha256 -Path $item.FullName } else { $null }
            authenticode = $signature
            acceptedSigner = [bool]$acceptedSigner
            accepted = [bool]($item -and !$item.PSIsContainer -and ![bool]($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -and
                $rootClass -cne 'rejected' -and $signature -and [string]$signature.status -ceq 'Valid' -and [bool]$signature.chainValid -and
                @($signature.chain).Count -ge 2 -and $acceptedSigner)
        })
    }
    return [pscustomobject][ordered]@{
        schemaVersion = 1
        policy = 'Every dynamically loaded module must be a regular non-reparse file beneath the exact candidate runtime or canonical Windows System32/SysWOW64/WinSxS roots, with a valid chain; runtime modules require the exact accepted TDF leaf thumbprint and Windows modules require an exact Microsoft Corporation organization RDN.'
        runtimeRoot = $runtimeCanonical
        windowsSystemRoots = $systemRoots
        modules = @($records.ToArray())
        passed = $records.Count -ne 0 -and @($records | Where-Object { ![bool]$_.accepted }).Count -eq 0
    }
}

function Assert-G04DCLoadBearingModuleEvidence {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] $Evidence)
    if (![bool]$Evidence.passed -or @($Evidence.modules).Count -eq 0 -or @($Evidence.modules | Where-Object { ![bool]$_.accepted }).Count -ne 0) {
        throw '[RUNTIME_IDENTITY_INVALID] Dynamically loaded module provenance is incomplete or outside the exact root/signature policy.'
    }
    return $true
}

function Assert-G04DCOutputEvidence {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] $Evidence)
    if (![bool]$Evidence.regularFile -or [bool]$Evidence.reparsePoint -or [long]$Evidence.sizeBytes -le 8) {
        throw '[OUTPUT_MISSING_OR_CORRUPT] Output file identity is invalid.'
    }
    if ([string]$Evidence.magic -cne '%PDF-') { throw '[OUTPUT_MISSING_OR_CORRUPT] PDF magic is absent.' }
    if (![bool]$Evidence.qpdfStrict -or [bool]$Evidence.encrypted -or ![bool]$Evidence.pdfjsOpened -or [int]$Evidence.pageCount -lt 1) {
        throw '[OUTPUT_MISSING_OR_CORRUPT] qpdf/PDF.js verification failed.'
    }
    return $true
}

function Assert-G04DCFileAccessEvidence {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] $Evidence)
    if (![bool]$Evidence.aclGrantSetExact -or ![bool]$Evidence.ownedWritablePathInventoriesCaptured) {
        throw '[FILE_ACCESS_BOUNDARY_INVALID] AppContainer ACL or owned writable-path inventory is not exact.'
    }
    if (![bool]$Evidence.actualAccessTelemetryCaptured -or ![bool]$Evidence.effectiveDenialOutsideAllowedRootsProven) {
        throw '[FILE_ACCESS_BOUNDARY_INVALID] Actual file-access telemetry and effective outside-root denial were not proven.'
    }
    if (![bool]$Evidence.appContainerExternalStorageAbsent -or ![bool]$Evidence.appContainerRegistryResidueAbsent) {
        throw '[FILE_ACCESS_BOUNDARY_INVALID] AppContainer package storage or registry residue remains after profile deletion.'
    }
    if (![bool]$Evidence.runtimeTreeUnchanged -or ![bool]$Evidence.fixtureUnchanged) {
        throw '[FILE_ACCESS_BOUNDARY_INVALID] Runtime or fixture changed during the sandbox probes.'
    }
    if (@($Evidence.probes).Count -ne 2 -or @($Evidence.probes | Where-Object { ![bool]$_.captured }).Count -ne 0) {
        throw '[FILE_ACCESS_BOUNDARY_INVALID] Version and conversion file-access observations are incomplete.'
    }
    return $true
}

function Remove-G04DCOwnedRoot {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [string]$OwnedRoot,
        [Parameter(Mandatory = $true)] [string]$MarkerPath,
        [Parameter(Mandatory = $true)] [string]$MarkerContent,
        [Parameter(Mandatory = $true)] [string]$RequiredParent
    )
    $parentCanonical = [IO.Path]::GetFullPath($RequiredParent).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
    $ownedCanonical = [IO.Path]::GetFullPath($OwnedRoot)
    $ownedItem = Get-Item -LiteralPath $ownedCanonical -Force -ErrorAction SilentlyContinue
    $markerItem = Get-Item -LiteralPath $MarkerPath -Force -ErrorAction SilentlyContinue
    $reparseEntries = @()
    $inspectionError = $null
    if ($ownedItem -and $ownedItem.PSIsContainer -and ![bool]($ownedItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        try { $reparseEntries = @(Get-ChildItem -LiteralPath $ownedCanonical -Recurse -Force -ErrorAction Stop | Where-Object { [bool]($_.Attributes -band [IO.FileAttributes]::ReparsePoint) }) }
        catch { $inspectionError = $_.Exception.GetType().FullName }
    }
    $markerContentMatches = $false
    $markerSha256 = $null
    if ($markerItem -and !$markerItem.PSIsContainer -and ![bool]($markerItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        try {
            $markerContentMatches = [IO.File]::ReadAllText($markerItem.FullName, [Text.Encoding]::UTF8) -ceq $MarkerContent
            $markerSha256 = Get-G04DCSha256 -Path $markerItem.FullName
        }
        catch { $inspectionError = $_.Exception.GetType().FullName }
    }
    $owned = $ownedCanonical.StartsWith($parentCanonical + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -and
        $ownedItem -and $ownedItem.PSIsContainer -and ![bool]($ownedItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -and
        $markerItem -and !$markerItem.PSIsContainer -and ![bool]($markerItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -and
        $markerContentMatches -and !$inspectionError -and $reparseEntries.Count -eq 0
    $removalError = $null
    $removed = $false
    if ($owned) {
        try {
            Remove-Item -LiteralPath $ownedCanonical -Recurse -Force -ErrorAction Stop
            $removed = !(Test-Path -LiteralPath $ownedCanonical)
        }
        catch { $removalError = $_.Exception.GetType().FullName }
    }
    return [pscustomobject][ordered]@{
        markerOwnedPathsOnly = [bool]$owned
        ownedRoot = $ownedCanonical
        markerPath = $MarkerPath
        markerSha256 = $markerSha256
        reparseEntryCount = $reparseEntries.Count
        inspectionError = $inspectionError
        removalError = $removalError
        unrelatedProcessSurvived = $true
        removed = $removed
    }
}

function Assert-G04DCCleanupEvidence {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] $Evidence)
    if (![bool]$Evidence.markerOwnedPathsOnly -or ![bool]$Evidence.unrelatedProcessSurvived) {
        throw '[CLEANUP_OWNERSHIP_MISMATCH] Cleanup escaped owned paths or killed an unrelated process.'
    }
    return $true
}

function New-G04DCArtifactManifest {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [string]$EvidenceDirectory,
        [string[]]$ExcludedRelativePaths = @('runtime', 'download')
    )
    $root = (Resolve-Path -LiteralPath $EvidenceDirectory).Path
    $files = @(Get-ChildItem -LiteralPath $root -File -Recurse -Force | Where-Object {
        $relative = $_.FullName.Substring($root.Length + 1).Replace('\', '/')
        @($ExcludedRelativePaths | Where-Object { $relative -eq $_ -or $relative.StartsWith($_ + '/', [StringComparison]::Ordinal) }).Count -eq 0 -and
        $relative -ne 'artifact-manifest.json'
    } | Sort-Object FullName | ForEach-Object {
        $relative = $_.FullName.Substring($root.Length + 1).Replace('\', '/')
        [pscustomobject][ordered]@{ path = $relative; sizeBytes = [long]$_.Length; sha256 = Get-G04DCSha256 -Path $_.FullName }
    })
    $manifest = [pscustomobject][ordered]@{ schemaVersion = 1; files = $files }
    Write-G04DCJson -Path (Join-Path $root 'artifact-manifest.json') -Value $manifest
    return $manifest
}

function Assert-G04DCArtifactManifest {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] [string]$EvidenceDirectory)
    $root = (Resolve-Path -LiteralPath $EvidenceDirectory).Path
    $manifestPath = Join-Path $root 'artifact-manifest.json'
    if (!(Test-Path -LiteralPath $manifestPath -PathType Leaf)) { throw '[ARTIFACT_MANIFEST_INVALID] Evidence manifest is missing.' }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ([int]$manifest.schemaVersion -ne 1 -or @($manifest.files).Count -eq 0) { throw '[ARTIFACT_MANIFEST_INVALID] Evidence manifest schema or file list is invalid.' }
    $declared = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($file in @($manifest.files)) {
        $relative = ([string]$file.path).Replace('\', '/')
        if ([string]::IsNullOrWhiteSpace($relative) -or $relative.StartsWith('/', [StringComparison]::Ordinal) -or $relative -match '(^|/)\.\.(/|$)' -or !$declared.Add($relative)) {
            throw '[ARTIFACT_MANIFEST_INVALID] Evidence manifest contains an unsafe or duplicate path.'
        }
        $candidatePath = [IO.Path]::GetFullPath((Join-Path $root $relative.Replace('/', [IO.Path]::DirectorySeparatorChar)))
        $rootPrefix = $root.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
        if (!$candidatePath.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase)) { throw '[ARTIFACT_MANIFEST_INVALID] Evidence manifest path escapes its artifact root.' }
        $item = Get-Item -LiteralPath $candidatePath -Force -ErrorAction SilentlyContinue
        if (!$item -or $item.PSIsContainer -or [bool]($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or [long]$item.Length -ne [long]$file.sizeBytes -or (Get-G04DCSha256 -Path $item.FullName) -cne [string]$file.sha256) {
            throw "[ARTIFACT_MANIFEST_INVALID] Evidence file failed identity verification: $relative"
        }
    }
    $actual = @(Get-ChildItem -LiteralPath $root -File -Recurse -Force | ForEach-Object { $_.FullName.Substring($root.Length + 1).Replace('\', '/') } | Where-Object { $_ -cne 'artifact-manifest.json' })
    if (@($actual | Where-Object { !$declared.Contains($_) }).Count -ne 0 -or @($declared | Where-Object { $actual -cnotcontains $_ }).Count -ne 0) {
        throw '[ARTIFACT_MANIFEST_INVALID] Evidence manifest does not exactly cover the downloaded bounded artifact.'
    }
    return $manifest
}

Export-ModuleMember -Function @(
    'Get-G04DCExpectedMsi', 'Write-G04DCJson', 'Get-G04DCSha256', 'Get-G04DCAuthenticodeEvidence',
    'Test-G04DCRestrictedIpAddress', 'Assert-G04DCAcquisitionUri', 'Assert-G04DCPinnedRemoteEndpoint',
    'Resolve-G04DCRedirectTransition', 'Assert-G04DCRedirectChainEvidence',
    'Assert-G04DCBoundedDownloadLength', 'Copy-G04DCBoundedHttpsBody', 'Assert-G04DCFailedDownloadCleanup',
    'Get-G04DCCanonicalHash', 'Get-G04DCMsiIdentity', 'Assert-G04DCMsiIdentity',
    'Invoke-G04DCAcquireMsi', 'Export-G04DCMsiDatabase', 'Get-G04DCFeatureAnalysis',
    'Assert-G04DCFeatureAnalysis', 'Get-G04DCInstalledFeatureStates', 'Assert-G04DCInstalledFeatureStates',
    'Get-G04DCInstalledComponentStates', 'Resolve-G04DCExpectedComponentStates', 'Assert-G04DCInstalledComponentStates',
    'Get-G04DCMutationClosure', 'Assert-G04DCMinimalMutationClosure', 'Assert-G04DCAdminMutationClosure',
    'Assert-G04DCInstalledFileOwnership',
    'Get-G04DCExternalRuntimeTargetPaths',
    'Get-G04DCRegistryValueState', 'Get-G04DCRegistryDefaultValueState',
    'ConvertTo-G04DCPackedGuid', 'Get-G04DCMsiRegistrationState', 'Assert-G04DCMsiRegistrationAbsent', 'Assert-G04DCMsiRegistrationInstalled',
    'Get-G04DCMachineState', 'Compare-G04DCMachineState',
    'Assert-G04DCNonMutation', 'Assert-G04DCRunnerIsolation', 'Assert-G04DCExternalRuntimeDependencies',
    'Assert-G04DCProcessEvidence', 'Get-G04DCLoadBearingModuleEvidence', 'Assert-G04DCLoadBearingModuleEvidence', 'Assert-G04DCOutputEvidence',
    'Assert-G04DCFileAccessEvidence',
    'Remove-G04DCOwnedRoot', 'Assert-G04DCCleanupEvidence', 'New-G04DCArtifactManifest', 'Assert-G04DCArtifactManifest'
)
