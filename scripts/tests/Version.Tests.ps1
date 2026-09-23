Import-Module "$PSScriptRoot/../release/Version.psm1" -Force

Assert-Equal (ConvertTo-ReleaseVersion '0.1.0') '0.1.0'
foreach ($value in '', 'v1.0.0', '01.0.0', '1.0', '1.0.0-rc.1', '1.0.0+build',
    '1.0.0;echo x', "1.0.0`n", ' 1.0.0', '1.0.0 ') {
  Assert-Throws { ConvertTo-ReleaseVersion $value } 'version'
}
Assert-Equal (Compare-ReleaseVersion '0.10.0' '0.9.0') 1
Assert-Equal (Compare-ReleaseVersion '1.0.0' '1.0.0') 0
Assert-Equal (Compare-ReleaseVersion '1.9.99' '2.0.0') -1
Assert-Equal (Compare-ReleaseVersion '999999999999999999999.0.0' '2.0.0') 1

$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$fixture = Join-Path $repo "target/release-tests/version-$([Guid]::NewGuid().ToString('N'))"
$null = New-Item -ItemType Directory -Path $fixture -Force
$config = Get-Content "$PSScriptRoot/../release/config.json" -Raw | ConvertFrom-Json
# Copy only tracked source/configuration files. Never copy target, credentials or Git state.
$tracked = @(& git -C $repo ls-files)
if ($LASTEXITCODE -ne 0) { throw 'Cannot list fixture inputs.' }
foreach ($path in $tracked) {
  if ($path -notmatch '^(Cargo.toml|README.md|docs/publishing.md|crates/.*\.(toml|rs|md|stderr))$') { continue }
  $destination = Join-Path $fixture $path
  $null = New-Item -ItemType Directory -Path (Split-Path $destination) -Force
  Copy-Item -LiteralPath (Join-Path $repo $path) -Destination $destination
}
$original = [IO.File]::ReadAllText("$fixture/Cargo.toml")
$external = [IO.File]::ReadAllText("$fixture/crates/assuan-library/tests/fixtures/renamed/Cargo.toml")
$preview = @(Get-VersionEdits $fixture '0.2.0')
Assert-True ($preview.Count -gt 7)
Assert-Equal ([IO.File]::ReadAllText("$fixture/Cargo.toml")) $original
Set-ReleaseVersion $fixture $preview
Assert-Equal ([IO.File]::ReadAllText("$fixture/crates/assuan-library/tests/fixtures/renamed/Cargo.toml")) $external
Assert-True ([IO.File]::ReadAllText("$fixture/Cargo.toml").Contains('edition = "2024"'))
Assert-True ([IO.File]::ReadAllText("$fixture/Cargo.toml").Contains('zeroize = { version = "1"'))
Assert-Equal (@(Get-VersionEdits $fixture '0.2.0').Count) 0
Assert-Throws { Get-VersionEdits $fixture '0.1.0' } 'downgrade'
Assert-Throws { Set-ReleaseVersion $fixture $preview } 'changed'

$metadataText = & cargo metadata --manifest-path "$fixture/Cargo.toml" --no-deps --format-version 1
if ($LASTEXITCODE -ne 0) { throw 'Fixture cargo metadata failed.' }
$metadata = $metadataText | ConvertFrom-Json
Assert-Equal $metadata.workspace_members.Count 7
foreach ($package in $metadata.packages) {
  Assert-Equal $package.version '0.2.0'
  foreach ($dependency in $package.dependencies) {
    if ($dependency.name -in $config.crates) {
      Assert-Equal $dependency.req '^0.2.0'
      Assert-True ($null -ne $dependency.path)
    }
  }
}
# Validate all preconditions before writing the first file.
$next = @(Get-VersionEdits $fixture '0.3.0')
$last = Join-Path $fixture $next[-1].path
[IO.File]::AppendAllText($last, "`nChanged concurrently.`n")
Assert-Throws { Set-ReleaseVersion $fixture $next } 'changed'
Assert-True ([IO.File]::ReadAllText("$fixture/Cargo.toml").Contains('version = "0.2.0"'))
Assert-Throws { Set-ReleaseVersion $fixture @([pscustomobject]@{path='../outside';before='';after=''}) } 'path'
Assert-Throws { Set-ReleaseVersion $fixture @($next[0], $next[0]) } 'Duplicate'

# Preview works through the public command too, without changing the manifest.
$cli = Join-Path $PSScriptRoot '../set-release-version.ps1'
$beforeCli = [IO.File]::ReadAllText("$fixture/Cargo.toml")
$previewJson = & pwsh -NoProfile -File $cli -Root $fixture -Version 0.3.0 -Check
Assert-Equal $LASTEXITCODE 0
Assert-True (@($previewJson | ConvertFrom-Json).Count -gt 7)
Assert-Equal ([IO.File]::ReadAllText("$fixture/Cargo.toml")) $beforeCli
$invalidOutput = & pwsh -NoProfile -File $cli -Root $fixture -Version 'v0.3.0' -Check 2>&1
Assert-True ($LASTEXITCODE -ne 0)
Assert-True (($invalidOutput -join ' ').Contains('Invalid release version'))
Assert-Equal ([IO.File]::ReadAllText("$fixture/Cargo.toml")) $beforeCli

Assert-True (-not ([IO.File]::ReadAllText("$fixture/README.md").Contains('No package has been published.')))
Assert-True ([IO.File]::ReadAllText("$fixture/docs/publishing.md").Contains('assuan-library-0.2.0.crate'))
Assert-True ([IO.File]::ReadAllText("$fixture/crates/assuan-library/README.md").Contains('version = "0.2.0"'))

# A changed dependency layout must fail, rather than silently miss a version.
$clientPath = "$fixture/crates/assuan-client/Cargo.toml"
$clientManifest = [IO.File]::ReadAllText($clientPath)
[IO.File]::WriteAllText($clientPath, $clientManifest.Replace(
  'assuan-protocol = { version = "0.2.0", path = "../assuan-protocol" }',
  'assuan-protocol = { path = "../assuan-protocol" }'))
Assert-Throws { Get-VersionEdits $fixture '0.3.0' } 'Unsupported internal dependency'
[IO.File]::WriteAllText($clientPath, $clientManifest)
[IO.File]::AppendAllText($clientPath, "`n[dependencies.assuan-protocol]`nversion = `"0.2.0`"`n")
Assert-Throws { Get-VersionEdits $fixture '0.3.0' } 'Unsupported internal dependency table'
[IO.File]::WriteAllText($clientPath, $clientManifest)
Write-Host 'Version validation, preview, idempotence, dependency metadata and stale-edit checks passed.'
