<#
.SYNOPSIS
Builds one version of the public guides, rustdoc and LLM documentation.
.DESCRIPTION
Run cargo doc first. Output must not contain this version already; previous
builds are never removed. No Git, deployment or registry operations occur.
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory)][string] $Version,
  [Parameter(Mandatory)][Uri] $BaseUrl,
  [string] $Output = 'target/release-docs',
  [string] $ApiDirectory = 'target/doc'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Import-Module "$PSScriptRoot/release/Docs.psm1" -Force
$root = Split-Path -Parent $PSScriptRoot
# Resolve relative paths against the workspace, regardless of the caller's cwd.
if (-not [IO.Path]::IsPathRooted($Output)) { $Output = Join-Path $root $Output }
if (-not [IO.Path]::IsPathRooted($ApiDirectory)) { $ApiDirectory = Join-Path $root $ApiDirectory }
$result = Build-VersionDocs $root $Version $BaseUrl $Output $ApiDirectory
$result | ConvertTo-Json -Depth 5
