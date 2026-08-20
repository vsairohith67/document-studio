$ErrorActionPreference = 'Stop'

$desktopRoot = Split-Path -Parent $PSScriptRoot
$repositoryRoot = Resolve-Path (Join-Path $desktopRoot '..\..')
$expectedCsp = "default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; font-src 'self' data: blob:; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-src 'none'; form-action 'none';"

$package = Get-Content -Raw (Join-Path $desktopRoot 'package.json') | ConvertFrom-Json
$lockPath = Join-Path $repositoryRoot 'package-lock.json'
$lockSummaryJson = & node -e "const fs=require('fs');const l=JSON.parse(fs.readFileSync(process.argv[1],'utf8'));const p=l.packages;console.log(JSON.stringify({pdfjs:p['node_modules/pdfjs-dist']?.version,tanstack:p['node_modules/@tanstack/react-virtual']?.version,playwrightTest:p['node_modules/@playwright/test']?.version,virtualCore:p['node_modules/@tanstack/virtual-core']?.version,playwright:p['node_modules/playwright']?.version,playwrightCore:p['node_modules/playwright-core']?.version}))" $lockPath
if ($LASTEXITCODE -ne 0) { throw 'Node could not inspect package-lock.json.' }
$locked = $lockSummaryJson | ConvertFrom-Json
$expectedPackages = @(
  [pscustomobject]@{ Name = 'pdfjs-dist'; Version = '6.2.108'; Locked = $locked.pdfjs; Development = $false },
  [pscustomobject]@{ Name = '@tanstack/react-virtual'; Version = '3.14.9'; Locked = $locked.tanstack; Development = $false },
  [pscustomobject]@{ Name = '@playwright/test'; Version = '1.62.1'; Locked = $locked.playwrightTest; Development = $true }
)
foreach ($entry in $expectedPackages) {
  $manifestVersion = if ($entry.Development) {
    $package.devDependencies.($entry.Name)
  } else {
    $package.dependencies.($entry.Name)
  }
  if ($manifestVersion -ne $entry.Version) {
    throw "$($entry.Name) must be pinned exactly to $($entry.Version)."
  }
  if ($entry.Locked -ne $entry.Version) {
    throw "$($entry.Name) lock version $($entry.Locked) does not match $($entry.Version)."
  }
}
if ($locked.virtualCore -ne '3.17.7') {
  throw 'The reviewed TanStack virtual-core resolution changed.'
}
if ($locked.playwright -ne '1.62.1' -or $locked.playwrightCore -ne '1.62.1') {
  throw 'The reviewed Playwright transitive resolution changed.'
}

$pdfManifestPath = Join-Path $desktopRoot 'public\pdfjs\pdfjs-manifest.json'
$pdfManifest = Get-Content -Raw $pdfManifestPath | ConvertFrom-Json
$workerPath = Join-Path $desktopRoot 'public\pdfjs\pdf.worker.mjs'
if ($pdfManifest.version -ne '6.2.108' -or $pdfManifest.build -ne 'legacy' -or !$pdfManifest.localOnly) {
  throw 'The staged PDF.js manifest does not match the reviewed local legacy build.'
}
$sha256 = [System.Security.Cryptography.SHA256]::Create()
$workerStream = [System.IO.File]::OpenRead($workerPath)
try {
  $workerHash = ([System.BitConverter]::ToString($sha256.ComputeHash($workerStream))).Replace('-', '').ToLowerInvariant()
} finally {
  $workerStream.Dispose()
  $sha256.Dispose()
}
if ($workerHash -ne $pdfManifest.workerSha256 -or
    $workerHash -ne 'b4e582882f5e811f4d1b7b511f68d9a0c3209141e6f68856f01408c5cc155131') {
  throw 'The PDF.js worker hash changed or does not match its staged manifest.'
}

$tauriConfigurationPath = Join-Path $desktopRoot 'src-tauri\tauri.conf.json'
$tauriConfigurationStream = [System.IO.File]::OpenRead($tauriConfigurationPath)
$tauriConfigurationHasher = [System.Security.Cryptography.SHA256]::Create()
try {
  $tauriConfigurationHash = ([System.BitConverter]::ToString(
    $tauriConfigurationHasher.ComputeHash($tauriConfigurationStream)
  )).Replace('-', '').ToLowerInvariant()
} finally {
  $tauriConfigurationHasher.Dispose()
  $tauriConfigurationStream.Dispose()
}
if ($tauriConfigurationHash -ne 'b97a850398a47fb391bae2f076ea7e37775c5fc88b6de264cadd5f38638e4217') {
  throw 'tauri.conf.json changed from the manually tested G03 production configuration.'
}
$tauri = Get-Content -Raw $tauriConfigurationPath | ConvertFrom-Json
if ($tauri.app.security.csp -ne $expectedCsp) {
  throw 'The reviewed G03 CSP changed.'
}
if (@($tauri.app.windows).Count -ne 1 -or
    $tauri.app.windows[0].label -and $tauri.app.windows[0].label -ne 'main' -or
    $tauri.app.windows[0].create -eq $false) {
  throw 'Production must retain exactly one automatically created main window.'
}
$capability = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\capabilities\default.json') | ConvertFrom-Json
if (($capability.permissions -join '|') -ne 'core:default|dialog:allow-open') {
  throw 'G03 must not expand or remove the accepted Tauri capabilities.'
}

$ipc = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\ipc.rs')
if ($ipc -notmatch 'Result<Response, OperationError>' -or $ipc -notmatch '\.map\(Response::new\)') {
  throw 'viewer_read_range must return raw tauri::ipc::Response bytes.'
}
$runtimeSource = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\lib.rs')
foreach ($testOnlyDeclaration in @(
  'const TEST_WEBVIEW2_DATA_DIRECTORY_ENV: &str = "DOCUMENT_STUDIO_TEST_WEBVIEW2_DATA_DIR";',
  'const TEST_WEBVIEW2_CDP_PORT_ENV: &str = "DOCUMENT_STUDIO_TEST_CDP_PORT";',
  'const TEST_APP_DATA_ENV: &str = "DOCUMENT_STUDIO_TEST_APP_DATA";',
  'const WEBVIEW2_REQUIRED_DISABLED_FEATURES: &str ='
)) {
  $escapedDeclaration = [regex]::Escape($testOnlyDeclaration)
  if ($runtimeSource -notmatch "#\[cfg\(feature = `"test-runtime`"\)\]\s+$escapedDeclaration") {
    throw "Test-only WebView2 declaration is not feature gated: $testOnlyDeclaration"
  }
}
if ($runtimeSource -match 'WEBVIEW2_USER_DATA_FOLDER' -or
    $runtimeSource -notmatch '#\[cfg\(not\(feature = "test-runtime"\)\)\]\s+fn remove_remote_debugging_arguments' -or
    $runtimeSource -notmatch '#\[cfg\(feature = "test-runtime"\)\]\s+let test_webview_override = prepare_test_webview_context' -or
    $runtimeSource -notmatch 'WebviewWindowBuilder::from_config' -or
    $runtimeSource -notmatch '\.data_directory\(test_webview_override\.settings\.data_directory\.clone\(\)\)' -or
    $runtimeSource -notmatch '\.additional_browser_args\(arguments\)' -or
    $runtimeSource -notmatch '\.run\(context\)') {
  throw 'Runtime source does not preserve the production/test WebView2 window boundary.'
}
$webViewSmoke = Get-Content -Raw (Join-Path $desktopRoot 'scripts\test-webview2-smoke.ps1')
if ($webViewSmoke -notmatch '\[System\.Net\.Sockets\.TcpListener\]::new\(\[System\.Net\.IPAddress\]::Loopback, 0\)' -or
    $webViewSmoke -notmatch "'DOCUMENT_STUDIO_TEST_WEBVIEW2_DATA_DIR'" -or
    $webViewSmoke -notmatch "'DOCUMENT_STUDIO_TEST_CDP_PORT'" -or
    $webViewSmoke -match 'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS|WEBVIEW2_USER_DATA_FOLDER' -or
    $webViewSmoke -match '--remote-allow-origins=\*|\b9333\b') {
  throw 'The WebView2 smoke must use the test builder, dynamic loopback CDP and an isolated profile.'
}
$viewerSource = Get-ChildItem -LiteralPath (Join-Path $desktopRoot 'src\viewer') -File -Recurse |
  Where-Object { $_.Extension -in @('.ts', '.tsx') } |
  ForEach-Object { Get-Content -Raw -LiteralPath $_.FullName }
if (($viewerSource -join "`n") -match '(?i)https?://|file://') {
  throw 'Viewer production source contains a network or file URL.'
}

$distAssets = Join-Path $desktopRoot 'dist\assets'
if (Test-Path -LiteralPath $distAssets) {
  $productionJavaScript = Get-ChildItem -LiteralPath $distAssets -Filter '*.js' -File |
    ForEach-Object { Get-Content -Raw -LiteralPath $_.FullName }
  $productionJavaScriptText = $productionJavaScript -join "`n"
  if ($productionJavaScriptText -match 'viewer_open_test_fixture|DOCUMENT_STUDIO_TEST_|WEBVIEW2_USER_DATA_FOLDER|remote-debugging-port|remote-allow-origins|browserTestTransport') {
    throw 'A WebView2 smoke hook leaked into the production frontend bundle.'
  }
}
$releaseBinary = Join-Path $repositoryRoot 'target\release\document-studio.exe'
if (Test-Path -LiteralPath $releaseBinary) {
  $releaseText = [System.Text.Encoding]::ASCII.GetString(
    [System.IO.File]::ReadAllBytes($releaseBinary)
  )
  foreach ($forbidden in @(
    'DOCUMENT_STUDIO_TEST_CDP_PORT',
    'DOCUMENT_STUDIO_TEST_WEBVIEW2_DATA_DIR',
    'DOCUMENT_STUDIO_TEST_APP_DATA',
    'WEBVIEW2_USER_DATA_FOLDER',
    'VITE_NOT_READY',
    'WEBVIEW2_CDP_NOT_READY',
    'g03-webview2-',
    'viewer_open_test_fixture',
    'browserTestTransport',
    'playwright'
  )) {
    if ($releaseText.Contains($forbidden)) {
      throw "The production executable contains test-only WebView2 token $forbidden."
    }
  }
  if (-not $releaseText.Contains('--remote-debugging-port') -or
      -not $releaseText.Contains('--remote-allow-origins')) {
    throw 'The production executable no longer contains the required remote-debug argument deny-list.'
  }
}
$browserTransportSource = Get-Content -Raw (Join-Path $desktopRoot 'src\viewer\browserTestTransport.ts')
if ($browserTransportSource -notmatch "import\.meta\.env\.MODE === 'test-browser'") {
  throw 'The browser test transport is not compile-time gated to test-browser mode.'
}

Write-Output 'G03 dependency pins, worker parity, CSP, capability, raw IPC and production/test boundaries verified; production remote-debug tokens are deny-list only.'
