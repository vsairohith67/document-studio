Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (!('DocumentStudio.G04DC.KillOnCloseJob' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;

namespace DocumentStudio.G04DC {
    public sealed class KillOnCloseJob : IDisposable {
        [StructLayout(LayoutKind.Sequential)]
        private struct BasicLimitInformation {
            public long PerProcessUserTimeLimit;
            public long PerJobUserTimeLimit;
            public uint LimitFlags;
            public UIntPtr MinimumWorkingSetSize;
            public UIntPtr MaximumWorkingSetSize;
            public uint ActiveProcessLimit;
            public UIntPtr Affinity;
            public uint PriorityClass;
            public uint SchedulingClass;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct IoCounters {
            public ulong ReadOperationCount;
            public ulong WriteOperationCount;
            public ulong OtherOperationCount;
            public ulong ReadTransferCount;
            public ulong WriteTransferCount;
            public ulong OtherTransferCount;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct ExtendedLimitInformation {
            public BasicLimitInformation BasicLimitInformation;
            public IoCounters IoInfo;
            public UIntPtr ProcessMemoryLimit;
            public UIntPtr JobMemoryLimit;
            public UIntPtr PeakProcessMemoryUsed;
            public UIntPtr PeakJobMemoryUsed;
        }

        private const uint KillOnJobClose = 0x00002000;
        private IntPtr handle;

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern IntPtr CreateJobObject(IntPtr attributes, string name);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool SetInformationJobObject(IntPtr job, int informationClass, IntPtr information, uint informationLength);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool TerminateJobObject(IntPtr job, uint exitCode);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool CloseHandle(IntPtr handle);

        public KillOnCloseJob() {
            handle = CreateJobObject(IntPtr.Zero, null);
            if (handle == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error(), "CreateJobObject");
            ExtendedLimitInformation limits = new ExtendedLimitInformation();
            limits.BasicLimitInformation.LimitFlags = KillOnJobClose;
            IntPtr buffer = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(ExtendedLimitInformation)));
            try {
                Marshal.StructureToPtr(limits, buffer, false);
                if (!SetInformationJobObject(handle, 9, buffer, (uint)Marshal.SizeOf(typeof(ExtendedLimitInformation)))) {
                    throw new Win32Exception(Marshal.GetLastWin32Error(), "SetInformationJobObject");
                }
            }
            catch {
                CloseHandle(handle);
                handle = IntPtr.Zero;
                throw;
            }
            finally { Marshal.FreeHGlobal(buffer); }
        }

        public void Assign(Process process) {
            if (process == null) throw new ArgumentNullException("process");
            if (process.HasExited) return;
            if (!AssignProcessToJobObject(handle, process.Handle) && !process.HasExited) {
                throw new Win32Exception(Marshal.GetLastWin32Error(), "AssignProcessToJobObject");
            }
        }

        public void TerminateAndVerify(Process process, int waitMilliseconds) {
            if (process == null || process.HasExited) return;
            if (!TerminateJobObject(handle, 0xD5040003) && !process.HasExited) {
                throw new Win32Exception(Marshal.GetLastWin32Error(), "TerminateJobObject");
            }
            if (!process.WaitForExit(waitMilliseconds) || !process.HasExited) {
                throw new InvalidOperationException("Owned helper did not reach terminal exit.");
            }
        }

        public void Dispose() {
            if (handle != IntPtr.Zero) {
                if (!CloseHandle(handle)) throw new Win32Exception(Marshal.GetLastWin32Error(), "CloseHandle(job)");
                handle = IntPtr.Zero;
            }
        }
    }
}
'@
}

if (!('DocumentStudio.G04DC.ClassRegistryDigestCollector' -as [type])) {
    Add-Type -Path (Join-Path $PSScriptRoot 'ClassRegistryDigest.cs') -ErrorAction Stop
}

$script:G04DCCommonModulePath = $PSCommandPath

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
    param(
        [Parameter(Mandatory = $true)] [AllowEmptyCollection()] [object[]]$Rows,
        [Parameter(DontShow = $true)] [AllowNull()] $CaptureContext,
        [Parameter(DontShow = $true)] [string]$CapturePhase = 'canonical-hash'
    )
    if (!$CaptureContext) {
        $canonical = (($Rows | ForEach-Object { $_ | ConvertTo-Json -Compress -Depth 12 }) | Sort-Object) -join "`n"
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($canonical)
        $sha = [System.Security.Cryptography.SHA256]::Create()
        try { return ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant() }
        finally { $sha.Dispose() }
    }

    Assert-G04DCMachineStateCaptureBudget -Context $CaptureContext -Phase $CapturePhase -ItemCount 0
    if ($Rows.Count -gt 1000000) { throw '[MACHINE_STATE_CAPTURE_FAILED] Canonical hash exceeded the bounded row ceiling.' }
    $temporaryParent = if (![string]::IsNullOrWhiteSpace($env:RUNNER_TEMP) -and (Test-Path -LiteralPath $env:RUNNER_TEMP -PathType Container)) {
        [IO.Path]::GetFullPath($env:RUNNER_TEMP)
    }
    else { [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') }
    $ownedRoot = Join-Path $temporaryParent ('document-studio-g04dc-canonical-hash-' + [guid]::NewGuid().ToString('N'))
    $markerPath = Join-Path $ownedRoot '.g04d-c-owned-canonical-hash'
    $inputPath = Join-Path $ownedRoot 'rows.ndjson'
    $markerText = "DOCUMENT-STUDIO-G04DC-CANONICAL-HASH-OWNED`n"
    $ownedRootCreated = $false
    try {
        [IO.Directory]::CreateDirectory($ownedRoot) | Out-Null
        $ownedRootCreated = $true
        [IO.File]::WriteAllText($markerPath, $markerText, [Text.UTF8Encoding]::new($false))
        $stream = [IO.FileStream]::new($inputPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
        $writer = $null
        try {
            $writer = [IO.StreamWriter]::new($stream, [Text.UTF8Encoding]::new($false))
            try {
                $totalInputBytes = 0L
                for ($index = 0; $index -lt $Rows.Count; $index++) {
                    Assert-G04DCMachineStateCaptureBudget -Context $CaptureContext -Phase $CapturePhase -ItemCount $index
                    $line = $Rows[$index] | ConvertTo-Json -Compress -Depth 12
                    $lineBytes = [Text.Encoding]::UTF8.GetByteCount($line)
                    if ($lineBytes -gt 16777216) { throw '[MACHINE_STATE_CAPTURE_FAILED] Canonical hash row exceeded the 16 MiB ceiling.' }
                    $totalInputBytes += $lineBytes + 2L
                    if ($totalInputBytes -gt 134217728) { throw '[MACHINE_STATE_CAPTURE_FAILED] Canonical hash input exceeded the 128 MiB ceiling.' }
                    $writer.WriteLine($line)
                    if (($index + 1) % 128 -eq 0 -or $index + 1 -eq $Rows.Count) {
                        Assert-G04DCMachineStateCaptureBudget -Context $CaptureContext -Phase $CapturePhase -ItemCount ($index + 1) -WriteProgress
                    }
                }
                $writer.Flush()
                $stream.Flush($true)
            }
            finally { if ($writer) { $writer.Dispose() } }
        }
        catch { $stream.Dispose(); throw }

        $hashEvidence = @(Invoke-G04DCBoundedCaptureProcess -Context $CaptureContext -Phase $CapturePhase -ScriptBlock {
            param([string]$InputPath, [string]$MarkerPath, [string]$ExpectedMarker)
            Set-StrictMode -Version Latest
            $ErrorActionPreference = 'Stop'
            $inputItem = Get-Item -LiteralPath $InputPath -Force -ErrorAction Stop
            $markerItem = Get-Item -LiteralPath $MarkerPath -Force -ErrorAction Stop
            if ($inputItem.PSIsContainer -or $markerItem.PSIsContainer -or
                [bool]($inputItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
                [bool]($markerItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
                $inputItem.DirectoryName -cne $markerItem.DirectoryName -or
                [long]$inputItem.Length -gt 134217728 -or
                [IO.File]::ReadAllText($MarkerPath, [Text.UTF8Encoding]::new($false)) -cne $ExpectedMarker) {
                throw '[MACHINE_STATE_CAPTURE_FAILED] Canonical hash helper input is not marker-owned or bounded.'
            }
            $sortedRows = @(Get-Content -LiteralPath $InputPath -Encoding UTF8 | Sort-Object)
            $sha = [Security.Cryptography.SHA256]::Create()
            try {
                $newline = [byte[]]@(10)
                for ($index = 0; $index -lt $sortedRows.Count; $index++) {
                    if ($index -ne 0) { [void]$sha.TransformBlock($newline, 0, 1, $newline, 0) }
                    $bytes = [Text.Encoding]::UTF8.GetBytes([string]$sortedRows[$index])
                    if ($bytes.Length -ne 0) { [void]$sha.TransformBlock($bytes, 0, $bytes.Length, $bytes, 0) }
                }
                [void]$sha.TransformFinalBlock([byte[]]::new(0), 0, 0)
                [pscustomobject][ordered]@{
                    rowCount = $sortedRows.Count
                    sha256 = ([BitConverter]::ToString($sha.Hash)).Replace('-', '').ToLowerInvariant()
                }
            }
            finally { $sha.Dispose() }
        } -ArgumentList @($inputPath, $markerPath, $markerText) -ItemCount $Rows.Count)
        if ($hashEvidence.Count -ne 1 -or [long]$hashEvidence[0].rowCount -ne $Rows.Count -or [string]$hashEvidence[0].sha256 -notmatch '^[0-9a-f]{64}$') {
            throw '[MACHINE_STATE_CAPTURE_FAILED] Canonical hash helper returned invalid evidence.'
        }
        Assert-G04DCMachineStateCaptureBudget -Context $CaptureContext -Phase $CapturePhase -ItemCount $Rows.Count
        return [string]$hashEvidence[0].sha256
    }
    finally {
        if ($ownedRootCreated) {
            try {
                $rootItem = Get-Item -LiteralPath $ownedRoot -Force -ErrorAction Stop
                $markerItem = Get-Item -LiteralPath $markerPath -Force -ErrorAction Stop
                if (!$rootItem.PSIsContainer -or [bool]($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
                    $markerItem.PSIsContainer -or [bool]($markerItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
                    [IO.File]::ReadAllText($markerPath, [Text.UTF8Encoding]::new($false)) -cne $markerText) {
                    throw 'canonical hash ownership mismatch'
                }
                foreach ($knownPath in @($inputPath, $markerPath)) {
                    if (Test-Path -LiteralPath $knownPath) {
                        $knownItem = Get-Item -LiteralPath $knownPath -Force
                        if ($knownItem.PSIsContainer -or [bool]($knownItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'noncanonical hash entry' }
                        [IO.File]::Delete($knownPath)
                    }
                }
                [IO.Directory]::Delete($ownedRoot, $false)
            }
            catch { throw "[MACHINE_STATE_CAPTURE_HELPER_CLEANUP_FAILED] Canonical hash helper path cleanup failed ($($_.Exception.GetType().FullName))." }
        }
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

function New-G04DCMachineStateCaptureContext {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [ValidatePattern('^[a-z0-9][a-z0-9-]{0,63}$')] [string]$CaptureLabel,
        [AllowNull()] [string]$ProgressPath,
        [AllowNull()] [string]$PerformancePath,
        [ValidateRange(1, 3600000)] [long]$CaptureTargetMilliseconds = 480000,
        [ValidateRange(1, 3600000)] [long]$OverallBudgetMilliseconds = 720000,
        [ValidateRange(1, 3600000)] [long]$PhaseBudgetMilliseconds = 240000
    )
    if ($CaptureTargetMilliseconds -gt $OverallBudgetMilliseconds -or $PhaseBudgetMilliseconds -gt $OverallBudgetMilliseconds) {
        throw '[MACHINE_STATE_CAPTURE_CONFIGURATION_INVALID] Capture target or phase ceiling exceeds the overall ceiling.'
    }
    $writer = $null
    if (![string]::IsNullOrWhiteSpace($ProgressPath)) {
        $progressCanonical = [IO.Path]::GetFullPath($ProgressPath)
        $parent = Split-Path -Parent $progressCanonical
        if (!(Test-Path -LiteralPath $parent -PathType Container)) {
            throw '[MACHINE_STATE_CAPTURE_CONFIGURATION_INVALID] Progress evidence parent directory is absent.'
        }
        $stream = [IO.FileStream]::new($progressCanonical, [IO.FileMode]::Append, [IO.FileAccess]::Write, [IO.FileShare]::Read)
        try { $writer = [IO.StreamWriter]::new($stream, [Text.UTF8Encoding]::new($false)) }
        catch { $stream.Dispose(); throw }
    }
    $stopwatch = [Diagnostics.Stopwatch]::StartNew()
    return [pscustomobject][ordered]@{
        schemaVersion = 1
        captureId = [guid]::NewGuid().ToString('D')
        captureLabel = $CaptureLabel
        progressPath = if ([string]::IsNullOrWhiteSpace($ProgressPath)) { $null } else { [IO.Path]::GetFullPath($ProgressPath) }
        performancePath = if ([string]::IsNullOrWhiteSpace($PerformancePath)) { $null } else { [IO.Path]::GetFullPath($PerformancePath) }
        captureTargetMilliseconds = $CaptureTargetMilliseconds
        overallBudgetMilliseconds = $OverallBudgetMilliseconds
        phaseBudgetMilliseconds = $PhaseBudgetMilliseconds
        stopwatch = $stopwatch
        writer = $writer
        sequence = 0L
        activePhase = $null
        activePhaseStartMilliseconds = 0L
        activeItemCount = 0L
        activeSubstage = $null
        activeSubstageStartMilliseconds = 0L
        activeSubstageRowCount = 0L
        activeSubstageRawByteCount = 0L
        activeSubstages = [System.Collections.Generic.List[object]]::new()
        phases = [System.Collections.Generic.List[object]]::new()
        metrics = [ordered]@{}
        completed = $false
    }
}

function Write-G04DCMachineStateProgressRecord {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] $Context,
        [Parameter(Mandatory = $true)] [ValidateSet('phase-start', 'phase-progress', 'phase-end', 'capture-end', 'capture-failure')] [string]$Event,
        [AllowNull()] [string]$Phase,
        [Parameter(Mandatory = $true)] [ValidateSet('running', 'success', 'failed', 'budget-exceeded')] [string]$Status,
        [ValidateRange(0, [long]::MaxValue)] [long]$ItemCount = 0
    )
    if (!$Context.writer) { return }
    $Context.sequence = [long]$Context.sequence + 1
    $record = [ordered]@{
        schemaVersion = 1
        captureId = [string]$Context.captureId
        captureLabel = [string]$Context.captureLabel
        sequence = [long]$Context.sequence
        event = $Event
        phase = if ([string]::IsNullOrWhiteSpace($Phase)) { $null } else { $Phase }
        elapsedMilliseconds = [long]$Context.stopwatch.ElapsedMilliseconds
        itemCount = $ItemCount
        status = $Status
    }
    $line = $record | ConvertTo-Json -Compress -Depth 4
    $Context.writer.WriteLine($line)
    $Context.writer.Flush()
    $Context.writer.BaseStream.Flush($true)
}

function Write-G04DCMachineStateSubstageRecord {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] $Context,
        [Parameter(Mandatory = $true)] [ValidateSet('substage-start', 'substage-progress', 'substage-end')] [string]$Event,
        [Parameter(Mandatory = $true)] [ValidatePattern('^[a-z0-9][a-z0-9-]{0,63}$')] [string]$Substage,
        [Parameter(Mandatory = $true)] [ValidateSet('running', 'success', 'failed', 'budget-exceeded')] [string]$Status,
        [ValidateRange(0, [long]::MaxValue)] [long]$SubstageElapsedMilliseconds = 0,
        [ValidateRange(0, [long]::MaxValue)] [long]$RowCount = 0,
        [ValidateRange(0, [long]::MaxValue)] [long]$RawByteCount = 0
    )
    if (!$Context.writer) { return }
    $Context.sequence = [long]$Context.sequence + 1
    $record = [ordered]@{
        schemaVersion = 1
        captureId = [string]$Context.captureId
        captureLabel = [string]$Context.captureLabel
        sequence = [long]$Context.sequence
        event = $Event
        phase = [string]$Context.activePhase
        substage = $Substage
        elapsedMilliseconds = [long]$Context.stopwatch.ElapsedMilliseconds
        substageElapsedMilliseconds = $SubstageElapsedMilliseconds
        rowCount = $RowCount
        rawByteCount = $RawByteCount
        status = $Status
    }
    $line = $record | ConvertTo-Json -Compress -Depth 4
    $Context.writer.WriteLine($line)
    $Context.writer.Flush()
    $Context.writer.BaseStream.Flush($true)
}

function Start-G04DCMachineStateSubstage {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] $Context,
        [Parameter(Mandatory = $true)] [ValidatePattern('^[a-z0-9][a-z0-9-]{0,63}$')] [string]$Substage
    )
    if ([string]::IsNullOrWhiteSpace([string]$Context.activePhase) -or
        ![string]::IsNullOrWhiteSpace([string]$Context.activeSubstage)) {
        throw '[MACHINE_STATE_CAPTURE_CONFIGURATION_INVALID] Machine-state substage lifecycle is invalid.'
    }
    $Context.activeSubstage = $Substage
    $Context.activeSubstageStartMilliseconds = [long]$Context.stopwatch.ElapsedMilliseconds
    $Context.activeSubstageRowCount = 0L
    $Context.activeSubstageRawByteCount = 0L
    Write-G04DCMachineStateSubstageRecord -Context $Context -Event 'substage-start' -Substage $Substage -Status 'running'
}

function Write-G04DCMachineStateSubstageProgress {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] $Context,
        [Parameter(Mandatory = $true)] [string]$Substage,
        [ValidateRange(0, [long]::MaxValue)] [long]$RowCount = 0,
        [ValidateRange(0, [long]::MaxValue)] [long]$RawByteCount = 0
    )
    if ([string]$Context.activeSubstage -cne $Substage) {
        throw '[MACHINE_STATE_CAPTURE_CONFIGURATION_INVALID] Substage progress does not match the active substage.'
    }
    $Context.activeSubstageRowCount = $RowCount
    $Context.activeSubstageRawByteCount = $RawByteCount
    $substageElapsed = [long]$Context.stopwatch.ElapsedMilliseconds - [long]$Context.activeSubstageStartMilliseconds
    Write-G04DCMachineStateSubstageRecord -Context $Context -Event 'substage-progress' -Substage $Substage -Status 'running' -SubstageElapsedMilliseconds $substageElapsed -RowCount $RowCount -RawByteCount $RawByteCount
}

function Complete-G04DCMachineStateSubstage {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] $Context,
        [Parameter(Mandatory = $true)] [string]$Substage,
        [Parameter(Mandatory = $true)] [ValidateSet('success', 'failed', 'budget-exceeded')] [string]$Status,
        [ValidateRange(0, [long]::MaxValue)] [long]$RowCount = 0,
        [ValidateRange(0, [long]::MaxValue)] [long]$RawByteCount = 0,
        [ValidateRange(0, [long]::MaxValue)] [long]$MeasuredElapsedMilliseconds = 0
    )
    if ([string]$Context.activeSubstage -cne $Substage) {
        throw '[MACHINE_STATE_CAPTURE_CONFIGURATION_INVALID] Substage completion does not match the active substage.'
    }
    $wallElapsed = [long]$Context.stopwatch.ElapsedMilliseconds - [long]$Context.activeSubstageStartMilliseconds
    $substageElapsed = if ($MeasuredElapsedMilliseconds -gt 0) { $MeasuredElapsedMilliseconds } else { $wallElapsed }
    $Context.activeSubstages.Add([pscustomobject][ordered]@{
        substage = $Substage
        elapsedMilliseconds = $substageElapsed
        wallElapsedMilliseconds = $wallElapsed
        rowCount = $RowCount
        rawByteCount = $RawByteCount
        status = $Status
    })
    Write-G04DCMachineStateSubstageRecord -Context $Context -Event 'substage-end' -Substage $Substage -Status $Status -SubstageElapsedMilliseconds $substageElapsed -RowCount $RowCount -RawByteCount $RawByteCount
    $Context.activeSubstage = $null
    $Context.activeSubstageStartMilliseconds = 0L
    $Context.activeSubstageRowCount = 0L
    $Context.activeSubstageRawByteCount = 0L
}

function Start-G04DCMachineStatePhase {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] $Context,
        [Parameter(Mandatory = $true)] [ValidatePattern('^[a-z0-9][a-z0-9-]{0,63}$')] [string]$Phase
    )
    if ($Context.completed -or ![string]::IsNullOrWhiteSpace([string]$Context.activePhase)) {
        throw '[MACHINE_STATE_CAPTURE_CONFIGURATION_INVALID] Machine-state phase lifecycle is invalid.'
    }
    $Context.activePhase = $Phase
    $Context.activePhaseStartMilliseconds = [long]$Context.stopwatch.ElapsedMilliseconds
    $Context.activeItemCount = 0L
    $Context.activeSubstages.Clear()
    Write-G04DCMachineStateProgressRecord -Context $Context -Event 'phase-start' -Phase $Phase -Status 'running' -ItemCount 0
    Assert-G04DCMachineStateCaptureBudget -Context $Context -Phase $Phase -ItemCount 0
}

function Assert-G04DCMachineStateCaptureBudget {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] $Context,
        [Parameter(Mandatory = $true)] [string]$Phase,
        [ValidateRange(0, [long]::MaxValue)] [long]$ItemCount = 0,
        [switch]$WriteProgress
    )
    if ([string]$Context.activePhase -cne $Phase) {
        throw '[MACHINE_STATE_CAPTURE_CONFIGURATION_INVALID] Deadline check does not match the active phase.'
    }
    $Context.activeItemCount = $ItemCount
    $elapsed = [long]$Context.stopwatch.ElapsedMilliseconds
    $phaseElapsed = $elapsed - [long]$Context.activePhaseStartMilliseconds
    if ($elapsed -gt [long]$Context.overallBudgetMilliseconds -or $phaseElapsed -gt [long]$Context.phaseBudgetMilliseconds) {
        Write-G04DCMachineStateProgressRecord -Context $Context -Event 'phase-progress' -Phase $Phase -Status 'budget-exceeded' -ItemCount $ItemCount
        throw "[MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED] phase=$Phase elapsedMilliseconds=$elapsed phaseElapsedMilliseconds=$phaseElapsed itemCount=$ItemCount"
    }
    if ($WriteProgress) {
        Write-G04DCMachineStateProgressRecord -Context $Context -Event 'phase-progress' -Phase $Phase -Status 'running' -ItemCount $ItemCount
    }
}

function Get-G04DCMachineStateRemainingBudgetMilliseconds {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] $Context,
        [Parameter(Mandatory = $true)] [string]$Phase,
        [ValidateRange(0, [long]::MaxValue)] [long]$ItemCount = 0
    )
    Assert-G04DCMachineStateCaptureBudget -Context $Context -Phase $Phase -ItemCount $ItemCount
    $elapsed = [long]$Context.stopwatch.ElapsedMilliseconds
    $phaseElapsed = $elapsed - [long]$Context.activePhaseStartMilliseconds
    $overallRemaining = [long]$Context.overallBudgetMilliseconds - $elapsed
    $phaseRemaining = [long]$Context.phaseBudgetMilliseconds - $phaseElapsed
    return [Math]::Max(1L, [Math]::Min($overallRemaining, $phaseRemaining))
}

function Invoke-G04DCBoundedCaptureProcess {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] $Context,
        [Parameter(Mandatory = $true)] [string]$Phase,
        [Parameter(Mandatory = $true)] [scriptblock]$ScriptBlock,
        [AllowEmptyCollection()] [string[]]$ArgumentList = @(),
        [ValidateRange(0, [long]::MaxValue)] [long]$ItemCount = 0
    )
    $argumentJson = ConvertTo-Json -InputObject ([pscustomobject]@{ arguments = [object[]]@($ArgumentList) }) -Compress
    if ($argumentJson.Length -gt 16384) { throw '[MACHINE_STATE_CAPTURE_CONFIGURATION_INVALID] Bounded helper argument list is too large.' }
    $argumentBase64 = [Convert]::ToBase64String([Text.UTF8Encoding]::new($false).GetBytes($argumentJson))
    $temporaryParent = if (![string]::IsNullOrWhiteSpace($env:RUNNER_TEMP) -and (Test-Path -LiteralPath $env:RUNNER_TEMP -PathType Container)) {
        [IO.Path]::GetFullPath($env:RUNNER_TEMP)
    }
    else { [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') }
    $ownedRoot = Join-Path $temporaryParent ('document-studio-g04dc-capture-helper-' + [guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($ownedRoot) | Out-Null
    $markerPath = Join-Path $ownedRoot '.g04d-c-owned-helper'
    $operationPath = Join-Path $ownedRoot 'operation.ps1'
    $wrapperPath = Join-Path $ownedRoot 'wrapper.ps1'
    $outputPath = Join-Path $ownedRoot 'output.clixml'
    $markerText = "DOCUMENT-STUDIO-G04DC-CAPTURE-HELPER-OWNED`n"
    [IO.File]::WriteAllText($markerPath, $markerText, [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText($operationPath, $ScriptBlock.ToString(), [Text.UTF8Encoding]::new($false))
    $wrapperSource = @'
param(
    [Parameter(Mandatory = $true)] [string]$OperationPath,
    [Parameter(Mandatory = $true)] [string]$OutputPath,
    [Parameter(Mandatory = $true)] [string]$ArgumentJsonBase64
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$json = [Text.UTF8Encoding]::new($false).GetString([Convert]::FromBase64String($ArgumentJsonBase64))
$decoded = $json | ConvertFrom-Json
[object[]]$argumentList = @($decoded.arguments)
$operation = [scriptblock]::Create([IO.File]::ReadAllText($OperationPath, [Text.UTF8Encoding]::new($false)))
$result = @(& $operation @argumentList)
$result | Export-Clixml -LiteralPath $OutputPath -Depth 20 -Encoding UTF8
'@
    [IO.File]::WriteAllText($wrapperPath, $wrapperSource, [Text.UTF8Encoding]::new($false))
    $powershellPath = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $powershellPath
    $startInfo.Arguments = "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File `"$wrapperPath`" -OperationPath `"$operationPath`" -OutputPath `"$outputPath`" -ArgumentJsonBase64 $argumentBase64"
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    $job = $null
    $started = $false
    $output = @()
    try {
        $job = [DocumentStudio.G04DC.KillOnCloseJob]::new()
        if (!$process.Start()) { throw '[MACHINE_STATE_CAPTURE_FAILED] Bounded capture helper did not start.' }
        $started = $true
        $job.Assign($process)
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $remaining = [int][Math]::Min([int]::MaxValue, (Get-G04DCMachineStateRemainingBudgetMilliseconds -Context $Context -Phase $Phase -ItemCount $ItemCount))
        if (!$process.WaitForExit($remaining)) {
            try { $job.TerminateAndVerify($process, 5000) }
            catch { throw "[MACHINE_STATE_CAPTURE_HELPER_CLEANUP_FAILED] Bounded capture helper termination was not proven ($($_.Exception.GetType().FullName))." }
            $elapsed = [long]$Context.stopwatch.ElapsedMilliseconds
            $phaseElapsed = $elapsed - [long]$Context.activePhaseStartMilliseconds
            Write-G04DCMachineStateProgressRecord -Context $Context -Event 'phase-progress' -Phase $Phase -Status 'budget-exceeded' -ItemCount $ItemCount
            throw "[MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED] phase=$Phase elapsedMilliseconds=$elapsed phaseElapsedMilliseconds=$phaseElapsed itemCount=$ItemCount"
        }
        $process.WaitForExit()
        [void]$stdoutTask.Result
        [void]$stderrTask.Result
        if ($process.ExitCode -ne 0 -or !(Test-Path -LiteralPath $outputPath -PathType Leaf)) {
            throw "[MACHINE_STATE_CAPTURE_FAILED] Bounded capture helper failed (exit=$($process.ExitCode))."
        }
        $outputItem = Get-Item -LiteralPath $outputPath -Force
        if ([bool]($outputItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or [long]$outputItem.Length -gt 134217728) {
            throw '[MACHINE_STATE_CAPTURE_FAILED] Bounded capture helper output is noncanonical or oversized.'
        }
        $output = @(Import-Clixml -LiteralPath $outputPath)
        Assert-G04DCMachineStateCaptureBudget -Context $Context -Phase $Phase -ItemCount ($ItemCount + $output.Count)
    }
    finally {
        if ($started -and !$process.HasExited) {
            try { $job.TerminateAndVerify($process, 5000) }
            catch { throw "[MACHINE_STATE_CAPTURE_HELPER_CLEANUP_FAILED] Bounded capture helper final termination was not proven ($($_.Exception.GetType().FullName))." }
        }
        $process.Dispose()
        if ($job) {
            try { $job.Dispose() }
            catch { throw "[MACHINE_STATE_CAPTURE_HELPER_CLEANUP_FAILED] Bounded capture helper job cleanup failed ($($_.Exception.GetType().FullName))." }
        }
        try {
            if ((Get-Content -LiteralPath $markerPath -Raw) -cne $markerText) { throw 'marker mismatch' }
            foreach ($knownPath in @($outputPath, $wrapperPath, $operationPath, $markerPath)) {
                if (Test-Path -LiteralPath $knownPath) {
                    $knownItem = Get-Item -LiteralPath $knownPath -Force
                    if ($knownItem.PSIsContainer -or [bool]($knownItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'noncanonical helper entry' }
                    [IO.File]::Delete($knownPath)
                }
            }
            [IO.Directory]::Delete($ownedRoot, $false)
        }
        catch {
            throw "[MACHINE_STATE_CAPTURE_HELPER_CLEANUP_FAILED] Bounded capture helper path cleanup failed ($($_.Exception.GetType().FullName))."
        }
    }
    return $output
}

function Get-G04DCBoundedFileSha256 {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [string]$Path,
        [Parameter(Mandatory = $true)] $CaptureContext,
        [Parameter(Mandatory = $true)] [string]$CapturePhase,
        [ValidateRange(0, [long]::MaxValue)] [long]$ItemCount = 0,
        [Parameter(DontShow = $true)] [AllowNull()] [hashtable]$ReadAdapter
    )
    if (!$ReadAdapter) {
        $ReadAdapter = @{
            Open = { param([string]$CandidatePath) [IO.FileStream]::new($CandidatePath, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read, 1048576, [IO.FileOptions]::SequentialScan) }
            Read = { param($Handle, [byte[]]$Buffer) $Handle.Read($Buffer, 0, $Buffer.Length) }
            Close = { param($Handle) $Handle.Dispose() }
        }
    }
    foreach ($operation in @('Open', 'Read', 'Close')) {
        if (!$ReadAdapter.ContainsKey($operation) -or $ReadAdapter[$operation] -isnot [scriptblock]) {
            throw '[MACHINE_STATE_CAPTURE_FAILED] Bounded hash adapter is incomplete.'
        }
    }
    $stream = $null
    $sha256 = [Security.Cryptography.SHA256]::Create()
    $buffer = [byte[]]::new(1048576)
    $bytesReadTotal = 0L
    $nextProgress = 67108864L
    try {
        $stream = & $ReadAdapter.Open ([IO.Path]::GetFullPath($Path))
        while ($true) {
            $read = [int](& $ReadAdapter.Read $stream $buffer)
            if ($read -lt 0 -or $read -gt $buffer.Length) { throw '[MACHINE_STATE_CAPTURE_FAILED] Bounded hash adapter returned an invalid read length.' }
            if ($read -eq 0) { break }
            [void]$sha256.TransformBlock($buffer, 0, $read, $null, 0)
            $bytesReadTotal += $read
            $writeProgress = $bytesReadTotal -ge $nextProgress
            Assert-G04DCMachineStateCaptureBudget -Context $CaptureContext -Phase $CapturePhase -ItemCount $ItemCount -WriteProgress:$writeProgress
            if ($writeProgress) { $nextProgress = $bytesReadTotal + 67108864L }
        }
        [void]$sha256.TransformFinalBlock([byte[]]::new(0), 0, 0)
        return ([BitConverter]::ToString($sha256.Hash).Replace('-', '').ToLowerInvariant())
    }
    finally {
        if ($null -ne $stream) { & $ReadAdapter.Close $stream }
        $sha256.Dispose()
    }
}

function Write-G04DCBoundedJsonString {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [IO.StreamWriter]$Writer,
        [Parameter(Mandatory = $true)] [AllowEmptyString()] [string]$Value,
        [Parameter(Mandatory = $true)] $CaptureContext,
        [Parameter(Mandatory = $true)] [string]$CapturePhase,
        [Parameter(Mandatory = $true)] [ref]$TokenCount
    )
    $Writer.Write('"')
    for ($index = 0; $index -lt $Value.Length; $index++) {
        $character = $Value[$index]
        $code = [int]$character
        switch ($code) {
            8 { $Writer.Write('\b'); break }
            9 { $Writer.Write('\t'); break }
            10 { $Writer.Write('\n'); break }
            12 { $Writer.Write('\f'); break }
            13 { $Writer.Write('\r'); break }
            34 { $Writer.Write('\"'); break }
            92 { $Writer.Write('\\'); break }
            default {
                if ($code -lt 32) { $Writer.Write(('\u{0:x4}' -f $code)) }
                else { $Writer.Write($character) }
            }
        }
        if (($index + 1) % 4096 -eq 0) {
            Assert-G04DCMachineStateCaptureBudget -Context $CaptureContext -Phase $CapturePhase -ItemCount ([long]$TokenCount.Value)
        }
    }
    $Writer.Write('"')
}

function Write-G04DCBoundedJsonValue {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [IO.StreamWriter]$Writer,
        [AllowNull()] $Value,
        [Parameter(Mandatory = $true)] $CaptureContext,
        [Parameter(Mandatory = $true)] [string]$CapturePhase,
        [Parameter(Mandatory = $true)] [ref]$TokenCount,
        [ValidateRange(0, 20)] [int]$Depth = 0
    )
    if ($Depth -gt 20) { throw '[MACHINE_STATE_CAPTURE_FAILED] Machine-state JSON exceeded the bounded depth.' }
    $TokenCount.Value = [long]$TokenCount.Value + 1
    if ([long]$TokenCount.Value % 1024 -eq 0) {
        Assert-G04DCMachineStateCaptureBudget -Context $CaptureContext -Phase $CapturePhase -ItemCount ([long]$TokenCount.Value) -WriteProgress
    }
    if ($null -eq $Value) { $Writer.Write('null'); return }
    if ($Value -is [string] -or $Value -is [char] -or $Value -is [guid] -or $Value -is [datetime]) {
        $text = if ($Value -is [datetime]) { ([datetime]$Value).ToString('o') } else { [string]$Value }
        Write-G04DCBoundedJsonString -Writer $Writer -Value $text -CaptureContext $CaptureContext -CapturePhase $CapturePhase -TokenCount $TokenCount
        return
    }
    if ($Value -is [bool]) { $Writer.Write($(if ([bool]$Value) { 'true' } else { 'false' })); return }
    if ($Value.GetType().IsEnum) {
        Write-G04DCBoundedJsonString -Writer $Writer -Value ([string]$Value) -CaptureContext $CaptureContext -CapturePhase $CapturePhase -TokenCount $TokenCount
        return
    }
    $typeCode = [Type]::GetTypeCode($Value.GetType())
    if ($typeCode -in @(
        [TypeCode]::Byte, [TypeCode]::SByte, [TypeCode]::Int16, [TypeCode]::UInt16,
        [TypeCode]::Int32, [TypeCode]::UInt32, [TypeCode]::Int64, [TypeCode]::UInt64,
        [TypeCode]::Single, [TypeCode]::Double, [TypeCode]::Decimal
    )) {
        if (($Value -is [double] -and ([double]::IsNaN($Value) -or [double]::IsInfinity($Value))) -or
            ($Value -is [single] -and ([single]::IsNaN($Value) -or [single]::IsInfinity($Value)))) {
            throw '[MACHINE_STATE_CAPTURE_FAILED] Machine-state JSON contains a non-finite number.'
        }
        $Writer.Write(([IFormattable]$Value).ToString($null, [Globalization.CultureInfo]::InvariantCulture))
        return
    }
    if ($Value -is [Collections.IDictionary]) {
        $Writer.Write('{')
        $first = $true
        foreach ($key in $Value.Keys) {
            if (!$first) { $Writer.Write(',') }
            $first = $false
            Write-G04DCBoundedJsonString -Writer $Writer -Value ([string]$key) -CaptureContext $CaptureContext -CapturePhase $CapturePhase -TokenCount $TokenCount
            $Writer.Write(':')
            Write-G04DCBoundedJsonValue -Writer $Writer -Value $Value[$key] -CaptureContext $CaptureContext -CapturePhase $CapturePhase -TokenCount $TokenCount -Depth ($Depth + 1)
        }
        $Writer.Write('}')
        return
    }
    if ($Value -is [Collections.IEnumerable]) {
        $Writer.Write('[')
        $first = $true
        foreach ($entry in $Value) {
            if (!$first) { $Writer.Write(',') }
            $first = $false
            Write-G04DCBoundedJsonValue -Writer $Writer -Value $entry -CaptureContext $CaptureContext -CapturePhase $CapturePhase -TokenCount $TokenCount -Depth ($Depth + 1)
        }
        $Writer.Write(']')
        return
    }
    $properties = @($Value.PSObject.Properties | Where-Object { $_.MemberType -in @('NoteProperty', 'Property', 'AliasProperty') })
    if ($properties.Count -eq 0) { throw '[MACHINE_STATE_CAPTURE_FAILED] Machine-state JSON contains an unsupported value type.' }
    $Writer.Write('{')
    for ($propertyIndex = 0; $propertyIndex -lt $properties.Count; $propertyIndex++) {
        if ($propertyIndex -ne 0) { $Writer.Write(',') }
        $property = $properties[$propertyIndex]
        Write-G04DCBoundedJsonString -Writer $Writer -Value ([string]$property.Name) -CaptureContext $CaptureContext -CapturePhase $CapturePhase -TokenCount $TokenCount
        $Writer.Write(':')
        Write-G04DCBoundedJsonValue -Writer $Writer -Value $property.Value -CaptureContext $CaptureContext -CapturePhase $CapturePhase -TokenCount $TokenCount -Depth ($Depth + 1)
    }
    $Writer.Write('}')
}

function Write-G04DCBoundedMachineStateJson {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [string]$Path,
        [Parameter(Mandatory = $true)] $Value,
        [Parameter(Mandatory = $true)] $CaptureContext,
        [Parameter(Mandatory = $true)] [string]$CapturePhase
    )
    $canonicalPath = [IO.Path]::GetFullPath($Path)
    $parent = Split-Path -Parent $canonicalPath
    if (!(Test-Path -LiteralPath $parent -PathType Container) -or (Test-Path -LiteralPath $canonicalPath)) {
        throw '[MACHINE_STATE_CAPTURE_CONFIGURATION_INVALID] Machine-state evidence path is absent or already exists.'
    }
    $temporaryPath = "$canonicalPath.$($CaptureContext.captureId).partial"
    $stream = $null
    $writer = $null
    $tokenCount = 0L
    try {
        $stream = [IO.FileStream]::new($temporaryPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        $writer = [IO.StreamWriter]::new($stream, [Text.UTF8Encoding]::new($false), 65536, $true)
        Write-G04DCBoundedJsonValue -Writer $writer -Value $Value -CaptureContext $CaptureContext -CapturePhase $CapturePhase -TokenCount ([ref]$tokenCount)
        $writer.WriteLine()
        $writer.Flush()
        $stream.Flush($true)
        Assert-G04DCMachineStateCaptureBudget -Context $CaptureContext -Phase $CapturePhase -ItemCount $tokenCount -WriteProgress
        $writer.Dispose()
        $writer = $null
        $stream.Dispose()
        $stream = $null
        [IO.File]::Move($temporaryPath, $canonicalPath)
    }
    finally {
        if ($writer) { $writer.Dispose() }
        if ($stream) { $stream.Dispose() }
        if (Test-Path -LiteralPath $temporaryPath -PathType Leaf) { Remove-Item -LiteralPath $temporaryPath -Force }
    }
}

function Assert-G04DCMachineStatePerformanceEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [string]$Path,
        [AllowNull()] [string]$RequiredPhase
    )
    $performance = Get-Content -LiteralPath $Path -Raw -ErrorAction Stop | ConvertFrom-Json
    $phases = @($performance.phases)
    if ([int]$performance.schemaVersion -ne 1 -or ![bool]$performance.passed -or
        [long]$performance.totalElapsedMilliseconds -gt [long]$performance.hardCeilingMilliseconds -or
        @($phases | Where-Object { [string]$_.status -cne 'success' -or [long]$_.elapsedMilliseconds -gt [long]$performance.phaseCeilingMilliseconds }).Count -ne 0 -or
        (![string]::IsNullOrWhiteSpace($RequiredPhase) -and @($phases | Where-Object { [string]$_.phase -ceq $RequiredPhase }).Count -ne 1)) {
        throw '[MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED] phase=performance-gate elapsedMilliseconds=0 phaseElapsedMilliseconds=0 itemCount=0'
    }
    return $performance
}

function Complete-G04DCMachineStatePhase {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] $Context,
        [Parameter(Mandatory = $true)] [string]$Phase,
        [ValidateRange(0, [long]::MaxValue)] [long]$ItemCount = 0
    )
    if (![string]::IsNullOrWhiteSpace([string]$Context.activeSubstage)) {
        throw '[MACHINE_STATE_CAPTURE_CONFIGURATION_INVALID] Active substage was not completed before its phase.'
    }
    Assert-G04DCMachineStateCaptureBudget -Context $Context -Phase $Phase -ItemCount $ItemCount
    $elapsed = [long]$Context.stopwatch.ElapsedMilliseconds
    $phaseElapsed = $elapsed - [long]$Context.activePhaseStartMilliseconds
    Write-G04DCMachineStateProgressRecord -Context $Context -Event 'phase-end' -Phase $Phase -Status 'success' -ItemCount $ItemCount
    $Context.phases.Add([pscustomobject][ordered]@{
        phase = $Phase
        elapsedMilliseconds = $phaseElapsed
        itemCount = $ItemCount
        status = 'success'
        substages = @($Context.activeSubstages.ToArray())
    })
    $Context.activePhase = $null
    $Context.activePhaseStartMilliseconds = 0L
    $Context.activeItemCount = 0L
    $Context.activeSubstages.Clear()
}

function Complete-G04DCMachineStateCapture {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] $Context,
        [Parameter(Mandatory = $true)] [bool]$Passed,
        [AllowNull()] [string]$FailureMessage
    )
    if ($Context.completed) { return }
    if ($Passed -and [long]$Context.stopwatch.ElapsedMilliseconds -gt [long]$Context.overallBudgetMilliseconds) {
        $elapsed = [long]$Context.stopwatch.ElapsedMilliseconds
        throw "[MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED] phase=capture-success-gate elapsedMilliseconds=$elapsed phaseElapsedMilliseconds=0 itemCount=0"
    }
    $status = if ($FailureMessage -and $FailureMessage.StartsWith('[MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED]', [StringComparison]::Ordinal)) { 'budget-exceeded' } else { 'failed' }
    if (![string]::IsNullOrWhiteSpace([string]$Context.activeSubstage)) {
        $activeSubstage = [string]$Context.activeSubstage
        Complete-G04DCMachineStateSubstage -Context $Context -Substage $activeSubstage -Status $status -RowCount ([long]$Context.activeSubstageRowCount) -RawByteCount ([long]$Context.activeSubstageRawByteCount)
    }
    if (![string]::IsNullOrWhiteSpace([string]$Context.activePhase)) {
        $phase = [string]$Context.activePhase
        $phaseElapsed = [long]$Context.stopwatch.ElapsedMilliseconds - [long]$Context.activePhaseStartMilliseconds
        if (!$Passed) {
            Write-G04DCMachineStateProgressRecord -Context $Context -Event 'phase-end' -Phase $phase -Status $status -ItemCount ([long]$Context.activeItemCount)
            $Context.phases.Add([pscustomobject][ordered]@{
                phase = $phase
                elapsedMilliseconds = $phaseElapsed
                itemCount = [long]$Context.activeItemCount
                status = $status
                substages = @($Context.activeSubstages.ToArray())
            })
        }
    }
    $Context.stopwatch.Stop()
    $terminalPhase = if (![string]::IsNullOrWhiteSpace([string]$Context.activePhase)) {
        [string]$Context.activePhase
    }
    elseif ($FailureMessage -match 'phase=([a-z0-9-]+)') { $Matches[1] }
    else { $null }
    Write-G04DCMachineStateProgressRecord -Context $Context -Event $(if ($Passed) { 'capture-end' } else { 'capture-failure' }) -Phase $terminalPhase -Status $(if ($Passed) { 'success' } else { $status }) -ItemCount ([long]$Context.activeItemCount)
    if (![string]::IsNullOrWhiteSpace([string]$Context.performancePath)) {
        $performance = [ordered]@{
            schemaVersion = 1
            captureId = [string]$Context.captureId
            captureLabel = [string]$Context.captureLabel
            totalElapsedMilliseconds = [long]$Context.stopwatch.ElapsedMilliseconds
            phases = @($Context.phases.ToArray())
            metrics = [pscustomobject]$Context.metrics
            captureTargetMilliseconds = [long]$Context.captureTargetMilliseconds
            hardCeilingMilliseconds = [long]$Context.overallBudgetMilliseconds
            phaseCeilingMilliseconds = [long]$Context.phaseBudgetMilliseconds
            passed = $Passed
            failurePhase = if ($Passed) { $null } else { $terminalPhase }
        }
        Write-G04DCJson -Path ([string]$Context.performancePath) -Value $performance
    }
    $Context.completed = $true
    if ($Context.writer) {
        $Context.writer.Dispose()
        $Context.writer = $null
    }
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

function Get-G04DCMsiComponentRegistrationState {
    [CmdletBinding()]
    param(
        [AllowEmptyCollection()] [string[]]$ComponentCodes = @(),
        [Parameter(Mandatory = $true)] [string]$PackedProductCode,
        [Parameter(DontShow = $true)] [AllowNull()] [hashtable]$AccessAdapter,
        [Parameter(DontShow = $true)] [AllowNull()] $CaptureContext,
        [Parameter(DontShow = $true)] [string]$CapturePhase = 'msi-registration'
    )
    if ($PackedProductCode -notmatch '^[0-9A-F]{32}$') {
        throw '[MSI_REGISTRATION_INVALID] Packed product code is malformed.'
    }
    if (!$AccessAdapter) {
        $AccessAdapter = @{
            OpenBaseRoot = {
                param([string]$Scope)
                $hive = if ($Scope -ceq 'system') { [Microsoft.Win32.RegistryHive]::LocalMachine } else { [Microsoft.Win32.RegistryHive]::CurrentUser }
                $relative = if ($Scope -ceq 'system') {
                    'SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\UserData\S-1-5-18\Components'
                }
                else {
                    'SOFTWARE\Microsoft\Installer\Components'
                }
                $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey($hive, [Microsoft.Win32.RegistryView]::Registry64)
                try {
                    $root = $base.OpenSubKey($relative, $false)
                    [pscustomobject][ordered]@{ rootExists = $null -ne $root; handle = $root }
                }
                finally { $base.Dispose() }
            }
            OpenComponentKey = { param($RootHandle, [string]$PackedComponent) $RootHandle.OpenSubKey($PackedComponent, $false) }
            GetValueNames = { param($Handle) @($Handle.GetValueNames()) }
            GetValueKind = { param($Handle, [string]$Name) [string]$Handle.GetValueKind($Name) }
            GetValue = {
                param($Handle, [string]$Name)
                $missing = [object]::new()
                $observed = $Handle.GetValue($Name, $missing, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
                [pscustomobject][ordered]@{ present = ![object]::ReferenceEquals($missing, $observed); value = $observed }
            }
            CloseKey = { param($Handle) if ($Handle -is [IDisposable]) { $Handle.Dispose() } }
        }
    }
    foreach ($operation in @('OpenBaseRoot', 'OpenComponentKey', 'GetValueNames', 'GetValueKind', 'GetValue', 'CloseKey')) {
        if (!$AccessAdapter.ContainsKey($operation) -or $AccessAdapter[$operation] -isnot [scriptblock]) {
            throw '[MSI_REGISTRATION_CAPTURE_FAILED] MSI component registry adapter is incomplete.'
        }
    }

    function Read-G04DCMsiComponentValueState($RootState, [string]$PackedComponent) {
        if (!$RootState -or !$RootState.PSObject.Properties['rootExists'] -or !$RootState.PSObject.Properties['handle']) {
            throw '[MSI_REGISTRATION_CAPTURE_FAILED] MSI component base-root state is invalid.'
        }
        if (![bool]$RootState.rootExists) {
            return New-G04DCRegistryValueState -KeyExists $false -ValueName $PackedProductCode -ValuePresent $false
        }
        $handle = $null
        try {
            $handle = & $AccessAdapter.OpenComponentKey $RootState.handle $PackedComponent
            if ($null -eq $handle) {
                return New-G04DCRegistryValueState -KeyExists $false -ValueName $PackedProductCode -ValuePresent $false
            }
            $names = @(& $AccessAdapter.GetValueNames $handle)
            if (!(Test-G04DCRegistryValueNamePresent -Names $names -ValueName $PackedProductCode)) {
                return New-G04DCRegistryValueState -KeyExists $true -ValueName $PackedProductCode -ValuePresent $false
            }
            try {
                $valueType = [string](& $AccessAdapter.GetValueKind $handle $PackedProductCode)
                $valueResult = & $AccessAdapter.GetValue $handle $PackedProductCode
            }
            catch [System.IO.IOException] {
                $namesAfterRace = @(& $AccessAdapter.GetValueNames $handle)
                if (!(Test-G04DCRegistryValueNamePresent -Names $namesAfterRace -ValueName $PackedProductCode)) {
                    return New-G04DCRegistryValueState -KeyExists $true -ValueName $PackedProductCode -ValuePresent $false
                }
                throw '[MSI_REGISTRATION_CAPTURE_FAILED] MSI component product value changed during classification.'
            }
            if (!$valueResult -or !$valueResult.PSObject.Properties['present']) {
                throw '[MSI_REGISTRATION_CAPTURE_FAILED] MSI component registry adapter returned an invalid value state.'
            }
            if (![bool]$valueResult.present) {
                $namesAfterRace = @(& $AccessAdapter.GetValueNames $handle)
                if (!(Test-G04DCRegistryValueNamePresent -Names $namesAfterRace -ValueName $PackedProductCode)) {
                    return New-G04DCRegistryValueState -KeyExists $true -ValueName $PackedProductCode -ValuePresent $false
                }
                throw '[MSI_REGISTRATION_CAPTURE_FAILED] MSI component product value changed during classification.'
            }
            $bounded = ConvertTo-G04DCBoundedRegistryValue -ValueType $valueType -Value $valueResult.value
            return New-G04DCRegistryValueState -KeyExists $true -ValueName $PackedProductCode -ValuePresent $true -ValueType $valueType -Value $bounded
        }
        finally {
            if ($null -ne $handle) { & $AccessAdapter.CloseKey $handle }
        }
    }

    $systemRoot = $null
    $userRoot = $null
    try {
        $systemRoot = & $AccessAdapter.OpenBaseRoot 'system'
        $userRoot = & $AccessAdapter.OpenBaseRoot 'user'
        $records = [System.Collections.Generic.List[object]]::new()
        # Preserve the legacy `Where-Object { $_ }` contract exactly: null and
        # empty strings are absent inputs, while whitespace is malformed and
        # must reach ConvertTo-G04DCPackedGuid so capture fails closed.
        $orderedCodes = @($ComponentCodes | Where-Object { $_ } | Sort-Object -Unique)
        for ($index = 0; $index -lt $orderedCodes.Count; $index++) {
            $componentCode = [string]$orderedCodes[$index]
            $packedComponent = ConvertTo-G04DCPackedGuid -Guid $componentCode
            $systemPath = "Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\UserData\S-1-5-18\Components\$packedComponent"
            $userPath = "Registry::HKEY_CURRENT_USER\SOFTWARE\Microsoft\Installer\Components\$packedComponent"
            $systemValueState = Read-G04DCMsiComponentValueState -RootState $systemRoot -PackedComponent $packedComponent
            $userValueState = Read-G04DCMsiComponentValueState -RootState $userRoot -PackedComponent $packedComponent
            $records.Add([pscustomobject][ordered]@{
                componentCode = $componentCode
                packedComponent = $packedComponent
                systemPath = $systemPath
                systemProductValuePresent = [bool]$systemValueState.valuePresent
                systemProductValueState = $systemValueState
                userPath = $userPath
                userProductValuePresent = [bool]$userValueState.valuePresent
                userProductValueState = $userValueState
            })
            if ($CaptureContext -and (($index + 1) % 64 -eq 0 -or $index + 1 -eq $orderedCodes.Count)) {
                Assert-G04DCMachineStateCaptureBudget -Context $CaptureContext -Phase $CapturePhase -ItemCount ($index + 1) -WriteProgress
            }
        }
        return @($records.ToArray())
    }
    catch {
        if ($_.Exception.Message.StartsWith('[MSI_REGISTRATION_', [StringComparison]::Ordinal) -or
            $_.Exception.Message.StartsWith('[MACHINE_STATE_CAPTURE_', [StringComparison]::Ordinal)) { throw }
        throw "[MSI_REGISTRATION_CAPTURE_FAILED] Read-only MSI component classification failed ($($_.Exception.GetType().FullName))."
    }
    finally {
        foreach ($rootState in @($userRoot, $systemRoot)) {
            if ($rootState -and $rootState.PSObject.Properties['handle'] -and $null -ne $rootState.handle) {
                try { & $AccessAdapter.CloseKey $rootState.handle }
                catch { throw "[MSI_REGISTRATION_CAPTURE_FAILED] MSI component base-root cleanup failed ($($_.Exception.GetType().FullName))." }
            }
        }
    }
}

function Get-G04DCMsiRegistrationState {
    [CmdletBinding()]
    param(
        [AllowEmptyCollection()] [string[]]$ComponentCodes = @(),
        [Parameter(DontShow = $true)] [AllowNull()] [hashtable]$ComponentAccessAdapter,
        [Parameter(DontShow = $true)] [AllowNull()] $CaptureContext,
        [Parameter(DontShow = $true)] [string]$CapturePhase = 'msi-registration'
    )
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
    $localPackageSignature = if ($localPackageItem -and !$localPackageItem.PSIsContainer) {
        if ($CaptureContext) {
            $signatureRows = @(Invoke-G04DCBoundedCaptureProcess -Context $CaptureContext -Phase $CapturePhase -ScriptBlock {
                param([string]$CommonModulePath, [string]$CandidatePath)
                Import-Module $CommonModulePath -Force
                Get-G04DCAuthenticodeEvidence -Path $CandidatePath
            } -ArgumentList @($script:G04DCCommonModulePath, $localPackageItem.FullName))
            if ($signatureRows.Count -ne 1) { throw '[MSI_REGISTRATION_CAPTURE_FAILED] Cached-package Authenticode helper returned an invalid result count.' }
            $signatureRows[0]
        }
        else { Get-G04DCAuthenticodeEvidence -Path $localPackageItem.FullName }
    }
    else { $null }
    $localPackageSha256 = if ($localPackageItem -and !$localPackageItem.PSIsContainer) {
        if ($CaptureContext) { Get-G04DCBoundedFileSha256 -Path $localPackageItem.FullName -CaptureContext $CaptureContext -CapturePhase $CapturePhase }
        else { Get-G04DCSha256 -Path $localPackageItem.FullName }
    }
    else { $null }
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
    $componentArguments = @{
        ComponentCodes = $ComponentCodes
        PackedProductCode = $packedProduct
        CaptureContext = $CaptureContext
        CapturePhase = $CapturePhase
    }
    if ($ComponentAccessAdapter) { $componentArguments.AccessAdapter = $ComponentAccessAdapter }
    $componentRegistrations = @(Get-G04DCMsiComponentRegistrationState @componentArguments)
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
            sha256 = $localPackageSha256
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

function ConvertFrom-G04DCNativeTextRows {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] [AllowEmptyString()] [string]$Text)
    if ($Text.Length -eq 0) { return @() }
    $splitRows = @($Text -split "`r?`n")
    if ($splitRows.Count -ne 0 -and [string]$splitRows[$splitRows.Count - 1] -ceq '') {
        $splitRows = @($splitRows | Select-Object -First ($splitRows.Count - 1))
    }
    return @($splitRows | ForEach-Object { ([string]$_).TrimEnd() })
}

function Get-G04DCRegistryTreeDigest {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [string]$NativePath,
        [Parameter(DontShow = $true)] [AllowNull()] $CaptureContext,
        [Parameter(DontShow = $true)] [string]$CapturePhase = 'registry-digest',
        [Parameter(DontShow = $true)] [AllowNull()] [string]$NativeExecutablePath,
        [Parameter(DontShow = $true)] [AllowNull()] [string]$NativeArguments,
        [Parameter(DontShow = $true)] [switch]$AllowTestNativePath,
        [Parameter(DontShow = $true)] [ValidateRange(1, 1073741824)] [long]$MaximumRawBytes = 134217728,
        [Parameter(DontShow = $true)] [ValidateRange(1, 1000000)] [int]$MaximumRows = 1000000,
        [Parameter(DontShow = $true)] [ValidateRange(1, 16777216)] [int]$MaximumRowCharacters = 16777216,
        [Parameter(DontShow = $true)] [ValidateRange(1, 16777216)] [int]$MaximumCanonicalRowBytes = 16777216,
        [Parameter(DontShow = $true)] [ValidateRange(1, 1073741824)] [long]$MaximumCanonicalBytes = 134217728,
        [Parameter(DontShow = $true)] [ValidateRange(1, 16777216)] [long]$MaximumStderrBytes = 1048576,
        [Parameter(DontShow = $true)] [ValidateRange(1, 1048576)] [int]$ReadBufferBytes = 65536
    )
    $allowedPattern = if ($AllowTestNativePath) { '^HKEY_(CURRENT_USER|LOCAL_MACHINE|CLASSES_ROOT)(\\|$)' } else { '^HKEY_(LOCAL_MACHINE|CLASSES_ROOT)(\\|$)' }
    if ($NativePath.Length -gt 2048 -or $NativePath -notmatch $allowedPattern) {
        throw '[MACHINE_STATE_CAPTURE_FAILED] Registry digest path is outside the bounded native model.'
    }
    $reg = Join-Path $env:SystemRoot 'System32\reg.exe'
    $testOverride = ![string]::IsNullOrWhiteSpace($NativeExecutablePath) -or ![string]::IsNullOrWhiteSpace($NativeArguments)
    if ($testOverride -and (!$AllowTestNativePath -or [string]::IsNullOrWhiteSpace($NativeExecutablePath) -or [string]::IsNullOrWhiteSpace($NativeArguments))) {
        throw '[MACHINE_STATE_CAPTURE_CONFIGURATION_INVALID] Native registry test override is incomplete.'
    }
    $executable = if ($testOverride) { [IO.Path]::GetFullPath($NativeExecutablePath) } else { [IO.Path]::GetFullPath($reg) }
    $arguments = if ($testOverride) { $NativeArguments } else { "query `"$NativePath`" /s" }
    $executableItem = Get-Item -LiteralPath $executable -Force -ErrorAction Stop
    if ($executableItem.PSIsContainer -or [bool]($executableItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw '[MACHINE_STATE_CAPTURE_CONFIGURATION_INVALID] Registry digest executable is not a regular non-reparse file.'
    }
    $encoding = [Text.Encoding]::GetEncoding(
        [Console]::OutputEncoding.CodePage,
        [Text.EncoderFallback]::ExceptionFallback,
        [Text.DecoderFallback]::ExceptionFallback
    )
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $executable
    $startInfo.Arguments = $arguments
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $startInfo.StandardOutputEncoding = $encoding
    $startInfo.StandardErrorEncoding = $encoding
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    $job = $null
    $collector = $null
    $stderrCapture = $null
    $started = $false
    $terminalExitObserved = $false
    $failure = $null
    $result = $null
    $standalone = [Diagnostics.Stopwatch]::StartNew()
    $getRemainingMilliseconds = {
        if ($CaptureContext) {
            return [long](Get-G04DCMachineStateRemainingBudgetMilliseconds -Context $CaptureContext -Phase $CapturePhase -ItemCount $(if ($collector) { [long]$collector.RowCount } else { 0L }))
        }
        $remaining = 240000L - [long]$standalone.ElapsedMilliseconds
        if ($remaining -lt 1) {
            throw "[MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED] phase=$CapturePhase elapsedMilliseconds=$($standalone.ElapsedMilliseconds) phaseElapsedMilliseconds=$($standalone.ElapsedMilliseconds) itemCount=$(if ($collector) { [long]$collector.RowCount } else { 0L })"
        }
        return $remaining
    }
    $throwCollectorFailure = {
        param($ErrorRecord)
        $errorText = $ErrorRecord.Exception.ToString()
        $reason = if ($errorText -match '\[(REGISTRY_DIGEST_[A-Z0-9_]+)\]') { $Matches[1] } else { 'REGISTRY_DIGEST_INTERNAL_FAILURE' }
        if ($reason -ceq 'REGISTRY_DIGEST_TIMEOUT') {
            $elapsed = if ($CaptureContext) { [long]$CaptureContext.stopwatch.ElapsedMilliseconds } else { [long]$standalone.ElapsedMilliseconds }
            $phaseElapsed = if ($CaptureContext) { $elapsed - [long]$CaptureContext.activePhaseStartMilliseconds } else { $elapsed }
            throw "[MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED] phase=$CapturePhase elapsedMilliseconds=$elapsed phaseElapsedMilliseconds=$phaseElapsed itemCount=$(if ($collector) { [long]$collector.RowCount } else { 0L })"
        }
        throw "[MACHINE_STATE_CAPTURE_FAILED] Registry digest collector rejected bounded native output (reason=$reason)."
    }
    try {
        if ($CaptureContext) { Start-G04DCMachineStateSubstage -Context $CaptureContext -Substage 'native-query-startup' }
        $job = [DocumentStudio.G04DC.KillOnCloseJob]::new()
        if (!$process.Start()) { throw '[MACHINE_STATE_CAPTURE_FAILED] reg.exe did not start.' }
        $started = $true
        $job.Assign($process)
        $collector = [DocumentStudio.G04DC.ClassRegistryDigestCollector]::new(
            $process.StandardOutput.BaseStream,
            $encoding,
            $MaximumRawBytes,
            $MaximumRows,
            $MaximumRowCharacters,
            $MaximumCanonicalRowBytes,
            $MaximumCanonicalBytes,
            $ReadBufferBytes
        )
        $stderrCapture = [DocumentStudio.G04DC.BoundedTextCapture]::new($process.StandardError.BaseStream, $encoding, $MaximumStderrBytes, $ReadBufferBytes)
        $stdoutTask = $collector.BeginRead()
        $stderrTask = $stderrCapture.BeginRead()
        if ($CaptureContext) {
            Complete-G04DCMachineStateSubstage -Context $CaptureContext -Substage 'native-query-startup' -Status 'success'
            Start-G04DCMachineStateSubstage -Context $CaptureContext -Substage 'native-query-read'
        }
        $nextSubstageProgress = [long]$standalone.ElapsedMilliseconds + 5000L
        while (!$process.HasExited -or !$stdoutTask.IsCompleted -or !$stderrTask.IsCompleted) {
            if ($stdoutTask.IsFaulted -or $stderrTask.IsFaulted) {
                if (!$process.HasExited) {
                    $job.TerminateAndVerify($process, 5000)
                }
                break
            }
            [void]$process.WaitForExit(250)
            [void](& $getRemainingMilliseconds)
            if ($CaptureContext -and [long]$standalone.ElapsedMilliseconds -ge $nextSubstageProgress) {
                Write-G04DCMachineStateSubstageProgress -Context $CaptureContext -Substage 'native-query-read' -RowCount ([long]$collector.RowCount) -RawByteCount ([long]$collector.RawByteCount)
                $nextSubstageProgress = [long]$standalone.ElapsedMilliseconds + 5000L
            }
        }
        if (!$process.HasExited) {
            $job.TerminateAndVerify($process, 5000)
        }
        $process.WaitForExit()
        $terminalExitObserved = $true
        try { $stdoutTask.GetAwaiter().GetResult() }
        catch { & $throwCollectorFailure $_ }
        try { $stderrTask.GetAwaiter().GetResult() }
        catch { & $throwCollectorFailure $_ }
        if ($process.ExitCode -ne 0) { throw "[MACHINE_STATE_CAPTURE_FAILED] reg.exe could not seal $NativePath (exit=$($process.ExitCode))." }
        try { $collector.AppendStderrText($stderrCapture.Text) }
        catch { & $throwCollectorFailure $_ }
        if ($CaptureContext) {
            Complete-G04DCMachineStateSubstage -Context $CaptureContext -Substage 'native-query-read' -Status 'success' -RowCount ([long]$collector.RowCount) -RawByteCount ([long]$collector.RawByteCount) -MeasuredElapsedMilliseconds ([long]$collector.ReadElapsedMilliseconds)
            Assert-G04DCMachineStateCaptureBudget -Context $CaptureContext -Phase $CapturePhase -ItemCount ([long]$collector.RowCount) -WriteProgress
            Start-G04DCMachineStateSubstage -Context $CaptureContext -Substage 'row-normalization'
        }
        try { $collector.Normalize([long](& $getRemainingMilliseconds)) }
        catch { & $throwCollectorFailure $_ }
        if ($CaptureContext) {
            Complete-G04DCMachineStateSubstage -Context $CaptureContext -Substage 'row-normalization' -Status 'success' -RowCount ([long]$collector.RowCount) -RawByteCount ([long]$collector.RawByteCount) -MeasuredElapsedMilliseconds ([long]$collector.NormalizationElapsedMilliseconds)
            Assert-G04DCMachineStateCaptureBudget -Context $CaptureContext -Phase $CapturePhase -ItemCount ([long]$collector.RowCount) -WriteProgress
            Start-G04DCMachineStateSubstage -Context $CaptureContext -Substage 'canonical-hash'
        }
        try { $digest = $collector.Hash([long](& $getRemainingMilliseconds)) }
        catch { & $throwCollectorFailure $_ }
        if ($CaptureContext) {
            Complete-G04DCMachineStateSubstage -Context $CaptureContext -Substage 'canonical-hash' -Status 'success' -RowCount ([long]$collector.RowCount) -RawByteCount ([long]$collector.RawByteCount) -MeasuredElapsedMilliseconds ([long]$collector.CanonicalHashElapsedMilliseconds)
            Assert-G04DCMachineStateCaptureBudget -Context $CaptureContext -Phase $CapturePhase -ItemCount ([long]$collector.RowCount) -WriteProgress
        }
        $result = [pscustomobject][ordered]@{
            path = $NativePath
            rowCount = [long]$collector.RowCount
            sha256 = [string]$digest
        }
    }
    catch { $failure = $_ }
    finally {
        $cleanupFailure = $null
        try {
            if ($CaptureContext -and ![string]::IsNullOrWhiteSpace([string]$CaptureContext.activeSubstage)) {
                $status = if ($failure -and $failure.Exception.Message.StartsWith('[MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED]', [StringComparison]::Ordinal)) { 'budget-exceeded' } else { 'failed' }
                Complete-G04DCMachineStateSubstage -Context $CaptureContext -Substage ([string]$CaptureContext.activeSubstage) -Status $status -RowCount $(if ($collector) { [long]$collector.RowCount } else { 0L }) -RawByteCount $(if ($collector) { [long]$collector.RawByteCount } else { 0L })
            }
            if ($CaptureContext) { Start-G04DCMachineStateSubstage -Context $CaptureContext -Substage 'helper-cleanup' }
            if ($started -and !$process.HasExited) {
                $job.TerminateAndVerify($process, 5000)
            }
            if ($started -and $process.HasExited) { $terminalExitObserved = $true }
            $process.Dispose()
            if ($job) { $job.Dispose() }
            if ($CaptureContext) {
                Complete-G04DCMachineStateSubstage -Context $CaptureContext -Substage 'helper-cleanup' -Status 'success' -RowCount $(if ($collector) { [long]$collector.RowCount } else { 0L }) -RawByteCount $(if ($collector) { [long]$collector.RawByteCount } else { 0L })
                if (!$failure) {
                    Assert-G04DCMachineStateCaptureBudget -Context $CaptureContext -Phase $CapturePhase -ItemCount $(if ($collector) { [long]$collector.RowCount } else { 0L })
                }
            }
        }
        catch { $cleanupFailure = $_ }
        $standalone.Stop()
        if ($cleanupFailure) { throw "[MACHINE_STATE_CAPTURE_HELPER_CLEANUP_FAILED] reg.exe owned helper cleanup failed ($($cleanupFailure.Exception.GetType().FullName))." }
    }
    if ($failure) {
        if ($failure.Exception.Message.StartsWith('[MACHINE_STATE_CAPTURE_BUDGET_EXCEEDED]', [StringComparison]::Ordinal) -and $started -and !$terminalExitObserved) {
            throw '[MACHINE_STATE_CAPTURE_HELPER_CLEANUP_FAILED] Registry digest timeout did not terminate the owned helper.'
        }
        throw $failure
    }
    return $result
}

function Get-G04DCRegistryValueDigest {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [string[]]$Paths,
        [Parameter(DontShow = $true)] [AllowNull()] $CaptureContext,
        [Parameter(DontShow = $true)] [string]$CapturePhase = 'registry-value-digest'
    )
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
    return [pscustomobject][ordered]@{ rowCount = $rows.Count; sha256 = Get-G04DCCanonicalHash -Rows $rows -CaptureContext $CaptureContext -CapturePhase $CapturePhase }
}

function Get-G04DCDirectoryTreeDigest {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [string[]]$Roots,
        [Parameter(DontShow = $true)] [AllowNull()] $CaptureContext,
        [Parameter(DontShow = $true)] [string]$CapturePhase = 'directory-digest'
    )
    $rows = [System.Collections.Generic.List[object]]::new()
    foreach ($candidateRoot in @($Roots | Sort-Object -Unique)) {
        if ([string]::IsNullOrWhiteSpace($candidateRoot)) { throw '[MACHINE_STATE_CAPTURE_FAILED] A shortcut boundary root was unresolved.' }
        $root = [IO.Path]::GetFullPath($candidateRoot).TrimEnd('\')
        $inventoryScript = {
            param([string]$InventoryRoot)
            Set-StrictMode -Version Latest
            $ErrorActionPreference = 'Stop'
            $rootItem = Get-Item -LiteralPath $InventoryRoot -Force -ErrorAction SilentlyContinue
            if (!$rootItem) {
                [pscustomobject][ordered]@{ fullPath = $null; path = ''; present = $false; directory = $true; reparsePoint = $false; sizeBytes = 0; lastWriteUtc = $null }
                return
            }
            if (!$rootItem.PSIsContainer -or [bool]($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                throw '[MACHINE_STATE_CAPTURE_FAILED] Directory boundary root is not a canonical non-reparse directory.'
            }
            $queue = [System.Collections.Generic.Queue[string]]::new()
            $queue.Enqueue($InventoryRoot)
            $count = 0
            while ($queue.Count -ne 0) {
                $directory = $queue.Dequeue()
                foreach ($item in @(Get-ChildItem -LiteralPath $directory -Force -ErrorAction Stop | Sort-Object FullName)) {
                    $isReparse = [bool]($item.Attributes -band [IO.FileAttributes]::ReparsePoint)
                    [pscustomobject][ordered]@{
                        fullPath = $item.FullName
                        path = $item.FullName.Substring($InventoryRoot.Length).TrimStart('\')
                        present = $true
                        directory = [bool]$item.PSIsContainer
                        reparsePoint = $isReparse
                        sizeBytes = if ($item.PSIsContainer) { 0 } else { [long]$item.Length }
                        lastWriteUtc = $item.LastWriteTimeUtc.ToString('o')
                    }
                    if ($item.PSIsContainer -and !$isReparse) { $queue.Enqueue($item.FullName) }
                    $count++
                    if ($count -gt 100000) { throw '[MACHINE_STATE_CAPTURE_FAILED] Directory boundary exceeded 100000 entries.' }
                }
            }
        }
        $inventory = if ($CaptureContext) {
            @(Invoke-G04DCBoundedCaptureProcess -Context $CaptureContext -Phase $CapturePhase -ScriptBlock $inventoryScript -ArgumentList @($root) -ItemCount $rows.Count)
        }
        else {
            @(& $inventoryScript $root)
        }
        foreach ($entry in $inventory) {
            $rowNumber = $rows.Count + 1
            $hash = if ([bool]$entry.present -and ![bool]$entry.directory -and ![bool]$entry.reparsePoint) {
                if ($CaptureContext) {
                    Get-G04DCBoundedFileSha256 -Path ([string]$entry.fullPath) -CaptureContext $CaptureContext -CapturePhase $CapturePhase -ItemCount $rowNumber
                }
                else { Get-G04DCSha256 -Path ([string]$entry.fullPath) }
            }
            else { $null }
            $rows.Add([pscustomobject][ordered]@{
                root = $root
                path = [string]$entry.path
                present = [bool]$entry.present
                directory = [bool]$entry.directory
                reparsePoint = [bool]$entry.reparsePoint
                sizeBytes = [long]$entry.sizeBytes
                sha256 = $hash
                lastWriteUtc = if ($null -eq $entry.lastWriteUtc) { $null } else { [string]$entry.lastWriteUtc }
            })
            if ($rows.Count -gt 100000) { throw '[MACHINE_STATE_CAPTURE_FAILED] Directory boundary exceeded 100000 entries.' }
            if ($CaptureContext -and ($rows.Count % 128 -eq 0 -or $rows.Count -eq $inventory.Count)) {
                Assert-G04DCMachineStateCaptureBudget -Context $CaptureContext -Phase $CapturePhase -ItemCount $rows.Count -WriteProgress
            }
        }
    }
    if ($CaptureContext) { Assert-G04DCMachineStateCaptureBudget -Context $CaptureContext -Phase $CapturePhase -ItemCount $rows.Count -WriteProgress }
    $canonicalRows = @($rows.ToArray())
    return [pscustomobject][ordered]@{ rowCount = $canonicalRows.Count; sha256 = Get-G04DCCanonicalHash -Rows $canonicalRows -CaptureContext $CaptureContext -CapturePhase $CapturePhase }
}

function Get-G04DCSafePropertyState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [AllowNull()] $InputObject,
        [Parameter(Mandatory = $true)] [string]$Name,
        [Parameter(Mandatory = $true)] [string]$FailureCode,
        [Parameter(DontShow = $true)] [AllowNull()] [hashtable]$AccessAdapter
    )
    if ($null -eq $InputObject -or [string]::IsNullOrWhiteSpace($Name) -or $Name.Length -gt 256) {
        throw "[$FailureCode] Property metadata lookup received an invalid object or name."
    }
    if (!$AccessAdapter) {
        $AccessAdapter = @{
            GetProperty = { param($CandidateObject, [string]$PropertyName) $CandidateObject.PSObject.Properties[$PropertyName] }
            GetValue = { param($Property, [string]$PropertyName) $Property.Value }
        }
    }
    foreach ($operation in @('GetProperty', 'GetValue')) {
        if (!$AccessAdapter.ContainsKey($operation) -or $AccessAdapter[$operation] -isnot [scriptblock]) {
            throw "[$FailureCode] Property access adapter is incomplete."
        }
    }
    try { $property = & $AccessAdapter.GetProperty $InputObject $Name }
    catch { throw "[$FailureCode] Property metadata lookup failed ($($_.Exception.GetType().FullName))." }
    if ($null -eq $property) {
        return [pscustomobject][ordered]@{ present = $false }
    }
    try { $value = & $AccessAdapter.GetValue $property $Name }
    catch { throw "[$FailureCode] Property retrieval failed for $Name ($($_.Exception.GetType().FullName))." }
    return [pscustomobject][ordered]@{ present = $true; value = $value }
}

function ConvertTo-G04DCScheduledTaskActionValue {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] [AllowNull()] $Value)
    $maximumStringCharacters = 16384
    $maximumArrayMembers = 128
    $maximumArrayMemberCharacters = 4096
    $maximumArrayCharacters = 65536

    if ($null -eq $Value) { return $null }
    if ($Value -is [string]) {
        if ($Value.Length -gt $maximumStringCharacters) {
            throw '[SCHEDULED_TASK_ACTION_CAPTURE_FAILED] Scheduled-task action string exceeds the bounded character ceiling.'
        }
        return [string]$Value
    }
    if ($Value -is [guid]) { return $Value.ToString('D') }
    if ($Value -is [char]) { return [string]$Value }
    if ($Value -is [array]) {
        if ($Value.Rank -ne 1) {
            throw '[SCHEDULED_TASK_ACTION_CAPTURE_FAILED] Scheduled-task action arrays must have exactly one dimension.'
        }
        if ($Value.Count -gt $maximumArrayMembers) {
            throw '[SCHEDULED_TASK_ACTION_CAPTURE_FAILED] Scheduled-task action array exceeds the bounded member ceiling.'
        }
        $totalCharacters = 0
        $converted = [System.Collections.Generic.List[object]]::new()
        foreach ($member in $Value) {
            if ($null -eq $member) { $converted.Add($null); continue }
            if ($member -is [string]) {
                if ($member.Length -gt $maximumArrayMemberCharacters) {
                    throw '[SCHEDULED_TASK_ACTION_CAPTURE_FAILED] Scheduled-task action array member exceeds the bounded character ceiling.'
                }
                $totalCharacters += $member.Length
                $converted.Add([string]$member)
                continue
            }
            if ($member -is [guid]) {
                $text = $member.ToString('D')
                $totalCharacters += $text.Length
                $converted.Add($text)
                continue
            }
            if ($member -is [char]) {
                $totalCharacters++
                $converted.Add([string]$member)
                continue
            }
            $typeCode = [Type]::GetTypeCode($member.GetType())
            if ($typeCode -notin @(
                [TypeCode]::Boolean, [TypeCode]::Byte, [TypeCode]::SByte,
                [TypeCode]::Int16, [TypeCode]::UInt16, [TypeCode]::Int32, [TypeCode]::UInt32,
                [TypeCode]::Int64, [TypeCode]::UInt64, [TypeCode]::Single, [TypeCode]::Double,
                [TypeCode]::Decimal
            )) {
                throw '[SCHEDULED_TASK_ACTION_CAPTURE_FAILED] Scheduled-task action arrays may contain only bounded primitive, string, GUID, character, or null values.'
            }
            if ($typeCode -in @([TypeCode]::Single, [TypeCode]::Double) -and
                ([double]::IsNaN([double]$member) -or [double]::IsInfinity([double]$member))) {
                throw '[SCHEDULED_TASK_ACTION_CAPTURE_FAILED] Scheduled-task action floating-point values must be finite.'
            }
            $converted.Add($member)
        }
        if ($totalCharacters -gt $maximumArrayCharacters) {
            throw '[SCHEDULED_TASK_ACTION_CAPTURE_FAILED] Scheduled-task action array exceeds the bounded aggregate character ceiling.'
        }
        return ,$converted.ToArray()
    }
    $scalarTypeCode = [Type]::GetTypeCode($Value.GetType())
    if ($scalarTypeCode -in @(
        [TypeCode]::Boolean, [TypeCode]::Byte, [TypeCode]::SByte,
        [TypeCode]::Int16, [TypeCode]::UInt16, [TypeCode]::Int32, [TypeCode]::UInt32,
        [TypeCode]::Int64, [TypeCode]::UInt64, [TypeCode]::Single, [TypeCode]::Double,
        [TypeCode]::Decimal
    )) {
        if ($scalarTypeCode -in @([TypeCode]::Single, [TypeCode]::Double) -and
            ([double]::IsNaN([double]$Value) -or [double]::IsInfinity([double]$Value))) {
            throw '[SCHEDULED_TASK_ACTION_CAPTURE_FAILED] Scheduled-task action floating-point values must be finite.'
        }
        return $Value
    }
    throw '[SCHEDULED_TASK_ACTION_CAPTURE_FAILED] Scheduled-task action property is not a bounded primitive, string, GUID, character, array, or null value.'
}

function Get-G04DCScheduledTaskActionValueShape {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] [AllowNull()] $Value)
    if ($null -eq $Value) { return 'null' }
    if ($Value -is [string]) {
        if ($Value.Length -eq 0) { return 'emptyString' }
        return 'string'
    }
    if ($Value -is [bool]) { return 'boolean' }
    if ($Value -is [array]) { return 'array' }
    return 'number'
}

function ConvertTo-G04DCScheduledTaskSensitiveActionValueEvidence {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] [AllowNull()] $Value)

    $converted = ConvertTo-G04DCScheduledTaskActionValue -Value $Value
    $shape = Get-G04DCScheduledTaskActionValueShape -Value $converted
    $hashRow = [pscustomobject][ordered]@{ value = $converted }
    $hash = Get-G04DCCanonicalHash -Rows @($hashRow)
    if ($shape -cne 'array') {
        return [pscustomobject][ordered]@{
            valueShape = $shape
            sha256 = $hash
        }
    }

    $memberShapes = [System.Collections.Generic.List[string]]::new()
    foreach ($member in $converted) {
        $memberShapes.Add((Get-G04DCScheduledTaskActionValueShape -Value $member))
    }
    return [pscustomobject][ordered]@{
        valueShape = 'array'
        memberCount = $converted.Count
        memberShapes = $memberShapes.ToArray()
        sha256 = $hash
    }
}

function Get-G04DCScheduledTaskActionCimClassName {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [AllowNull()] $Action,
        [Parameter(DontShow = $true)] [AllowNull()] [hashtable]$PropertyAccessAdapter
    )
    $failureCode = 'SCHEDULED_TASK_ACTION_CAPTURE_FAILED'
    $safePropertyArguments = @{ FailureCode = $failureCode }
    if ($PropertyAccessAdapter) { $safePropertyArguments.AccessAdapter = $PropertyAccessAdapter }
    $cimClassState = Get-G04DCSafePropertyState -InputObject $Action -Name 'CimClass' @safePropertyArguments
    $classNameState = $null
    if ($cimClassState.present -and $null -ne $cimClassState.value) {
        $classNameState = Get-G04DCSafePropertyState -InputObject $cimClassState.value -Name 'CimClassName' @safePropertyArguments
    }
    if (!$classNameState -or !$classNameState.present) {
        $classNameState = Get-G04DCSafePropertyState -InputObject $Action -Name 'CimClassName' @safePropertyArguments
    }
    if (!$classNameState.present -or $classNameState.value -isnot [string] -or
        [string]::IsNullOrWhiteSpace([string]$classNameState.value) -or ([string]$classNameState.value).Length -gt 256) {
        throw '[SCHEDULED_TASK_ACTION_CAPTURE_FAILED] Scheduled-task action CIM class cannot be identified.'
    }
    return [string]$classNameState.value
}

function ConvertTo-G04DCScheduledTaskActionEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [AllowNull()] $Action,
        [Parameter(Mandatory = $true)] [ValidateRange(0, 4095)] [int]$Index,
        [Parameter(DontShow = $true)] [AllowNull()] [hashtable]$PropertyAccessAdapter
    )
    $cimClassArguments = @{ Action = $Action }
    if ($PropertyAccessAdapter) { $cimClassArguments.PropertyAccessAdapter = $PropertyAccessAdapter }
    $cimClass = Get-G04DCScheduledTaskActionCimClassName @cimClassArguments
    $actionKind = switch -Regex ($cimClass) {
        '^(?i:MSFT_TaskExecAction)$' { 'exec'; break }
        '^(?i:MSFT_TaskComHandlerAction)$' { 'comHandler'; break }
        '^(?i:MSFT_TaskEmailAction)$' { 'email'; break }
        '^(?i:MSFT_TaskShowMessageAction)$' { 'showMessage'; break }
        default { 'other' }
    }
    $selectedProperties = switch ($actionKind) {
        'exec' { @('Id', 'Execute', 'Arguments', 'WorkingDirectory') }
        'comHandler' { @('Id', 'ClassId', 'Data') }
        'email' { @('Id', 'Server', 'Subject', 'To', 'Cc', 'Bcc', 'ReplyTo', 'From', 'Body') }
        'showMessage' { @('Id', 'Title', 'Message') }
        default { @('Id', 'Name') }
    }
    $sensitiveProperties = switch ($actionKind) {
        'comHandler' { @('Data') }
        'email' { @('Server', 'Subject', 'To', 'Cc', 'Bcc', 'ReplyTo', 'From', 'Body') }
        'showMessage' { @('Title', 'Message') }
        default { @() }
    }
    $properties = [ordered]@{}
    foreach ($name in $selectedProperties) {
        $safePropertyArguments = @{ InputObject = $Action; Name = $name; FailureCode = 'SCHEDULED_TASK_ACTION_CAPTURE_FAILED' }
        if ($PropertyAccessAdapter) { $safePropertyArguments.AccessAdapter = $PropertyAccessAdapter }
        $state = Get-G04DCSafePropertyState @safePropertyArguments
        if ($state.present) {
            $properties[$name] = if ($name -cin $sensitiveProperties) {
                ConvertTo-G04DCScheduledTaskSensitiveActionValueEvidence -Value $state.value
            }
            else {
                ConvertTo-G04DCScheduledTaskActionValue -Value $state.value
            }
        }
    }
    return [pscustomobject][ordered]@{
        index = $Index
        cimClass = $cimClass
        actionKind = $actionKind
        properties = [pscustomobject]$properties
    }
}

function ConvertTo-G04DCScheduledTaskEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [AllowNull()] $Task,
        [Parameter(Mandatory = $true)] [scriptblock]$ExportTaskAdapter
    )
    $taskNameState = Get-G04DCSafePropertyState -InputObject $Task -Name 'TaskName' -FailureCode 'SCHEDULED_TASK_CAPTURE_FAILED'
    $taskPathState = Get-G04DCSafePropertyState -InputObject $Task -Name 'TaskPath' -FailureCode 'SCHEDULED_TASK_CAPTURE_FAILED'
    $stateState = Get-G04DCSafePropertyState -InputObject $Task -Name 'State' -FailureCode 'SCHEDULED_TASK_CAPTURE_FAILED'
    $actionsState = Get-G04DCSafePropertyState -InputObject $Task -Name 'Actions' -FailureCode 'SCHEDULED_TASK_CAPTURE_FAILED'
    if (!$taskNameState.present -or $taskNameState.value -isnot [string] -or [string]::IsNullOrWhiteSpace([string]$taskNameState.value) -or ([string]$taskNameState.value).Length -gt 1024 -or
        !$taskPathState.present -or $taskPathState.value -isnot [string] -or ([string]$taskPathState.value).Length -gt 1024 -or
        !$stateState.present -or $null -eq $stateState.value -or !$actionsState.present -or $null -eq $actionsState.value) {
        throw '[SCHEDULED_TASK_CAPTURE_FAILED] Scheduled-task identity, state, or ordered action collection is unavailable.'
    }
    $taskName = [string]$taskNameState.value
    $taskPath = [string]$taskPathState.value
    $actions = @($actionsState.value)
    if ($actions.Count -gt 4096) { throw '[SCHEDULED_TASK_CAPTURE_FAILED] Scheduled-task action count exceeds the bounded ceiling.' }
    $actionEvidence = [System.Collections.Generic.List[object]]::new()
    for ($index = 0; $index -lt $actions.Count; $index++) {
        $actionEvidence.Add((ConvertTo-G04DCScheduledTaskActionEvidence -Action $actions[$index] -Index $index))
    }

    try { $taskXmlOutput = @(& $ExportTaskAdapter $Task $taskName $taskPath) }
    catch { throw "[SCHEDULED_TASK_DEFINITION_CAPTURE_FAILED] Export-ScheduledTask failed ($($_.Exception.GetType().FullName))." }
    if ($taskXmlOutput.Count -ne 1 -or $taskXmlOutput[0] -isnot [string] -or
        [string]::IsNullOrWhiteSpace([string]$taskXmlOutput[0]) -or ([string]$taskXmlOutput[0]).Length -gt 1048576) {
        throw '[SCHEDULED_TASK_DEFINITION_CAPTURE_FAILED] Export-ScheduledTask did not return one bounded task definition.'
    }
    $taskXml = [string]$taskXmlOutput[0]
    $settings = [System.Xml.XmlReaderSettings]::new()
    $settings.DtdProcessing = [System.Xml.DtdProcessing]::Prohibit
    $settings.XmlResolver = $null
    $settings.MaxCharactersInDocument = 1048576
    $stringReader = [System.IO.StringReader]::new($taskXml)
    $xmlReader = $null
    $taskElementObserved = $false
    try {
        $xmlReader = [System.Xml.XmlReader]::Create($stringReader, $settings)
        while ($xmlReader.Read()) {
            if ($xmlReader.NodeType -eq [System.Xml.XmlNodeType]::Element -and $xmlReader.Depth -eq 0 -and $xmlReader.LocalName -ceq 'Task') { $taskElementObserved = $true }
        }
    }
    catch { throw "[SCHEDULED_TASK_DEFINITION_CAPTURE_FAILED] Exported task XML is not a bounded parseable definition ($($_.Exception.GetType().FullName))." }
    finally {
        if ($xmlReader) { $xmlReader.Dispose() }
        $stringReader.Dispose()
    }
    if (!$taskElementObserved) { throw '[SCHEDULED_TASK_DEFINITION_CAPTURE_FAILED] Exported task XML has no Task element.' }

    return [pscustomobject][ordered]@{
        taskPath = $taskPath
        taskName = $taskName
        state = [string]$stateState.value
        actions = @($actionEvidence.ToArray())
        definitionSha256 = Get-G04DCCanonicalHash -Rows @($taskXml)
    }
}

function Get-G04DCScheduledTaskCatalogEvidence {
    [CmdletBinding()]
    param(
        [Parameter(DontShow = $true)] [AllowNull()] [object[]]$Tasks,
        [Parameter(DontShow = $true)] [AllowNull()] [scriptblock]$ExportTaskAdapter,
        [Parameter(DontShow = $true)] [AllowNull()] $CaptureContext,
        [Parameter(DontShow = $true)] [string]$CapturePhase = 'scheduled-tasks'
    )
    if (!$PSBoundParameters.ContainsKey('Tasks')) {
        try { $Tasks = @(Get-ScheduledTask -ErrorAction Stop) }
        catch { throw "[SCHEDULED_TASK_CAPTURE_FAILED] Get-ScheduledTask failed ($($_.Exception.GetType().FullName))." }
    }
    if (!$ExportTaskAdapter) {
        $ExportTaskAdapter = {
            param($Task, [string]$TaskName, [string]$TaskPath)
            Export-ScheduledTask -TaskName $TaskName -TaskPath $TaskPath -ErrorAction Stop
        }
    }
    $rows = [System.Collections.Generic.List[object]]::new()
    $taskIndex = 0
    foreach ($task in @($Tasks)) {
        $rows.Add((ConvertTo-G04DCScheduledTaskEvidence -Task $task -ExportTaskAdapter $ExportTaskAdapter))
        $taskIndex++
        if ($CaptureContext -and ($taskIndex % 16 -eq 0 -or $taskIndex -eq @($Tasks).Count)) {
            Assert-G04DCMachineStateCaptureBudget -Context $CaptureContext -Phase $CapturePhase -ItemCount $taskIndex -WriteProgress
        }
    }
    return @($rows.ToArray() | Sort-Object taskPath, taskName)
}

function Test-G04DCScheduledTaskActionLibreOfficeReference {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)] $ActionEvidence)
    if ([string]$ActionEvidence.actionKind -cne 'exec') { return $false }
    foreach ($name in @('Execute', 'Arguments', 'WorkingDirectory')) {
        $state = Get-G04DCSafePropertyState -InputObject $ActionEvidence.properties -Name $name -FailureCode 'SCHEDULED_TASK_ACTION_CAPTURE_FAILED'
        if ($state.present -and $null -ne $state.value -and [string]$state.value -match '(?i)libreoffice|soffice') { return $true }
    }
    return $false
}

function Get-G04DCMachineState {
    [CmdletBinding()]
    param(
        [AllowEmptyCollection()] [object[]]$ProtectedRegistryRows = @(),
        [AllowEmptyCollection()] [string[]]$ProtectedFontFileNames = @(),
        [AllowEmptyCollection()] [string[]]$ProtectedExternalFilePaths = @(),
        [AllowEmptyCollection()] [string[]]$ProtectedMsiComponentCodes = @(),
        [ValidatePattern('^[a-z0-9][a-z0-9-]{0,63}$')] [string]$CaptureLabel = 'machine-state',
        [AllowNull()] [string]$ProgressPath,
        [AllowNull()] [string]$PerformancePath,
        [ValidateRange(1, 3600000)] [long]$CaptureTargetMilliseconds = 480000,
        [ValidateRange(1, 3600000)] [long]$OverallBudgetMilliseconds = 720000,
        [ValidateRange(1, 3600000)] [long]$PhaseBudgetMilliseconds = 240000,
        [AllowNull()] [string]$StateOutputPath,
        [Parameter(DontShow = $true)] [AllowNull()] [hashtable]$MsiComponentAccessAdapter
    )
    $contextArguments = @{
        CaptureLabel = $CaptureLabel
        ProgressPath = $ProgressPath
        PerformancePath = $PerformancePath
        CaptureTargetMilliseconds = $CaptureTargetMilliseconds
        OverallBudgetMilliseconds = $OverallBudgetMilliseconds
        PhaseBudgetMilliseconds = $PhaseBudgetMilliseconds
    }
    $captureContext = New-G04DCMachineStateCaptureContext @contextArguments
    $captureContext.metrics['protectedRegistryRowCount'] = @($ProtectedRegistryRows).Count
    $captureContext.metrics['protectedFontFileCount'] = @($ProtectedFontFileNames).Count
    $captureContext.metrics['externalRuntimeTargetCount'] = @($ProtectedExternalFilePaths).Count
    $captureContext.metrics['expectedMsiComponentCount'] = @($ProtectedMsiComponentCodes | Where-Object { ![string]::IsNullOrWhiteSpace([string]$_) } | Sort-Object -Unique).Count
    try {
    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'associations'
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
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'associations' -ItemCount $associations.Count

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'font-registry-catalog'
    $fontRoots = @(
        'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts',
        'Registry::HKEY_CURRENT_USER\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts'
    )
    $fontCatalogRows = @($fontRoots | ForEach-Object {
        $fontRoot = $_
        @(Get-G04DCRegistryValues -Path $fontRoot) | ForEach-Object { [pscustomobject][ordered]@{ path = $fontRoot; name = $_.name; value = $_.value } }
    })
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'font-registry-catalog' -ItemCount $fontCatalogRows.Count

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'protected-font-files'
    $protectedFontIndex = 0
    $msiFontTargets = @($ProtectedFontFileNames | Sort-Object -Unique | ForEach-Object {
        $protectedFontIndex++
        $fontFileName = $_
        $fontPath = Join-Path (Join-Path $env:SystemRoot 'Fonts') $fontFileName
        $fontItem = Get-Item -LiteralPath $fontPath -Force -ErrorAction SilentlyContinue
        [pscustomobject][ordered]@{
            fileName = $fontFileName
            path = $fontPath
            filePresent = [bool]$fontItem
            fileReparsePoint = if ($fontItem) { [bool]($fontItem.Attributes -band [IO.FileAttributes]::ReparsePoint) } else { $false }
            fileSizeBytes = if ($fontItem) { [long]$fontItem.Length } else { 0 }
            fileSha256 = if ($fontItem -and !$fontItem.PSIsContainer) { Get-G04DCBoundedFileSha256 -Path $fontItem.FullName -CaptureContext $captureContext -CapturePhase 'protected-font-files' -ItemCount $protectedFontIndex } else { $null }
            registryMatches = @($fontCatalogRows | Where-Object {
                ([IO.Path]::GetFileName([string]$_.value) -ieq $fontFileName) -or ([string]$_.name -imatch [regex]::Escape([IO.Path]::GetFileNameWithoutExtension($fontFileName)))
            })
        }
    })
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'protected-font-files' -ItemCount $msiFontTargets.Count

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'external-runtime-targets'
    $externalRuntimeIndex = 0
    $externalRuntimeTargets = @($ProtectedExternalFilePaths | Sort-Object -Unique | ForEach-Object {
        $externalRuntimeIndex++
        $targetPath = [IO.Path]::GetFullPath($_)
        $targetItem = Get-Item -LiteralPath $targetPath -Force -ErrorAction SilentlyContinue
        $signature = if ($targetItem -and !$targetItem.PSIsContainer) {
            $signatureRows = @(Invoke-G04DCBoundedCaptureProcess -Context $captureContext -Phase 'external-runtime-targets' -ScriptBlock {
                param([string]$CommonModulePath, [string]$CandidatePath)
                Import-Module $CommonModulePath -Force
                Get-G04DCAuthenticodeEvidence -Path $CandidatePath
            } -ArgumentList @($script:G04DCCommonModulePath, $targetItem.FullName) -ItemCount $externalRuntimeIndex)
            if ($signatureRows.Count -ne 1) { throw '[MACHINE_STATE_CAPTURE_FAILED] Authenticode helper returned an invalid result count.' }
            $signatureRows[0]
        }
        else { $null }
        [pscustomobject][ordered]@{
            path = $targetPath
            present = [bool]$targetItem
            regularFile = [bool]($targetItem -and !$targetItem.PSIsContainer)
            reparsePoint = if ($targetItem) { [bool]($targetItem.Attributes -band [IO.FileAttributes]::ReparsePoint) } else { $false }
            sizeBytes = if ($targetItem -and !$targetItem.PSIsContainer) { [long]$targetItem.Length } else { 0 }
            sha256 = if ($targetItem -and !$targetItem.PSIsContainer) { Get-G04DCBoundedFileSha256 -Path $targetItem.FullName -CaptureContext $captureContext -CapturePhase 'external-runtime-targets' -ItemCount $externalRuntimeIndex } else { $null }
            authenticodeStatus = if ($signature) { [string]$signature.status } else { $null }
            signer = if ($signature) { [string]$signature.signerSubject } else { $null }
            signerThumbprint = if ($signature) { [string]$signature.signerThumbprint } else { $null }
            signerChainValid = if ($signature) { [bool]$signature.chainValid } else { $false }
            signerChain = if ($signature) { @($signature.chain) } else { @() }
            fileVersion = if ($targetItem -and !$targetItem.PSIsContainer) { [string]$targetItem.VersionInfo.FileVersion } else { $null }
        }
    })
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'external-runtime-targets' -ItemCount $externalRuntimeTargets.Count

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'service-catalog'
    $serviceSource = @(Invoke-G04DCBoundedCaptureProcess -Context $captureContext -Phase 'service-catalog' -ScriptBlock {
        Get-CimInstance Win32_Service -ErrorAction Stop
    })
    $serviceRows = [System.Collections.Generic.List[object]]::new()
    $serviceIndex = 0
    foreach ($service in @($serviceSource | Sort-Object Name)) {
        $serviceRows.Add([pscustomobject][ordered]@{
            name = $service.Name
            displayName = $service.DisplayName
            state = $service.State
            status = $service.Status
            startMode = $service.StartMode
            pathName = $service.PathName
            serviceType = $service.ServiceType
            startName = $service.StartName
            errorControl = $service.ErrorControl
            desktopInteract = [bool]$service.DesktopInteract
            processId = [int]$service.ProcessId
        })
        $serviceIndex++
        if ($serviceIndex % 64 -eq 0 -or $serviceIndex -eq $serviceSource.Count) {
            Assert-G04DCMachineStateCaptureBudget -Context $captureContext -Phase 'service-catalog' -ItemCount $serviceIndex -WriteProgress
        }
    }
    $services = @($serviceRows.ToArray())
    $libreOfficeServices = @($services | Where-Object { $_.name -match '(?i)libreoffice|soffice' -or $_.pathName -match '(?i)libreoffice|soffice' })
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'service-catalog' -ItemCount $services.Count

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'service-registry-digest'
    $serviceRegistryCatalog = Get-G04DCRegistryTreeDigest -NativePath 'HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Services' -CaptureContext $captureContext -CapturePhase 'service-registry-digest'
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'service-registry-digest' -ItemCount $serviceRegistryCatalog.rowCount

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'app-paths'
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
    $appPathCatalogRowsList = [System.Collections.Generic.List[object]]::new()
    foreach ($scope in $appPathScopes) {
        $childNames = @(Invoke-G04DCBoundedCaptureProcess -Context $captureContext -Phase 'app-paths' -ScriptBlock {
            param([string]$RegistryScope)
            if (Test-Path -LiteralPath $RegistryScope) {
                Get-ChildItem -LiteralPath $RegistryScope -ErrorAction Stop | ForEach-Object { $_.PSChildName }
            }
        } -ArgumentList @($scope) -ItemCount $appPathCatalogRowsList.Count)
        foreach ($childName in @($childNames | Sort-Object)) {
            $path = "$scope\$childName"
            $appPathCatalogRowsList.Add([pscustomobject][ordered]@{ path = $path; values = @(Get-G04DCRegistryValues -Path $path) })
            Assert-G04DCMachineStateCaptureBudget -Context $captureContext -Phase 'app-paths' -ItemCount $appPathCatalogRowsList.Count
        }
    }
    $appPathCatalogRows = @($appPathCatalogRowsList.ToArray())
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'app-paths' -ItemCount ($appPaths.Count + $appPathCatalogRows.Count)

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'class-key-catalog'
    $classKeyNames = @(Invoke-G04DCBoundedCaptureProcess -Context $captureContext -Phase 'class-key-catalog' -ScriptBlock {
        Get-ChildItem -LiteralPath 'Registry::HKEY_CLASSES_ROOT' -ErrorAction Stop | ForEach-Object { $_.PSChildName }
    } | Sort-Object)
    $progIdRows = [System.Collections.Generic.List[object]]::new()
    $classKeyIndex = 0
    foreach ($classKeyName in $classKeyNames) {
        if ($classKeyName -match '^(?i:LibreOffice\.|soffice\.)') {
            $progIdRows.Add([pscustomobject][ordered]@{
                key = $classKeyName
                defaultValueState = Get-G04DCRegistryDefaultValueState -Path "Registry::HKEY_CLASSES_ROOT\$classKeyName"
            })
        }
        $classKeyIndex++
        if ($classKeyIndex % 128 -eq 0 -or $classKeyIndex -eq $classKeyNames.Count) {
            Assert-G04DCMachineStateCaptureBudget -Context $captureContext -Phase 'class-key-catalog' -ItemCount $classKeyIndex -WriteProgress
        }
    }
    $progIds = @($progIdRows.ToArray())
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'class-key-catalog' -ItemCount ($classKeyNames.Count + $progIds.Count)

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'class-registry-digest'
    $classRegistryCatalog = Get-G04DCRegistryTreeDigest -NativePath 'HKEY_CLASSES_ROOT' -CaptureContext $captureContext -CapturePhase 'class-registry-digest'
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'class-registry-digest' -ItemCount $classRegistryCatalog.rowCount

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'startup'
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
    $startupFileIndex = 0
    $startupFileRows = @($startupFolderRoots | ForEach-Object {
        $startupRoot = $_
        if (Test-Path -LiteralPath $startupRoot.path) {
            Get-ChildItem -LiteralPath $startupRoot.path -Force -ErrorAction Stop | Sort-Object Name | ForEach-Object {
                $startupFileIndex++
                [pscustomobject][ordered]@{
                    type = 'file'
                    root = $startupRoot.name
                    name = $_.Name
                    directory = $_.PSIsContainer
                    reparsePoint = [bool]($_.Attributes -band [IO.FileAttributes]::ReparsePoint)
                    sizeBytes = if ($_.PSIsContainer) { 0 } else { [long]$_.Length }
                    sha256 = if (!$_.PSIsContainer -and ![bool]($_.Attributes -band [IO.FileAttributes]::ReparsePoint)) { Get-G04DCBoundedFileSha256 -Path $_.FullName -CaptureContext $captureContext -CapturePhase 'startup' -ItemCount $startupFileIndex } else { $null }
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
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'startup' -ItemCount $startupCatalogRows.Count

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'scheduled-tasks'
    $allTaskRows = @(Invoke-G04DCBoundedCaptureProcess -Context $captureContext -Phase 'scheduled-tasks' -ScriptBlock {
        param([string]$CommonModulePath)
        Import-Module $CommonModulePath -Force
        Get-G04DCScheduledTaskCatalogEvidence
    } -ArgumentList @($script:G04DCCommonModulePath))
    $tasks = @($allTaskRows | Where-Object {
        $_.taskName -match '(?i)libreoffice|soffice' -or $_.taskPath -match '(?i)libreoffice|soffice' -or
        (@($_.actions | Where-Object { Test-G04DCScheduledTaskActionLibreOfficeReference -ActionEvidence $_ }).Count -ne 0)
    })
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'scheduled-tasks' -ItemCount $allTaskRows.Count
    $captureContext.metrics['scheduledTaskCount'] = $allTaskRows.Count

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'shortcut-catalog'
    $shortcutCatalog = Get-G04DCDirectoryTreeDigest -Roots @(
        (Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs'),
        ([Environment]::GetFolderPath([Environment+SpecialFolder]::Programs)),
        ([Environment]::GetFolderPath([Environment+SpecialFolder]::CommonDesktopDirectory)),
        ([Environment]::GetFolderPath([Environment+SpecialFolder]::DesktopDirectory))
    ) -CaptureContext $captureContext -CapturePhase 'shortcut-catalog'
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'shortcut-catalog' -ItemCount $shortcutCatalog.rowCount
    $captureContext.metrics['shortcutEntryCount'] = $shortcutCatalog.rowCount

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'environment-catalog'
    $environmentCatalog = Get-G04DCRegistryValueDigest -Paths @(
        'Registry::HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\Session Manager\Environment',
        'Registry::HKEY_CURRENT_USER\Environment'
    ) -CaptureContext $captureContext -CapturePhase 'environment-catalog'
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'environment-catalog' -ItemCount $environmentCatalog.rowCount

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'firewall-catalog'
    $firewallSource = @(Invoke-G04DCBoundedCaptureProcess -Context $captureContext -Phase 'firewall-catalog' -ScriptBlock {
        Get-NetFirewallApplicationFilter -ErrorAction SilentlyContinue | ForEach-Object {
            [pscustomobject][ordered]@{ captureKind = 'application'; instanceId = $_.InstanceID; program = $_.Program; package = $_.Package }
        }
        Get-NetFirewallRule -ErrorAction SilentlyContinue | ForEach-Object {
            [pscustomobject][ordered]@{ captureKind = 'rule'; name = $_.Name; displayName = $_.DisplayName; enabled = $_.Enabled; direction = $_.Direction; action = $_.Action; profile = $_.Profile; edgeTraversalPolicy = $_.EdgeTraversalPolicy; instanceId = $_.InstanceID }
        }
        Get-NetFirewallPortFilter -ErrorAction SilentlyContinue | ForEach-Object {
            [pscustomobject][ordered]@{ captureKind = 'port'; instanceId = $_.InstanceID; protocol = $_.Protocol; localPort = $_.LocalPort; remotePort = $_.RemotePort; icmpType = $_.IcmpType }
        }
        Get-NetFirewallAddressFilter -ErrorAction SilentlyContinue | ForEach-Object {
            [pscustomobject][ordered]@{ captureKind = 'address'; instanceId = $_.InstanceID; localAddress = $_.LocalAddress; remoteAddress = $_.RemoteAddress }
        }
        Get-NetFirewallServiceFilter -ErrorAction SilentlyContinue | ForEach-Object {
            [pscustomobject][ordered]@{ captureKind = 'service'; instanceId = $_.InstanceID; service = $_.Service }
        }
        Get-NetFirewallInterfaceFilter -ErrorAction SilentlyContinue | ForEach-Object {
            [pscustomobject][ordered]@{ captureKind = 'interface'; instanceId = $_.InstanceID; interfaceAlias = $_.InterfaceAlias }
        }
        Get-NetFirewallSecurityFilter -ErrorAction SilentlyContinue | ForEach-Object {
            [pscustomobject][ordered]@{ captureKind = 'security'; instanceId = $_.InstanceID; authentication = $_.Authentication; encryption = $_.Encryption; localUser = $_.LocalUser; remoteUser = $_.RemoteUser; remoteMachine = $_.RemoteMachine; overrideBlockRules = $_.OverrideBlockRules }
        }
    })
    $allFirewallApplicationPrograms = @{}
    foreach ($filter in @($firewallSource | Where-Object { [string]$_.captureKind -ceq 'application' })) {
        $allFirewallApplicationPrograms[[string]$filter.instanceId] = [string]$filter.program
    }
    $allFirewallRows = @($firewallSource | Where-Object { [string]$_.captureKind -ceq 'rule' } | Sort-Object name | ForEach-Object {
        [pscustomobject][ordered]@{
            name = $_.name
            displayName = $_.displayName
            enabled = [string]$_.enabled
            direction = [string]$_.direction
            action = [string]$_.action
            profile = [string]$_.profile
            edgeTraversalPolicy = [string]$_.edgeTraversalPolicy
            program = $allFirewallApplicationPrograms[[string]$_.instanceId]
        }
    })
    $allFirewallFilterRows = @($firewallSource | Where-Object { [string]$_.captureKind -cne 'rule' } | ForEach-Object {
        switch ([string]$_.captureKind) {
            'application' { [pscustomobject][ordered]@{ type = 'application'; instanceId = $_.instanceId; program = $_.program; package = $_.package } }
            'port' { [pscustomobject][ordered]@{ type = 'port'; instanceId = $_.instanceId; protocol = [string]$_.protocol; localPort = [string]$_.localPort; remotePort = [string]$_.remotePort; icmpType = [string]$_.icmpType } }
            'address' { [pscustomobject][ordered]@{ type = 'address'; instanceId = $_.instanceId; localAddress = [string]$_.localAddress; remoteAddress = [string]$_.remoteAddress } }
            'service' { [pscustomobject][ordered]@{ type = 'service'; instanceId = $_.instanceId; service = $_.service } }
            'interface' { [pscustomobject][ordered]@{ type = 'interface'; instanceId = $_.instanceId; interfaceAlias = [string]$_.interfaceAlias } }
            'security' { [pscustomobject][ordered]@{ type = 'security'; instanceId = $_.instanceId; authentication = [string]$_.authentication; encryption = [string]$_.encryption; localUser = [string]$_.localUser; remoteUser = [string]$_.remoteUser; remoteMachine = [string]$_.remoteMachine; overrideBlockRules = [string]$_.overrideBlockRules } }
        }
    })
    for ($firewallIndex = 64; $firewallIndex -lt $firewallSource.Count; $firewallIndex += 64) {
        Assert-G04DCMachineStateCaptureBudget -Context $captureContext -Phase 'firewall-catalog' -ItemCount $firewallIndex -WriteProgress
    }
    $firewall = @($allFirewallRows | Where-Object { $_.displayName -match '(?i)libreoffice|soffice' -or $_.name -match '(?i)libreoffice|soffice' -or $_.program -match '(?i)libreoffice|soffice' })
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'firewall-catalog' -ItemCount ($allFirewallRows.Count + $allFirewallFilterRows.Count)
    $captureContext.metrics['firewallRuleCount'] = $allFirewallRows.Count
    $captureContext.metrics['firewallFilterCount'] = $allFirewallFilterRows.Count

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'protected-registry-targets'
    $protectedTargets = [System.Collections.Generic.List[object]]::new()
    $protectedRegistryIndex = 0
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
        $protectedRegistryIndex++
        if ($protectedRegistryIndex % 64 -eq 0 -or $protectedRegistryIndex -eq @($ProtectedRegistryRows).Count) {
            Assert-G04DCMachineStateCaptureBudget -Context $captureContext -Phase 'protected-registry-targets' -ItemCount $protectedRegistryIndex -WriteProgress
        }
    }
    $protectedTargets = @($protectedTargets.ToArray() | Sort-Object path, name -Unique)
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'protected-registry-targets' -ItemCount $protectedTargets.Count

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'process-catalog'
    $processSource = @(Invoke-G04DCBoundedCaptureProcess -Context $captureContext -Phase 'process-catalog' -ScriptBlock {
        Get-CimInstance Win32_Process -ErrorAction Stop
    })
    $processRows = [System.Collections.Generic.List[object]]::new()
    $processIndex = 0
    foreach ($candidateProcess in $processSource) {
        if ($candidateProcess.Name -match '^(?i:soffice|libreoffice).*' -or $candidateProcess.ExecutablePath -match '(?i)libreoffice') {
            $processRows.Add([pscustomobject][ordered]@{ pid = [int]$candidateProcess.ProcessId; parentPid = [int]$candidateProcess.ParentProcessId; name = $candidateProcess.Name; executablePath = $candidateProcess.ExecutablePath })
        }
        $processIndex++
        if ($processIndex % 64 -eq 0 -or $processIndex -eq $processSource.Count) {
            Assert-G04DCMachineStateCaptureBudget -Context $captureContext -Phase 'process-catalog' -ItemCount $processIndex -WriteProgress
        }
    }
    $processes = @($processRows.ToArray() | Sort-Object pid)
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'process-catalog' -ItemCount $processSource.Count

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'installed-product-catalog'
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
    $productCatalogList = [System.Collections.Generic.List[object]]::new()
    foreach ($scope in $uninstallRoots) {
        $productChildNames = @(Invoke-G04DCBoundedCaptureProcess -Context $captureContext -Phase 'installed-product-catalog' -ScriptBlock {
            param([string]$RegistryScope)
            if (Test-Path -LiteralPath $RegistryScope) {
                Get-ChildItem -LiteralPath $RegistryScope -ErrorAction Stop | ForEach-Object { $_.PSChildName }
            }
        } -ArgumentList @($scope) -ItemCount $productCatalogList.Count)
        foreach ($childName in @($productChildNames | Sort-Object)) {
            $path = "$scope\$childName"
            $productCatalogList.Add([pscustomobject][ordered]@{ path = $path; values = @(Get-G04DCRegistryValues -Path $path) })
            if ($productCatalogList.Count % 64 -eq 0 -or $productCatalogList.Count -eq $productChildNames.Count) {
                Assert-G04DCMachineStateCaptureBudget -Context $captureContext -Phase 'installed-product-catalog' -ItemCount $productCatalogList.Count -WriteProgress
            }
        }
    }
    $productCatalogRows = @($productCatalogList.ToArray())
    $otherProductCatalogRows = @($productCatalogRows | Where-Object { !([string]$_.path).EndsWith($productCode, [StringComparison]::OrdinalIgnoreCase) })
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'installed-product-catalog' -ItemCount $productCatalogRows.Count

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'installer-cache-catalog'
    $installerCacheCatalog = Get-G04DCDirectoryTreeDigest -Roots @((Join-Path $env:SystemRoot 'Installer')) -CaptureContext $captureContext -CapturePhase 'installer-cache-catalog'
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'installer-cache-catalog' -ItemCount $installerCacheCatalog.rowCount
    $captureContext.metrics['installerCacheEntryCount'] = $installerCacheCatalog.rowCount

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'pending-reboot'
    $pendingFileRenameKey = 'Registry::HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\Session Manager'
    $pendingReboot = [pscustomobject][ordered]@{
        componentBasedServicing = Test-Path -LiteralPath 'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending'
        windowsUpdate = Test-Path -LiteralPath 'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired'
        pendingFileRenameOperationsState = Get-G04DCRegistryValueState -Path $pendingFileRenameKey -ValueName 'PendingFileRenameOperations'
    }
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'pending-reboot' -ItemCount 3

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'msi-registration'
    $serviceDigestRows = @($services)
    $msiRegistrationArguments = @{
        ComponentCodes = $ProtectedMsiComponentCodes
        CaptureContext = $captureContext
        CapturePhase = 'msi-registration'
    }
    if ($MsiComponentAccessAdapter) { $msiRegistrationArguments.ComponentAccessAdapter = $MsiComponentAccessAdapter }
    $msiRegistration = Get-G04DCMsiRegistrationState @msiRegistrationArguments
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'msi-registration' -ItemCount @($msiRegistration.componentRegistrations).Count

    Start-G04DCMachineStatePhase -Context $captureContext -Phase 'canonical-state-finalization'
    $state = [pscustomobject][ordered]@{
        capturedAtUtc = [DateTime]::UtcNow.ToString('o')
        fontCatalogCount = $fontCatalogRows.Count
        fontCatalogSha256 = Get-G04DCCanonicalHash -Rows $fontCatalogRows -CaptureContext $captureContext -CapturePhase 'canonical-state-finalization'
        msiFontTargets = $msiFontTargets
        externalRuntimeTargets = $externalRuntimeTargets
        associations = $associations
        libreOfficeServices = $libreOfficeServices
        serviceCatalogCount = $services.Count
        serviceCatalogSha256 = Get-G04DCCanonicalHash -Rows $serviceDigestRows -CaptureContext $captureContext -CapturePhase 'canonical-state-finalization'
        serviceRegistryCatalogCount = $serviceRegistryCatalog.rowCount
        serviceRegistryCatalogSha256 = $serviceRegistryCatalog.sha256
        appPaths = $appPaths
        appPathCatalogCount = $appPathCatalogRows.Count
        appPathCatalogSha256 = Get-G04DCCanonicalHash -Rows $appPathCatalogRows -CaptureContext $captureContext -CapturePhase 'canonical-state-finalization'
        libreOfficeProgIds = $progIds
        classKeyCatalogCount = $classKeyNames.Count
        classKeyCatalogSha256 = Get-G04DCCanonicalHash -Rows $classKeyNames -CaptureContext $captureContext -CapturePhase 'canonical-state-finalization'
        classRegistryCatalogCount = $classRegistryCatalog.rowCount
        classRegistryCatalogSha256 = $classRegistryCatalog.sha256
        msiProtectedRegistryTargets = $protectedTargets
        scheduledTasks = $tasks
        scheduledTaskCatalogCount = $allTaskRows.Count
        scheduledTaskCatalogSha256 = Get-G04DCCanonicalHash -Rows $allTaskRows -CaptureContext $captureContext -CapturePhase 'canonical-state-finalization'
        startup = $startup
        startupCatalogCount = $startupCatalogRows.Count
        startupCatalogSha256 = Get-G04DCCanonicalHash -Rows $startupCatalogRows -CaptureContext $captureContext -CapturePhase 'canonical-state-finalization'
        shortcutCatalogCount = $shortcutCatalog.rowCount
        shortcutCatalogSha256 = $shortcutCatalog.sha256
        firewallRules = $firewall
        firewallCatalogCount = $allFirewallRows.Count
        firewallCatalogSha256 = Get-G04DCCanonicalHash -Rows $allFirewallRows -CaptureContext $captureContext -CapturePhase 'canonical-state-finalization'
        firewallFilterCatalogCount = $allFirewallFilterRows.Count
        firewallFilterCatalogSha256 = Get-G04DCCanonicalHash -Rows $allFirewallFilterRows -CaptureContext $captureContext -CapturePhase 'canonical-state-finalization'
        machinePath = [Environment]::GetEnvironmentVariable('PATH', 'Machine')
        userPath = [Environment]::GetEnvironmentVariable('PATH', 'User')
        environmentCatalogCount = $environmentCatalog.rowCount
        environmentCatalogSha256 = $environmentCatalog.sha256
        installedProduct = $products
        msiRegistration = $msiRegistration
        otherInstalledProductCatalogCount = $otherProductCatalogRows.Count
        otherInstalledProductCatalogSha256 = Get-G04DCCanonicalHash -Rows $otherProductCatalogRows -CaptureContext $captureContext -CapturePhase 'canonical-state-finalization'
        installedProductCatalogCount = $productCatalogRows.Count
        installedProductCatalogSha256 = Get-G04DCCanonicalHash -Rows $productCatalogRows -CaptureContext $captureContext -CapturePhase 'canonical-state-finalization'
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
    Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'canonical-state-finalization' -ItemCount 1
    if (![string]::IsNullOrWhiteSpace($StateOutputPath)) {
        Start-G04DCMachineStatePhase -Context $captureContext -Phase 'state-serialization'
        Write-G04DCBoundedMachineStateJson -Path $StateOutputPath -Value $state -CaptureContext $captureContext -CapturePhase 'state-serialization'
        Complete-G04DCMachineStatePhase -Context $captureContext -Phase 'state-serialization' -ItemCount 1
    }
    Complete-G04DCMachineStateCapture -Context $captureContext -Passed $true -FailureMessage $null
    return $state
    }
    catch {
        $original = $_
        try { Complete-G04DCMachineStateCapture -Context $captureContext -Passed $false -FailureMessage $original.Exception.Message }
        catch { throw "[MACHINE_STATE_CAPTURE_TELEMETRY_FAILED] $($_.Exception.Message) Original: $($original.Exception.Message)" }
        throw $original
    }
    finally {
        if ($captureContext.writer) {
            $captureContext.writer.Dispose()
            $captureContext.writer = $null
        }
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
    'New-G04DCMachineStateCaptureContext', 'Write-G04DCMachineStateProgressRecord',
    'Start-G04DCMachineStatePhase', 'Assert-G04DCMachineStateCaptureBudget',
    'Complete-G04DCMachineStatePhase', 'Complete-G04DCMachineStateCapture', 'Assert-G04DCMachineStatePerformanceEvidence',
    'Get-G04DCSafePropertyState', 'ConvertTo-G04DCScheduledTaskActionEvidence',
    'ConvertTo-G04DCScheduledTaskEvidence', 'Get-G04DCScheduledTaskCatalogEvidence',
    'ConvertTo-G04DCPackedGuid', 'Get-G04DCMsiComponentRegistrationState', 'Get-G04DCMsiRegistrationState', 'Assert-G04DCMsiRegistrationAbsent', 'Assert-G04DCMsiRegistrationInstalled',
    'Get-G04DCMachineState', 'Compare-G04DCMachineState',
    'Assert-G04DCNonMutation', 'Assert-G04DCRunnerIsolation', 'Assert-G04DCExternalRuntimeDependencies',
    'Assert-G04DCProcessEvidence', 'Get-G04DCLoadBearingModuleEvidence', 'Assert-G04DCLoadBearingModuleEvidence', 'Assert-G04DCOutputEvidence',
    'Assert-G04DCFileAccessEvidence',
    'Remove-G04DCOwnedRoot', 'Assert-G04DCCleanupEvidence', 'New-G04DCArtifactManifest', 'Assert-G04DCArtifactManifest'
)
