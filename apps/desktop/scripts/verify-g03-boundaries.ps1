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

$tauri = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\tauri.conf.json') | ConvertFrom-Json
if ($tauri.app.security.csp -ne $expectedCsp) {
  throw 'The reviewed G03 CSP changed.'
}
$capability = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\capabilities\default.json') | ConvertFrom-Json
if (($capability.permissions -join '|') -ne 'core:default|dialog:allow-open') {
  throw 'G03 must not expand or remove the accepted Tauri capabilities.'
}

$ipc = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\ipc.rs')
if ($ipc -notmatch 'Result<Response, OperationError>' -or $ipc -notmatch '\.map\(Response::new\)') {
  throw 'viewer_read_range must return raw tauri::ipc::Response bytes.'
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
  if (($productionJavaScript -join "`n") -match 'viewer_open_test_fixture') {
    throw 'The test-only native fixture command leaked into the production frontend bundle.'
  }
}
$browserTransportSource = Get-Content -Raw (Join-Path $desktopRoot 'src\viewer\browserTestTransport.ts')
if ($browserTransportSource -notmatch "import\.meta\.env\.MODE === 'test-browser'") {
  throw 'The browser test transport is not compile-time gated to test-browser mode.'
}

Write-Output 'G03 dependency pins, worker parity, CSP, capability, raw IPC and production/test boundaries verified.'
