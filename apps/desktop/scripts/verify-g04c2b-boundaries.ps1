$ErrorActionPreference = 'Stop'

$desktopRoot = Split-Path -Parent $PSScriptRoot
$repoRoot = (Resolve-Path (Join-Path $desktopRoot '..\..')).Path
$migrationRoot = Join-Path $desktopRoot 'src-tauri\migrations'

function Get-Sha256Hex([string]$Path) {
  $stream = [System.IO.File]::OpenRead($Path)
  try {
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try { return ([BitConverter]::ToString($sha256.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
    finally { $sha256.Dispose() }
  } finally { $stream.Dispose() }
}

function Test-ExactMigrationInventory([string[]]$Actual, [string[]]$Expected) {
  return ($Actual -join '|') -eq ($Expected -join '|')
}
$packageText = Get-Content -Raw (Join-Path $desktopRoot 'package.json')
$package = $packageText | ConvertFrom-Json
$cargo = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\Cargo.toml')
$contracts = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\contracts.rs')
$metrics = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\balanced_metrics.rs')
$backend = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\balanced_compression.rs')
$registry = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\operation_registry.rs')
$database = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\database.rs')
$ipc = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\ipc.rs')
$lib = Get-Content -Raw (Join-Path $desktopRoot 'src-tauri\src\lib.rs')
$api = Get-Content -Raw (Join-Path $desktopRoot 'src\api.ts')
$renderer = Get-Content -Raw (Join-Path $desktopRoot 'src\viewer\balancedCompression.ts')
$lifecycle = Get-Content -Raw (Join-Path $desktopRoot 'src\viewer\balancedCompressionLifecycle.ts')
$lifecycleTests = Get-Content -Raw (Join-Path $desktopRoot 'src\viewer\balancedCompressionLifecycle.test.ts')
$workspace = Get-Content -Raw (Join-Path $desktopRoot 'src\OptimizeWorkspace.tsx')
$workspaceTests = Get-Content -Raw (Join-Path $desktopRoot 'src\OptimizeWorkspace.test.tsx')
$session = Get-Content -Raw (Join-Path $desktopRoot 'src\viewer\pdfSession.ts')
$typedContracts = Get-Content -Raw (Join-Path $repoRoot 'packages\contracts\src\index.ts')
$operationSchema = Get-Content -Raw (Join-Path $repoRoot 'packages\contracts\operation.schema.json')
$ipcSchema = Get-Content -Raw (Join-Path $repoRoot 'packages\contracts\ipc.schema.json')
$migration = Get-Content -Raw (Join-Path $migrationRoot '0007_balanced_compression_audits.sql')
$f1Migration = Get-Content -Raw (Join-Path $migrationRoot '0008_batch_preview_foundation.sql')
$ci = Get-Content -Raw (Join-Path $repoRoot '.github\workflows\ci.yml')
$nativeTests = $backend
$rendererTests = Get-Content -Raw (Join-Path $desktopRoot 'src\viewer\balancedCompression.test.ts')
$contractFixture = Get-Content -Raw (Join-Path $repoRoot 'packages\contracts\fixtures\pdf-compress-balanced-contracts.json')

$acceptedMigrations = [ordered]@{
  '0001_metadata.sql' = '0c6dd547dd13b33ceedf7e6488d27748dd472587f85a29fe61c1abe51c23a59e'
  '0002_jobs.sql' = '0c813621ab174456f20c697e975c5e0174674340244e47bb114b6c2f4d3ac7b6'
  '0003_workflows.sql' = 'bed67e0bbfa4cc04821d2a1f423596a4ca93203b3aa4944938cb92e7fe262592'
  '0004_job_operation_plans.sql' = '72ae58cbfbf9e4a26c417222d606f90335aff3647f6933bab7058e74d0d206eb'
  '0005_job_operation_specs_and_warnings.sql' = '750753c6ec832909798fc358c534144105b0de5f0d8d2646771045cdd88f61b1'
  '0006_job_completion_outcomes.sql' = '94ea0eaf4fa3963a0807a7597890c70ec60acdee776530607a2fcf1ff226364c'
  '0007_balanced_compression_audits.sql' = '6e22a3751ce49c52d6ac2b8cb57aad65820ee0fb5d7af383a06c613e7dc9f27e'
}
foreach ($entry in $acceptedMigrations.GetEnumerator()) {
  if ((Get-Sha256Hex (Join-Path $migrationRoot $entry.Key)) -ne $entry.Value) {
    throw "G04C2B accepted migration changed: $($entry.Key)."
  }
}

$migrationNames = @(Get-ChildItem -LiteralPath $migrationRoot -File |
  Sort-Object Name | ForEach-Object Name)
$expectedMigrations = @($acceptedMigrations.Keys) + '0008_batch_preview_foundation.sql'
if (!(Test-ExactMigrationInventory $migrationNames $expectedMigrations)) {
  throw 'G04C2B requires exact accepted migrations 1-7 followed only by G04F1 migration 8.'
}
$unexpectedInventory = @($expectedMigrations) + '0009_unexpected.sql'
$renumberedInventory = @($expectedMigrations)
$renumberedInventory[-1] = '0009_batch_preview_foundation.sql'
if ((Test-ExactMigrationInventory $unexpectedInventory $expectedMigrations) -or
    (Test-ExactMigrationInventory $renumberedInventory $expectedMigrations)) {
  throw 'G04C2B migration inventory negative probes did not fail closed.'
}
if (!$database.Contains('name: "balanced_compression_audits"') -or
    !$database.Contains('include_str!("../migrations/0007_balanced_compression_audits.sql")')) {
  throw 'G04C2B migration 7 is not registered in the immutable migration list.'
}
if (!$database.Contains('name: "batch_preview_foundation"') -or
    !$database.Contains('include_str!("../migrations/0008_batch_preview_foundation.sql")') -or
    !$f1Migration.Contains('CREATE TABLE batch_runs')) {
  throw 'G04C2B allows only the registered additive G04F1 migration 8 after its frozen migration 7.'
}

if (!$package.scripts.'verify:g04c2b' -or
    !$ci.Contains('npm run verify:g04c2b --workspace @document-studio/desktop')) {
  throw 'G04C2B boundary verification is not registered in exact-head CI.'
}
if (!$cargo.Contains('base64 = "=0.22.1"') -or
    !$cargo.Contains('image = { version = "=0.25.10", default-features = false, features = ["jpeg", "png", "webp"] }')) {
  throw 'G04C2B must reuse the exact accepted Rust image encoder and locked base64 support.'
}
foreach ($forbidden in @('lopdf', 'pdfium', 'mupdf', 'poppler', 'pdfbox', 'ghostscript', 'reqwest')) {
  $pattern = '(?im)(^|["\s])' + [regex]::Escape($forbidden) + '(["\s=:]|$)'
  if (($cargo + "`n" + $packageText) -match $pattern) {
    throw "G04C2B contains forbidden parser/renderer/network dependency $forbidden."
  }
}

foreach ($required in @(
  'BALANCED_COMPRESSION_OPERATION_ID: &str = "pdf.compress-balanced"',
  'BALANCED_COMPRESSION_VERSION: &str = "1.0.0"',
  'BALANCED_COMPRESSION_PROFILE: &str = "balanced-v1"',
  'BALANCED_COMPRESSION_JPEG_QUALITY: u8 = 82',
  'BALANCED_COMPRESSION_MAX_AFFECTED_PAGES: usize = 128',
  'BALANCED_COMPRESSION_MAX_TOTAL_PIXELS: u64 = 268_435_456',
  'PDFJS_VERSION: &str = "6.2.108"'
)) {
  if (!$contracts.Contains($required)) { throw "G04C2B fixed contract is missing: $required" }
}
foreach ($required in @(
  'BALANCED_SSIM_MINIMUM: f64 = 0.985',
  'BALANCED_PSNR_MINIMUM_DB: f64 = 36.0',
  'BALANCED_CHANGED_DELTA_THRESHOLD: u8 = 12',
  'const WINDOW_SIZE: usize = 11',
  'const GAUSSIAN_SIGMA: f64 = 1.5',
  'const C1: f64 = 6.5025',
  'const C2: f64 = 58.5225',
  'u128::from(changed_pixels) * 200_u128 <= u128::from(total_pixels)',
  '0.2126 * f64::from(pixel[0]) + 0.7152 * f64::from(pixel[1]) + 0.0722 * f64::from(pixel[2])'
)) {
  if (!$metrics.Contains($required)) { throw "G04C2B exact metric definition is missing: $required" }
}

foreach ($required in @(
  'pub fn pdf_compress_balanced_manifest()',
  'value.outputs.multiplicity = "zero-or-one"',
  '"pdfjs-page-visual-gate"',
  '"changed-pixel-ratio"'
)) {
  if (!$registry.Contains($required)) { throw "G04C2B manifest boundary is missing: $required" }
}
if (!$operationSchema.Contains('"zero-or-one"') -or
    !$ipcSchema.Contains('#/$defs/balancedCompressionJobCreateRequest') -or
    !$ipcSchema.Contains('"const": "balanced-v1"') -or
    !$typedContracts.Contains("operationId: 'pdf.compress-balanced'") -or
    !$typedContracts.Contains("inputPaths: [string]") -or
    !$contractFixture.Contains('"multiplicity": "zero-or-one"')) {
  throw 'G04C2B shared contracts do not preserve the fixed one-input, fixed-profile, zero-or-one contract.'
}

foreach ($required in @(
  'const JSON_CAPTURE_LIMIT_BYTES: usize = 16 * 1024 * 1024',
  'const MIN_IMAGE_AXIS: u32 = 256',
  'const MIN_IMAGE_PIXELS: u64 = 65_536',
  'const MAX_IMAGE_PIXELS: u64 = 16_777_216',
  'const MAX_GRAPH_DEPTH: usize = 16',
  'const MAX_GRAPH_OBJECTS: usize = 4_096',
  'const MAX_CANDIDATES: usize = 256',
  'const MIN_DOCUMENT_SAVINGS: u64 = 65_536',
  '"/DCTDecode" | "/FlateDecode"',
  'dict.get("/BitsPerComponent").and_then(JsonValue::as_u64) != Some(8)',
  'dict.get("/ColorSpace").and_then(JsonValue::as_str) != Some("/DeviceRGB")',
  'ensure_unencrypted(&runtime, &workspace, source_relative, &token)?',
  'refuse_signatures(&source_json)?',
  'OsString::from("--stream-data=preserve")',
  'OsString::from("--object-streams=preserve")',
  'OsString::from("--preserve-unreferenced")',
  'OsString::from("--deterministic-id")',
  'OsString::from(format!("--update-from-json={PATCH_RELATIVE}"))',
  'document_savings_gate_passes(active.source_size, active.candidate_size)',
  'register_verified_balanced_output',
  'complete_no_benefit'
)) {
  if (!$backend.Contains($required)) { throw "G04C2B service boundary is missing: $required" }
}
if ($backend.IndexOf('ensure_unencrypted(&runtime, &workspace, source_relative, &token)?') -gt
    $backend.IndexOf('strict_qpdf_check(&runtime, &workspace, source_relative, &token)?')) {
  throw 'G04C2B must identify encryption before strict content inspection.'
}
foreach ($prohibited in @('--remove-info', '--linearize', '--decrypt', 'Promise.all(', 'http://', 'https://')) {
  if ($backend.Contains($prohibited)) { throw "G04C2B backend contains prohibited transformation/network path $prohibited." }
}

foreach ($required in @(
  'const RENDER_SCALE = 2',
  'const SIDE_TIMEOUT_MS = 60_000',
  'const TOTAL_TIMEOUT_MS = 30 * 60_000',
  'window.setTimeout(abortStage, TOTAL_TIMEOUT_MS)',
  'stageController.signal',
  'createBoundedRangeReader(MAX_RANGE_READS)',
  "background: '#ffffff'",
  "intent: 'print'",
  'alpha: false',
  'getImageData',
  "'source'",
  "'candidate'",
  'candidatePixels.rgba.fill(0)',
  'task.cancel()'
)) {
  if (!$renderer.Contains($required) -and !$rendererTests.Contains($required)) {
    throw "G04C2B sequential local visual gate is missing: $required"
  }
}
if (!$session.Contains('MAX_RANGE_READS') -or !$session.Contains('MAX_RANGE_BYTES = 1024 * 1024')) {
  throw 'G04C2B does not reuse the accepted four-read, 1 MiB native range boundary.'
}
foreach ($prohibited in @('toBlob(', 'convertToBlob(', 'Promise.all(', 'fetch(', 'http://', 'https://')) {
  if ($renderer.Contains($prohibited)) { throw "G04C2B renderer contains prohibited path $prohibited." }
}
if (!$workspace.Contains('const snapshot = await operation.dispose().catch(() => null)') -or
    !$workspace.Contains('pendingVisual.current.get(created.id)') -or
    !$workspace.Contains('const visualOwner = useRef<') -or
    !$workspace.Contains('void runBalancedVisual(pending.session, nextOperation)') -or
    !$workspace.Contains('operationGeneration.current += 1') -or
    !$lifecycle.Contains('request = (async () =>') -or
    !$lifecycle.Contains('await this.jobs.cancel({ jobId })') -or
    !$lifecycle.Contains('!this.cancellationRequested') -or
    !$backend.Contains('let token = active.token.clone();') -or
    !$backend.Contains('&& token.is_cancelled()') -or
    !$backend.Contains('copy_rgb8_with_cancellation') -or
    !$workspaceTests.Contains('cancels and reloads the private job when browser visual verification fails') -or
    !$workspaceTests.Contains('drains the exact replacement visual session after the previous terminal renderer unwinds') -or
    !$workspaceTests.Contains('does not repopulate balanced audit evidence after switching profiles') -or
    !$lifecycleTests.Contains('deduplicates known-job reconciliation across unmount, render abort, and repeated requests')) {
  throw 'G04C2B browser-render failure and unmount paths do not reconcile the private backend job exactly once.'
}

foreach ($required in @(
  "invoke<JobRecord>('jobs_create_balanced'",
  "invoke<JobRecord>('balanced_compression_submit_page', rgba",
  "'x-document-studio-render-side'",
  "'x-document-studio-page-nonce'"
)) {
  if (!$api.Contains($required)) { throw "G04C2B authenticated raw client IPC is missing: $required" }
}
foreach ($required in @(
  'pub fn jobs_create_balanced(',
  'pub async fn balanced_compression_submit_page(',
  "request: Request<'_>",
  'InvokeBody::Raw(bytes)',
  'jobs_balanced_audit',
  'Ok(redact_viewer_job_paths(job))',
  'let may_expose_published_output = matches!(',
  'BALANCED_COMPRESSION_OPERATION_ID | TEXT_TO_PDF_OPERATION_ID',
  'private_candidate.outputs[0].final_path.is_none()'
)) {
  if (!$ipc.Contains($required)) { throw "G04C2B Rust IPC is missing: $required" }
}
foreach ($required in @(
  'ipc::jobs_create_balanced',
  'ipc::balanced_compression_submit_page',
  'ipc::jobs_balanced_audit'
)) {
  if (!$lib.Contains($required)) { throw "G04C2B command registration is missing: $required" }
}

if (!$migration.Contains(') STRICT;') -or !$migration.Contains("CHECK (profile = 'balanced-v1')") -or
    !$migration.Contains('CHECK (compared_pages = affected_pages)')) {
  throw 'G04C2B migration does not preserve strict audit invariants.'
}
foreach ($prohibited in @('document_text', 'image_data', 'stream_bytes', 'rgba', 'source_path', 'destination_path', 'qpdf_json', 'error_message')) {
  if ($migration -match "(?i)\b$prohibited\b") {
    throw "G04C2B migration attempts to persist prohibited content: $prohibited"
  }
}

foreach ($required in @(
  'document_savings_gate_requires_both_exact_thresholds',
  'request_requires_one_pdf_and_the_fixed_profile',
  'encrypted_pdf_is_refused_before_strict_content_inspection',
  'signed_and_tampered_spec_jobs_fail_closed_without_output',
  'changed_source_is_refused_before_candidate_creation',
  'resource_graph_accepts_shared_and_nested_safe_uses_but_refuses_cycles',
  'frozen_corpus_completes_no_benefit_without_visual_or_output',
  'stale_visual_upload_fails_closed_and_cleans_private_candidate',
  'service_publishes_only_after_visual_and_both_size_gates'
)) {
  if (!$nativeTests.Contains($required)) { throw "G04C2B native acceptance evidence is missing: $required" }
}

Write-Output 'G04C2B fixed contract, strict migration, safe image allow-list, bounded object graph, deterministic qpdf patch, exact metrics, sequential local visual gate, truthful size/no-benefit outcome, privacy, cancellation and refusal boundaries verified.'
