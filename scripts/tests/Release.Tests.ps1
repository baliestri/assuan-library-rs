# Execute the actual workflow's Git steps against disposable local repositories.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$versions = Get-TestReleaseVersions $repo
$releaseTag = "v$($versions.release)"
$releaseTagRef = "refs/tags/$releaseTag"
$workflow = [IO.File]::ReadAllText("$repo/.github/workflows/release.yml").Replace("`r`n","`n")
function Get-ReleaseStep([string] $Name) {
  $namePattern = [regex]::Escape($Name)
  $match = [regex]::Match($workflow, "(?ms)^      - name: $namePattern\n(?:(?!^      - ).)*?^        run: \|\n(?<code>(?:          [^\n]*\n|\n)+)")
  if (-not $match.Success) { throw "Missing workflow step: $Name" }
  return [scriptblock]::Create([regex]::Replace($match.Groups['code'].Value, '(?m)^          ', ''))
}
$dispatch = Get-ReleaseStep 'Validate dispatch'
$prepare = Get-ReleaseStep 'Prepare release commit'
$promote = Get-ReleaseStep 'Promote validated commit and tag'
$sync = Get-ReleaseStep 'Synchronize develop'
$report = Get-ReleaseStep 'Report release outcome'
$root = Join-Path $repo "target/release-tests/release-$([Guid]::NewGuid().ToString('N'))"
$null = New-Item -ItemType Directory -Path "$root/hooks" -Force
$envNames = @('VERSION','AUTHENTICATION','BOOTSTRAP_CONFIGURED','EVENT_SHA','SOURCE_SHA',
  'GITHUB_REF','GITHUB_OUTPUT','GITHUB_STEP_SUMMARY','JOBS','GIT_CONFIG_GLOBAL','GIT_CONFIG_NOSYSTEM')
$saved = @{}
foreach ($name in $envNames) { $saved[$name] = [Environment]::GetEnvironmentVariable($name) }
function Test-Git {
  param([Parameter(ValueFromRemainingArguments)][string[]] $Arguments)
  $result = @(& git @Arguments)
  if ($LASTEXITCODE -ne 0) { throw "Fixture Git failed: $($Arguments[0])" }
  return ,$result
}
function New-ReleaseFixture([string] $Name) {
  $dir = Join-Path $root $Name
  $null = New-Item -ItemType Directory -Path "$dir/checkout" -Force
  Test-Git init --bare --initial-branch=develop "$dir/origin.git" | Out-Null
  Test-Git init --initial-branch=develop "$dir/checkout" | Out-Null
  $checkout = "$dir/checkout"
  Test-Git -C $checkout config user.name 'Release fixture' | Out-Null
  Test-Git -C $checkout config user.email 'fixture@example.invalid' | Out-Null
  Test-Git -C $checkout config commit.gpgsign false | Out-Null
  Test-Git -C $checkout config core.hooksPath "$root/hooks" | Out-Null
  Test-Git -C $checkout config core.autocrlf false | Out-Null
  $paths = @('Cargo.toml','README.md','docs/publishing.md','crates/assuan-library/README.md',
    'scripts/release/Version.psm1','scripts/release/config.json')
  $config = Get-Content "$repo/scripts/release/config.json" -Raw | ConvertFrom-Json
  $paths += @($config.crates | ForEach-Object { "crates/$_/Cargo.toml" })
  foreach ($path in $paths) {
    $destination = Join-Path $checkout $path
    $null = [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination))
    Copy-Item -LiteralPath "$repo/$path" -Destination $destination
  }
  Test-Git -C $checkout add -- . | Out-Null
  Test-Git -C $checkout commit -m 'Fixture base' | Out-Null
  Test-Git -C $checkout branch main | Out-Null
  Test-Git -C $checkout remote add origin "$dir/origin.git" | Out-Null
  Test-Git -C $checkout push origin main develop | Out-Null
  $base = (Test-Git -C $checkout rev-parse HEAD)[0]
  # The workflow must override inherited/local signing preferences itself.
  Test-Git -C $checkout config commit.gpgsign true | Out-Null
  Test-Git -C $checkout config tag.gpgsign true | Out-Null
  return [pscustomobject]@{path=$checkout;remote="$dir/origin.git";base=$base}
}
function Invoke-Preparation($Fixture) {
  $env:VERSION = $versions.release
  $env:EVENT_SHA = $Fixture.base
  $env:GITHUB_OUTPUT = Join-Path (Split-Path $Fixture.path) 'outputs.txt'
  Push-Location $Fixture.path
  try {
    & $prepare | Out-Null
    $sha = (Test-Git rev-parse HEAD)[0]
    $env:SOURCE_SHA = $sha
    Assert-True ([IO.File]::ReadAllText($env:GITHUB_OUTPUT).Contains("source_sha=$sha"))
    Assert-Equal (Test-Git show -s --format=%an HEAD)[0] 'github-actions[bot]'
    Assert-Equal (Test-Git show -s --format=%ae HEAD)[0] '41898282+github-actions[bot]@users.noreply.github.com'
    Assert-Equal (Test-Git rev-parse HEAD^)[0] $Fixture.base
    Assert-True (-not ((Test-Git cat-file commit HEAD) -match '^gpgsig '))
    return $sha
  } finally { Pop-Location }
}
try {
  [IO.File]::WriteAllText("$root/gitconfig",'')
  $env:GIT_CONFIG_GLOBAL = "$root/gitconfig"
  $env:GIT_CONFIG_NOSYSTEM = '1'
  $env:GITHUB_STEP_SUMMARY = "$root/summary.md"
  $env:GITHUB_REF = 'refs/heads/develop'
  $env:AUTHENTICATION = 'bootstrap'
  $env:BOOTSTRAP_CONFIGURED = 'true'
  $env:VERSION = $versions.release
  & $dispatch
  $env:BOOTSTRAP_CONFIGURED = 'false'
  Assert-Throws { & $dispatch } 'CARGO_BOOTSTRAP_TOKEN'
  $env:AUTHENTICATION = 'oidc'
  & $dispatch
  foreach ($value in '1.0.0;echo x', "1.0.0`n", 'v1.0.0', '01.0.0') {
    $env:VERSION = $value
    Assert-Throws { & $dispatch } 'version'
  }
  $env:VERSION = $versions.release
  $env:GITHUB_REF = 'refs/heads/main'
  Assert-Throws { & $dispatch } 'develop'
  $env:GITHUB_REF = 'refs/heads/develop'

  $fixture = New-ReleaseFixture 'normal'
  $sha = Invoke-Preparation $fixture
  Push-Location $fixture.path
  try {
    & $promote | Out-Null
    Assert-Equal (Test-Git --git-dir=$($fixture.remote) rev-parse refs/heads/main)[0] $sha
    Assert-Equal (Test-Git --git-dir=$($fixture.remote) rev-parse $releaseTagRef)[0] $sha
    Assert-Equal (Test-Git cat-file -t $releaseTagRef)[0] 'commit'
    # Repeating promotion accepts exactly the same refs without moving a tag.
    & $promote | Out-Null
    & $sync | Out-Null
    Assert-Equal (Test-Git --git-dir=$($fixture.remote) rev-parse refs/heads/develop)[0] $sha
  } finally { Pop-Location }

  $fixture = New-ReleaseFixture 'divergent-develop'
  $sha = Invoke-Preparation $fixture
  Push-Location $fixture.path
  try {
    & $promote | Out-Null
    Test-Git switch develop | Out-Null
    [IO.File]::WriteAllText("$PWD/independent.txt",'An independent develop change.')
    Test-Git add -- independent.txt | Out-Null
    Test-Git commit -m 'Advance develop' | Out-Null
    $advanced = (Test-Git rev-parse HEAD)[0]
    Test-Git push origin HEAD:refs/heads/develop | Out-Null
    & $sync | Out-Null
    $parents = (Test-Git show -s --format=%P HEAD)[0].Split(' ')
    Assert-Equal $parents.Count 2
    Assert-Equal $parents[0] $advanced
    Assert-Equal $parents[1] $sha
    Assert-True (-not ((Test-Git cat-file commit HEAD) -match '^gpgsig '))
    Assert-True (Test-Path -LiteralPath "$PWD/independent.txt")
  } finally { Pop-Location }

  $fixture = New-ReleaseFixture 'tag-conflict'
  $sha = Invoke-Preparation $fixture
  Push-Location $fixture.path
  try {
    Test-Git -c tag.gpgsign=false tag $releaseTag $fixture.base | Out-Null
    Test-Git push origin $releaseTagRef | Out-Null
    Assert-Throws { & $promote } 'tag conflicts'
    Assert-Equal (Test-Git --git-dir=$($fixture.remote) rev-parse refs/heads/main)[0] $fixture.base
  } finally { Pop-Location }

  $fixture = New-ReleaseFixture 'main-conflict'
  $sha = Invoke-Preparation $fixture
  Push-Location $fixture.path
  try {
    Test-Git switch main | Out-Null
    Test-Git commit --allow-empty -m 'Main changed independently' | Out-Null
    $advanced = (Test-Git rev-parse HEAD)[0]
    Test-Git push origin HEAD:refs/heads/main | Out-Null
    Test-Git switch --detach $sha | Out-Null
    Assert-Throws { & $promote } 'Main diverged'
    Assert-Equal (Test-Git --git-dir=$($fixture.remote) rev-parse refs/heads/main)[0] $advanced
    Assert-Equal ((Test-Git ls-remote origin $releaseTagRef).Count) 0
  } finally { Pop-Location }

  $fixture = New-ReleaseFixture 'merge-conflict'
  $sha = Invoke-Preparation $fixture
  Push-Location $fixture.path
  try {
    & $promote | Out-Null
    Test-Git switch develop | Out-Null
    $manifest = [IO.File]::ReadAllText("$PWD/Cargo.toml").Replace(
      "version = `"$($versions.current)`"", "version = `"$($versions.next)`"")
    [IO.File]::WriteAllText("$PWD/Cargo.toml",$manifest)
    Test-Git add -- Cargo.toml | Out-Null
    Test-Git commit -m 'Conflicting version edit' | Out-Null
    $advanced = (Test-Git rev-parse HEAD)[0]
    Test-Git push origin HEAD:refs/heads/develop | Out-Null
    Assert-Throws { & $sync } 'git|conflict'
    Assert-Equal (Test-Git --git-dir=$($fixture.remote) rev-parse refs/heads/develop)[0] $advanced
    Assert-Equal (Test-Git --git-dir=$($fixture.remote) rev-parse $releaseTagRef)[0] $sha
  } finally { Pop-Location }

  $jobNames = @('prepare','ci','interop','fuzz','build','publish','docs','sync')
  foreach ($failure in 'failure','cancelled','skipped') {
    $jobs = @{}
    foreach ($name in $jobNames) { $jobs[$name] = @{result='success'} }
    $jobs.publish.result = $failure
    $env:JOBS = $jobs | ConvertTo-Json -Depth 5
    Assert-Throws { & $report } 'incomplete'
  }
  $jobs.publish.result = 'success'
  $env:JOBS = $jobs | ConvertTo-Json -Depth 5
  & $report
  Write-Host 'Real local Git preparation, unsigned bot merges, exact tag promotion, conflicts and failure summary passed.'
} finally {
  foreach ($name in $envNames) { [Environment]::SetEnvironmentVariable($name,$saved[$name]) }
}
