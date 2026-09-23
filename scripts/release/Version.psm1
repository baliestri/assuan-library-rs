# Stable release versions and narrowly scoped edits to this workspace.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:ReleaseConfig = Get-Content "$PSScriptRoot/config.json" -Raw | ConvertFrom-Json

function ConvertTo-ReleaseVersion {
  <# .SYNOPSIS
  Validates stable SemVer without trimming or accepting shell syntax.
  #>
  param([AllowEmptyString()][string] $Text)
  if ($Text -cnotmatch '\A(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\z') {
    throw 'Invalid release version.'
  }
  return $Text
}

function Compare-ReleaseVersion {
  <# .SYNOPSIS
  Compares validated version components without fixed-width integer overflow.
  #>
  param([string] $Left, [string] $Right)
  $leftParts = (ConvertTo-ReleaseVersion $Left).Split('.')
  $rightParts = (ConvertTo-ReleaseVersion $Right).Split('.')
  foreach ($index in 0..2) {
    $comparison = ([Numerics.BigInteger]::Parse($leftParts[$index])).CompareTo(
      [Numerics.BigInteger]::Parse($rightParts[$index]))
    if ($comparison -ne 0) { return [Math]::Sign($comparison) }
  }
  return 0
}

function Get-ReleasePaths {
  return @('Cargo.toml') + @($script:ReleaseConfig.crates | ForEach-Object { "crates/$_/Cargo.toml" }) +
    @('README.md', 'crates/assuan-library/README.md', 'docs/publishing.md')
}

function Resolve-ReleasePath([string] $Root, [string] $Path) {
  if ($Path -cnotin (Get-ReleasePaths)) { throw 'Unsupported release edit path.' }
  $base = [IO.Path]::GetFullPath($Root)
  if (-not (Test-Path -LiteralPath $base -PathType Container)) { throw 'Missing release root path.' }
  $current = $base
  # Reject reparse points along the selected path, including the supplied root.
  if ((Get-Item -LiteralPath $current).Attributes -band [IO.FileAttributes]::ReparsePoint) {
    throw 'Linked release root path is not supported.'
  }
  foreach ($component in $Path.Split('/')) {
    $current = Join-Path $current $component
    $item = Get-Item -LiteralPath $current -ErrorAction Stop
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Linked release edit path is not supported.' }
  }
  return $current
}

function Update-ManifestVersion([string] $Content, [string] $Version, [bool] $Workspace) {
  $section = ''
  $workspaceVersions = 0
  $lines = [regex]::Split($Content, '(?<=\n)')
  $result = foreach ($line in $lines) {
    if ($line -match '^\s*\[([^\]\r\n]+)\]\s*(?:#.*)?\r?\n?\z') { $section = $Matches[1] }
    if ($Workspace -and $section -ceq 'workspace.package' -and $line -match '^\s*version\s*=') {
      $pattern = '^(\s*version\s*=\s*")[^"]+("[^\r\n]*)(\r?\n)?\z'
      if ($line -notmatch $pattern) { throw 'Unsupported workspace version declaration.' }
      $workspaceVersions++
      [regex]::Replace($line, $pattern, { param($m) $m.Groups[1].Value + $Version + $m.Groups[2].Value + $m.Groups[3].Value })
      continue
    }
    if ($section -match '(^|\.)(dependencies|dev-dependencies|build-dependencies)$' -and
        $line -match '^\s*(assuan-[a-z]+)\s*=') {
      $name = $Matches[1]
      if ($name -in $script:ReleaseConfig.crates) {
        # The workspace currently uses one-line inline dependency tables.
        # Reject unknown layouts rather than silently missing an internal version.
        if ($line -notmatch '^\s*assuan-[a-z]+\s*=\s*\{[^\r\n]*\}\s*(?:#.*)?\r?\n?\z' -or
            ([regex]::Matches($line, '\bversion\s*=\s*"[^"]+"').Count -ne 1) -or
            $line -notmatch '\bpath\s*=\s*"[^"]+"') {
          throw "Unsupported internal dependency declaration: $name"
        }
        [regex]::Replace($line, '(\bversion\s*=\s*")[^"]+(")', { param($m) $m.Groups[1].Value + $Version + $m.Groups[2].Value })
        continue
      }
    }
    if ($section -match '(^|\.)(dependencies|dev-dependencies|build-dependencies)\.(assuan-[a-z]+)$' -and
        $Matches[3] -in $script:ReleaseConfig.crates) {
      throw 'Unsupported internal dependency table; update the release parser before releasing.'
    }
    $line
  }
  if ($Workspace -and $workspaceVersions -ne 1) { throw 'Expected one workspace version.' }
  return $result -join ''
}

function Get-VersionEdits {
  <# .SYNOPSIS
  Returns path/before/after edits without writing files or invoking Git.
  .DESCRIPTION
  Supports the workspace's current inline dependency tables. Unknown internal
  layouts fail closed. Equal versions are allowed for initial release preparation.
  #>
  param([string] $Root, [string] $Version)
  $Version = ConvertTo-ReleaseVersion $Version
  $manifest = [IO.File]::ReadAllText((Resolve-ReleasePath $Root 'Cargo.toml'))
  $section = [regex]::Match($manifest, '(?ms)^\[workspace\.package\]\r?\n(.*?)(?=^\[|\z)')
  $match = [regex]::Match($section.Groups[1].Value, '(?m)^version\s*=\s*"([^"]+)"')
  if (-not $match.Success) { throw 'Missing workspace version.' }
  $current = ConvertTo-ReleaseVersion $match.Groups[1].Value
  if ((Compare-ReleaseVersion $Version $current) -lt 0) { throw 'Release version downgrade is not allowed.' }
  foreach ($path in Get-ReleasePaths) {
    $before = [IO.File]::ReadAllText((Resolve-ReleasePath $Root $path))
    if ($path.EndsWith('Cargo.toml', [StringComparison]::Ordinal)) {
      $after = Update-ManifestVersion $before $Version ($path -ceq 'Cargo.toml')
    } else {
      $after = $before
      if ($path -ceq 'crates/assuan-library/README.md') {
        $pattern = '(?m)^(assuan-library = \{ version = ")[^"]+("[^\r\n]*)'
        if ([regex]::Matches($after, $pattern).Count -ne 1) { throw 'Expected one facade installation version.' }
        $after = [regex]::Replace($after, $pattern, { param($m) $m.Groups[1].Value + $Version + $m.Groups[2].Value })
        $after = $after.Replace('No package has been published yet. Inside a checkout, use a Cargo path',
          'For released versions, use crates.io. Inside a checkout, use a Cargo path')
      }
      if ($path -ceq 'README.md') {
        $after = $after.Replace('No package has been published.', 'Published versions are listed in GitHub Releases.')
        $after = $after.Replace('Use a path dependency on `crates/assuan-library` until publication.',
          'Use crates.io for released versions, or a path dependency on `crates/assuan-library` in a checkout.')
      }
      if ($path -ceq 'docs/publishing.md') {
        $after = $after.Replace("at version $current.", "at version $Version.")
        $after = $after.Replace("assuan-library-$current.crate", "assuan-library-$Version.crate")
        $after = $after.Replace("assuan-library-$current/Cargo.toml", "assuan-library-$Version/Cargo.toml")
        $after = [regex]::Replace($after, 'No release\r?\nhas been published\.', 'Published versions are listed in GitHub Releases.')
      }
    }
    if ($before -cne $after) { [pscustomobject]@{ path = $path; before = $before; after = $after } }
  }
}

function Set-ReleaseVersion {
  <# .SYNOPSIS
  Applies an allowlisted edit set after validating every original file.
  .DESCRIPTION
  Rejects duplicate paths, linked files and stale previews before any write.
  Filesystem write failures are reported; writes are not a multi-file transaction.
  #>
  param([string] $Root, [AllowEmptyCollection()][object[]] $Edits)
  $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
  $pending = foreach ($edit in $Edits) {
    $path = Resolve-ReleasePath $Root $edit.path
    if (-not $seen.Add($edit.path)) { throw 'Duplicate release edit path.' }
    if ([IO.File]::ReadAllText($path) -cne $edit.before) { throw 'Release input changed since preview.' }
    [pscustomobject]@{ path = $path; content = [string]$edit.after }
  }
  foreach ($file in $pending) {
    [IO.File]::WriteAllText($file.path, $file.content, [Text.UTF8Encoding]::new($false))
  }
}

Export-ModuleMember -Function ConvertTo-ReleaseVersion, Compare-ReleaseVersion, Get-VersionEdits, Set-ReleaseVersion
