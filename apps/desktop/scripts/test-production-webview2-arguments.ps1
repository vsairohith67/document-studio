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
$originalArguments = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
$originalUserDataFolder = $env:WEBVIEW2_USER_DATA_FOLDER

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

function Get-ProcessLog([string]$Path) {
  if (!(Test-Path -LiteralPath $Path)) { return '<not created>' }
  $content = Get-Content -LiteralPath $Path -Raw -ErrorAction SilentlyContinue
  if ([string]::IsNullOrWhiteSpace($content)) { return '<empty>' }
  if ($content.Length -le 4096) { return $content.Trim() }
  return $content.Substring($content.Length - 4096).Trim()
}

function Get-UdfProcesses([string]$UserDataFolder) {
  $escaped = [regex]::Escape($UserDataFolder)
  return @(Get-CimInstance Win32_Process |
    Where-Object {
      $_.Name -ieq 'msedgewebview2.exe' -and
      [string]$_.CommandLine -match $escaped
    } |
    Select-Object ProcessId, ParentProcessId, Name, CommandLine)
}

function Write-CaseDiagnostics(
  [string]$Name,
  [System.Diagnostics.Process]$Process,
  [System.Diagnostics.Stopwatch]$Stopwatch,
  [string]$UserDataFolder,
  [string]$StandardOutput,
  [string]$StandardError,
  [object[]]$ObservedProcesses
) {
  $exitCode = '<running>'
  if ($Process.HasExited) {
    $Process.WaitForExit()
    $exitCode = [string]$Process.ExitCode
  }
  $profileEntries = @()
  if (Test-Path -LiteralPath $UserDataFolder) {
    $profileEntries = @(Get-ChildItem -LiteralPath $UserDataFolder -Force -ErrorAction SilentlyContinue |
      Select-Object -First 20 -ExpandProperty Name)
  }
  Write-Output "Production WebView2 diagnostics: case=$Name; executable=$Executable; pid=$($Process.Id); exitCode=$exitCode; elapsedMs=$($Stopwatch.ElapsedMilliseconds); udf=$UserDataFolder; udfEntries=$($profileEntries -join ',')."
  foreach ($observed in $ObservedProcesses) {
    Write-Output "Observed process: pid=$($observed.ProcessId); parent=$($observed.ParentProcessId); name=$($observed.Name); command=$($observed.CommandLine)"
  }
  Write-Output "Process stdout: $(Get-ProcessLog $StandardOutput)"
  Write-Output "Process stderr: $(Get-ProcessLog $StandardError)"
}

function Invoke-Case([string]$Name, [string]$Arguments, [int]$ForbiddenPort) {
  $caseRoot = Join-Path $evidenceRoot $Name
  $userDataFolder = Join-Path $caseRoot 'webview2-user-data'
  $standardOutput = Join-Path $caseRoot 'application.stdout.log'
  $standardError = Join-Path $caseRoot 'application.stderr.log'
  New-Item -ItemType Directory -Path $userDataFolder | Out-Null
  $env:WEBVIEW2_USER_DATA_FOLDER = $userDataFolder
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $Arguments
  $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
  $process = Start-Process -FilePath $Executable -PassThru -WindowStyle Hidden `
    -RedirectStandardOutput $standardOutput -RedirectStandardError $standardError
  $owned = @()
  $observed = @()
  $browser = @()
  try {
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    do {
      if ($process.HasExited) {
        Write-CaseDiagnostics $Name $process $stopwatch $userDataFolder $standardOutput $standardError $observed
        throw "Production application exited before WebView2 started in $Name."
      }
      $owned = @(Get-OwnedDescendants $process.Id)
      $observed = @($owned)
      $browser = @($owned | Where-Object { $_.Name -ieq 'msedgewebview2.exe' })
      $profileReady = @(Get-ChildItem -LiteralPath $userDataFolder -Force -ErrorAction SilentlyContinue).Count -gt 0
      if ($browser.Count -gt 0 -and $profileReady) { break }
      Start-Sleep -Milliseconds 200
    } while ([DateTime]::UtcNow -lt $deadline)
    if ($browser.Count -eq 0 -or !$profileReady) {
      Write-CaseDiagnostics $Name $process $stopwatch $userDataFolder $standardOutput $standardError $observed
      throw "No ready owned WebView2 child and profile were created in $Name."
    }
    $userDataPattern = [regex]::Escape($userDataFolder)
    $browserWithOwnedProfile = @($browser | Where-Object {
      [string]$_.CommandLine -match $userDataPattern
    })
    if ($browserWithOwnedProfile.Count -eq 0) {
      Write-CaseDiagnostics $Name $process $stopwatch $userDataFolder $standardOutput $standardError $observed
      throw "The owned WebView2 process did not use the isolated profile in $Name."
    }
    foreach ($child in $browser) {
      $command = [string]$child.CommandLine
      if ($command -match '(?i)--remote-debugging-|--remote-allow-origins') {
        throw "Production WebView2 inherited a forbidden remote-debug argument in $Name."
      }
    }
    if (Test-LoopbackPort $ForbiddenPort) { throw "A forbidden CDP listener appeared in $Name." }
    Write-Output "Production WebView2 ready: case=$Name; elapsedMs=$($stopwatch.ElapsedMilliseconds); pid=$($process.Id); browserPid=$($browserWithOwnedProfile[0].ProcessId); udf=$userDataFolder."
  } finally {
    if (!$process.HasExited) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }
    if (!$process.WaitForExit(10000)) {
      throw "Production application cleanup did not complete in $Name."
    }
    $profileProcesses = @(Get-UdfProcesses $userDataFolder)
    foreach ($child in @($owned + $profileProcesses) | Sort-Object ProcessId -Unique -Descending) {
      Stop-Process -Id $child.ProcessId -Force -ErrorAction SilentlyContinue
    }
    $cleanupDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
      $profileProcesses = @(Get-UdfProcesses $userDataFolder)
      if ($profileProcesses.Count -eq 0) { break }
      Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $cleanupDeadline)
    if ($profileProcesses.Count -ne 0) {
      throw "Owned WebView2 cleanup did not complete in $Name."
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
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $originalArguments
  $env:WEBVIEW2_USER_DATA_FOLDER = $originalUserDataFolder
  Remove-Item -LiteralPath $evidenceRoot -Recurse -Force -ErrorAction SilentlyContinue
}
