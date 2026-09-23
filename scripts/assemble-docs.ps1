<#
.SYNOPSIS
Assembles a new Pages directory from a complete JSON array of version bundles.
.DESCRIPTION
Each item contains version, archive and sha256. Relative archive paths are
resolved against the manifest's directory. Relative Manifest/Output paths are
resolved against the workspace; output must be a new path under target/.
Historical bundles are not regenerated and no deployment occurs.
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory)][string] $Manifest,
  [Parameter(Mandatory)][string] $Output,
  [Parameter(Mandatory)][Uri] $BaseUrl
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Import-Module "$PSScriptRoot/release/Archive.psm1" -Force
$root = Split-Path -Parent $PSScriptRoot
if (-not [IO.Path]::IsPathRooted($Manifest)) { $Manifest = Join-Path $root $Manifest }
if (-not [IO.Path]::IsPathRooted($Output)) { $Output = Join-Path $root $Output }
$Manifest = [IO.Path]::GetFullPath($Manifest)
$bundles = @(Get-Content -LiteralPath $Manifest -Raw | ConvertFrom-Json)
foreach ($bundle in $bundles) {
  if (-not [IO.Path]::IsPathRooted($bundle.archive)) {
    $bundle.archive = Join-Path ([IO.Path]::GetDirectoryName($Manifest)) $bundle.archive
  }
}
Merge-DocsVersions $bundles $Output $BaseUrl | ConvertTo-Json -Depth 4
