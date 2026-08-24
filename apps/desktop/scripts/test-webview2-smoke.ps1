param(
  [ValidateSet('None', 'ViteNotReady', 'CdpNotReady')]
  [string]$FailureInjection = 'None',
  [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Net.Http

$desktopRoot = Split-Path -Parent $PSScriptRoot
$repositoryRoot = (Resolve-Path (Join-Path $desktopRoot '..\..')).Path
$repositoryRootForward = $repositoryRoot.Replace('\', '/')
$cacheRoot = Join-Path $repositoryRoot '.cache'
$evidenceRoot = Join-Path $cacheRoot ('g03-webview2-' + [guid]::NewGuid().ToString('N'))
$appData = Join-Path $evidenceRoot 'app-data'
$g04b2Output = Join-Path $appData 'g04b2-output'
$webViewData = Join-Path $appData 'webview2-user-data'
$webViewCommandData = Join-Path $webViewData 'EBWebView'
$fixture = (Resolve-Path (Join-Path $repositoryRoot 'report\Document_Studio_Master_Blueprint.pdf')).Path
$devUrl = 'http://localhost:1420'
$vite = $null
$desktop = $null
$reservation = $null
$cdpPort = $null
$runtimeMarker = $null
$viteReady = $false
$cdpReady = $false
$playwrightInvoked = $false
$previousEnvironment = @{}

function Get-ProcessSnapshot {
  @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue)
}

function Get-DescendantProcessIds {
  param(
    [int]$RootProcessId,
    [object[]]$Snapshot = $(Get-ProcessSnapshot)
  )
  $descendants = [System.Collections.Generic.HashSet[int]]::new()
  [void]$descendants.Add($RootProcessId)
  do {
    $added = $false
    foreach ($process in $Snapshot) {
      if ($descendants.Contains([int]$process.ParentProcessId) -and
          $descendants.Add([int]$process.ProcessId)) {
        $added = $true
      }
    }
  } while ($added)
  @($descendants)
}

function Get-OwnedWebViewProcesses {
  if ($null -eq $desktop) { return @() }
  $snapshot = Get-ProcessSnapshot
  $descendants = @(Get-DescendantProcessIds -RootProcessId $desktop.Id -Snapshot $snapshot)
  @($snapshot | Where-Object {
    $_.Name -eq 'msedgewebview2.exe' -and $descendants -contains [int]$_.ProcessId
  })
}

function Get-WebViewDataFolderFromCommandLine {
  param([string]$CommandLine)
  if ($CommandLine -match '--user-data-dir=(?:"([^"]+)"|([^ ]+))') {
    if ($matches[1]) { return $matches[1] }
    return $matches[2]
  }
  $null
}

function Get-NormalizedComparisonPath {
  param([string]$Path)
  $fullPath = [System.IO.Path]::GetFullPath($Path)
  if ($fullPath.StartsWith('\\?\')) { $fullPath = $fullPath.Substring(4) }
  $fullPath.TrimEnd('\')
}

function Test-ExactWebViewDataFolder {
  param([string]$CommandLine)
  $actual = Get-WebViewDataFolderFromCommandLine -CommandLine $CommandLine
  if (-not $actual) { return $false }
  (Get-NormalizedComparisonPath $actual) -eq
    (Get-NormalizedComparisonPath $webViewCommandData)
}

function Get-SanitizedRelevantSwitches {
  param([string]$CommandLine)
  $switches = @()
  foreach ($pattern in @(
    '--remote-debugging-port=[^ ]+',
    '--remote-allow-origins=[^ ]+',
    '--user-data-dir=(?:"[^"]+"|[^ ]+)',
    '--browser-subprocess-path=(?:"[^"]+"|[^ ]+)'
  )) {
    foreach ($match in [regex]::Matches([string]$CommandLine, $pattern)) {
      $switch = $match.Value
      $switch = $switch.Replace($webViewCommandData, '<WEBVIEW2_UDF>')
      $switch = $switch.Replace($webViewData, '<WEBVIEW2_UDF>')
      $switch = $switch.Replace($evidenceRoot, '<EVIDENCE>')
      $switch = $switch.Replace($repositoryRoot, '<REPO>')
      $switch = $switch.Replace($repositoryRootForward, '<REPO>')
      $switches += $switch
    }
  }
  $switches -join ' '
}

function Get-HttpResponseText {
  param(
    [string]$Url,
    [int]$TimeoutMilliseconds = 500
  )
  $client = [System.Net.Http.HttpClient]::new()
  $client.Timeout = [TimeSpan]::FromMilliseconds($TimeoutMilliseconds)
  try {
    $response = $client.GetAsync($Url).GetAwaiter().GetResult()
    if (-not $response.IsSuccessStatusCode) { return $null }
    $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
  } catch {
    $null
  } finally {
    $client.Dispose()
  }
}

function Wait-ForVite {
  $deadline = [DateTime]::UtcNow.AddSeconds(30)
  while ([DateTime]::UtcNow -lt $deadline) {
    $vite.Refresh()
    if ($vite.HasExited) {
      throw 'VITE_NOT_READY: Vite exited before the exact Tauri dev URL became ready.'
    }
    $response = Get-HttpResponseText -Url $devUrl
    if ($null -ne $response) {
      $script:viteReady = $true
      return
    }
    Start-Sleep -Milliseconds 100
  }
  throw 'VITE_NOT_READY: the exact Tauri dev URL did not return HTTP 200 within 30 seconds.'
}

function Wait-ForDesktop {
  $deadline = [DateTime]::UtcNow.AddSeconds(30)
  while ([DateTime]::UtcNow -lt $deadline) {
    $desktop.Refresh()
    if ($desktop.HasExited) {
      throw "DESKTOP_NOT_READY: the test-runtime process exited with code $($desktop.ExitCode)."
    }
    $owned = @(Get-OwnedWebViewProcesses)
    $browser = @($owned | Where-Object { [string]$_.CommandLine -notmatch '(?:^| )--type=' }) |
      Select-Object -First 1
    if ((Test-Path -LiteralPath $runtimeMarker) -and $null -ne $browser) {
      $runtimeEvidence = Get-Content -LiteralPath $runtimeMarker -Raw
      if ($runtimeEvidence -notmatch '(?m)^WEBVIEW2_ENVIRONMENT=clean\r?$' -or
          $runtimeEvidence -notmatch '(?m)^COREWEBVIEW2_MAX_INSTANCES=20\r?$') {
        throw 'DESKTOP_NOT_READY: the test-runtime did not prove the exact post-policy environment.'
      }
      if (-not (Test-ExactWebViewDataFolder -CommandLine ([string]$browser.CommandLine))) {
        throw 'DESKTOP_NOT_READY: WebView2 did not use the isolated test user-data folder.'
      }
      if ($FailureInjection -ne 'CdpNotReady' -and
          [string]$browser.CommandLine -notmatch "--remote-debugging-port=$cdpPort(?: |$)") {
        throw 'DESKTOP_NOT_READY: the dynamic CDP argument did not reach the WebView2 browser process.'
      }
      if ($FailureInjection -ne 'CdpNotReady' -and
          [string]$browser.CommandLine -notmatch "--remote-allow-origins=http://127\.0\.0\.1:$cdpPort(?: |$)") {
        throw 'DESKTOP_NOT_READY: the exact loopback CDP allow-origin did not reach the WebView2 browser process.'
      }
      $expectedDebugSwitchCount = if ($FailureInjection -eq 'CdpNotReady') { 0 } else { 1 }
      $debugSwitches = @([regex]::Matches(
        [string]$browser.CommandLine,
        '(?i)--remote-debugging-[^\s]+'
      ))
      $allowOriginSwitches = @([regex]::Matches(
        [string]$browser.CommandLine,
        '(?i)--remote-allow-origins=[^\s]+'
      ))
      if ($debugSwitches.Count -ne $expectedDebugSwitchCount -or
          $allowOriginSwitches.Count -ne $expectedDebugSwitchCount -or
          [string]$browser.CommandLine -match '(?i)document-studio-hostile|remote-debugging-pipe|remote-debugging-port=65535|remote-allow-origins=\*') {
        throw 'DESKTOP_NOT_READY: inherited WebView2 environment switches survived beside the explicit test builder arguments.'
      }
      return
    }
    Start-Sleep -Milliseconds 100
  }
  throw 'DESKTOP_NOT_READY: runtime setup and an owned WebView2 browser were not ready within 30 seconds.'
}

function Wait-ForCdp {
  $deadline = [DateTime]::UtcNow.AddSeconds(45)
  $endpoint = "http://127.0.0.1:$cdpPort/json/version"
  while ([DateTime]::UtcNow -lt $deadline) {
    $desktop.Refresh()
    if ($desktop.HasExited) {
      throw "WEBVIEW2_CDP_NOT_READY: the desktop exited with code $($desktop.ExitCode)."
    }
    $text = Get-HttpResponseText -Url $endpoint
    if ($text) {
      try { $version = $text | ConvertFrom-Json } catch { $version = $null }
      $listeners = @(Get-NetTCPConnection -LocalPort $cdpPort -State Listen -ErrorAction SilentlyContinue)
      $snapshot = Get-ProcessSnapshot
      $descendants = @(Get-DescendantProcessIds -RootProcessId $desktop.Id -Snapshot $snapshot)
      $ownedListener = @($listeners | Where-Object {
        $_.LocalAddress -eq '127.0.0.1' -and $descendants -contains [int]$_.OwningProcess
      }) | Select-Object -First 1
      $listenerProcess = if ($ownedListener) {
        $snapshot | Where-Object { [int]$_.ProcessId -eq [int]$ownedListener.OwningProcess } |
          Select-Object -First 1
      }
      if ($version -and $version.Browser -and $version.webSocketDebuggerUrl -and
          $listenerProcess -and $listenerProcess.Name -eq 'msedgewebview2.exe' -and
          [string]$listenerProcess.CommandLine -match "--remote-debugging-port=$cdpPort(?: |$)") {
        $script:cdpReady = $true
        return
      }
    }
    Start-Sleep -Milliseconds 100
  }
  throw 'WEBVIEW2_CDP_NOT_READY: owned loopback WebView2 CDP did not become ready within 45 seconds.'
}

function Get-ProcessStatus {
  param([System.Diagnostics.Process]$Process)
  if ($null -eq $Process) { return 'not-started' }
  $Process.Refresh()
  if ($Process.HasExited) {
    try { $Process.WaitForExit() } catch {}
    try {
      if ($null -eq $Process.ExitCode -or [string]$Process.ExitCode -eq '') { return 'exited:unknown' }
      return "exited:$($Process.ExitCode)"
    } catch { return 'exited:unknown' }
  }
  'alive'
}

function Get-BoundedSanitizedLog {
  param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) { return '<absent>' }
  $lines = @(Get-Content -LiteralPath $Path -Tail 200 -ErrorAction SilentlyContinue)
  $text = $lines -join [Environment]::NewLine
  $text = $text.Replace($webViewCommandData, '<WEBVIEW2_UDF>')
  $text = $text.Replace($webViewData, '<WEBVIEW2_UDF>')
  $text = $text.Replace($evidenceRoot, '<EVIDENCE>')
  $text = $text.Replace($repositoryRoot, '<REPO>')
  $text = $text.Replace($repositoryRootForward, '<REPO>')
  while ([System.Text.Encoding]::UTF8.GetByteCount($text) -gt 65536 -and $text.Length -gt 1024) {
    $text = $text.Substring([Math]::Min(1024, $text.Length))
  }
  $text
}

function Write-FailureDiagnostics {
  param([string]$FailureCode)
  $owned = @(Get-OwnedWebViewProcesses)
  $actualUdf = if (@($owned | Where-Object {
    Test-ExactWebViewDataFolder -CommandLine ([string]$_.CommandLine)
  }).Count -gt 0) { '<WEBVIEW2_UDF>' } else { '<not-confirmed>' }
  $listener = if ($cdpPort) {
    @(Get-NetTCPConnection -LocalPort $cdpPort -State Listen -ErrorAction SilentlyContinue)
  } else {
    @()
  }
  Write-Output '--- G03 WEBVIEW2 SMOKE DIAGNOSTICS ---'
  Write-Output "failureCode=$FailureCode"
  Write-Output "dynamicPort=$cdpPort"
  Write-Output "vitePid=$(if ($vite) { $vite.Id } else { 'none' }) viteStatus=$(Get-ProcessStatus $vite)"
  Write-Output "desktopPid=$(if ($desktop) { $desktop.Id } else { 'none' }) desktopStatus=$(Get-ProcessStatus $desktop)"
  Write-Output "runtimeMarker=$(if ($runtimeMarker -and (Test-Path -LiteralPath $runtimeMarker)) { 'present' } else { 'absent' })"
  Write-Output "viteExactUrlReady=$viteReady cdpListenerPresent=$(@($listener).Count -gt 0) cdpReady=$cdpReady"
  Write-Output "playwrightInvoked=$playwrightInvoked"
  Write-Output "ownedWebViewPids=$((@($owned | ForEach-Object { $_.ProcessId }) -join ','))"
  foreach ($process in $owned) {
    Write-Output "webView pid=$($process.ProcessId) ppid=$($process.ParentProcessId) switches=$(Get-SanitizedRelevantSwitches ([string]$process.CommandLine))"
  }
  Write-Output "intendedWebViewData=<WEBVIEW2_UDF> actualWebViewData=$actualUdf"
  foreach ($name in @('vite.stdout.log', 'vite.stderr.log', 'desktop.stdout.log', 'desktop.stderr.log')) {
    Write-Output "--- $name (last 200 lines, max 64 KiB) ---"
    Write-Output (Get-BoundedSanitizedLog -Path (Join-Path $evidenceRoot $name))
  }
  Write-Output '--- END G03 WEBVIEW2 SMOKE DIAGNOSTICS ---'
}

function Stop-OwnedProcessTree {
  param([System.Diagnostics.Process]$RootProcess)
  if ($null -eq $RootProcess) { return }
  $snapshot = Get-ProcessSnapshot
  $ids = @(Get-DescendantProcessIds -RootProcessId $RootProcess.Id -Snapshot $snapshot) |
    Sort-Object -Descending
  foreach ($processId in $ids) {
    $process = Get-Process -Id $processId -ErrorAction SilentlyContinue
    if ($process) { Stop-Process -Id $processId -Force -ErrorAction SilentlyContinue }
  }
  try { $RootProcess.WaitForExit(10000) | Out-Null } catch {}
}

function Stop-OwnedWebViewProcesses {
  $deadline = [DateTime]::UtcNow.AddSeconds(15)
  do {
    $matching = @(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" -ErrorAction SilentlyContinue |
      Where-Object { Test-ExactWebViewDataFolder -CommandLine ([string]$_.CommandLine) })
    if ($matching.Count -eq 0) { return }
    Start-Sleep -Milliseconds 100
  } while ([DateTime]::UtcNow -lt $deadline)
  foreach ($process in $matching) {
    Stop-Process -Id $process.ProcessId -Force -ErrorAction SilentlyContinue
  }
}

function Remove-EvidenceRoot {
  if (-not (Test-Path -LiteralPath $evidenceRoot)) { return }
  $resolvedCache = [System.IO.Path]::GetFullPath($cacheRoot).TrimEnd('\') + '\'
  $resolvedEvidence = [System.IO.Path]::GetFullPath((Resolve-Path -LiteralPath $evidenceRoot).Path)
  if (-not $resolvedEvidence.StartsWith($resolvedCache, [System.StringComparison]::OrdinalIgnoreCase) -or
      [System.IO.Path]::GetFileName($resolvedEvidence) -notlike 'g03-webview2-*') {
    throw 'Refusing to remove WebView2 smoke evidence outside the owned cache boundary.'
  }
  for ($attempt = 0; $attempt -lt 20; $attempt++) {
    try {
      Remove-Item -LiteralPath $resolvedEvidence -Recurse -Force
      return
    } catch {
      if ($attempt -eq 19) { throw }
      Start-Sleep -Milliseconds 250
    }
  }
}

New-Item -ItemType Directory -Path $appData -Force | Out-Null
New-Item -ItemType Directory -Path $g04b2Output | Out-Null
New-Item -ItemType Directory -Path $webViewData | Out-Null
if (@(Get-ChildItem -LiteralPath $webViewData -Force).Count -ne 0) {
  throw 'The isolated WebView2 user-data folder was not empty before launch.'
}

foreach ($name in @(
  'DOCUMENT_STUDIO_TEST_APP_DATA',
  'DOCUMENT_STUDIO_TEST_VIEWER_PATH',
  'DOCUMENT_STUDIO_TEST_OUTPUT_DIRECTORY',
  'DOCUMENT_STUDIO_TEST_CDP_PORT',
  'DOCUMENT_STUDIO_TEST_WEBVIEW2_DATA_DIR',
  'WEBVIEW2_BROWSER_EXECUTABLE_FOLDER',
  'WEBVIEW2_USER_DATA_FOLDER',
  'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS',
  'WEBVIEW2_CHANNEL_SEARCH_KIND',
  'WEBVIEW2_RELEASE_CHANNELS',
  'WEBVIEW2_RELEASE_CHANNEL_PREFERENCE',
  'WEBVIEW2_WAIT_FOR_SCRIPT_DEBUGGER',
  'WEBVIEW2_PIPE_FOR_SCRIPT_DEBUGGER',
  'WEBVIEW2_DEFAULT_BACKGROUND_COLOR',
  'WEBVIEW2_FUTURE_HOSTILE_OVERRIDE',
  'COREWEBVIEW2_MAX_INSTANCES'
)) {
  $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}

$failure = $null
Push-Location $repositoryRoot
try {
  if (-not $SkipBuild) {
    npm run build --workspace '@document-studio/desktop'
    if ($LASTEXITCODE -ne 0) { throw 'BUILD_NOT_READY: the frontend build failed.' }
    cargo build -p document-studio --locked --features test-runtime
    if ($LASTEXITCODE -ne 0) { throw 'BUILD_NOT_READY: the test-runtime application build failed.' }
  }

  $viteArguments = @(
    'run', 'dev', '--workspace', '@document-studio/desktop', '--',
    '--host', 'localhost', '--port', '1420', '--strictPort'
  )
  if ($FailureInjection -eq 'ViteNotReady') {
    $viteArguments = @(
      'run', 'dev', '--workspace', '@document-studio/desktop', '--',
      '--host', 'localhost', '--port', '65536', '--strictPort'
    )
  }
  $vite = Start-Process -FilePath 'npm.cmd' -ArgumentList $viteArguments `
    -WorkingDirectory $repositoryRoot -WindowStyle Hidden -PassThru `
    -RedirectStandardOutput (Join-Path $evidenceRoot 'vite.stdout.log') `
    -RedirectStandardError (Join-Path $evidenceRoot 'vite.stderr.log')
  Wait-ForVite

  $reservation = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
  $reservation.Start()
  $cdpPort = ([System.Net.IPEndPoint]$reservation.LocalEndpoint).Port
  if ($cdpPort -lt 1024 -or $cdpPort -gt 65535) {
    throw 'CDP_PORT_INVALID: the operating system returned an unsafe loopback port.'
  }
  $reservationListener = @(Get-NetTCPConnection -LocalPort $cdpPort -State Listen -ErrorAction SilentlyContinue)
  if ($reservationListener.Count -ne 1 -or
      $reservationListener[0].LocalAddress -ne '127.0.0.1' -or
      [int]$reservationListener[0].OwningProcess -ne $PID) {
    throw 'CDP_PORT_INVALID: the loopback reservation was not exclusively owned by the harness.'
  }

  [Environment]::SetEnvironmentVariable('DOCUMENT_STUDIO_TEST_APP_DATA', $appData, 'Process')
  [Environment]::SetEnvironmentVariable('DOCUMENT_STUDIO_TEST_VIEWER_PATH', $fixture, 'Process')
  [Environment]::SetEnvironmentVariable(
    'DOCUMENT_STUDIO_TEST_OUTPUT_DIRECTORY',
    $g04b2Output,
    'Process'
  )
  [Environment]::SetEnvironmentVariable(
    'DOCUMENT_STUDIO_TEST_WEBVIEW2_DATA_DIR',
    $webViewData,
    'Process'
  )
  if ($FailureInjection -eq 'CdpNotReady') {
    [Environment]::SetEnvironmentVariable('DOCUMENT_STUDIO_TEST_CDP_PORT', $null, 'Process')
  } else {
    [Environment]::SetEnvironmentVariable(
      'DOCUMENT_STUDIO_TEST_CDP_PORT',
      [string]$cdpPort,
      'Process'
    )
  }
  [Environment]::SetEnvironmentVariable(
    'webview2_browser_executable_folder',
    (Join-Path $evidenceRoot 'missing-hostile-runtime'),
    'Process'
  )
  [Environment]::SetEnvironmentVariable(
    'WEBVIEW2_USER_DATA_FOLDER',
    (Join-Path $evidenceRoot 'hostile-inherited-user-data'),
    'Process'
  )
  [Environment]::SetEnvironmentVariable(
    'WebView2_Additional_Browser_Arguments',
    '--remote-debugging-pipe --remote-debugging-port=65535 --remote-allow-origins=* --document-studio-hostile-marker=sec1c',
    'Process'
  )
  [Environment]::SetEnvironmentVariable('WEBVIEW2_CHANNEL_SEARCH_KIND', '1', 'Process')
  [Environment]::SetEnvironmentVariable('WEBVIEW2_RELEASE_CHANNELS', '0', 'Process')
  [Environment]::SetEnvironmentVariable('WEBVIEW2_RELEASE_CHANNEL_PREFERENCE', '1', 'Process')
  [Environment]::SetEnvironmentVariable('WEBVIEW2_WAIT_FOR_SCRIPT_DEBUGGER', '1', 'Process')
  [Environment]::SetEnvironmentVariable(
    'WEBVIEW2_PIPE_FOR_SCRIPT_DEBUGGER',
    'document-studio-hostile-debugger-pipe',
    'Process'
  )
  [Environment]::SetEnvironmentVariable('WEBVIEW2_DEFAULT_BACKGROUND_COLOR', '00000000', 'Process')
  [Environment]::SetEnvironmentVariable(
    'WeBvIeW2_FuTuRe_HoStIlE_Override',
    'document-studio-hostile-future-value',
    'Process'
  )
  [Environment]::SetEnvironmentVariable('CoReWeBvIeW2_MaX_InStAnCeS', '1', 'Process')

  $reservation.Stop()
  $reservation = $null
  if (Get-NetTCPConnection -LocalPort $cdpPort -State Listen -ErrorAction SilentlyContinue) {
    throw 'CDP_PORT_INVALID: the reserved loopback port did not release before application launch.'
  }
  $desktop = Start-Process -FilePath (Join-Path $repositoryRoot 'target\debug\document-studio.exe') `
    -WorkingDirectory $repositoryRoot -WindowStyle Hidden -PassThru `
    -RedirectStandardOutput (Join-Path $evidenceRoot 'desktop.stdout.log') `
    -RedirectStandardError (Join-Path $evidenceRoot 'desktop.stderr.log')
  $runtimeMarker = Join-Path $appData "runtime-started-$($desktop.Id)"
  Wait-ForDesktop
  Wait-ForCdp

  $playwrightInvoked = $true
  node (Join-Path $PSScriptRoot 'webview2-smoke.mjs') |
    Tee-Object -FilePath (Join-Path $evidenceRoot 'result.json')
  if ($LASTEXITCODE -ne 0) { throw "WEBVIEW2_ASSERTION_FAILED: smoke exited with code $LASTEXITCODE." }
} catch {
  $failure = $_
  $failureCode = ([string]$_.Exception.Message -split ':', 2)[0]
  Write-FailureDiagnostics -FailureCode $failureCode
} finally {
  if ($reservation) { $reservation.Stop() }
  Stop-OwnedProcessTree -RootProcess $desktop
  Stop-OwnedWebViewProcesses
  Stop-OwnedProcessTree -RootProcess $vite
  foreach ($name in $previousEnvironment.Keys) {
    [Environment]::SetEnvironmentVariable($name, $previousEnvironment[$name], 'Process')
  }
  if ($cdpPort) {
    $portDeadline = [DateTime]::UtcNow.AddSeconds(10)
    while ((Get-NetTCPConnection -LocalPort $cdpPort -State Listen -ErrorAction SilentlyContinue) -and
           [DateTime]::UtcNow -lt $portDeadline) {
      Start-Sleep -Milliseconds 100
    }
    if (Get-NetTCPConnection -LocalPort $cdpPort -State Listen -ErrorAction SilentlyContinue) {
      if (-not $failure) { $failure = [System.Exception]::new('CLEANUP_FAILED: the dynamic CDP port remained open.') }
    }
  }
  try { Remove-EvidenceRoot } catch {
    if (-not $failure) { $failure = $_ }
  }
  Pop-Location
}

if ($failure) {
  [Console]::Error.WriteLine([string]$failure.Exception.Message)
  exit 1
}
Write-Output 'WebView2 smoke passed: dynamic loopback CDP, isolated user data, G03 range IPC, G04B2 authenticated raw pixel IPC/encoding/publication, runtime diagnostics, and owned cleanup verified.'
