[CmdletBinding()]
param(
    [string]$RepositoryRoot,
    [string]$SourceRoot,
    [string]$PythonPolicyPath,
    [string]$BootstrapEvidencePath,
    [string]$ExpectedSha,
    [switch]$PassThru,
    [switch]$Quiet
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($RepositoryRoot)) { $RepositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path }
if ([string]::IsNullOrWhiteSpace($SourceRoot)) { $SourceRoot = (Resolve-Path $PSScriptRoot).Path }
if ([string]::IsNullOrWhiteSpace($PythonPolicyPath)) { $PythonPolicyPath = Join-Path $RepositoryRoot 'scripts\g04d_c_powershell_source_policy.py' }

if ($PSVersionTable.PSEdition -cne 'Desktop' -or $PSVersionTable.PSVersion.Major -ne 5 -or $PSVersionTable.PSVersion.Minor -ne 1) {
    throw '[G04DC_POWERSHELL_VERSION_INVALID] The source parser gate requires Windows PowerShell 5.1.'
}

$repo = (Resolve-Path -LiteralPath $RepositoryRoot).Path
$source = (Resolve-Path -LiteralPath $SourceRoot).Path
$policy = (Resolve-Path -LiteralPath $PythonPolicyPath).Path
$pythonCandidates = @(Get-Command python.exe -CommandType Application -ErrorAction Stop)
if ($pythonCandidates.Count -eq 0) { throw '[G04DC_SOURCE_POLICY_UNAVAILABLE] python.exe was not found.' }
$python = [string]$pythonCandidates[0].Source
$powerShellVersion = $PSVersionTable.PSVersion.ToString()
$checkedOutSha = $null
if (![string]::IsNullOrWhiteSpace($ExpectedSha) -or ![string]::IsNullOrWhiteSpace($BootstrapEvidencePath)) {
    $checkedOutSha = [string](& git -C $repo rev-parse HEAD)
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($checkedOutSha)) {
        throw '[G04DC_SOURCE_BOOTSTRAP_INVALID] Could not resolve the checked-out SHA.'
    }
    $checkedOutSha = $checkedOutSha.Trim()
    if (![string]::IsNullOrWhiteSpace($ExpectedSha) -and $checkedOutSha -cne $ExpectedSha) {
        throw '[G04DC_SOURCE_BOOTSTRAP_INVALID] Checked-out SHA does not match the expected SHA.'
    }
}

$sourceFileCount = 0
$sourceFiles = @()
$asciiGateStatus = 'NOT_RUN'
$parserGateStatus = 'NOT_RUN'
$parserErrorCount = 0
$incompleteTokenCount = 0
$malformedStringTokenCount = 0
$unknownTokenCount = 0

function New-G04DCSourceGateResult {
    return [pscustomobject][ordered]@{
        schemaVersion = 1
        checkedOutSha = $checkedOutSha
        windowsPowerShellVersion = $powerShellVersion
        sourceFileCount = $sourceFileCount
        sourceFiles = @($sourceFiles)
        asciiGateStatus = $asciiGateStatus
        parserGateStatus = $parserGateStatus
        parserErrorCount = $parserErrorCount
        incompleteTokenCount = $incompleteTokenCount
        malformedStringTokenCount = $malformedStringTokenCount
        unknownTokenCount = $unknownTokenCount
        precheckScriptStarted = $false
    }
}

function Write-G04DCBootstrapEvidence {
    if ([string]::IsNullOrWhiteSpace($BootstrapEvidencePath)) { return }
    $bootstrapPath = [IO.Path]::GetFullPath($BootstrapEvidencePath)
    $bootstrapParent = [IO.Path]::GetDirectoryName($bootstrapPath)
    if (!(Test-Path -LiteralPath $bootstrapParent -PathType Container)) {
        throw '[G04DC_SOURCE_BOOTSTRAP_INVALID] Bootstrap evidence parent does not exist.'
    }
    $json = (New-G04DCSourceGateResult | ConvertTo-Json -Depth 8) + [Environment]::NewLine
    [IO.File]::WriteAllText($bootstrapPath, $json, [Text.UTF8Encoding]::new($false))
}

try {
    $rawReportText = @(& $python -B $policy --repository-root $repo --source-root $source --json 2>&1)
    $pythonExitCode = $LASTEXITCODE
    try { $rawReport = ($rawReportText -join "`n") | ConvertFrom-Json }
    catch { throw '[G04DC_SOURCE_POLICY_UNAVAILABLE] Raw-byte validator did not return structured evidence.' }

    $sourceFileCount = [int]$rawReport.sourceFileCount
    $sourceFiles = @($rawReport.sourceFiles | ForEach-Object { [string]$_ })
    $asciiGateStatus = [string]$rawReport.asciiGateStatus
    if ($pythonExitCode -ne 0 -or $asciiGateStatus -cne 'PASS') {
        $asciiGateStatus = 'FAIL'
        Write-G04DCBootstrapEvidence
        $violations = @($rawReport.violations)
        if ($violations.Count -eq 0) { throw '[G04DC_SOURCE_POLICY_UNAVAILABLE] Raw-byte validator failed without a bounded violation.' }
        if (!$Quiet) {
            foreach ($violation in $violations) {
                Write-Output ("{0} byte offset {1}" -f [string]$violation.path, [long]$violation.offset)
            }
            Write-Output 'ASCII-byte gate result: FAIL'
        }
        $first = $violations[0]
        throw ("[G04DC_SOURCE_ASCII_INVALID] {0} byte offset {1}" -f [string]$first.path, [long]$first.offset)
    }

    $parserIssues = [System.Collections.Generic.List[object]]::new()
    foreach ($relativePath in $sourceFiles) {
        $candidate = Join-Path $repo ($relativePath.Replace('/', '\'))
        $tokens = $null
        $parseErrors = $null
        [System.Management.Automation.Language.Parser]::ParseFile($candidate, [ref]$tokens, [ref]$parseErrors) | Out-Null
        $fileIncomplete = @($parseErrors | Where-Object { $_.IncompleteInput })
        $fileMalformedStrings = @($parseErrors | Where-Object { $_.ErrorId -match 'TerminatorExpectedAtEndOfString|UnexpectedToken' })
        $fileUnknownTokens = @($tokens | Where-Object { $_.Kind -eq [System.Management.Automation.Language.TokenKind]::Unknown })
        $parserErrorCount += @($parseErrors).Count
        $incompleteTokenCount += $fileIncomplete.Count
        $malformedStringTokenCount += $fileMalformedStrings.Count
        $unknownTokenCount += $fileUnknownTokens.Count
        foreach ($parseError in @($parseErrors)) {
            $parserIssues.Add([pscustomobject][ordered]@{
                path = $relativePath
                line = [int]$parseError.Extent.StartLineNumber
                column = [int]$parseError.Extent.StartColumnNumber
                errorId = [string]$parseError.ErrorId
                incompleteInput = [bool]$parseError.IncompleteInput
            })
        }
        foreach ($unknownToken in $fileUnknownTokens) {
            $parserIssues.Add([pscustomobject][ordered]@{
                path = $relativePath
                line = [int]$unknownToken.Extent.StartLineNumber
                column = [int]$unknownToken.Extent.StartColumnNumber
                errorId = 'UnknownToken'
                incompleteInput = $false
            })
        }
    }

    if ($parserErrorCount -ne 0 -or $incompleteTokenCount -ne 0 -or $malformedStringTokenCount -ne 0 -or $unknownTokenCount -ne 0) {
        $parserGateStatus = 'FAIL'
        Write-G04DCBootstrapEvidence
        if (!$Quiet) {
            foreach ($issue in $parserIssues) {
                Write-Output ("{0} line {1} column {2} {3}" -f $issue.path, $issue.line, $issue.column, $issue.errorId)
            }
            Write-Output 'Windows PowerShell 5.1 parser gate result: FAIL'
        }
        $firstIssue = $parserIssues[0]
        throw ("[G04DC_SOURCE_PARSE_INVALID] {0} line {1} column {2} {3}" -f $firstIssue.path, $firstIssue.line, $firstIssue.column, $firstIssue.errorId)
    }

    $parserGateStatus = 'PASS'
    Write-G04DCBootstrapEvidence
    $result = New-G04DCSourceGateResult
    if (!$Quiet) {
        Write-Output "Windows PowerShell version: $powerShellVersion"
        Write-Output "G04D-C source files scanned: $sourceFileCount"
        Write-Output 'ASCII-byte gate result: PASS'
        Write-Output 'Windows PowerShell 5.1 parser gate result: PASS'
    }
    if ($PassThru) { return $result }
}
catch {
    if ($asciiGateStatus -cne 'NOT_RUN') {
        try { Write-G04DCBootstrapEvidence } catch {}
    }
    throw
}
