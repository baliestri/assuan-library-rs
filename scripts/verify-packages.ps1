# Verify committed local packages without publishing or using registry credentials.
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$names = @(
  'assuan-sexpr', 'assuan-protocol', 'assuan-transport',
  'assuan-client', 'assuan-server', 'assuan-macros', 'assuan-library'
)

Push-Location (Split-Path -Parent $PSScriptRoot)
try {
  $status = @(& git status --porcelain --untracked-files=all)
  if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect Git status.' }
  if ($status.Count -ne 0) { throw 'Package verification requires a clean Git tree, including untracked files.' }
  foreach ($tool in 'cargo', 'tar') {
    $null = Get-Command $tool -ErrorAction Stop
  }
  $metadataText = & cargo metadata --no-deps --format-version 1
  if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed.' }
  $metadata = $metadataText | ConvertFrom-Json
  $packages = @($metadata.packages | Where-Object { $_.id -in $metadata.workspace_members })
  if ($packages.Count -ne $names.Count -or
      (Compare-Object ($packages.name | Sort-Object) ($names | Sort-Object))) {
    throw 'The workspace must contain exactly the seven expected public crates.'
  }

  & cargo package --workspace --registry crates-io
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
    foreach ($required in 'Cargo.toml', 'Cargo.toml.orig', 'README.md', 'LICENSE.md', 'src/lib.rs') {
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
    [PSCustomObject]@{
      Package = $prefix
      Files = $entries.Count
      SHA256 = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash
    }
  }
  $report | Format-Table -AutoSize
  Write-Host 'All seven local package archives passed verification. Nothing was published.'
}
finally {
  Pop-Location
}
