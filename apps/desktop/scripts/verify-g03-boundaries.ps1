$ErrorActionPreference = 'Stop'

$desktopRoot = Split-Path -Parent $PSScriptRoot
$repositoryRoot = Resolve-Path (Join-Path $desktopRoot '..\..')
$expectedCsp = "default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; font-src 'self' data: blob:; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-src 'none'; form-action 'none';"

function Get-Sha256File([string]$Path) {
  $stream = [System.IO.File]::OpenRead($Path)
  $hasher = [System.Security.Cryptography.SHA256]::Create()
  try {
    return ([System.BitConverter]::ToString($hasher.ComputeHash($stream))).Replace('-', '').ToLowerInvariant()
  } finally {
    $hasher.Dispose()
    $stream.Dispose()
  }
}

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

$pdfManifestPath = Join-Path $desktopRoot 'scripts\pdfjs-assets-6.2.108.json'
$stagedPdfRoot = Join-Path $desktopRoot 'public\pdfjs'
$stagedPdfManifestPath = Join-Path $stagedPdfRoot 'pdfjs-manifest.json'
$pdfManifestText = Get-Content -Raw $pdfManifestPath
$pdfManifest = $pdfManifestText | ConvertFrom-Json
if ($pdfManifest.version -ne '6.2.108' -or $pdfManifest.build -ne 'legacy' -or !$pdfManifest.localOnly) {
  throw 'The staged PDF.js manifest does not match the reviewed local legacy build.'
}
if ((Get-Content -Raw $stagedPdfManifestPath) -ne $pdfManifestText) {
  throw 'The staged PDF.js manifest differs from the checked-in exact manifest.'
}
$expectedPdfPaths = @($pdfManifest.files | ForEach-Object { $_.path }) + @('pdfjs-manifest.json') | Sort-Object
$resolvedStagedPdfRoot = (Resolve-Path -LiteralPath $stagedPdfRoot).Path.TrimEnd('\')
$actualPdfPaths = Get-ChildItem -LiteralPath $stagedPdfRoot -File -Recurse | ForEach-Object {
  $_.FullName.Substring($resolvedStagedPdfRoot.Length + 1).Replace('\', '/')
} | Sort-Object
if ((Compare-Object $expectedPdfPaths $actualPdfPaths).Count -ne 0) {
  throw 'The staged PDF.js directory contains missing or unexpected files.'
}
foreach ($file in $pdfManifest.files) {
  if ($file.path -match '(?i)(\.map$|debug|viewer\.html|quickjs|nowasm_fallback|(^|/)web/|(^|/)test/|(^|/)examples/)') {
    throw "The PDF.js exact manifest contains a forbidden asset: $($file.path)"
  }
  $path = Join-Path $stagedPdfRoot ($file.path.Replace('/', '\'))
  $status = Get-Item -LiteralPath $path
  $hash = Get-Sha256File $path
  if ($status.Length -ne $file.sizeBytes -or $hash -ne $file.sha256) {
    throw "The PDF.js staged asset failed exact size/hash verification: $($file.path)"
  }
}
$worker = @($pdfManifest.files | Where-Object { $_.path -eq 'pdf.worker.mjs' })
if ($worker.Count -ne 1 -or $worker[0].sha256 -ne 'b4e582882f5e811f4d1b7b511f68d9a0c3209141e6f68856f01408c5cc155131') {
  throw 'The PDF.js worker hash changed or is not uniquely allow-listed.'
}

$tauriConfigurationPath = Join-Path $desktopRoot 'src-tauri\tauri.conf.json'
$tauriConfigurationHash = Get-Sha256File $tauriConfigurationPath
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
if ($runtimeSource -notmatch 'to_ascii_lowercase' -or
    $runtimeSource -notmatch 'contains\("--remote-debugging-"\)' -or
    $runtimeSource -notmatch 'contains\("--remote-allow-origins"\)' -or
    $runtimeSource -match 'split_whitespace\(\)\s*\.filter') {
  throw 'Production WebView2 arguments are not cleared by the fail-closed remote-debug family sanitizer.'
}
$webViewSmoke = Get-Content -Raw (Join-Path $desktopRoot 'scripts\test-webview2-smoke.ps1')
if ($webViewSmoke -notmatch '\[System\.Net\.Sockets\.TcpListener\]::new\(\[System\.Net\.IPAddress\]::Loopback, 0\)' -or
    $webViewSmoke -notmatch "'DOCUMENT_STUDIO_TEST_WEBVIEW2_DATA_DIR'" -or
    $webViewSmoke -notmatch "'DOCUMENT_STUDIO_TEST_CDP_PORT'" -or
    $webViewSmoke -match 'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS|WEBVIEW2_USER_DATA_FOLDER' -or
    $webViewSmoke -match '--remote-allow-origins=\*|\b9333\b') {
  throw 'The WebView2 smoke must use the test builder, dynamic loopback CDP and an isolated profile.'
}
$productionWebViewSmoke = Get-Content -Raw (Join-Path $desktopRoot 'scripts\test-production-webview2-arguments.ps1')
if ($productionWebViewSmoke -notmatch 'Get-OwnedDescendants' -or
    $productionWebViewSmoke -notmatch 'document-studio-production-webview2-' -or
    $productionWebViewSmoke -notmatch 'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS' -or
    $productionWebViewSmoke -notmatch '\(\?i\)--remote-debugging-\|--remote-allow-origins' -or
    $productionWebViewSmoke -match 'Get-Process\s+msedgewebview2|taskkill') {
  throw 'The production WebView2 argument smoke must isolate profiles and inspect/clean only owned descendants.'
}
$viewerSource = Get-ChildItem -LiteralPath (Join-Path $desktopRoot 'src\viewer') -File -Recurse |
  Where-Object { $_.Extension -in @('.ts', '.tsx') } |
  ForEach-Object { Get-Content -Raw -LiteralPath $_.FullName }
if (($viewerSource -join "`n") -match '(?i)https?://|file://') {
  throw 'Viewer production source contains a network or file URL.'
}
$contractSource = Get-Content -Raw (Join-Path $repositoryRoot 'packages\contracts\src\index.ts')
$rustContractSource = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\contracts.rs')
$rustViewerSource = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\viewer_sessions.rs')
$pdfSessionSource = Get-Content -Raw (Join-Path $desktopRoot 'src\viewer\pdfSession.ts')
$pdfSessionTests = Get-Content -Raw (Join-Path $desktopRoot 'src\viewer\pdfSession.test.ts')
$pdfMergeSource = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\pdf_merge.rs')
if ($contractSource -notmatch 'CORE_PDF_MAX_PAGES = 4096' -or
    $rustContractSource -notmatch 'CORE_PDF_MAX_PAGES: u32 = 4096' -or
    $pdfSessionSource -notmatch 'validatePdfPageCount\(document\.numPages\);' -or
    $pdfSessionSource -notmatch 'PDF_PAGE_COUNT_UNSUPPORTED' -or
    $pdfSessionTests -notmatch 'CORE_PDF_MAX_PAGES \+ 1' -or
    $pdfMergeSource -notmatch 'page_count == 0 \|\| page_count > u64::from\(CORE_PDF_MAX_PAGES\)' -or
    $pdfMergeSource -notmatch '\.try_reserve_exact\(page_count\)') {
  throw 'PDF page counts are not rejected at the PDF.js/qpdf boundary before page-sized allocation.'
}
if ($contractSource -notmatch 'RANGE_CHUNK_BYTES = 256 \* 1024' -or
    $contractSource -notmatch 'MAX_RANGE_READS = 4' -or
    $contractSource -notmatch 'MAX_QUEUED_RANGE_COUNT = 64' -or
    $contractSource -notmatch 'MAX_QUEUED_RANGE_BYTES = 16 \* 1024 \* 1024' -or
    $rustViewerSource -notmatch 'VIEWER_RANGE_CHUNK_BYTES: u64 = 256 \* 1024' -or
    $pdfSessionSource -notmatch 'PDF_RANGE_QUEUE_LIMIT_EXCEEDED' -or
    $pdfSessionSource -notmatch 'checkedQueueTotal' -or
    $pdfSessionSource -notmatch 'transportEpoch' -or
    $pdfSessionSource -notmatch 'references \+= 1' -or
    $pdfSessionTests -notmatch 'rejects count plus one' -or
    $pdfSessionTests -notmatch 'rejects bytes plus one' -or
    $pdfSessionTests -notmatch 'replacement document' -or
    $pdfSessionTests -notmatch 'sanitizes native range failures') {
  throw 'PDF range queue admission, deduplication, cancellation, or cross-language chunk limits drifted.'
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
  if (-not $releaseText.Contains('--remote-debugging-') -or
      -not $releaseText.Contains('--remote-allow-origins')) {
    throw 'The production executable no longer contains the required remote-debug argument deny-list.'
  }
}
$browserTransportSource = Get-Content -Raw (Join-Path $desktopRoot 'src\viewer\browserTestTransport.ts')
if ($browserTransportSource -notmatch "import\.meta\.env\.MODE === 'test-browser'") {
  throw 'The browser test transport is not compile-time gated to test-browser mode.'
}

Write-Output 'G03 dependency pins, exact PDF.js assets, 4,096-page admission bound, CSP, capability, raw IPC and production/test boundaries verified; production remote-debug arguments fail closed.'
