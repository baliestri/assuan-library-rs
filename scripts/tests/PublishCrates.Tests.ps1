# All commands crossing the registry boundary are scoped mocks. No network or upload.
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$publishFixtureRoot = Join-Path $repo "target/release-tests/publish-$([Guid]::NewGuid().ToString('N'))"
$null = New-Item -ItemType Directory -Path "$publishFixtureRoot/scripts/release", "$publishFixtureRoot/target/package" -Force
Copy-Item "$PSScriptRoot/../publish-crates.ps1" "$publishFixtureRoot/scripts/publish-crates.ps1"
Copy-Item "$PSScriptRoot/../release/Version.psm1", "$PSScriptRoot/../release/config.json" "$publishFixtureRoot/scripts/release/"
$publishFixtureConfig = Get-Content "$PSScriptRoot/../release/config.json" -Raw | ConvertFrom-Json
$publishFixtureMetadata = @{
  packages = @($publishFixtureConfig.crates | ForEach-Object { @{ id=$_; name=$_; version='0.1.0' } })
  workspace_members = @($publishFixtureConfig.crates)
  target_directory = "$publishFixtureRoot/target"
}
foreach ($name in $publishFixtureConfig.crates) {
  [IO.File]::WriteAllText("$publishFixtureRoot/target/package/$name-0.1.0.crate", 'fixture archive')
}
$publishFixtureChecksum = (Get-FileHash "$publishFixtureRoot/target/package/$($publishFixtureConfig.crates[0])-0.1.0.crate").Hash.ToLowerInvariant()

$publishFixtureState = @{}
function Reset-PublishFixture {
  $publishFixtureState.Clear()
  $initial = @{
    Calls=[Collections.Generic.List[string]]::new(); Uploads=[Collections.Generic.List[string]]::new()
    Dirty=$false; BadToolchain=$false; BadVersion=$false; MissingCrate=$false; CargoFailure=''
    HttpFailure=0; NetworkFailure=$false; Delays=0; Requests=0; Sleeps=0; Yanked=$false; BadChecksum=$false; DryFailure=$false
  }
  foreach ($key in $initial.Keys) { $publishFixtureState[$key] = $initial[$key] }
  [IO.File]::WriteAllText("$publishFixtureRoot/Cargo.lock", 'fixture lock')
}
function git {
  $global:LASTEXITCODE = 0
  if (($args -join ' ') -cne 'status --porcelain --untracked-files=all') { throw 'Unexpected Git call.' }
  if ($publishFixtureState.Dirty) { return '?? unexpected-file' }
}
function cargo {
  $global:LASTEXITCODE = 0
  $command = $args -join ' '
  $publishFixtureState.Calls.Add($command)
  if ($command -ceq '--version') {
    if ($publishFixtureState.BadToolchain) { return 'cargo 0.0.0 (fixture)' }
    return "cargo $($publishFixtureConfig.rust) (fixture)"
  }
  if ($command -ceq 'metadata --locked --no-deps --format-version 1') {
    $metadata = $publishFixtureMetadata | ConvertTo-Json -Depth 5 | ConvertFrom-Json
    if ($publishFixtureState.BadVersion) { $metadata.packages[0].version = '0.2.0' }
    if ($publishFixtureState.MissingCrate) { $metadata.packages = @($metadata.packages | Select-Object -Skip 1) }
    return $metadata | ConvertTo-Json -Depth 5
  }
  if ($command -ceq 'publish --workspace --dry-run --locked --registry crates-io') {
    if ($publishFixtureState.DryFailure) { $global:LASTEXITCODE = 1 }
    return
  }
  if ($command -match '^publish -p ([a-z-]+) --locked --registry crates-io$') {
    $crate = $Matches[1]
    $publishFixtureState.Uploads.Add($crate)
    if ($publishFixtureState.CargoFailure -ceq $crate) { $global:LASTEXITCODE = 1 }
    return
  }
  throw "Unexpected Cargo call: $command"
}
function Invoke-RestMethod {
  param($Uri, $Headers, $TimeoutSec, $MaximumRetryCount)
  Assert-Equal $TimeoutSec 30
  Assert-Equal $MaximumRetryCount 0
  Assert-Equal $Headers.Count 1
  Assert-True ($Uri -match '^https://crates.io/api/v1/crates/([a-z-]+)/0\.1\.0$')
  $crate = $Matches[1]
  $publishFixtureState.Requests++
  if ($publishFixtureState.NetworkFailure) { throw [Net.Http.HttpRequestException]::new('Fixture network failure.') }
  $status = $publishFixtureState.HttpFailure
  if ($publishFixtureState.Delays -gt 0) { $publishFixtureState.Delays--; $status=404 }
  if ($status) {
    $response = [Net.Http.HttpResponseMessage]::new([Net.HttpStatusCode]$status)
    throw [Microsoft.PowerShell.Commands.HttpResponseException]::new('Registry fixture error.', $response)
  }
  $checksum = if ($publishFixtureState.BadChecksum) { '0' * 64 } else { $publishFixtureChecksum }
  return @{ version=@{ crate=$crate; num='0.1.0'; yanked=$publishFixtureState.Yanked; checksum=$checksum } }
}
function Start-Sleep {
  param($Seconds)
  Assert-Equal $Seconds 5
  $publishFixtureState.Sleeps++
  if ($publishFixtureState.Sleeps -gt 2) { throw 'Fixture prevented an unbounded poll.' }
}
function Invoke-PublishFixture([switch] $DryRun) {
  $messages = [Collections.Generic.List[string]]::new()
  $failure = ''
  try {
    & "$publishFixtureRoot/scripts/publish-crates.ps1" -Version 0.1.0 -DryRun:$DryRun 6>&1 |
      ForEach-Object { $messages.Add($_.ToString()) }
  }
  catch { $failure = $_.Exception.Message }
  return [pscustomobject]@{ Error=$failure; Log=($messages -join "`n") }
}

$savedToken = $env:CARGO_REGISTRY_TOKEN
$savedNamedToken = $env:CARGO_REGISTRIES_CRATES_IO_TOKEN
try {
  $env:CARGO_REGISTRY_TOKEN = $null
  $env:CARGO_REGISTRIES_CRATES_IO_TOKEN = $null
  Reset-PublishFixture
  $result = Invoke-PublishFixture -DryRun
  Assert-Equal $result.Error ''
  Assert-Equal $publishFixtureState.Uploads.Count 0
  Assert-Equal $publishFixtureState.Requests 0
  Assert-True ($publishFixtureState.Calls.Contains('publish --workspace --dry-run --locked --registry crates-io'))
  $result = Invoke-PublishFixture
  Assert-True ($result.Error -match 'explicit crates.io token')
  Assert-True ($result.Log -match 'No upload was attempted')

  $env:CARGO_REGISTRY_TOKEN = 'fixture-token-never-sent'
  foreach ($case in 'Dirty', 'BadToolchain', 'BadVersion', 'MissingCrate') {
    Reset-PublishFixture
    $publishFixtureState[$case] = $true
    $result = Invoke-PublishFixture
    Assert-True ($result.Error.Length -gt 0)
    Assert-Equal $publishFixtureState.Uploads.Count 0
    Assert-Equal $publishFixtureState.Requests 0
  }
  Reset-PublishFixture
  $result = Invoke-PublishFixture
  Assert-Equal $result.Error ''
  Assert-Equal ($publishFixtureState.Uploads -join ',') ($publishFixtureConfig.crates -join ',')
  Assert-Equal $publishFixtureState.Requests 7
  Assert-True ($result.Log -match 'Published and confirmed all 7 crates')
  Assert-True ($result.Log -notmatch 'fixture-token')

  # A Cargo error stops immediately, even when prior uploads are confirmed.
  foreach ($position in 0, 2) {
    Reset-PublishFixture
    $publishFixtureState.CargoFailure = $publishFixtureConfig.crates[$position]
    $result = Invoke-PublishFixture
    Assert-True ($result.Error -match 'Cargo publish failed')
    Assert-Equal $publishFixtureState.Uploads.Count ($position + 1)
    Assert-Equal $publishFixtureState.Requests $position
    Assert-True ($result.Log -match 'Do not blindly rerun')
    if ($position -eq 0) { Assert-True ($result.Log -match 'Confirmed publications: \(none\)') }
    else { Assert-True ($result.Log -match 'Confirmed publications: assuan-sexpr, assuan-protocol') }
  }
  foreach ($case in 'Yanked', 'BadChecksum', 'HttpFailure', 'NetworkFailure') {
    Reset-PublishFixture
    $publishFixtureState[$case] = if ($case -eq 'HttpFailure') { 503 } else { $true }
    $result = Invoke-PublishFixture
    Assert-True ($result.Error -match 'Cannot confirm|does not match')
    Assert-Equal $publishFixtureState.Uploads.Count 1
    Assert-Equal $publishFixtureState.Sleeps 0
  }
  Reset-PublishFixture
  $publishFixtureState.Delays = 1
  $env:CARGO_REGISTRY_TOKEN = $null
  $env:CARGO_REGISTRIES_CRATES_IO_TOKEN = 'fixture-named-token'
  $result = Invoke-PublishFixture
  Assert-Equal $result.Error ''
  Assert-Equal $publishFixtureState.Sleeps 1
  Assert-Equal $publishFixtureState.Requests 8
  Reset-PublishFixture
  $publishFixtureState.DryFailure = $true
  Assert-True ((Invoke-PublishFixture -DryRun).Error -match 'dry-run failed')
  Assert-Equal $publishFixtureState.Uploads.Count 0
}
finally {
  $env:CARGO_REGISTRY_TOKEN = $savedToken
  $env:CARGO_REGISTRIES_CRATES_IO_TOKEN = $savedNamedToken
}
Write-Host 'Direct publication ordering, dry-run, credentials, registry confirmation and partial-failure checks passed.'
