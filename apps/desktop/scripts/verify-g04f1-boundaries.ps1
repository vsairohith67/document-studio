$ErrorActionPreference = 'Stop'

$desktopRoot = Split-Path -Parent $PSScriptRoot
$repoRoot = (Resolve-Path (Join-Path $desktopRoot '..\..')).Path
$tauriRoot = Join-Path $desktopRoot 'src-tauri'
$migrationRoot = Join-Path $tauriRoot 'migrations'

function Get-Sha256Hex([string]$Path) {
  $stream = [System.IO.File]::OpenRead($Path)
  try {
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try { return ([BitConverter]::ToString($sha256.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
    finally { $sha256.Dispose() }
  } finally { $stream.Dispose() }
}

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
    throw "G04F1 changed prior migration $($entry.Key)."
  }
}
$migrationNames = @(Get-ChildItem -LiteralPath $migrationRoot -File | Sort-Object Name | ForEach-Object Name)
$expectedNames = @($acceptedMigrations.Keys) + '0008_batch_preview_foundation.sql'
if (($migrationNames -join '|') -ne ($expectedNames -join '|')) {
  throw "G04F1 migration inventory is not exactly migrations 1-8: $($migrationNames -join ', ')."
}

$migration = Get-Content -Raw (Join-Path $migrationRoot '0008_batch_preview_foundation.sql')
foreach ($required in @('CREATE TABLE batch_runs', 'CREATE TABLE batch_run_jobs', 'plan_key_sha256 TEXT', 'optimistic_version INTEGER', 'batch_runs_one_live_plan_idx', 'REFERENCES jobs(id) ON DELETE RESTRICT', ') STRICT;')) {
  if (!$migration.Contains($required)) { throw "G04F1 migration invariant is missing: $required" }
}
foreach ($forbidden in @('preview_json', 'preview_payload', 'document_content', 'scheduler')) {
  if ($migration -match [regex]::Escape($forbidden)) { throw "G04F1 migration contains prohibited storage or behavior: $forbidden" }
}
if ($migration -match '(?im)^\s*[a-z_][a-z0-9_]*\s+BLOB(?:\s|,|$)') {
  throw 'G04F1 migration contains a prohibited BLOB storage column.'
}

$batch = Get-Content -Raw (Join-Path $tauriRoot 'src\batch.rs')
$testMarker = $batch.IndexOf('#[cfg(test)]')
$batchProduction = if ($testMarker -ge 0) { $batch.Substring(0, $testMarker) } else { $batch }
$database = Get-Content -Raw (Join-Path $tauriRoot 'src\database.rs')
$registry = Get-Content -Raw (Join-Path $tauriRoot 'src\operation_registry.rs')
$ipc = Get-Content -Raw (Join-Path $tauriRoot 'src\ipc.rs')
$rustContracts = Get-Content -Raw (Join-Path $tauriRoot 'src\contracts.rs')
$typescriptContracts = Get-Content -Raw (Join-Path $repoRoot 'packages\contracts\src\index.ts')
$schema = Get-Content -Raw (Join-Path $repoRoot 'packages\contracts\batch.schema.json')
$ui = Get-Content -Raw (Join-Path $desktopRoot 'src\BatchWorkspace.tsx')
$optimizeUi = Get-Content -Raw (Join-Path $desktopRoot 'src\OptimizeWorkspace.tsx')
$convertUi = Get-Content -Raw (Join-Path $desktopRoot 'src\ConvertWorkspace.tsx')

foreach ($required in @(
  'BatchPreviewRequest', 'BatchCreateRequest', 'BatchPreviewRow', 'BatchPreviewResponse',
  'BatchDiskEstimate', 'BATCH_PREVIEW_MAX_BYTES', 'BatchProgress', 'optimistic_version'
)) {
  if (!$rustContracts.Contains($required)) { throw "G04F1 Rust contract is missing: $required" }
}
foreach ($required in @(
  "operation_id == PDF_COMPRESS_LOSSLESS_OPERATION_ID",
  "operation_version == PDF_COMPRESS_LOSSLESS_VERSION",
  'create_batch_with_jobs', 'TransactionBehavior::Immediate',
  'BATCH_PLAN_STALE', 'BATCH_INSUFFICIENT_SPACE', 'windows_file_names_equal'
)) {
  if (!$batch.Contains($required) -and !$database.Contains($required) -and !$registry.Contains($required)) {
    throw "G04F1 implementation invariant is missing: $required"
  }
}
foreach ($forbidden in @('spawn_registered_worker', 'execute_with_registered_token', 'PdfCompressionService', 'std::thread::spawn', 'tokio::spawn')) {
  if ($batch -match [regex]::Escape($forbidden)) { throw "G04F1 batch module starts or schedules work: $forbidden" }
}
if ($batch -match '(?m)^\s*(println!|eprintln!|dbg!)' -or $batch -match '(tracing|log)::') {
  throw 'G04F1 batch preview may not log paths or preview payloads.'
}
if (!$registry.Contains('Batch V1 supports only pdf.compress-lossless@1.0.0.')) {
  throw 'G04F1 exact operation/version eligibility is missing.'
}
foreach ($required in @('parse_naming_template', 'TemplatePart::Stem', 'TemplatePart::Index', 'EscapedOpen', 'EscapedClose', 'verify_available_read_only')) {
  if (!$batch.Contains($required)) { throw "G04F1 naming/dependency boundary is missing: $required" }
}
foreach ($required in @('canonical_path_sha256', 'destination_entry_names', 'plan_collisions_against_names')) {
  if (!$batch.Contains($required)) { throw "G04F1 stale/collision proof is missing: $required" }
}
if ($batchProduction.Contains('FILE_SHARE_DELETE')) {
  throw 'G04F1 destination guard may not allow delete sharing.'
}
if ($batchProduction.Contains('.get_or_prepare()')) {
  throw 'G04F1 preview may not materialize the qpdf runtime cache.'
}
foreach ($forbidden in @('request.requested_names', '.to_lowercase()', 'BATCH_PREVIEW_STALE')) {
  if ($batch.Contains($forbidden)) { throw "G04F1 retained an obsolete preview boundary: $forbidden" }
}
foreach ($operation in @('pdf.compress-balanced', 'image.to-pdf', 'pdf.merge', 'pdf.split', 'viewer.page-plan', 'unknown.operation')) {
  if (!$batch.Contains($operation)) { throw "G04F1 negative eligibility evidence is missing: $operation" }
}
foreach ($command in @('batches_preview', 'batches_create', 'batches_get')) {
  if (!$ipc.Contains($command)) { throw "G04F1 IPC command is missing: $command" }
}
if (!$database.Contains("jobs.state = 'queued'") -or !$database.Contains('SELECT 1 FROM batch_run_jobs WHERE job_id = jobs.id')) {
  throw 'G04F1 recovery does not preserve unstarted queued batch children.'
}
if (!$database.Contains('Some(JobCompletionKind::NoBenefit)') -or !$typescriptContracts.Contains('noBenefitChildren')) {
  throw 'G04F1 does not preserve explicit no-benefit child outcomes.'
}
$schemaObject = $schema | ConvertFrom-Json
$previewResponseSchema = $schemaObject.'$defs'.batchPreviewResponse | ConvertTo-Json -Depth 20
if ($previewResponseSchema -match 'sourcePath|canonicalPath|destinationDirectory|fileIdentity|modifiedAt' -or !$schema.Contains('"additionalProperties": false')) {
  throw 'G04F1 preview response schema leaks paths or is not closed.'
}
foreach ($required in @('No child was started', 'does not start a worker', 'settledChildren')) {
  if (!$ui.Contains($required) -and !$typescriptContracts.Contains($required)) { throw "G04F1 truthful UI/progress copy is missing: $required" }
}
if (!$optimizeUi.Contains('onOpenBatch') -or !$convertUi.Contains('onOpenBatch')) {
  throw 'G04F1 Batch navigation is missing from an enabled workspace rail.'
}
if ($ui.Contains('batch-preview-card" aria-live=')) {
  throw 'G04F1 may not announce the entire preview card as a live region.'
}

$capability = Get-Content -Raw (Join-Path $tauriRoot 'capabilities\default.json') | ConvertFrom-Json
if (($capability.permissions -join '|') -ne 'core:default|dialog:allow-open') {
  throw 'G04F1 changed the accepted minimum Tauri capability set.'
}

Write-Output 'G04F1 migration order, exact eligibility, path-free canonical preview, stale/disk/collision gates, atomic metadata-only creation, settled progress, no-benefit preservation, non-resuming recovery and no worker/capability expansion verified.'
