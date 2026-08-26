$ErrorActionPreference = 'Stop'

$desktopRoot = Split-Path -Parent $PSScriptRoot
$qpdfSource = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\qpdf.rs')
$mergeSource = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\pdf_merge.rs')
$ipcSource = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\ipc.rs')
$registrySource = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\operation_registry.rs')
$optimizeSource = Get-Content -Raw (Join-Path $desktopRoot 'src\OptimizeWorkspace.tsx')
$contractSource = Get-Content -Raw (Join-Path $desktopRoot '..\..\packages\contracts\src\index.ts')

$builderMatch = [regex]::Match(
  $qpdfSource,
  '(?s)pub fn build_lossless_compression_arguments\(.+?\n\}'
)
if (!$builderMatch.Success) {
  throw 'The G04A qpdf argument builder is missing.'
}
$builder = $builderMatch.Value
$orderedArguments = @(
  'input_relative_path.as_os_str().to_owned()',
  'OsString::from("--stream-data=compress")',
  'OsString::from("--object-streams=generate")',
  'OsString::from("--recompress-flate")',
  'OsString::from("--compression-level=9")',
  'staging_relative_path.as_os_str().to_owned()'
)
$position = -1
foreach ($argument in $orderedArguments) {
  $next = $builder.IndexOf($argument, $position + 1, [System.StringComparison]::Ordinal)
  if ($next -lt 0) { throw "The exact G04A qpdf argv entry is missing or out of order: $argument" }
  $position = $next
}
foreach ($forbidden in @(
  '--remove-info', '--remove-metadata', '--remove-page-labels', '--flatten-annotations',
  'cmd.exe', 'powershell.exe', 'Command::new', 'std::process::Command'
)) {
  if ($builder.Contains($forbidden)) { throw "The G04A qpdf boundary contains forbidden token: $forbidden" }
}

if ($ipcSource -notmatch 'OperationKind::PdfCompressLossless' -or
    $ipcSource -notmatch 'PdfCompressionService::new' -or
    $registrySource -notmatch 'pdf\.compress-lossless' -or
    $contractSource -notmatch "operationId: 'pdf\.compress-lossless'" -or
    $contractSource -notmatch 'inputPaths: \[string\]') {
  throw 'The exact typed public G04A operation is not wired end to end.'
}
foreach ($required in @(
  'qpdf_structural_inventory', 'qpdf_annotation_inventory', 'verify_source_unchanged',
  'publish_verified_staging_with_observer', 'COMPRESSED_STAGING_RELATIVE_PATH'
)) {
  if ($mergeSource -notmatch [regex]::Escape($required)) {
    throw "The G04A verification/publication boundary is missing $required."
  }
}

$capability = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\capabilities\default.json') | ConvertFrom-Json
if (($capability.permissions -join '|') -ne 'core:default|dialog:allow-open') {
  throw 'G04A changed the accepted minimum Tauri capability set.'
}
$cargo = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\Cargo.toml')
foreach ($forbiddenDependency in @('libvips', 'pdfium', 'libreoffice', 'reqwest', 'ocrmypdf', 'tesseract')) {
  if ($cargo -match "(?im)^\s*$forbiddenDependency\s*=") {
    throw "G04A added forbidden dependency $forbiddenDependency."
  }
}
foreach ($forbiddenUi in @('aggressive compression', 'batch mode')) {
  if ($optimizeSource -match [regex]::Escape($forbiddenUi)) {
    throw "The G04A UI contains excluded scope: $forbiddenUi"
  }
}
if ($optimizeSource -notmatch 'rewriting invalidates existing digital signatures' -or
    $optimizeSource -notmatch 'smaller, unchanged, or larger' -or
    $optimizeSource -notmatch 'pdf\.compress-lossless') {
  throw 'The G04A UI is missing signature or truthful size behavior.'
}

Write-Output 'G04A exact direct qpdf argv, typed one-PDF contract, accepted sandbox/publication reuse, minimum capability set, verification inventory, and scope exclusions verified.'
