$ErrorActionPreference = 'Stop'

$desktopRoot = Split-Path -Parent $PSScriptRoot
$repoRoot = (Resolve-Path (Join-Path $desktopRoot '..\..')).Path
$corpusRoot = Join-Path $desktopRoot 'src-tauri\tests\fixtures\g04c2-balanced-corpus'
$manifestPath = Join-Path $corpusRoot 'corpus-manifest.json'
$qpdf = Join-Path $desktopRoot 'src-tauri\resources\qpdf\12.3.2\bin\qpdf.exe'
$python = (Get-Command python -ErrorAction Stop).Source

& $python -B (Join-Path $repoRoot 'scripts\g04c2_corpus.py') check
if ($LASTEXITCODE -ne 0) { throw 'G04C2 corpus validator failed.' }

if (!(Test-Path -LiteralPath $qpdf -PathType Leaf)) {
  throw "Reviewed qpdf executable is missing: $qpdf"
}

$manifest = Get-Content -Raw $manifestPath | ConvertFrom-Json
foreach ($fixture in $manifest.generatedPdfs) {
  $pdf = Join-Path $corpusRoot $fixture.path
  & $qpdf $pdf --suppress-recovery --check
  if ($LASTEXITCODE -ne 0) { throw "qpdf strict check failed: $($fixture.path)" }
  $pageCount = & $qpdf $pdf --show-npages
  if ($LASTEXITCODE -ne 0 -or [int]$pageCount -ne [int]$fixture.pageCount) {
    throw "qpdf page count differs for $($fixture.path): $pageCount"
  }
}

Write-Output 'G04C2 corpus validator, deterministic regeneration, negative probes and qpdf strict page checks verified.'
