$ErrorActionPreference = 'Stop'

$desktopRoot = Split-Path -Parent $PSScriptRoot
$repoRoot = (Resolve-Path (Join-Path $desktopRoot '..\..')).Path
$tauriRoot = Join-Path $desktopRoot 'src-tauri'
$migrationRoot = Join-Path $tauriRoot 'migrations'
$fontRoot = Join-Path $tauriRoot 'resources\fonts\g04e1'

function Get-Sha256Hex([string]$Path) {
  $stream = [System.IO.File]::OpenRead($Path)
  try {
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try { return ([BitConverter]::ToString($sha256.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
    finally { $sha256.Dispose() }
  } finally { $stream.Dispose() }
}

function Require-Text([string]$Text, [string[]]$Values, [string]$Boundary) {
  foreach ($value in $Values) {
    if (!$Text.Contains($value)) { throw "G04E1 $Boundary is missing: $value" }
  }
}

$acceptedMigrations = [ordered]@{
  '0001_metadata.sql' = '0c6dd547dd13b33ceedf7e6488d27748dd472587f85a29fe61c1abe51c23a59e'
  '0002_jobs.sql' = '0c813621ab174456f20c697e975c5e0174674340244e47bb114b6c2f4d3ac7b6'
  '0003_workflows.sql' = 'bed67e0bbfa4cc04821d2a1f423596a4ca93203b3aa4944938cb92e7fe262592'
  '0004_job_operation_plans.sql' = '72ae58cbfbf9e4a26c417222d606f90335aff3647f6933bab7058e74d0d206eb'
  '0005_job_operation_specs_and_warnings.sql' = '750753c6ec832909798fc358c534144105b0de5f0d8d2646771045cdd88f61b1'
  '0006_job_completion_outcomes.sql' = '94ea0eaf4fa3963a0807a7597890c70ec60acdee776530607a2fcf1ff226364c'
  '0007_balanced_compression_audits.sql' = '6e22a3751ce49c52d6ac2b8cb57aad65820ee0fb5d7af383a06c613e7dc9f27e'
  '0008_batch_preview_foundation.sql' = 'e46f07232238bad18b4e5dfa1bacca9c3bdd5e51c95b334c7cf2eff1530ba43c'
}
foreach ($entry in $acceptedMigrations.GetEnumerator()) {
  if ((Get-Sha256Hex (Join-Path $migrationRoot $entry.Key)) -ne $entry.Value) {
    throw "G04E1 changed accepted migration $($entry.Key)."
  }
}
$migrationNames = @(Get-ChildItem -LiteralPath $migrationRoot -File | Sort-Object Name | ForEach-Object Name)
if (($migrationNames -join '|') -ne (@($acceptedMigrations.Keys) -join '|')) {
  throw "G04E1 migration inventory is not exactly the accepted migrations 1-8: $($migrationNames -join ', ')."
}

$cargo = Get-Content -Raw (Join-Path $tauriRoot 'Cargo.toml')
$windowsSection = [regex]::Match($cargo, "(?s)\[target\.'cfg\(windows\)'\.dependencies\](.*?)(?:\r?\n\[|\z)").Groups[1].Value
Require-Text $windowsSection @(
  'webview2-com = "=0.38.2"',
  'windows = { version = "=0.61.3", default-features = false, features = [',
  '"Win32_System_Com"',
  '"Win32_UI_Shell"'
) 'Windows dependency surface'
foreach ($forbidden in @('webview2-com-sys =', 'windows-core =', 'default-features = true', 'git =', 'path =')) {
  if ($windowsSection.Contains($forbidden)) { throw "G04E1 added a prohibited Windows dependency form: $forbidden" }
}
if ([regex]::Matches($cargo, '(?m)^webview2-com\s*=').Count -ne 1 -or [regex]::Matches($cargo, '(?m)^windows\s*=').Count -ne 1) {
  throw 'G04E1 direct WebView2/windows dependency edges are not unique.'
}

$lock = Get-Content -Raw (Join-Path $repoRoot 'Cargo.lock')
foreach ($package in @(
  @('webview2-com', '0.38.2', '7130243a7a5b33c54a444e54842e6a9e133de08b5ad7b5861cd8ed9a6a5bc96a'),
  @('windows', '0.61.3', '9babd3a767a4c1aef6900409f85f5d53ce2544ccdfaa86dad48c91782c6d6893')
)) {
  $blockPattern = "(?ms)^\[\[package\]\]\r?\nname = `"$($package[0])`"\r?\nversion = `"$($package[1])`".*?(?=^\[\[package\]\]|\z)"
  $blocks = [regex]::Matches($lock, $blockPattern)
  if ($blocks.Count -ne 1 -or !$blocks[0].Value.Contains("checksum = `"$($package[2])`"")) {
    throw "G04E1 lock proof failed for $($package[0]) $($package[1])."
  }
}
$rootPackage = [regex]::Match($lock, '(?ms)^\[\[package\]\]\r?\nname = "document-studio".*?(?=^\[\[package\]\]|\z)').Value
Require-Text $rootPackage @('"webview2-com"', '"windows"') 'root lock dependency edge'

$expectedFonts = [ordered]@{
  'NotoSans-Regular.ttf' = @('621572', '478c558ea716033cd60c03438f628dfa75694dcf6b5f6d505a2f05fd2b4f3823', 'NotoSans-Regular', 'Version 2.015')
  'NotoSansDevanagari-Regular.ttf' = @('244284', '306b53ecfb182a504dd8a7446093c316387d2fd8dc350d0792ed1753fe0996cd', 'NotoSansDevanagari-Regular', 'Version 2.006')
  'NotoSansTelugu-Regular.ttf' = @('235176', 'b274780b69d1d23fe84b55e809a152cb2ac5306d33864b1f87622f6971871aae', 'NotoSansTelugu-Regular', 'Version 2.005')
}
$expectedInventory = @(
  'font-manifest.json',
  'licenses/NotoSans-OFL-1.1.txt',
  'licenses/NotoSansDevanagari-OFL-1.1.txt',
  'licenses/NotoSansTelugu-OFL-1.1.txt',
  'NotoSans-Regular.ttf',
  'NotoSansDevanagari-Regular.ttf',
  'NotoSansTelugu-Regular.ttf'
) | Sort-Object
$fontInventory = @(Get-ChildItem -LiteralPath $fontRoot -File -Recurse | ForEach-Object {
  $_.FullName.Substring($fontRoot.Length + 1).Replace('\', '/')
} | Sort-Object)
if (($fontInventory -join '|') -ne ($expectedInventory -join '|')) {
  throw "G04E1 packaged font inventory is not exact: $($fontInventory -join ', ')."
}
$fontManifestPath = Join-Path $fontRoot 'font-manifest.json'
$fontManifest = Get-Content -Raw $fontManifestPath | ConvertFrom-Json
if ($fontManifest.operation -ne 'text.to-pdf@1.0.0' -or $fontManifest.fonts.Count -ne 3 -or $fontManifest.policy.systemFallback -ne $false -or $fontManifest.policy.variableFonts -ne $false) {
  throw 'G04E1 font manifest policy is not exact and closed.'
}
foreach ($entry in $expectedFonts.GetEnumerator()) {
  $path = Join-Path $fontRoot $entry.Key
  if ((Get-Item -LiteralPath $path).Length -ne [int64]$entry.Value[0] -or (Get-Sha256Hex $path) -ne $entry.Value[1]) {
    throw "G04E1 font bytes changed: $($entry.Key)."
  }
  $record = @($fontManifest.fonts | Where-Object { $_.postScriptName -eq $entry.Value[2] })
  if ($record.Count -ne 1 -or !$record[0].version.StartsWith($entry.Value[3]) -or $record[0].weight -ne 400 -or $record[0].style -ne 'Regular' -or $record[0].spdxLicense -ne 'OFL-1.1') {
    throw "G04E1 font manifest metadata is invalid: $($entry.Key)."
  }
}
foreach ($notice in @('NotoSans-OFL-1.1.txt', 'NotoSansDevanagari-OFL-1.1.txt', 'NotoSansTelugu-OFL-1.1.txt')) {
  $text = Get-Content -Raw (Join-Path $fontRoot "licenses\$notice")
  Require-Text $text @('SIL OPEN FONT LICENSE Version 1.1', 'Copyright') "OFL notice $notice"
}

$textCore = Get-Content -Raw (Join-Path $tauriRoot 'src\text_to_pdf.rs')
$renderer = Get-Content -Raw (Join-Path $tauriRoot 'src\text_to_pdf_renderer.rs')
$service = Get-Content -Raw (Join-Path $tauriRoot 'src\text_to_pdf_service.rs')
$qpdf = Get-Content -Raw (Join-Path $tauriRoot 'src\qpdf.rs')
$rendererProduction = $renderer.Substring(0, $renderer.LastIndexOf('#[cfg(test)]'))
$serviceProduction = $service.Substring(0, $service.LastIndexOf('#[cfg(test)]'))

Require-Text $textCore @(
  'TXT_MAX_RAW_BYTES: usize = 8_388_608', 'TXT_MAX_LOGICAL_LINES: usize = 100_000',
  'TXT_MAX_LINE_BYTES: usize = 65_536', 'TXT_MAX_HTML_BYTES: usize = 42_991_616',
  'TXT_MAX_CSS_BYTES: usize = 65_536', 'TXT_APPROVED_FONT_BYTES: usize = 1_101_032',
  'TXT_MAX_SERVED_BYTES: usize = 44_158_184', 'TXT_MAX_PDF_BYTES: u64 = 536_870_912',
  'TXT_INVALID_UTF8', 'TXT_UNSUPPORTED_BOM', 'TXT_SIZE_LIMIT', 'TXT_LINE_COUNT_LIMIT',
  'TXT_LINE_BYTES_LIMIT', 'TXT_CONTROL_CHARACTER', 'TXT_BIDI_CONTROL', 'TXT_NONCHARACTER',
  'TXT_UNSUPPORTED_UNICODE', 'TXT_SHAPING_COMPLEXITY_LIMIT', 'TXT_RESPONSE_SIZE_LIMIT',
  'https://txt-renderer.document-studio.invalid/1/document.html',
  'https://txt-renderer.document-studio.invalid/1/document.css',
  "default-src 'none'", "script-src 'none'", "connect-src 'none'", "style-src 'self'", "font-src 'self'",
  'font-synthesis:none', 'white-space:pre-wrap', 'overflow-wrap:anywhere', 'tab-size:4'
) 'input, response and canonical document boundary'

Require-Text $rendererProduction @(
  'DOCUMENT_URL', 'CSS_URL',
  'ICoreWebView2_22', 'ICoreWebView2Environment6', 'ICoreWebView2_7',
  'AddWebResourceRequestedFilterWithRequestSourceKinds', 'COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL',
  'COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL', 'SHCreateMemStream', 'stream.Stat',
  'CreateWebResourceResponse', 'WebResourceResponseReceivedEventHandler', 'DOMContentLoadedEventHandler',
  'SetIsScriptEnabled(false)', 'SetIsWebMessageEnabled(false)', 'SetAreHostObjectsAllowed(false)',
  'SetAreDevToolsEnabled(false)', 'SetAreDefaultContextMenusEnabled(false)',
  'SetAreDefaultScriptDialogsEnabled(false)', 'SetIsBuiltInErrorPageEnabled(false)',
  'SetIsStatusBarEnabled(false)', 'SetIsZoomControlEnabled(false)',
  'SetAreBrowserAcceleratorKeysEnabled(false)', 'PrintToPdf', 'SetIsVisible(false)',
  'SetAllowExternalDrop(false)', 'owns_generation', 'complete_once', 'controller.Close()',
  'CallbackGuard', 'completion_token', 'Weak<CallbackAuthority>', 'ControllerOwner',
  'deny_stale_resource_request'
) 'hidden WebView2 resource and callback boundary'
foreach ($forbidden in @('ExecuteScript', 'AddHostObject', 'PostWebMessage', 'QueryInterface', 'transmute', 'AddRef', 'http://', 'localhost', '127.0.0.1', 'file://', 'data:')) {
  if ($rendererProduction.Contains($forbidden)) { throw "G04E1 renderer production source contains a prohibited path: $forbidden" }
}

Require-Text $qpdf @(
  'OsString::from("--empty")', 'OsString::from("--suppress-recovery")',
  'OsString::from("--stream-data=preserve")', 'OsString::from("--object-streams=preserve")',
  'OsString::from("--remove-info")', 'OsString::from("--remove-metadata")',
  'OsString::from("--remove-page-labels")', 'OsString::from("--pages")'
) 'qpdf page-only normalization boundary'
Require-Text $serviceProduction @(
  'source.verify_unchanged_hash', 'publish_verified_staging_with_observer', 'TXT_FINAL_HASH_MISMATCH',
  'TXT_MAX_PAGES', 'TXT_MAX_PDF_BYTES', 'interpret_structural_check_exit', 'OsString::from("--check")',
  'interpret_encryption_check_exit', 'FontFile2', '/MediaBox', '/CropBox',
  '/Rotate', '/OpenAction', '/JavaScript', '/AcroForm', '/Names', 'UDF_MARKER',
  'validate_recovery_renderer_workspaces', 'cleanup_renderer_workspace', 'clear_unpublished_intent',
  'DependencyDiagnostic', 'id: "webview2"', 'runtime_version',
  'check_deadline_until_publication_commit'
) 'verification, publication, recovery and diagnostic boundary'
foreach ($forbidden in @('std::process::Command', 'cmd.exe', 'powershell.exe', 'ShellExecute', 'runtime download')) {
  if ($rendererProduction.Contains($forbidden) -or $serviceProduction.Contains($forbidden)) {
    throw "G04E1 production source contains a prohibited execution path: $forbidden"
  }
}

$registry = Get-Content -Raw (Join-Path $tauriRoot 'src\operation_registry.rs')
$ipc = Get-Content -Raw (Join-Path $tauriRoot 'src\ipc.rs')
$schema = Get-Content -Raw (Join-Path $repoRoot 'packages\contracts\ipc.schema.json') | ConvertFrom-Json
$ui = Get-Content -Raw (Join-Path $desktopRoot 'src\TextToPdfWorkspace.tsx')
$visualProducer = Get-Content -Raw (Join-Path $desktopRoot 'scripts\prepare-g04e1-visual-evidence.mjs')
$browserTests = Get-Content -Raw (Join-Path $desktopRoot 'e2e\viewer.spec.ts')
Require-Text $registry @('text_to_pdf_manifest', 'TEXT_TO_PDF_OPERATION_ID', 'TEXT_TO_PDF_VERSION', '"pageSize"', '"orientation"') 'operation registry boundary'
Require-Text $ipc @('text_open_dialog', 'jobs_create_text_to_pdf', 'jobs_open_text_output', 'TXT_PUBLISHED_OUTPUT_CHANGED', 'open_verified_pdf', 'TextToPdfService') 'opaque IPC boundary'
$requestSchema = $schema.'$defs'.textToPdfJobCreateRequest
if ($requestSchema.additionalProperties -ne $false -or $requestSchema.properties.operationId.const -ne 'text.to-pdf') {
  throw 'G04E1 IPC schema is not closed or operation-exact.'
}
Require-Text $ui @(
  'Local · offline', 'Strict UTF-8 only', '100,000 logical lines', '65,536 UTF-8 bytes',
  'No system font', 'English, Hindi (Devanagari), and Telugu', 'Page size', 'Orientation',
  'existing files are never overwritten', 'Cancellation requested', 'Current stage:',
  'Open in Viewer', 'Show saved path', 'Copy saved path',
  'The verified saved path is shown and focused below.', 'aria-live="polite"', 'queueMicrotask'
) 'privacy and accessible UI boundary'
if ($ui.Contains('source.text') -or $ui.Contains('documentBody') -or $ui.Contains('textPreview')) {
  throw 'G04E1 UI contains a private document-content field.'
}
Require-Text $visualProducer @(
  'DOCUMENT_STUDIO_G04E1_VISUAL_EVIDENCE_DIR', 'mixed-a4-portrait.pdf',
  'native_webview2_qpdf_acceptance_covers_all_page_settings_and_mixed_scripts'
) 'native-to-PDF.js visual evidence producer'
Require-Text $browserTests @(
  'G04E1 real hidden-WebView2 output renders and extracts representative text through PDF.js',
  "expect(extracted).toContain('English')", 'evidence.nonWhite'
) 'representative PDF.js render and extraction proof'

$tauriConfig = Get-Content -Raw (Join-Path $tauriRoot 'tauri.conf.json') | ConvertFrom-Json
if ($tauriConfig.bundle.resources -notcontains 'resources/fonts/g04e1/**/*') {
  throw 'G04E1 exact font resource tree is not bundled.'
}
$capability = Get-Content -Raw (Join-Path $tauriRoot 'capabilities\default.json') | ConvertFrom-Json
if (($capability.permissions -join '|') -ne 'core:default|dialog:allow-open') {
  throw 'G04E1 changed the accepted minimum Tauri capability set.'
}

$desktopPackage = Get-Content -Raw (Join-Path $desktopRoot 'package.json') | ConvertFrom-Json
if ($desktopPackage.scripts.'verify:g04e1' -ne 'powershell -NoProfile -ExecutionPolicy Bypass -File ./scripts/verify-g04e1-boundaries.ps1') {
  throw 'G04E1 boundary verifier is not registered exactly.'
}
if ($desktopPackage.scripts.'pretest:browser' -ne 'node ./scripts/prepare-g04b-visual-evidence.mjs && node ./scripts/prepare-g04e1-visual-evidence.mjs') {
  throw 'G04E1 native PDF.js evidence is not wired before browser acceptance.'
}
$ci = Get-Content -Raw (Join-Path $repoRoot '.github\workflows\ci.yml')
if (!$ci.Contains('npm run verify:g04e1 --workspace @document-studio/desktop')) {
  throw 'G04E1 boundary verifier is not wired into exact-head CI.'
}
Require-Text $ci @(
  'native_renderer_fault_matrix_closes_exact_generation_at_bounded_checkpoints',
  'native_text_service_cancellation_matrix_cleans_owned_state_and_preserves_commits',
  'native_webview2_qpdf_acceptance_covers_all_page_settings_and_mixed_scripts'
) 'native exact-head CI proof'

Write-Output 'G04E1 exact dependencies, immutable fonts/OFL, strict UTF-8 and Unicode bounds, hidden intercepted WebView2, no-network callbacks, qpdf verification/publication/recovery, opaque IPC, accessible UI, no migration and minimum capability boundaries verified.'
