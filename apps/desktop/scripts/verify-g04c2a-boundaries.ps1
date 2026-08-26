$ErrorActionPreference = 'Stop'

$desktopRoot = Split-Path -Parent $PSScriptRoot
$repoRoot = (Resolve-Path (Join-Path $desktopRoot '..\..')).Path
$tauriRoot = Join-Path $desktopRoot 'src-tauri'
$migrationRoot = Join-Path $tauriRoot 'migrations'

function Get-Sha256Hex([string]$Path) {
  $stream = [System.IO.File]::OpenRead($Path)
  try {
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
      return ([System.BitConverter]::ToString($sha256.ComputeHash($stream))).Replace('-', '').ToLowerInvariant()
    } finally {
      $sha256.Dispose()
    }
  } finally {
    $stream.Dispose()
  }
}

$expectedMigrationHashes = [ordered]@{
  '0001_metadata.sql' = '0c6dd547dd13b33ceedf7e6488d27748dd472587f85a29fe61c1abe51c23a59e'
  '0002_jobs.sql' = '0c813621ab174456f20c697e975c5e0174674340244e47bb114b6c2f4d3ac7b6'
  '0003_workflows.sql' = 'bed67e0bbfa4cc04821d2a1f423596a4ca93203b3aa4944938cb92e7fe262592'
  '0004_job_operation_plans.sql' = '72ae58cbfbf9e4a26c417222d606f90335aff3647f6933bab7058e74d0d206eb'
  '0005_job_operation_specs_and_warnings.sql' = '750753c6ec832909798fc358c534144105b0de5f0d8d2646771045cdd88f61b1'
}
foreach ($entry in $expectedMigrationHashes.GetEnumerator()) {
  $actual = Get-Sha256Hex (Join-Path $migrationRoot $entry.Key)
  if ($actual -ne $entry.Value) { throw "G04C2A changed accepted migration $($entry.Key)." }
}

$migrationNames = @(Get-ChildItem -LiteralPath $migrationRoot -File | Sort-Object Name | ForEach-Object Name)
$expectedNames = @($expectedMigrationHashes.Keys) + '0006_job_completion_outcomes.sql'
if ($migrationNames.Count -lt $expectedNames.Count -or
    (($migrationNames[0..($expectedNames.Count - 1)] -join '|') -ne ($expectedNames -join '|'))) {
  throw "G04C2A accepted migration prefix is not exactly migrations 1-6: $($migrationNames -join ', ')."
}

$migration = Get-Content -Raw (Join-Path $migrationRoot '0006_job_completion_outcomes.sql')
foreach ($required in @(
  'CREATE TABLE job_completion_outcomes',
  'job_id TEXT PRIMARY KEY',
  'REFERENCES jobs(id) ON DELETE CASCADE',
  "completion_kind IN ('published', 'no-benefit')",
  "reason IS 'savings-threshold-not-met'",
  ') STRICT;'
)) {
  if (!$migration.Contains($required)) { throw "G04C2A migration invariant is missing: $required" }
}
foreach ($forbidden in @(' BLOB', ' JSON', 'path', 'content', 'candidate')) {
  if ($migration -match [regex]::Escape($forbidden)) { throw "G04C2A migration contains prohibited storage: $forbidden" }
}

$database = Get-Content -Raw (Join-Path $tauriRoot 'src\database.rs')
$rustContracts = Get-Content -Raw (Join-Path $tauriRoot 'src\contracts.rs')
$stateMachine = Get-Content -Raw (Join-Path $tauriRoot 'src\job_engine.rs')
$ipc = Get-Content -Raw (Join-Path $tauriRoot 'src\ipc.rs')
$typescriptContracts = Get-Content -Raw (Join-Path $repoRoot 'packages\contracts\src\index.ts')
$schema = Get-Content -Raw (Join-Path $repoRoot 'packages\contracts\job.schema.json')
$outcomeUi = Get-Content -Raw (Join-Path $desktopRoot 'src\JobCompletionOutcome.tsx')
$historyUi = Get-Content -Raw (Join-Path $desktopRoot 'src\App.tsx')
$optimizeUi = Get-Content -Raw (Join-Path $desktopRoot 'src\OptimizeWorkspace.tsx')

if (($database | Select-String -Pattern 'version: 6,' -AllMatches).Matches.Count -ne 1 -or
    ($database | Select-String -Pattern '0006_job_completion_outcomes.sql' -AllMatches).Matches.Count -ne 1) {
  throw 'G04C2A migration 6 is not registered exactly once.'
}
foreach ($required in @(
  'pub(crate) fn complete_no_benefit',
  'TransactionBehavior::Immediate',
  "state = 'completed', stage = NULL, completed_units = total_units",
  'cancellation_requested_at IS NULL AND resolved_output_name IS NULL',
  'pub(crate) fn complete_published',
  'validate_loaded_completion_outcome'
)) {
  if (!$database.Contains($required)) { throw "G04C2A repository invariant is missing: $required" }
}
if ($stateMachine -match '\(Verifying,\s*Completed') {
  throw 'G04C2A added the forbidden generic Verifying to Completed transition.'
}
if (!$stateMachine.Contains('assert!(!can_transition(JobState::Verifying, JobState::Completed));')) {
  throw 'G04C2A state-machine rejection evidence is missing.'
}

foreach ($required in @(
  'pub enum JobCompletionKind',
  'pub enum JobCompletionReason',
  'pub completion_kind: Option<JobCompletionKind>',
  'pub reason: Option<JobCompletionReason>'
)) {
  if (!$rustContracts.Contains($required)) { throw "G04C2A Rust contract is missing: $required" }
}
foreach ($required in @(
  "export type JobCompletionKind = 'published' | 'no-benefit'",
  "export type JobCompletionReason = 'savings-threshold-not-met'",
  'completionKind: JobCompletionKind | null',
  'reason: JobCompletionReason | null'
)) {
  if (!$typescriptContracts.Contains($required)) { throw "G04C2A TypeScript contract is missing: $required" }
}
$schemaObject = $schema | ConvertFrom-Json
foreach ($field in @('completionKind', 'reason')) {
  if ($schemaObject.required -notcontains $field) { throw "G04C2A JSON Schema does not require $field." }
}
if ($schema -notmatch 'savings-threshold-not-met' -or $schema -notmatch 'no-benefit') {
  throw 'G04C2A JSON Schema outcome combinations are missing.'
}

foreach ($required in @(
  'No worthwhile size reduction',
  'The private candidate did not meet both requirements: at least 5% and 64 KiB smaller. No file was created, and your original stayed unchanged.',
  'No output created'
)) {
  if (!$outcomeUi.Contains($required)) { throw "G04C2A exact UI copy is missing: $required" }
}
if (!$historyUi.Contains('Completed — no benefit') -or !$historyUi.Contains('No benefit')) {
  throw 'G04C2A accessible history outcome is missing.'
}
if (!$optimizeUi.Contains("job.completionKind === 'no-benefit'")) {
  throw 'G04C2A generic optimize outcome rendering is missing.'
}

if ($ipc -match '(?i)complete[_-]no[_-]benefit') {
  throw 'G04C2A exposed its internal terminalization helper directly through IPC.'
}

$capability = Get-Content -Raw (Join-Path $tauriRoot 'capabilities\default.json') | ConvertFrom-Json
if (($capability.permissions -join '|') -ne 'core:default|dialog:allow-open') {
  throw 'G04C2A changed the accepted minimum Tauri capability set.'
}

Write-Output 'G04C2A accepted migration 1-6 prefix, strict outcomes, internal CAS, legacy-null contracts, fail-closed loading, no-output UI, internal-only terminalization and capability boundary verified.'
