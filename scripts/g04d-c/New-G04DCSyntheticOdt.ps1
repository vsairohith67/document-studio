param(
    [Parameter(Mandatory = $true)] [string]$Destination
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (Test-Path -LiteralPath $Destination) {
    throw "Refusing to overwrite synthetic fixture: $Destination"
}
$parent = Split-Path -Parent $Destination
if (!(Test-Path -LiteralPath $parent)) { New-Item -ItemType Directory -Path $parent | Out-Null }

Add-Type -AssemblyName System.IO.Compression
$stream = [System.IO.File]::Open($Destination, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::ReadWrite, [System.IO.FileShare]::None)
$archive = [System.IO.Compression.ZipArchive]::new($stream, [System.IO.Compression.ZipArchiveMode]::Create, $false)
try {
    $entries = [ordered]@{
        'mimetype' = 'application/vnd.oasis.opendocument.text'
        'content.xml' = @'
<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.3"><office:body><office:text><text:p>Document Studio G04D-C synthetic runtime smoke</text:p></office:text></office:body></office:document-content>
'@
        'META-INF/manifest.xml' = @'
<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.text" manifest:version="1.3"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>
'@
    }
    foreach ($pair in $entries.GetEnumerator()) {
        $compression = if ($pair.Key -ceq 'mimetype') { [System.IO.Compression.CompressionLevel]::NoCompression } else { [System.IO.Compression.CompressionLevel]::Optimal }
        $entry = $archive.CreateEntry($pair.Key, $compression)
        $entryStream = $entry.Open()
        $writer = [System.IO.StreamWriter]::new($entryStream, [System.Text.UTF8Encoding]::new($false))
        try { $writer.Write([string]$pair.Value) }
        finally { $writer.Dispose() }
    }
}
finally {
    $archive.Dispose()
    $stream.Dispose()
}

Write-Output $Destination
