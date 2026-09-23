<#
.SYNOPSIS
Downloads and verifies documentation bundles from completed stable GitHub Releases.
.DESCRIPTION
Repository is an owner/name on github.com. Output must be a new directory under
workspace target/. Uses paginated GitHub CLI queries and writes bundles.json for
assemble-docs.ps1 only after every eligible release has valid assets. Existing
output is never deleted or overwritten. Failed downloads remain for inspection.
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory)][string] $Repository,
  [Parameter(Mandatory)][string] $Output
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Import-Module "$PSScriptRoot/release/Archive.psm1" -Force
if ($Repository -cnotmatch '\A[A-Za-z0-9][A-Za-z0-9-]{0,38}/[A-Za-z0-9][A-Za-z0-9._-]{0,99}\z' -or
    $Repository.EndsWith('.git', [StringComparison]::OrdinalIgnoreCase)) {
  throw 'Expected a GitHub owner/repository name.'
}
if ($env:GH_HOST -and $env:GH_HOST -cne 'github.com') { throw 'Only the github.com host is supported.' }
$root = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
if (-not [IO.Path]::IsPathRooted($Output)) { $Output = Join-Path $root $Output }
$Output = [IO.Path]::GetFullPath($Output)
$relative = [IO.Path]::GetRelativePath($root, $Output).Replace('\','/')
if (-not $relative.StartsWith('target/', [StringComparison]::Ordinal)) { throw 'Output must be inside workspace target/.' }
$current = $root
foreach ($part in @('') + $relative.Split('/')) {
  if ($part -ne '') { $current = Join-Path $current $part }
  if ((Test-Path -LiteralPath $current) -and
      ((Get-Item -LiteralPath $current).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw 'Linked output paths are not supported.'
  }
}
if (Test-Path -LiteralPath $Output) { throw 'Collection output already exists.' }

function Get-GitHubPages([string] $Endpoint) {
  $json = & gh api --hostname github.com --paginate --slurp $Endpoint
  if ($LASTEXITCODE -ne 0) { throw 'GitHub release inventory query failed.' }
  $pages = ($json -join "`n") | ConvertFrom-Json -NoEnumerate
  if ($pages -isnot [array]) { throw 'Invalid GitHub inventory response.' }
  foreach ($page in $pages) {
    if ($page -isnot [array]) { throw 'Invalid GitHub inventory page.' }
    foreach ($item in $page) { $item }
  }
}

$releases = @(Get-GitHubPages "repos/$Repository/releases?per_page=100")
$versions = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$bundles = [Collections.Generic.List[object]]::new()
$null = [IO.Directory]::CreateDirectory($Output)
foreach ($release in $releases) {
  if ($release.draft -isnot [bool] -or $release.prerelease -isnot [bool]) { throw 'Invalid GitHub release flags.' }
  if ($release.draft -or $release.prerelease) { continue }
  if ($release.tag_name -isnot [string]) { throw 'Invalid GitHub release tag.' }
  if ($release.tag_name -cnotmatch '\Av((0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*))\z') { continue }
  $version = $Matches[1]
  if (-not $release.published_at) { throw 'Stable release is not completed.' }
  if (-not $versions.Add($version)) { throw 'Duplicate documentation version.' }
  if ([string]$release.id -cnotmatch '\A[1-9][0-9]*\z') { throw 'Invalid GitHub release identifier.' }
  $assets = @(Get-GitHubPages "repos/$Repository/releases/$($release.id)/assets?per_page=100")
  $archiveName = "docs-v$version.zip"
  $checksumName = "docs-v$version.sha256"
  foreach ($name in $archiveName, $checksumName) {
    $matchingAssets = @($assets | Where-Object { $_.name -ceq $name })
    if ($matchingAssets.Count -ne 1) { throw "Missing or duplicate documentation asset for v${version}: $name" }
    $asset = $matchingAssets[0]
    if ($asset.state -cne 'uploaded' -or
        $asset.browser_download_url -cne "https://github.com/$Repository/releases/download/v$version/$name") {
      throw 'Invalid documentation asset host, repository, name or state.'
    }
  }
  $destination = Join-Path $Output $version
  $null = [IO.Directory]::CreateDirectory($destination)
  & gh release download "v$version" --repo "github.com/$Repository" --pattern $archiveName --pattern $checksumName --dir $destination
  if ($LASTEXITCODE -ne 0) { throw "Documentation download failed for v$version." }
  $archive = Join-Path $destination $archiveName
  $checksum = Join-Path $destination $checksumName
  foreach ($file in $archive, $checksum) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf) -or
        ((Get-Item -LiteralPath $file).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Missing or linked documentation asset.' }
  }
  if ((Get-Item -LiteralPath $checksum).Length -ne 65) { throw 'Invalid documentation checksum file.' }
  $hashText = [IO.File]::ReadAllText($checksum)
  if ($hashText -cnotmatch '\A[0-9a-f]{64}\n\z') { throw 'Invalid documentation checksum file.' }
  $hash = $hashText.Substring(0,64)
  $null = Expand-VerifiedBundle $archive $hash (Join-Path $Output "verified-$version") $version
  $bundles.Add([pscustomobject][ordered]@{version=$version;archive="$version/$archiveName";sha256=$hash})
}
if ($bundles.Count -eq 0) { throw 'No completed stable documentation releases exist.' }
$manifest = Join-Path $Output 'bundles.json'
$json = (ConvertTo-Json -InputObject $bundles.ToArray() -Depth 4).Replace("`r`n","`n") + "`n"
[IO.File]::WriteAllText($manifest, $json, [Text.UTF8Encoding]::new($false))
Write-Output $manifest
