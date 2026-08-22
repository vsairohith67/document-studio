$ErrorActionPreference = 'Stop'

$desktopRoot = Split-Path -Parent $PSScriptRoot
$repoRoot = (Resolve-Path (Join-Path $desktopRoot '..\..')).Path
$cargo = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\Cargo.toml')
$packageText = Get-Content -Raw (Join-Path $desktopRoot 'package.json')
$package = $packageText | ConvertFrom-Json
$contracts = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\contracts.rs')
$typedContracts = Get-Content -Raw (Join-Path $repoRoot 'packages\contracts\src\index.ts')
$registry = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\operation_registry.rs')
$ipc = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\ipc.rs')
$backend = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\pdf_to_images.rs')
$api = Get-Content -Raw (Join-Path $desktopRoot 'src\api.ts')
$session = Get-Content -Raw (Join-Path $desktopRoot 'src\viewer\pdfSession.ts')
$renderer = Get-Content -Raw (Join-Path $desktopRoot 'src\viewer\pdfToImages.ts')
$workspace = Get-Content -Raw (Join-Path $desktopRoot 'src\PdfToImagesWorkspace.tsx')
$nativeTests = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\tests\pdf_to_images.rs')
$browserTests = Get-Content -Raw (Join-Path $desktopRoot 'e2e\viewer.spec.ts')
$webViewSmoke = Get-Content -Raw (Join-Path $desktopRoot 'scripts\webview2-smoke.mjs')
$adr = Get-Content -Raw (Join-Path $repoRoot 'docs\adr\ADR-013-g04b-image-pdf-conversion-dependencies.md')
$ci = Get-Content -Raw (Join-Path $repoRoot '.github\workflows\ci.yml')

if ($package.dependencies.'pdfjs-dist' -ne '6.2.108') {
  throw 'G04B2 must use exact accepted pdfjs-dist 6.2.108.'
}
if (!$package.scripts.'verify:g04b2' -or
    !$ci.Contains('npm run verify:g04b2 --workspace @document-studio/desktop') -or
    !$ci.Contains('npm run test:webview2 --workspace @document-studio/desktop')) {
  throw 'G04B2 verifier/native WebView2 acceptance is not wired into exact-head CI.'
}
if (!$cargo.Contains('image = { version = "=0.25.10", default-features = false, features = ["jpeg", "png", "webp"] }')) {
  throw 'G04B2 must use the accepted narrow image 0.25.10 encoder.'
}
foreach ($forbidden in @('pdfium', 'mupdf', 'poppler', 'pdfbox', 'ghostscript', 'reqwest')) {
  if (($cargo + "`n" + $packageText) -match "(?im)(^|[`"\s])$forbidden([`"\s=:]|$)") {
    throw "G04B2 contains forbidden renderer/network dependency $forbidden."
  }
}
$capability = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\capabilities\default.json') | ConvertFrom-Json
if (($capability.permissions -join '|') -ne 'core:default|dialog:allow-open') {
  throw 'G04B2 changed the accepted minimum Tauri capability set.'
}

foreach ($required in @(
  'PDF_TO_IMAGES_OPERATION_ID: &str = "pdf.to-images"',
  'PDF_TO_IMAGES_VERSION: &str = "1.0.0"',
  'PDF_TO_IMAGES_MAX_OUTPUTS: usize = 128',
  'PDF_TO_IMAGES_MAX_TOTAL_PIXELS: u64 = 67_108_864',
  'PDF_TO_IMAGES_JPEG_QUALITY: u8 = 92',
  'PDFJS_VERSION: &str = "6.2.108"',
  'IMAGE_MAX_DIMENSION: u32 = 8_192',
  'IMAGE_MAX_PIXELS: u64 = 16_777_216'
)) {
  if (!$contracts.Contains($required)) { throw "G04B2 contract boundary is missing: $required" }
}
$requestBlock = [regex]::Match($typedContracts, '(?s)export interface PdfToImagesJobCreateRequest\s*\{(?<body>.*?)\}').Groups['body'].Value
foreach ($required in @('viewerSessionId: string', 'destinationGrantId: string', 'dpi: 72 | 150 | 300')) {
  if (!$requestBlock.Contains($required)) { throw "G04B2 opaque typed request is missing $required." }
}
if ($requestBlock -match '\b(sourcePath|destinationPath|destinationDirectory)\b') {
  throw 'G04B2 typed request exposes a source/destination path to React.'
}

foreach ($required in @(
  'pub fn pdf_to_images_manifest()', 'PDF_TO_IMAGES_OPERATION_ID', 'PDF_TO_IMAGES_VERSION',
  '"format": { "enum": ["jpeg", "png", "webp"] }',
  '"dpi": { "enum": [72, 150, 300] }',
  'value.outputs.multiplicity = "multiple"', '"authenticated-binary-ipc"'
)) {
  if (!$registry.Contains($required)) { throw "G04B2 operation registry is missing $required." }
}

foreach ($required in @(
  'isEvalSupported: false', 'enableXfa: false', 'renderForms: false',
  'AnnotationMode.DISABLE', 'disableAutoFetch: true', 'useSystemFonts: false'
)) {
  if (!$session.Contains($required)) { throw "G04B2 accepted PDF.js security setting is missing $required." }
}
foreach ($required in @(
  'dpi / 72', 'PDF_TO_IMAGES_MAX_OUTPUTS = 128',
  'PDF_TO_IMAGES_MAX_TOTAL_PIXELS = 67_108_864',
  "window.document.createElement('canvas')", 'alpha: false',
  "background: '#ffffff'", "intent: 'print'", 'getImageData',
  'submitPdfPixels', 'renderTask?.cancel()', 'page.cleanup()',
  'canvas.width = 0', 'canvas.height = 0'
)) {
  if (!$renderer.Contains($required)) { throw "G04B2 sequential renderer is missing $required." }
}
foreach ($prohibited in @('toBlob(', 'convertToBlob(', 'Promise.all(', 'fetch(', 'http://', 'https://')) {
  if ($renderer.Contains($prohibited)) { throw "G04B2 renderer contains prohibited path $prohibited." }
}

foreach ($required in @(
  "invoke<JobRecord>('pdf_to_images_submit_page', rgba, {",
  "'x-document-studio-job-id'", "'x-document-studio-render-session-id'",
  "'x-document-studio-page-ordinal'", "'x-document-studio-page-nonce'",
  "'x-document-studio-expected-width'", "'x-document-studio-expected-height'"
)) {
  if (!$api.Contains($required)) { throw "G04B2 raw client IPC is missing $required." }
}
$submitBlock = $api.Substring($api.IndexOf('submitPdfPixels'), $api.IndexOf('cancel:', $api.IndexOf('submitPdfPixels')) - $api.IndexOf('submitPdfPixels'))
if ($submitBlock -match 'base64') { throw 'G04B2 raw client IPC uses base64.' }
foreach ($required in @(
  "request: Request<'_>", 'InvokeBody::Raw(bytes)', 'bytes.len() <= 67_108_864',
  'spawn_blocking', 'pixel_upload_metadata(&request)',
  'x-document-studio-job-id', 'x-document-studio-render-session-id',
  'x-document-studio-page-ordinal', 'x-document-studio-page-nonce',
  'x-document-studio-expected-width', 'x-document-studio-expected-height',
  'PDF_TO_IMAGES_OPERATION_ID', 'redact_viewer_job_paths', 'tauri::webview_version().ok()'
)) {
  if (!$ipc.Contains($required)) { throw "G04B2 raw Rust IPC boundary is missing $required." }
}

foreach ($required in @(
  'metadata.render_session_id != active.render_session_id',
  'metadata.page_ordinal as usize != expected_ordinal',
  'metadata.nonce != page.ticket.nonce',
  'metadata.expected_width != page.ticket.expected_width',
  'metadata.expected_height != page.ticket.expected_height',
  'rgba.len() != expected_bytes', 'pixel[3] != 255',
  'checked_mul(u64::from(metadata.expected_height))', 'pixels.checked_mul(4)',
  'check_cancelled', 'PngEncoder::new_with_quality', 'JpegEncoder::new_with_quality',
  'PDF_TO_IMAGES_JPEG_QUALITY', 'WebPEncoder::new_lossless',
  'insert_png_density', 'PixelDensity::dpi', 'mean_absolute_error',
  'decoded.color().has_alpha()',
  'verify_exact_staging_membership', 'verify_unchanged_hash',
  'verify_all_staging_hashes', 'size != verified_size',
  'publish_verified_staging_with_observer', 'PARTIAL_PUBLICATION'
)) {
  if (!$backend.Contains($required)) { throw "G04B2 Rust service boundary is missing $required." }
}

foreach ($required in @(
  'PageThumbnail', "(['jpeg', 'png', 'webp'] as const)", '([72, 150, 300] as const)',
  'Select no more than 128 pages', 'Partial publication', 'RenderTask',
  'transport.abort()', 'openButtonRef.current?.focus()', 'source path stays outside React'
)) {
  if (!$workspace.Contains($required)) { throw "G04B2 accessible UI boundary is missing $required." }
}
foreach ($required in @(
  'png_jpeg_and_lossless_webp_are_encoded_verified_and_published',
  'png_and_jpeg_record_each_supported_density',
  'stale_wrong_page_replay_and_payload_mismatch_fail_closed',
  'exact_128_output_boundary_is_accepted_and_129_is_rejected',
  'dimension_pixel_and_aggregate_caps_are_enforced_before_session_access',
  'malformed_and_encrypted_pdfs_are_rejected_before_rendering',
  'cancellation_cleans_owned_staging_and_source_remains_immutable',
  'partial_publication_preserves_the_first_user_file_and_reconciles_the_rest',
  'verified_staging_hash_drift_fails_before_any_publication',
  'measure_maximum_page_encode_cancellation_response',
  'unexpected_staging_membership_fails_before_any_publication',
  'startup_recovery_fails_an_interrupted_render_job_and_removes_private_workspace',
  'destination_collision_never_overwrites_the_racing_user_file', 'PIXEL_ALPHA_INVALID'
)) {
  if (!$nativeTests.Contains($required)) { throw "G04B2 native acceptance evidence is missing $required." }
}
foreach ($required in @(
  'G04B2 PDF-to-images renders ordered opaque pages sequentially through authenticated binary transfer',
  'G04B2 PDF-to-images covers rotated crop boxes, embedded Type 3, soft masks, and ICC CMYK Lab rendering',
  'peakPdfImageTransfers', 'alphaOpaque', 'nonWhitePixels'
)) {
  if (!$browserTests.Contains($required)) { throw "G04B2 browser evidence is missing $required." }
}
foreach ($required in @(
  "'viewer_grant_test_destination'", "'pdf_to_images_submit_page'",
  'missingHeaderRejected', 'replayRejected', 'jsonPixelBodyRejected',
  'webview2RuntimeVersion', 'webview2-g04b2-page-0001.png'
)) {
  if (!$webViewSmoke.Contains($required)) { throw "G04B2 native WebView2 evidence is missing $required." }
}
if ($ipc -notmatch '#\[cfg\(feature = "test-runtime"\)\]\s+#\[tauri::command\]\s+pub fn viewer_grant_test_destination') {
  throw 'G04B2 native smoke destination grant is not test-runtime gated.'
}
foreach ($required in @(
  'pdfjs-dist` 6.2.108', 'private, unattached canvas', 'binary Tauri IPC',
  'WebView2 runtime version', 'does not promise byte-identical pixels',
  'getImageData()` is synchronous'
)) {
  if (!$adr.Contains($required)) { throw "G04B2 ADR is missing $required." }
}

Write-Output 'G04B2 exact PDF.js/image reuse, public contract, caps, sequential private canvas, authenticated raw IPC, Rust encoders/verifiers, multi-output lifecycle, cancellation, accessibility, tests, diagnostics and minimum capability boundary verified.'
