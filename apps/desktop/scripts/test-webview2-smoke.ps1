$ErrorActionPreference = 'Stop'

$desktopRoot = Split-Path -Parent $PSScriptRoot
$repositoryRoot = Resolve-Path (Join-Path $desktopRoot '..\..')
$evidenceRoot = Join-Path $repositoryRoot ('.cache\g03-webview2-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $evidenceRoot | Out-Null
$appData = Join-Path $evidenceRoot 'app-data'
New-Item -ItemType Directory -Path $appData | Out-Null
$fixture = Resolve-Path (Join-Path $repositoryRoot 'report\Document_Studio_Master_Blueprint.pdf')
$cdpPort = 9333
if (Get-NetTCPConnection -LocalPort $cdpPort -State Listen -ErrorAction SilentlyContinue) {
  throw "The isolated WebView2 test port $cdpPort is already in use."
}

Push-Location $repositoryRoot
try {
  npm run build --workspace '@document-studio/desktop'
  cargo build -p document-studio --locked --features test-runtime

  $vite = Start-Process -FilePath 'npm.cmd' `
    -ArgumentList @('run', 'dev', '--workspace', '@document-studio/desktop', '--', '--host', '127.0.0.1') `
    -WorkingDirectory $repositoryRoot -WindowStyle Hidden -PassThru `
    -RedirectStandardOutput (Join-Path $evidenceRoot 'vite.stdout.log') `
    -RedirectStandardError (Join-Path $evidenceRoot 'vite.stderr.log')
  for ($attempt = 0; $attempt -lt 100; $attempt++) {
    try {
      if ((Invoke-WebRequest -UseBasicParsing 'http://127.0.0.1:1420').StatusCode -eq 200) { break }
    } catch {}
    Start-Sleep -Milliseconds 100
  }

  $env:DOCUMENT_STUDIO_TEST_APP_DATA = $appData
  $env:DOCUMENT_STUDIO_TEST_VIEWER_PATH = $fixture
  $env:DOCUMENT_STUDIO_TEST_CDP_PORT = [string]$cdpPort
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$cdpPort --remote-allow-origins=http://127.0.0.1:$cdpPort"
  $desktop = Start-Process -FilePath (Join-Path $repositoryRoot 'target\debug\document-studio.exe') `
    -WorkingDirectory $repositoryRoot -WindowStyle Hidden -PassThru `
    -RedirectStandardOutput (Join-Path $evidenceRoot 'desktop.stdout.log') `
    -RedirectStandardError (Join-Path $evidenceRoot 'desktop.stderr.log')
  for ($attempt = 0; $attempt -lt 200; $attempt++) {
    try {
      if ((Invoke-RestMethod "http://127.0.0.1:$cdpPort/json/version").Browser) { break }
    } catch {}
    if ($desktop.HasExited) { throw 'The test-runtime desktop process exited before WebView2 CDP became ready.' }
    Start-Sleep -Milliseconds 100
  }
  node (Join-Path $PSScriptRoot 'webview2-smoke.mjs') | Tee-Object -FilePath (Join-Path $evidenceRoot 'result.json')
  if ($LASTEXITCODE -ne 0) { throw "WebView2 smoke failed with exit code $LASTEXITCODE." }
} finally {
  if ($desktop -and !$desktop.HasExited) { Stop-Process -Id $desktop.Id -Force }
  $viteListener = Get-NetTCPConnection -LocalPort 1420 -State Listen -ErrorAction SilentlyContinue
  if ($viteListener) { Stop-Process -Id $viteListener.OwningProcess -Force }
  Remove-Item Env:\DOCUMENT_STUDIO_TEST_APP_DATA -ErrorAction SilentlyContinue
  Remove-Item Env:\DOCUMENT_STUDIO_TEST_VIEWER_PATH -ErrorAction SilentlyContinue
  Remove-Item Env:\DOCUMENT_STUDIO_TEST_CDP_PORT -ErrorAction SilentlyContinue
  Remove-Item Env:\WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue
  Pop-Location
  Write-Output "WebView2 smoke evidence: $evidenceRoot"
}
