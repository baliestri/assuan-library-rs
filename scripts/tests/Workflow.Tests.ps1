$ErrorActionPreference = 'Stop'
# Contract checks complement actionlint's YAML/schema/expression parser.
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
function Resolve-GateSource([string] $Json, [string] $EventSha) {
  $text = [IO.File]::ReadAllText("$repo/.github/workflows/ci.yml").Replace("`r`n","`n")
  $match = [regex]::Match($text, '(?ms)      - name: Validate gate inputs\n.*?        run: \|\n(?<code>(?:          [^\n]*\n|\n)+)')
  if (-not $match.Success) { throw 'Missing input validation step.' }
  $code = [regex]::Replace($match.Groups['code'].Value, '(?m)^          ', '')
  $output = Join-Path $repo "target/gate-output-$([Guid]::NewGuid().ToString('N')).txt"
  $null = [IO.Directory]::CreateDirectory((Split-Path $output))
  $saved = @{}
  foreach ($name in 'CALL_INPUTS','EVENT_SHA','GITHUB_OUTPUT') { $saved[$name] = [Environment]::GetEnvironmentVariable($name) }
  try {
    $env:CALL_INPUTS = $Json; $env:EVENT_SHA = $EventSha; $env:GITHUB_OUTPUT = $output
    & ([scriptblock]::Create($code))
    return ([IO.File]::ReadAllText($output).Trim() -replace '^source_sha=', '')
  } finally {
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name]) }
  }
}

$sha = 'a' * 40
Assert-Equal (Resolve-GateSource '{}' $sha) $sha
Assert-Equal (Resolve-GateSource 'null' $sha) $sha
Assert-Throws { Resolve-GateSource '[]' $sha } 'inputs'
Assert-Equal (Resolve-GateSource ('{"source_sha":"' + $sha + '"}') ('b' * 40)) $sha
foreach ($bad in '', 'develop', 'v0.1.0', ('a' * 39), ('a' * 41), ('A' * 40), ($sha + "`n")) {
  $json = @{ source_sha = $bad } | ConvertTo-Json -Compress
  Assert-Throws { Resolve-GateSource $json $sha } 'SHA'
}
Assert-Throws { Resolve-GateSource '{"source_sha":42}' $sha } 'SHA'
$fixture = Join-Path $repo "target/release-tests/workflow-$([Guid]::NewGuid().ToString('N'))"
$null = [IO.Directory]::CreateDirectory($fixture)

function Assert-WorkflowContract([string] $Text) {
  $jobsText = $Text.Substring($Text.IndexOf("jobs:"))
  $expectedJobs = @([regex]::Matches($jobsText, '(?m)^  ([a-z][a-z_-]*):\r?$') | ForEach-Object { $_.Groups[1].Value } | Where-Object { $_ -ne 'gate' } | Sort-Object)
  $gateBlock = [regex]::Match($jobsText, '(?ms)^  gate:\r?\n.*\z').Value
  $needs = [regex]::Match($gateBlock, '(?m)^    needs: \[([^\]]+)\]').Groups[1].Value
  $actualJobs = @($needs.Split(',') | ForEach-Object { $_.Trim() } | Sort-Object)
  if (($actualJobs -join ',') -cne ($expectedJobs -join ',')) { throw 'Final gate must require every job.' }
  if (-not $gateBlock.Contains('if: ${{ always() }}') -or -not $gateBlock.Contains('JOB_RESULTS: ${{ toJSON(needs) }}')) { throw 'Final gate must reject skipped jobs.' }

  foreach ($inputName in 'source_sha') {
    if ($Text -notmatch "(?m)^      ${inputName}:$") { throw 'Missing workflow input.' }
  }
  if ($Text -match 'release_id|lock_artifact|gate-locks|Restore-GateLock') { throw 'Workflow must resolve its own dependencies.' }
  if ($Text -match '(?m)^\s*[a-z-]+: write\s*$|permissions: write-all|secrets:') { throw 'Unexpected write permission or secrets.' }
  foreach ($job in [regex]::Split($Text, '(?m)(?=^  [a-z][a-z_-]*:\r?$)')) {
    if ($job -notmatch 'actions/checkout@') { continue }
    $checkoutBlocks = [regex]::Matches($job, '(?ms)^      - uses: actions/checkout@.*?(?=^      - |\z)')
    foreach ($checkout in $checkoutBlocks) {
      if (-not $checkout.Value.Contains('ref: ${{ needs.prepare.outputs.source_sha }}')) { throw 'Checkout must use the validated SHA.' }
      if (-not $checkout.Value.Contains('persist-credentials: false')) { throw 'Checkout must discard credentials.' }
    }
    if ($job -notmatch '(?m)^    needs: prepare\r?$' -or -not $job.Contains('name: Verify checkout')) { throw 'Missing checkout verification dependency.' }
    if ($job -match '(?m)^\s*continue-on-error: true') { throw 'Gate failures cannot be ignored.' }
  }
  if (-not $Text.Contains('value: ${{ jobs.gate.outputs.source_sha }}')) { throw 'Missing completed gate SHA output.' }
  foreach ($block in [regex]::Matches($Text, '(?ms)^      - uses: actions/upload-artifact@.*?(?=^      - |^  [a-z]|\z)')) {
    if (-not $block.Value.Contains('github.run_id') -or -not $block.Value.Contains('github.run_attempt')) { throw 'Missing artifact run namespace.' }
  }
}
$ciText = [IO.File]::ReadAllText("$repo/.github/workflows/ci.yml")
foreach ($name in 'ci','interop','fuzz') {
  $text = [IO.File]::ReadAllText("$repo/.github/workflows/$name.yml")
  Assert-WorkflowContract $text
  $canonical = [regex]::Match($ciText, '(?ms)^  prepare:\r?\n.*?(?=^  [a-z])').Value
  Assert-Equal ([regex]::Match($text, '(?ms)^  prepare:\r?\n.*?(?=^  [a-z])').Value) $canonical
  $canonicalGate = [regex]::Match($ciText, '(?ms)      - name: Report completed gate.*\z').Value
  Assert-Equal ([regex]::Match($text, '(?ms)      - name: Report completed gate.*\z').Value) $canonicalGate
  Assert-Throws { Assert-WorkflowContract $text.Replace('ref: ${{ needs.prepare.outputs.source_sha }}', 'ref: develop') } 'SHA'
  Assert-Throws { Assert-WorkflowContract $text.Replace('ref: ${{ needs.prepare.outputs.source_sha }}', '# implicit checkout') } 'SHA'
  Assert-Throws { Assert-WorkflowContract $text.Replace('contents: read','contents: write') } 'permission'
  Assert-True ($text.Contains('push:') -and $text.Contains('pull_request:') -and $text.Contains('workflow_dispatch:'))
}
$ci = [IO.File]::ReadAllText("$repo/.github/workflows/ci.yml")
Assert-True ($ci.Contains('rhysd/actionlint:1.7.12@sha256:') -and $ci.Contains('sort -z'))
Assert-True ($ci.Contains('powershell-7.6.6-linux-x64.tar.gz') -and $ci.Contains('sha256sum --check'))
Assert-True ($ci.Contains('./scripts/test-release.ps1') -and $ci.Contains('./scripts/verify-docs.ps1'))
Assert-Equal ([regex]::Matches($ci, '--skip ').Count) 3
Assert-True ([IO.File]::ReadAllText("$repo/.github/workflows/fuzz.yml").Contains('schedule:'))
Write-Host 'Exact checkout, workflow inputs, permissions and negative contracts passed.'

# Exercise the actual final job script: skipped dependencies must not yield a SHA.
$workflow = [IO.File]::ReadAllText("$repo/.github/workflows/ci.yml").Replace("`r`n","`n")
$gateCode = [regex]::Match($workflow, '(?ms)      - name: Report completed gate\n.*?        run: \|\n(?<code>(?:          [^\n]*\n|\n)+)').Groups['code'].Value
Assert-True ($gateCode.Length -gt 0)
$gateScript = [scriptblock]::Create([regex]::Replace($gateCode, '(?m)^          ', ''))
$savedGate = @{}
foreach ($name in 'SOURCE_SHA','JOB_RESULTS','GITHUB_OUTPUT','GITHUB_STEP_SUMMARY') { $savedGate[$name] = [Environment]::GetEnvironmentVariable($name) }
try {
  $env:SOURCE_SHA = $sha
  $env:GITHUB_OUTPUT = "$fixture/result.txt"
  $env:GITHUB_STEP_SUMMARY = "$fixture/summary.txt"
  foreach ($result in 'failure','cancelled','skipped') {
    $env:JOB_RESULTS = @{prepare=@{result='success'};check=@{result=$result}} | ConvertTo-Json -Depth 4
    Assert-Throws { & $gateScript } 'did not succeed'
    Assert-True (-not (Test-Path $env:GITHUB_OUTPUT))
  }
  $env:JOB_RESULTS = '{"prepare":{"result":"success"},"check":{"result":"success"}}'
  & $gateScript
  Assert-Equal ([IO.File]::ReadAllText($env:GITHUB_OUTPUT).Trim()) "source_sha=$sha"
} finally {
  foreach ($name in $savedGate.Keys) { [Environment]::SetEnvironmentVariable($name,$savedGate[$name]) }
}

Assert-Throws { Assert-WorkflowContract $ci.Replace('dependencies, release_scripts]', 'dependencies]') } 'every job'
Assert-Throws { Assert-WorkflowContract $ci.Replace('if: ${{ always() }}', 'if: ${{ success() }}') } 'skipped'

Assert-True ($ci.Contains("-name '*.yml'") -and $ci.Contains("-name '*.yaml'"))

# Run every checkout verification against the real Git repository.
$actualSha = git -C $repo rev-parse HEAD
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect workflow fixture checkout.' }
$savedSource = $env:SOURCE_SHA
$savedSummary = $env:GITHUB_STEP_SUMMARY
Push-Location $repo
try {
  $env:GITHUB_STEP_SUMMARY = "$fixture/checkouts.txt"
  foreach ($workflowName in 'ci','interop','fuzz') {
    $workflowText = [IO.File]::ReadAllText("$repo/.github/workflows/$workflowName.yml")
    $blocks = [regex]::Matches($workflowText, '(?ms)      - name: Verify checkout\r?\n.*?        run: \|\r?\n(?<code>(?:          [^\n]*\n|\r?\n)+)')
    Assert-True ($blocks.Count -gt 0)
    foreach ($block in $blocks) {
      $code = [scriptblock]::Create([regex]::Replace($block.Groups['code'].Value, '(?m)^          ', ''))
      $env:SOURCE_SHA = $actualSha
      & $code
      $env:SOURCE_SHA = $sha
      Assert-Throws { & $code } 'checkout'
      $env:SOURCE_SHA = 'develop'
      Assert-Throws { & $code } 'checkout'
    }
  }
} finally {
  Pop-Location
  $env:SOURCE_SHA = $savedSource
  $env:GITHUB_STEP_SUMMARY = $savedSummary
}
