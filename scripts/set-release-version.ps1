<#
.SYNOPSIS
Previews or applies shared release-version changes without Git operations.
.EXAMPLE
./scripts/set-release-version.ps1 -Version 0.2.0 -Check
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory)][string] $Version,
  [switch] $Check,
  [string] $Root = (Split-Path -Parent $PSScriptRoot)
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Import-Module "$PSScriptRoot/release/Version.psm1" -Force
$edits = @(Get-VersionEdits -Root $Root -Version $Version)
if (-not $Check) { Set-ReleaseVersion -Root $Root -Edits $edits }
ConvertTo-Json -InputObject @($edits | Select-Object path, before, after) -Depth 4
