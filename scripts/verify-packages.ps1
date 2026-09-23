<#
.SYNOPSIS
Verifies package contents and performs a joint publish dry-run without upload.
.DESCRIPTION
Requires a clean Git tree, the pinned Cargo version and an existing Cargo.lock.
Checks allowed paths, package metadata, internal dependency versions and clean
VCS identity in Cargo's archives. The dry-run supports unpublished workspace
dependencies. No packages are uploaded and no release-state files are created.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$config = Get-Content "$PSScriptRoot/release/config.json" -Raw | ConvertFrom-Json
$names = @($config.crates)
Push-Location (Split-Path -Parent $PSScriptRoot)
try {
  $status = @(& git status --porcelain --untracked-files=all)
  if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect Git status.' }
  if ($status.Count -ne 0) { throw 'Package verification requires a clean Git tree, including untracked files.' }
  $releaseSha = & git rev-parse HEAD
  if ($LASTEXITCODE -ne 0) { throw 'Cannot pin package commit.' }
  foreach ($tool in 'cargo', 'tar') {
    $null = Get-Command $tool -ErrorAction Stop
  }
  $cargoVersion = & cargo --version
  if ($LASTEXITCODE -ne 0 -or -not $cargoVersion.StartsWith("cargo $($config.rust) ")) { throw 'Unexpected Cargo toolchain.' }
  $lockPath = Join-Path $PWD 'Cargo.lock'
  if (-not (Test-Path -LiteralPath $lockPath -PathType Leaf)) { throw 'Missing Cargo.lock; resolve dependencies before verification.' }
  $lockHash = (Get-FileHash -LiteralPath $lockPath -Algorithm SHA256).Hash
  $metadataText = & cargo metadata --locked --no-deps --format-version 1
  if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed.' }
  $metadata = $metadataText | ConvertFrom-Json
  $packages = @($metadata.packages | Where-Object { $_.id -in $metadata.workspace_members })
  if ($packages.Count -ne $names.Count -or
      (Compare-Object ($packages.name | Sort-Object) ($names | Sort-Object))) {
    throw 'The workspace must contain exactly the seven expected public crates.'
  }

  if (@($packages.version | Sort-Object -Unique).Count -ne 1) { throw 'All public crates must share one release version.' }
  & cargo package --workspace --locked --registry crates-io
  if ($LASTEXITCODE -ne 0) { throw 'Cargo package verification failed; no packages were published.' }

  $report = foreach ($name in $names) {
    $package = $packages | Where-Object name -EQ $name
    $prefix = "$name-$($package.version)"
    $archive = Join-Path $metadata.target_directory "package/$prefix.crate"
    if (-not (Test-Path -LiteralPath $archive -PathType Leaf)) { throw "Missing archive: $name" }
    $entries = @(& tar -tf $archive)
    if ($LASTEXITCODE -ne 0) { throw "Cannot list archive: $name" }
    $relative = foreach ($entry in $entries) {
      if (-not $entry.StartsWith("$prefix/", [StringComparison]::Ordinal)) {
        throw "Unexpected archive root: $name"
      }
      $path = $entry.Substring($prefix.Length + 1)
      if ($path -match '(^|/)\.\.(/|$)|\\' -or $path -notmatch
          '^(Cargo\.toml(\.orig)?|Cargo\.lock|README\.md|LICENSE\.md|\.cargo_vcs_info\.json|(src|tests|examples)/[a-zA-Z0-9_./-]+\.(rs|stderr))$') {
        throw "Unexpected packaged path in ${name}: $path"
      }
      $path
    }
    foreach ($required in 'Cargo.toml', 'Cargo.toml.orig', 'README.md', 'LICENSE.md', 'src/lib.rs', 'Cargo.lock', '.cargo_vcs_info.json') {
      if ($required -notin $relative) { throw "Missing $required in $name" }
    }
    $manifestLines = @(& tar -xOf $archive "$prefix/Cargo.toml")
    if ($LASTEXITCODE -ne 0) { throw "Cannot read normalized manifest: $name" }
    $manifest = $manifestLines -join "`n"
    foreach ($field in 'description', 'repository', 'readme', 'license', 'rust-version') {
      if ($manifest -notmatch "(?m)^$field = `"[^`"]+`"$") { throw "Missing package $field in $name" }
    }
    if ($manifest -notmatch '(?m)^\[package\.metadata\.docs\.rs\]$') {
      throw "Missing docs.rs metadata: $name"
    }
    # Cargo emits dependency tables, not inline source declarations, here.
    $sections = [regex]::Matches($manifest, '(?ms)^\[([^\r\n]+)\]\r?\n(.*?)(?=^\[|\z)')
    foreach ($section in $sections) {
      $header = $section.Groups[1].Value
      $body = $section.Groups[2].Value
      if ($header -notmatch '(^|\.)(dependencies|dev-dependencies|build-dependencies)\.') { continue }
      if ($body -match '(?m)^(path|git)\s*=') { throw "Unresolved source in ${name}: $header" }
      foreach ($internal in $names) {
        if ($header.EndsWith(".$internal", [StringComparison]::Ordinal)) {
          $dependency = $packages | Where-Object name -EQ $internal
          $version = [regex]::Escape($dependency.version)
          if ($body -notmatch "(?m)^version = `"\^?$version`"$") {
            throw "Mismatched internal dependency in ${name}: $internal"
          }
        }
      }
    }
    $vcsText = @(& tar -xOf $archive "$prefix/.cargo_vcs_info.json")
    if ($LASTEXITCODE -ne 0) { throw "Cannot read package VCS identity: $name" }
    $vcs = ($vcsText -join "`n") | ConvertFrom-Json
    if ($vcs.git.sha1 -cne $releaseSha -or
        ($vcs.git.PSObject.Properties.Name -contains 'dirty' -and $vcs.git.dirty)) { throw 'Package VCS identity does not match the pinned clean commit.' }
    [PSCustomObject][ordered]@{
      name = $name
      version = $package.version
      sha256 = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
      files = $entries.Count
    }
  }
  & cargo publish --workspace --dry-run --locked --registry crates-io
  if ($LASTEXITCODE -ne 0) { throw 'Joint Cargo publish dry-run failed.' }
  if ((Get-FileHash -LiteralPath $lockPath -Algorithm SHA256).Hash -cne $lockHash) { throw 'Cargo.lock changed during verification.' }
  $sha = & git rev-parse HEAD
  if ($LASTEXITCODE -ne 0) { throw 'Cannot read package commit.' }
  $status = @(& git status --porcelain --untracked-files=all)
  if ($LASTEXITCODE -ne 0 -or $status.Count -ne 0 -or $sha -cne $releaseSha) { throw 'Package checkout changed during verification.' }
  $report | Select-Object @{Name='Package';Expression={"$($_.name)-$($_.version)"}}, Files, SHA256 | Format-Table -AutoSize
  Write-Host 'All seven package contents and the joint publish dry-run passed. Nothing was published.'
}
finally {
  Pop-Location
}
