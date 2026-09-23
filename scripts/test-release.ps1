# Run explicitly registered release tests without installing a test framework.
[CmdletBinding()]
param([ValidateSet('All', 'Version', 'Docs', 'Archive', 'Workflow', 'Release', 'PublishCrates', 'DocsDeploy')][string] $Suite = 'All')

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. "$PSScriptRoot/tests/Support.ps1"
$registered = [ordered]@{
  Version = 'Version.Tests.ps1'
  Docs = 'Docs.Tests.ps1'
  Archive = 'Archive.Tests.ps1'
  Workflow = 'Workflow.Tests.ps1'
  Release = 'Release.Tests.ps1'
  PublishCrates = 'PublishCrates.Tests.ps1'
  DocsDeploy = 'DocsDeploy.Tests.ps1'
}
foreach ($name in $registered.Keys) {
  if ($Suite -ne 'All' -and $Suite -ne $name) { continue }
  & (Join-Path "$PSScriptRoot/tests" $registered[$name])
  Write-Host "PASS: $name"
}
