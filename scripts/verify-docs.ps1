<#
.SYNOPSIS
Checks deterministic guides, LLM documents and bundles against existing rustdoc.
.DESCRIPTION
Run cargo doc --locked --workspace --all-features --no-deps first.
Uses the pinned generator and writes only unique directories under target/.
#>
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Import-Module "$PSScriptRoot/release/Docs.psm1"
Import-Module "$PSScriptRoot/release/Archive.psm1"
$root = Split-Path -Parent $PSScriptRoot
$config = Get-Content "$PSScriptRoot/release/config.json" -Raw | ConvertFrom-Json
if ($PSVersionTable.PSVersion.ToString() -cne $config.powershell) { throw 'Unexpected documentation PowerShell version.' }
Push-Location $root
try {
  $metadata = & cargo metadata --locked --no-deps --format-version 1
  if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect documentation version.' }
  $metadata = $metadata | ConvertFrom-Json
  $version = ($metadata.packages | Where-Object name -CEQ 'assuan-library').version
  $output = Join-Path $root "target/docs-verification/$([Guid]::NewGuid().ToString('N'))"
  $base = [Uri]'https://example.test/assuan-library-rs/'
  $first = Build-VersionDocs $root $version $base "$output/a" "$root/target/doc"
  $second = Build-VersionDocs $root $version $base "$output/b" "$root/target/doc"
  if (($first.files | ConvertTo-Json -Depth 5 -Compress) -cne ($second.files | ConvertTo-Json -Depth 5 -Compress)) { throw 'Documentation file hashes are not deterministic.' }
  Test-DocsLinks "$output/a" $base
  Test-DocsLinks "$output/b" $base
  $bundleA = New-DocsBundle $first.root "$output/a.zip"
  $bundleB = New-DocsBundle $second.root "$output/b.zip"
  if ($bundleA.sha256 -cne $bundleB.sha256) { throw 'Documentation ZIP is not deterministic.' }
  Write-Host "Deterministic documentation and bundle verified: $version ($($first.files.Count) files)."
} finally { Pop-Location }
