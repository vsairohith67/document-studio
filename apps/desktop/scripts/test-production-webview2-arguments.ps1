param(
  [string]$Executable = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
if ([string]::IsNullOrWhiteSpace($Executable)) {
  $Executable = Join-Path $repositoryRoot 'target\release\document-studio.exe'
}
$Executable = (Resolve-Path -LiteralPath $Executable).Path
$evidenceRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
  'document-studio-production-webview2-' + [guid]::NewGuid().ToString('N')
)
$hostileUserDataFolder = Join-Path $evidenceRoot 'hostile-inherited-user-data'
$hostileRuntimeFolder = Join-Path $evidenceRoot 'missing-hostile-runtime'
$expectedProductionWebViewData = Join-Path $env:LOCALAPPDATA 'studio.document.app\EBWebView'
$environmentState = @{}
$managedEnvironment = [ordered]@{
  'webview2_browser_executable_folder' = $hostileRuntimeFolder
  'WEBVIEW2_USER_DATA_FOLDER' = $hostileUserDataFolder
  'WebView2_Additional_Browser_Arguments' = '--remote-debugging-pipe --remote-debugging-port=65535 --remote-allow-origins=* --document-studio-hostile-marker=sec1c'
  'WEBVIEW2_CHANNEL_SEARCH_KIND' = '1'
  'WEBVIEW2_RELEASE_CHANNELS' = '0'
  'WEBVIEW2_RELEASE_CHANNEL_PREFERENCE' = '1'
  'WEBVIEW2_WAIT_FOR_SCRIPT_DEBUGGER' = '1'
  'WEBVIEW2_PIPE_FOR_SCRIPT_DEBUGGER' = 'document-studio-hostile-debugger-pipe'
  'WEBVIEW2_DEFAULT_BACKGROUND_COLOR' = '00000000'
  'WeBvIeW2_FuTuRe_HoStIlE_Override' = 'document-studio-hostile-future-value'
  'CoReWeBvIeW2_MaX_InStAnCeS' = '1'
}

function Get-NormalizedPath([string]$Path) {
  $fullPath = [System.IO.Path]::GetFullPath($Path)
  if ($fullPath.StartsWith('\\?\')) { $fullPath = $fullPath.Substring(4) }
  $fullPath.TrimEnd('\')
}

function Get-ProcessSnapshot {
  @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
    Select-Object ProcessId, ParentProcessId, Name, CommandLine)
}

function Get-OwnedDescendants([int]$RootPid, [object[]]$Snapshot = $(Get-ProcessSnapshot)) {
  $ownedIds = [System.Collections.Generic.HashSet[int]]::new()
  [void]$ownedIds.Add($RootPid)
  do {
    $added = $false
    foreach ($candidate in $Snapshot) {
      if ($ownedIds.Contains([int]$candidate.ParentProcessId) -and
          $ownedIds.Add([int]$candidate.ProcessId)) {
        $added = $true
      }
    }
  } while ($added)
  @($Snapshot | Where-Object {
    [int]$_.ProcessId -ne $RootPid -and $ownedIds.Contains([int]$_.ProcessId)
  })
}

function Get-WebViewDataFolder([string]$CommandLine) {
  $match = [regex]::Match($CommandLine, '(?i)--user-data-dir=(?:"([^"]+)"|([^\s]+))')
  if (-not $match.Success) { return $null }
  if ($match.Groups[1].Success) { return $match.Groups[1].Value }
  $match.Groups[2].Value
}

function Stop-OwnedProcesses([System.Diagnostics.Process]$RootProcess, [object[]]$OwnedProcesses) {
  if ($null -ne $RootProcess) {
    try {
      $RootProcess.Refresh()
      if (-not $RootProcess.HasExited) {
        Stop-Process -Id $RootProcess.Id -Force -ErrorAction SilentlyContinue
      }
    } catch {}
  }
  foreach ($owned in @($OwnedProcesses | Sort-Object ProcessId -Descending)) {
    Stop-Process -Id ([int]$owned.ProcessId) -Force -ErrorAction SilentlyContinue
  }
  $deadline = [DateTime]::UtcNow.AddSeconds(10)
  do {
    $remaining = @($OwnedProcesses | Where-Object {
      Get-Process -Id ([int]$_.ProcessId) -ErrorAction SilentlyContinue
    })
    if ($remaining.Count -eq 0) { return }
    Start-Sleep -Milliseconds 100
  } while ([DateTime]::UtcNow -lt $deadline)
  throw "Owned WebView2 processes remained after cleanup: $($remaining.ProcessId -join ',')"
}

function Get-ProfileSnapshot([string]$Path) {
  $item = Get-Item -LiteralPath $Path
  [pscustomobject]@{
    Path = Get-NormalizedPath $item.FullName
    CreationTimeUtcTicks = $item.CreationTimeUtc.Ticks
    EntryCount = @(Get-ChildItem -LiteralPath $item.FullName -Force -ErrorAction SilentlyContinue).Count
  }
}

function Invoke-ProductionLaunch([string]$Name) {
  $stdoutPath = Join-Path $evidenceRoot "$Name.stdout.log"
  $stderrPath = Join-Path $evidenceRoot "$Name.stderr.log"
  $process = $null
  $owned = @()
  try {
    $process = Start-Process -FilePath $Executable -PassThru -WindowStyle Hidden `
      -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    $browser = $null
    while ([DateTime]::UtcNow -lt $deadline) {
      $process.Refresh()
      if ($process.HasExited) {
        throw "Production application exited before WebView2 started in $Name (exit $($process.ExitCode))."
      }
      $owned = @(Get-OwnedDescendants -RootPid $process.Id)
      $browser = @($owned | Where-Object {
        $_.Name -ieq 'msedgewebview2.exe' -and
        [string]$_.CommandLine -notmatch '(?i)(?:^|\s)--type='
      } | Select-Object -First 1)
      if ($browser.Count -eq 1) { break }
      Start-Sleep -Milliseconds 200
    }
    if ($browser.Count -ne 1) {
      throw "No owned production WebView2 browser process appeared in $Name."
    }

    $commandLine = [string]$browser[0].CommandLine
    $actualDataFolder = Get-WebViewDataFolder -CommandLine $commandLine
    if ([string]::IsNullOrWhiteSpace($actualDataFolder)) {
      throw "The production WebView2 browser did not expose its user-data boundary in $Name."
    }
    $actualDataFolder = Get-NormalizedPath $actualDataFolder
    if ($actualDataFolder -ne (Get-NormalizedPath $expectedProductionWebViewData)) {
      throw "Production WebView2 did not use the application-owned persistent profile in $Name."
    }
    if ($actualDataFolder -eq (Get-NormalizedPath $hostileUserDataFolder) -or
        $actualDataFolder.StartsWith((Get-NormalizedPath $evidenceRoot) + '\', [System.StringComparison]::OrdinalIgnoreCase)) {
      throw "Production WebView2 accepted the inherited hostile user-data folder in $Name."
    }
    foreach ($forbidden in @(
      '(?i)--remote-debugging-',
      '(?i)--remote-allow-origins',
      '(?i)document-studio-hostile',
      [regex]::Escape($hostileRuntimeFolder),
      [regex]::Escape($hostileUserDataFolder)
    )) {
      if ($commandLine -match $forbidden) {
        throw "Production WebView2 inherited a forbidden environment control in $Name."
      }
    }
    $profile = Get-ProfileSnapshot -Path $actualDataFolder
    if ($profile.EntryCount -eq 0) {
      throw "The application-owned production WebView2 profile was not populated in $Name."
    }
    Write-Output "Production WebView2 ready: case=$Name; appPid=$($process.Id); browserPid=$($browser[0].ProcessId); profile=<APP_OWNED_WEBVIEW2_PROFILE>."
    $profile
  } finally {
    if ($null -ne $process) {
      $owned = @($owned + @(Get-OwnedDescendants -RootPid $process.Id) |
        Sort-Object ProcessId -Unique)
    }
    Stop-OwnedProcesses -RootProcess $process -OwnedProcesses $owned
  }
}

New-Item -ItemType Directory -Path $evidenceRoot | Out-Null
New-Item -ItemType Directory -Path $hostileUserDataFolder | Out-Null

try {
  foreach ($name in $managedEnvironment.Keys) {
    $environmentState[$name] = [pscustomobject]@{
      Exists = Test-Path -LiteralPath "Env:$name"
      Value = [Environment]::GetEnvironmentVariable($name, 'Process')
    }
    [Environment]::SetEnvironmentVariable($name, $managedEnvironment[$name], 'Process')
  }

  $first = @(Invoke-ProductionLaunch -Name 'first') | Select-Object -Last 1
  if (-not (Test-Path -LiteralPath $first.Path -PathType Container)) {
    throw 'The persistent production profile disappeared after the first launch.'
  }
  $second = @(Invoke-ProductionLaunch -Name 'second') | Select-Object -Last 1
  if ($first.Path -ne $second.Path -or
      $first.CreationTimeUtcTicks -ne $second.CreationTimeUtcTicks) {
    throw 'Repeated production launch did not retain the same application-owned WebView2 profile.'
  }
  if (-not (Test-Path -LiteralPath $second.Path -PathType Container)) {
    throw 'The persistent production profile disappeared after the repeated launch.'
  }
  Write-Output 'Production WebView2 environment smoke passed: every inherited override was ignored, no CDP/debug switch survived, and two ordinary launches retained one application-owned persistent profile.'
} finally {
  foreach ($name in $managedEnvironment.Keys) {
    $saved = $environmentState[$name]
    if ($saved.Exists) {
      [Environment]::SetEnvironmentVariable($name, $saved.Value, 'Process')
    } else {
      [Environment]::SetEnvironmentVariable($name, $null, 'Process')
    }
  }
  $resolvedEvidence = [System.IO.Path]::GetFullPath((Resolve-Path -LiteralPath $evidenceRoot).Path)
  $temporaryRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()).TrimEnd('\') + '\'
  if (-not $resolvedEvidence.StartsWith($temporaryRoot, [System.StringComparison]::OrdinalIgnoreCase) -or
      [System.IO.Path]::GetFileName($resolvedEvidence) -notlike 'document-studio-production-webview2-*') {
    throw 'Refusing to remove production WebView2 evidence outside the owned temporary boundary.'
  }
  Remove-Item -LiteralPath $resolvedEvidence -Recurse -Force -ErrorAction SilentlyContinue
}
