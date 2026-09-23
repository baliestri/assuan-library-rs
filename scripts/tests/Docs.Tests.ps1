Import-Module "$PSScriptRoot/../release/Docs.psm1" -Force
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$fixture = Join-Path $repo "target/release-tests/docs-$([Guid]::NewGuid().ToString('N'))"
function Write-Fixture([string] $Path, [string] $Text) {
  $null = New-Item -ItemType Directory -Path (Split-Path $Path) -Force
  [IO.File]::WriteAllText($Path, $Text, [Text.UTF8Encoding]::new($false))
}
$root = "$fixture/source"
$api = "$fixture/api"
$base = [Uri]'https://example.test/assuan-library-rs/'
$null = New-Item -ItemType Directory -Path "$root/docs/site" -Force
Copy-Item "$repo/docs/site/page.html", "$repo/docs/site/site.css" "$root/docs/site/"
$selection = @{
  repository = 'https://github.com/baliestri/assuan-library-rs'
  tagFiles = @('src/extra.rs')
  sources = @(
    @{source='README.md';destination='index.md';title='Overview';includeInFull=$true;optional=$false},
    @{source='docs/guide.md';destination='guides/guide.md';title='Guide & examples';includeInFull=$true;optional=$false},
    @{source='example.rs';destination='examples/example.md';title='Example';includeInFull=$true;optional=$false},
    @{source='docs/ops.md';destination='guides/ops.md';title='Operations';includeInFull=$false;optional=$true}
  )
}
Write-Fixture "$root/docs/site/sources.json" ($selection | ConvertTo-Json -Depth 5)
Write-Fixture "$root/README.md" @'
# Overview

[Guide](docs/guide.md#details), [own](https://github.com/baliestri/assuan-library-rs/blob/develop/docs/guide.md), [outside](https://example.org/).
[code](src/extra.rs), [reference][guide], [self](#overview).

[guide]: docs/guide.md#details

`[literal](missing.md)`

```text
[literal](missing.md)
<a href="missing.html">literal</a>
```

<a href="docs/guide.md#details">HTML guide</a>

<https://github.com/baliestri/assuan-library-rs/blob/develop/docs/guide.md>
'@
Write-Fixture "$root/docs/guide.md" "# Guide`r`n`r`n## Details`r`n`r`n[Home](../README.md)`r`n"
Write-Fixture "$root/example.rs" '// A literal ``` fence and [link](not-a-link.md).'
Write-Fixture "$root/src/extra.rs" '// code linked by immutable tag'
Write-Fixture "$root/docs/ops.md" '# Operational content excluded from full'
Write-Fixture "$api/assuan_library/index.html" @'
<!doctype html><html><body id="api"><a href="../assuan_protocol/index.html#command">Protocol</a>
<a href="https://github.com/baliestri/assuan-library-rs/blob/develop/docs/guide.md">Guide</a>
<script>const literal = '<a href="missing.html">';</script></body></html>
'@
Write-Fixture "$api/assuan_protocol/index.html" '<h1 id="command">Command</h1><a href="#impl-From%3CT%3E">Generic</a><h2 id="impl-From%3CT%3E">Generic</h2>'
Write-Fixture "$api/search.js" 'const literal = "https://github.com/baliestri/assuan-library-rs/blob/develop/docs/guide.md";'
Write-Fixture "$api/src/example.rs.html" '<a href=#1-2>Source</a><a id=1></a><a id=2></a>'
Write-Fixture "$api/help.html" '<a href="index.html">All crates</a>'
$first = Build-VersionDocs $root '0.1.0' $base "$fixture/out-a" $api
$previousCulture = [Globalization.CultureInfo]::CurrentCulture
try {
  [Globalization.CultureInfo]::CurrentCulture = [Globalization.CultureInfo]::GetCultureInfo('tr-TR')
  $second = Build-VersionDocs $root '0.1.0' $base "$fixture/out-b" $api
} finally {
  [Globalization.CultureInfo]::CurrentCulture = $previousCulture
}
Assert-Equal $first.version '0.1.0'
Assert-Equal ($first.files | ConvertTo-Json -Depth 4 -Compress) ($second.files | ConvertTo-Json -Depth 4 -Compress)
$md = Get-Content "$fixture/out-a/0.1.0/index.md" -Raw
Assert-True ($md.Contains('https://example.test/assuan-library-rs/0.1.0/guides/guide.md#details'))
Assert-True ($md.Contains('blob/v0.1.0/src/extra.rs'))
Assert-True ($md.Contains('[literal](missing.md)'))
Assert-True ($md.Contains('https://example.org/'))
Assert-True (-not $md.Contains('blob/develop/'))
$html = Get-Content "$fixture/out-a/0.1.0/index.html" -Raw
Assert-True ($html.Contains('/0.1.0/guides/guide.html#details'))
Assert-True ($html.Contains('id="overview"'))
$full = Get-Content "$fixture/out-a/0.1.0/llms-full.txt" -Raw
Assert-True (-not $full.Contains('Operational content excluded from full'))
Assert-True ($full.Contains('Source:'))
Assert-True (-not $full.Contains("`r"))
Assert-True ((Get-Content "$fixture/out-a/0.1.0/llms.txt" -Raw).Contains('## Optional'))
Assert-Equal (Get-Content "$fixture/out-a/0.1.0/api/search.js" -Raw) (Get-Content "$api/search.js" -Raw)
Test-DocsLinks "$fixture/out-a" $base
Assert-Throws { Build-VersionDocs $root '0.1.0' $base "$fixture/out-a" $api } 'exists'
Assert-Throws { Build-VersionDocs $root '0.1.0' $base "$root/output" $api } 'overlap'
Assert-Throws { Build-VersionDocs $root '0.1.0' $base "$fixture/absent-api" "$fixture/missing-api" } 'API'
Write-Fixture "$fixture/out-a/0.1.0/broken.html" '<a href="guides/guide.html#absent">Broken</a>'
Assert-Throws { Test-DocsLinks "$fixture/out-a" $base } 'anchor'
Write-Fixture "$fixture/out-a/0.1.0/broken.html" '<a href="missing.html">Broken</a>'
Assert-Throws { Test-DocsLinks "$fixture/out-a" $base } 'Missing site link'
Write-Fixture "$root/docs/site/sources.json" (($selection | ConvertTo-Json -Depth 5).Replace('docs/ops.md','docs/future-ops.md'))
$optional = Build-VersionDocs $root '0.1.0' $base "$fixture/optional-absent" $api
Assert-True (-not (Test-Path "$($optional.root)/guides/ops.md"))
Write-Fixture "$root/docs/site/sources.json" ($selection | ConvertTo-Json -Depth 5)
Write-Fixture "$fixture/linked-input/guide.md" '# Must not read through a link'
$linkType = if ($IsWindows) { 'Junction' } else { 'SymbolicLink' }
$null = New-Item -ItemType $linkType -Path "$root/linked" -Target "$fixture/linked-input"
$selection.sources[1].source = 'linked/guide.md'
Write-Fixture "$root/docs/site/sources.json" ($selection | ConvertTo-Json -Depth 5)
Assert-Throws { Build-VersionDocs $root '0.1.0' $base "$fixture/linked-source" $api } 'Linked document path'
$selection.sources[1].source = 'docs/guide.md'
Write-Fixture "$root/docs/site/sources.json" ($selection | ConvertTo-Json -Depth 5)
Write-Fixture "$root/README.md" '[Missing](missing.md)'
Assert-Throws { Build-VersionDocs $root '0.1.0' $base "$fixture/unselected" $api } 'selected'
$selection.sources[0].source = '../outside.md'
Write-Fixture "$root/docs/site/sources.json" ($selection | ConvertTo-Json -Depth 5)
Assert-Throws { Build-VersionDocs $root '0.1.0' $base "$fixture/escape" $api } 'path'
Write-Host 'Deterministic documents, versioned links, code preservation and broken-link rejection passed.'
