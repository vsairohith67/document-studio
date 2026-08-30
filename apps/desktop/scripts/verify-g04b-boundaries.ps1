$ErrorActionPreference = 'Stop'

$desktopRoot = Split-Path -Parent $PSScriptRoot
$repoRoot = (Resolve-Path (Join-Path $desktopRoot '..\..')).Path
$cargo = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\Cargo.toml')
$lock = Get-Content -Raw (Join-Path $repoRoot 'Cargo.lock')
$contracts = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\contracts.rs')
$writer = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\image_to_pdf.rs')
$imageTests = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\tests\image_to_pdf.rs')
$viewerTests = Get-Content -Raw (Join-Path $desktopRoot 'e2e\viewer.spec.ts')
$registry = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\operation_registry.rs')
$migration = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\migrations\0005_job_operation_specs_and_warnings.sql')
$convert = Get-Content -Raw (Join-Path $desktopRoot 'src\ConvertWorkspace.tsx')
$contractSource = Get-Content -Raw (Join-Path $repoRoot 'packages\contracts\src\index.ts')
$package = Get-Content -Raw (Join-Path $desktopRoot 'package.json')
$visualProducer = Get-Content -Raw (Join-Path $desktopRoot 'scripts\prepare-g04b-visual-evidence.mjs')

$exactManifestLines = @(
  'flate2 = { version = "=1.1.9", default-features = false, features = ["rust_backend"] }',
  'image = { version = "=0.25.10", default-features = false, features = ["jpeg", "png", "webp"] }',
  'pdf-writer = { version = "=0.15.0", default-features = false }'
)
foreach ($line in $exactManifestLines) {
  if (!$cargo.Contains($line)) { throw "G04B exact narrow dependency declaration is missing: $line" }
}

$exactLockEntries = @(
  @('image', '0.25.10', '85ab80394333c02fe689eaf900ab500fbd0c2213da414687ebf995a65d5a6104'),
  @('pdf-writer', '0.15.0', 'f5e456864a7a304047bff84977dc6fb162bd956475d40ba50b2dcecaada7f753'),
  @('flate2', '1.1.9', '843fba2746e448b37e26a819579957415c8cef339bf08564fe8b7ddbd959573c')
)
foreach ($entry in $exactLockEntries) {
  $pattern = '(?s)\[\[package\]\]\r?\nname = "' + [regex]::Escape($entry[0]) + '"\r?\nversion = "' + [regex]::Escape($entry[1]) + '".*?checksum = "' + $entry[2] + '"'
  if ($lock -notmatch $pattern) { throw "G04B lock entry/checksum is missing for $($entry[0]) $($entry[1])." }
}

foreach ($forbiddenDependency in @('libvips', 'pdfium', 'mupdf', 'poppler', 'ghostscript', 'reqwest')) {
  if ($cargo -match "(?im)^\s*$forbiddenDependency\s*=") {
    throw "G04B added forbidden or unreviewed dependency $forbiddenDependency."
  }
}

$capability = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\capabilities\default.json') | ConvertFrom-Json
if (($capability.permissions -join '|') -ne 'core:default|dialog:allow-open') {
  throw 'G04B changed the accepted minimum Tauri capability set.'
}

foreach ($required in @(
  'IMAGE_TO_PDF_OPERATION_ID: &str = "image.to-pdf"',
  'IMAGE_TO_PDF_MAX_INPUTS: usize = 128',
  'IMAGE_MAX_DIMENSION: u32 = 8_192',
  'IMAGE_MAX_PIXELS: u64 = 16_777_216',
  'IMAGE_TO_PDF_MAX_TOTAL_PIXELS: u64 = 67_108_864',
  'IMAGE_TO_PDF_MAX_TOTAL_INPUT_BYTES: u64 = 536_870_912'
)) {
  if (!$contracts.Contains($required)) { throw "G04B contract boundary is missing: $required" }
}

foreach ($required in @(
  'apply_orientation', 'image_xobject', 'Filter::FlateDecode', 's_mask',
  'verify_output', 'source_hashes',
  'publish_verified_staging_with_observer', 'record_warning'
)) {
  if ($writer -notmatch [regex]::Escape($required)) { throw "G04B writer is missing $required." }
}
foreach ($required in @(
  'altered_persisted_settings_fail_closed_before_conversion',
  'let too_many = vec![source.clone(); 129]',
  'let maximum = vec![source.clone(); 128]',
  'IMAGE_PDF_PUBLICATION_FAILED',
  'reconcile_startup'
)) {
  if (!$imageTests.Contains($required)) { throw "G04B native acceptance evidence is missing: $required" }
}
if (!$viewerTests.Contains('G04B images-to-PDF output matches its source pixels through the accepted PDF.js renderer')) {
  throw 'G04B browser-backed visual evidence is missing.'
}
if (!$package.Contains('"pretest:browser": "node ./scripts/prepare-g04b-visual-evidence.mjs && node ./scripts/prepare-g04e1-visual-evidence.mjs"')) {
  throw 'G04B native visual fixture producer must run before Playwright timing starts.'
}
foreach ($required in @(
  'DOCUMENT_STUDIO_G04B_VISUAL_EVIDENCE_DIR',
  'jpeg_png_and_webp_publish_one_verified_page_each_in_selected_order',
  "'--', '--exact'",
  "'source.png', 'output.pdf'"
)) {
  if (!$visualProducer.Contains($required)) { throw "G04B pre-browser visual producer is missing: $required" }
}
if (!$viewerTests.Contains('DOCUMENT_STUDIO_BROWSER_EVIDENCE_ROOT') -or
    !$viewerTests.Contains("resolve(browserEvidenceRoot, 'g04b-browser-visual-evidence')")) {
  throw 'G04B browser visual test is not consuming the pre-generated native evidence.'
}
if ($registry -notmatch 'IMAGE_TO_PDF_OPERATION_ID' -or
    $contractSource -notmatch "operationId: 'image\.to-pdf'" -or
    $contractSource -notmatch 'inputPaths: \[string, \.\.\.string\[\]\]') {
  throw 'The typed image.to-pdf operation is not wired end to end.'
}

foreach ($required in @(
  'CREATE TABLE job_operation_specs',
  'length(CAST(settings_json AS BLOB)) BETWEEN 2 AND 65536',
  'CREATE TABLE job_warnings',
  'sanitized_detail TEXT NOT NULL',
  'input_index INTEGER',
  'page_index INTEGER'
)) {
  if (!$migration.Contains($required)) { throw "G04B metadata migration is missing: $required" }
}
if ($migration -match '(?im)^\s*[A-Za-z_][A-Za-z0-9_]*\s+BLOB\b') {
  throw 'G04B metadata migration contains a prohibited BLOB column.'
}

foreach ($required in @(
  'role="tablist"',
  'Images to PDF',
  'PDF to images',
  'event.altKey',
  "event.key === 'Delete'",
  'api.jobs.warnings'
)) {
  if (!$convert.Contains($required)) { throw "G04B truthful/accessibility UI boundary is missing: $required" }
}

Write-Output 'G04B exact dependency hashes/features, metadata-only settings/warnings, bounded image writer, accepted verifier/publication reuse, browser-backed visual evidence, minimum capability set, and accessible two-direction UI verified.'
