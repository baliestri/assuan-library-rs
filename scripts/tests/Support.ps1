Set-StrictMode -Version Latest

function Get-TestReleaseVersions([string] $Root) {
  $manifest = [IO.File]::ReadAllText((Join-Path $Root 'Cargo.toml'))
  $workspace = [regex]::Match($manifest, '(?ms)^\[workspace\.package\]\s*\n(?<body>.*?)(?=^\[|\z)')
  $version = [regex]::Match($workspace.Groups['body'].Value, '(?m)^version\s*=\s*"(?<major>0|[1-9][0-9]*)\.(?<minor>0|[1-9][0-9]*)\.(?<patch>0|[1-9][0-9]*)"\s*$')
  if (-not $version.Success) { throw 'Cannot read the workspace version for release tests.' }
  $major = [System.Numerics.BigInteger]::Parse($version.Groups['major'].Value)
  $minor = $version.Groups['minor'].Value
  $patch = $version.Groups['patch'].Value
  # Fixtures copy the checkout manifests, including an already prepared release.
  return [pscustomobject]@{
    current = "$major.$minor.$patch"
    release = "$($major + 1).0.0"
    next = "$($major + 2).0.0"
  }
}

function Assert-Equal($Actual, $Expected) {
  if ($Actual -cne $Expected) { throw "Assertion failed: expected '$Expected', received '$Actual'." }
}

function Assert-True($Condition) {
  if (-not $Condition) { throw 'Assertion failed: condition is false.' }
}

function Assert-Throws([scriptblock] $Action, [string] $Pattern) {
  try { & $Action | Out-Null }
  catch {
    if ($_.Exception.Message -notmatch $Pattern) { throw }
    return
  }
  throw "Assertion failed: expected an error matching '$Pattern'."
}
