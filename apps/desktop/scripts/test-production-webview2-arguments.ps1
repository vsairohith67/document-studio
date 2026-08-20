param(
  [string]$Executable = ""
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Resolve-Path (Join-Path $PSScriptRoot '..\..\..')
if ([string]::IsNullOrWhiteSpace($Executable)) {
  $Executable = Join-Path $repositoryRoot 'target\release\document-studio.exe'
}
$Executable = (Resolve-Path -LiteralPath $Executable).Path
$evidenceRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("document-studio-production-webview2-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $evidenceRoot | Out-Null
$originalLocalAppData = $env:LOCALAPPDATA
$originalAppData = $env:APPDATA
$originalArguments = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS

function Get-OwnedDescendants([int]$RootPid) {
  $all = @(Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, CommandLine)
  $owned = @()
  $parents = New-Object System.Collections.Generic.HashSet[int]
  [void]$parents.Add($RootPid)
  $changed = $true
  while ($changed) {
    $changed = $false
    foreach ($process in $all) {
      if ($parents.Contains([int]$process.ParentProcessId) -and !$parents.Contains([int]$process.ProcessId)) {
        [void]$parents.Add([int]$process.ProcessId)
        $owned += $process
        $changed = $true
      }
    }
  }
  return $owned
}

function Test-LoopbackPort([int]$Port) {
  $client = [System.Net.Sockets.TcpClient]::new()
  try {
    $result = $client.BeginConnect([System.Net.IPAddress]::Loopback, $Port, $null, $null)
    if (!$result.AsyncWaitHandle.WaitOne(250)) { return $false }
    try { $client.EndConnect($result); return $true } catch { return $false }
  } finally {
    $client.Dispose()
  }
}

function Invoke-Case([string]$Name, [string]$Arguments, [int]$ForbiddenPort) {
  $caseRoot = Join-Path $evidenceRoot $Name
  $local = Join-Path $caseRoot 'local'
  $roaming = Join-Path $caseRoot 'roaming'
  New-Item -ItemType Directory -Path $local,$roaming | Out-Null
  $env:LOCALAPPDATA = $local
  $env:APPDATA = $roaming
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $Arguments
  $process = Start-Process -FilePath $Executable -PassThru -WindowStyle Hidden
  $owned = @()
  try {
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    do {
      if ($process.HasExited) { throw "Production application exited before WebView2 started in $Name." }
      $owned = @(Get-OwnedDescendants $process.Id)
      $browser = @($owned | Where-Object { $_.Name -ieq 'msedgewebview2.exe' })
      if ($browser.Count -gt 0) { break }
      Start-Sleep -Milliseconds 200
    } while ([DateTime]::UtcNow -lt $deadline)
    if ($browser.Count -eq 0) { throw "No owned WebView2 child was created in $Name." }
    foreach ($child in $browser) {
      $command = [string]$child.CommandLine
      if ($command -match '(?i)--remote-debugging-|--remote-allow-origins') {
        throw "Production WebView2 inherited a forbidden remote-debug argument in $Name."
      }
    }
    if (Test-LoopbackPort $ForbiddenPort) { throw "A forbidden CDP listener appeared in $Name." }
  } finally {
    if (!$process.HasExited) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }
    $process.WaitForExit(10000) | Out-Null
    foreach ($child in $owned | Sort-Object ProcessId -Descending) {
      Stop-Process -Id $child.ProcessId -Force -ErrorAction SilentlyContinue
    }
  }
}

try {
  $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
  $listener.Start()
  $port = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
  $listener.Stop()
  Invoke-Case 'malicious' "--disable-gpu --remote-debugging-pipe --remote-debugging-port=$port --remote-allow-origins=*" $port
  Invoke-Case 'benign' '--disable-gpu --force-color-profile=srgb' $port
  Write-Output 'Production WebView2 inherited-argument smoke passed (malicious family cleared; benign value created no CDP).'
} finally {
  $env:LOCALAPPDATA = $originalLocalAppData
  $env:APPDATA = $originalAppData
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $originalArguments
  Remove-Item -LiteralPath $evidenceRoot -Recurse -Force -ErrorAction SilentlyContinue
}
