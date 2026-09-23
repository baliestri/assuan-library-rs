Import-Module "$PSScriptRoot/../release/Archive.psm1" -Force
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$fixture = Join-Path $repo "target/release-tests/docs-deploy-$([Guid]::NewGuid().ToString('N'))"
$null = [IO.Directory]::CreateDirectory($fixture)
function Write-DocsDeployFixture([string] $Path, [string] $Text) {
  $null = [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Path))
  [IO.File]::WriteAllText($Path, $Text, [Text.UTF8Encoding]::new($false))
}
function New-DocsDeployRelease([string] $Version, [int] $Id) {
  return [pscustomobject]@{id=$Id;tag_name="v$Version";draft=$false;prerelease=$false;published_at='2026-09-23T00:00:00Z'}
}
function New-DocsDeployAssets([string] $Version) {
  foreach ($name in "docs-v$Version.zip", "docs-v$Version.sha256") {
    [pscustomobject]@{name=$name;state='uploaded';browser_download_url="https://github.com/example/assuan/releases/download/v$Version/$name"}
  }
}
$global:DocsDeployMock = @{
  Releases=@((New-DocsDeployRelease '0.9.0' 1),(New-DocsDeployRelease '0.10.0' 2));
  Assets=@{'1'=@(New-DocsDeployAssets '0.9.0');'2'=@(New-DocsDeployAssets '0.10.0')};
  Sources=@{};Calls=[Collections.Generic.List[string]]::new();Failure=''
}
foreach ($version in '0.9.0','0.10.0') {
  $source = "$fixture/source/$version"
  Write-DocsDeployFixture "$source/index.html" "<h1>$version</h1>"
  Write-DocsDeployFixture "$source/llms.txt" "# Index $version`n"
  Write-DocsDeployFixture "$source/llms-full.txt" "# Full $version`n"
  Write-DocsDeployFixture "$source/guide.md" "Guide $version`n"
  $assetDirectory = "$fixture/assets/$version"
  $archive = "$assetDirectory/docs-v$version.zip"
  $bundle = New-DocsBundle $source $archive
  Write-DocsDeployFixture "$assetDirectory/docs-v$version.sha256" "$($bundle.sha256)`n"
  $global:DocsDeployMock.Sources["v$version"] = $assetDirectory
}
# Mock only the external CLI. Collection, JSON decoding, checksums, ZIP validation
# and site assembly below all run the production implementations.
function gh {
  $arguments = @($args)
  $global:LASTEXITCODE = 0
  $global:DocsDeployMock.Calls.Add(($arguments -join ' '))
  if ($global:DocsDeployMock.Failure -eq $arguments[0]) { $global:LASTEXITCODE = 1; return }
  if ($arguments[0] -eq 'api') {
    Assert-True ($arguments -contains '--paginate')
    Assert-True ($arguments -contains '--slurp')
    Assert-Equal $arguments[2] 'github.com'
    $endpoint = $arguments[-1]
    if ($endpoint -ceq 'repos/example/assuan/releases?per_page=100') { $items = @($global:DocsDeployMock.Releases) }
    elseif ($endpoint -cmatch '\Arepos/example/assuan/releases/([1-9][0-9]*)/assets\?per_page=100\z') {
      $items = @($global:DocsDeployMock.Assets[$Matches[1]])
    } else { throw 'Unexpected mocked API endpoint.' }
    # Put each result on its own page to detect implementations that take only
    # the first page (including asset pagination).
    $pages = [Collections.Generic.List[object]]::new()
    foreach ($item in $items) { $pages.Add(@($item)) }
    ConvertTo-Json -InputObject $pages.ToArray() -Depth 8 -Compress
    return
  }
  Assert-Equal $arguments[0] 'release'
  Assert-Equal $arguments[1] 'download'
  Assert-Equal $arguments[3] '--repo'
  Assert-Equal $arguments[4] 'github.com/example/assuan'
  Assert-Equal $arguments[5] '--pattern'
  Assert-Equal $arguments[7] '--pattern'
  Assert-Equal $arguments[9] '--dir'
  foreach ($name in $arguments[6],$arguments[8]) {
    [IO.File]::Copy((Join-Path $global:DocsDeployMock.Sources[$arguments[2]] $name),(Join-Path $arguments[10] $name),$false)
  }
}
$savedHost = $env:GH_HOST
try {
  $env:GH_HOST = 'github.com'
  $draft = New-DocsDeployRelease '99.0.0' 3
  $draft.draft = $true
  $pre = New-DocsDeployRelease '98.0.0' 4
  $pre.prerelease = $true
  $global:DocsDeployMock.Releases += @($draft,$pre,(New-DocsDeployRelease '0.11.0-rc.1' 5))
  $manifest = & "$repo/scripts/collect-docs.ps1" -Repository example/assuan -Output "$fixture/collected"
  $bundles = @(Get-Content -LiteralPath $manifest -Raw | ConvertFrom-Json)
  Assert-Equal $bundles.Count 2
  Assert-Equal @($global:DocsDeployMock.Calls | Where-Object { $_.StartsWith('release download') }).Count 2
  $siteJson = & "$repo/scripts/assemble-docs.ps1" -Manifest $manifest -Output "$fixture/site" -BaseUrl https://example.test/assuan/
  Assert-Equal (($siteJson | ConvertFrom-Json).latest) '0.10.0'
  foreach ($version in '0.9.0','0.10.0') {
    foreach ($name in 'index.html','llms.txt','llms-full.txt','guide.md') {
      Assert-Equal (Get-FileHash "$fixture/site/$version/$name").Hash (Get-FileHash "$fixture/source/$version/$name").Hash
    }
  }
  Assert-Equal (Get-FileHash "$fixture/site/llms-full.txt").Hash (Get-FileHash "$fixture/source/0.10.0/llms-full.txt").Hash
  $global:DocsDeployMock.Releases = @($global:DocsDeployMock.Releases[1],$global:DocsDeployMock.Releases[0])
  $reverseManifest = & "$repo/scripts/collect-docs.ps1" -Repository example/assuan -Output "$fixture/reverse"
  $reverseSite = & "$repo/scripts/assemble-docs.ps1" -Manifest $reverseManifest -Output "$fixture/reverse-site" -BaseUrl https://example.test/assuan/
  Assert-Equal (($reverseSite | ConvertFrom-Json).latest) '0.10.0'
  Assert-Equal (Get-FileHash "$fixture/site/0.9.0/guide.md").Hash (Get-FileHash "$fixture/reverse-site/0.9.0/guide.md").Hash
  Assert-Throws { & "$repo/scripts/collect-docs.ps1" example/assuan "$fixture/collected" } 'already exists'
  Assert-Throws { & "$repo/scripts/collect-docs.ps1" example/assuan '../outside' } 'target'
  foreach ($repository in 'https://github.com/example/assuan','../assuan','example/../assuan','--repo','example/assuan.git') {
    Assert-Throws { & "$repo/scripts/collect-docs.ps1" $repository "$fixture/bad-repository" } 'owner/repository'
  }
  $env:GH_HOST = 'attacker.test'
  Assert-Throws { & "$repo/scripts/collect-docs.ps1" example/assuan "$fixture/bad-host" } 'host'
  $env:GH_HOST = 'github.com'
  $global:DocsDeployMock.Releases = @((New-DocsDeployRelease '0.9.0' 1))
  $validAssets = @($global:DocsDeployMock.Assets['1'])
  $global:DocsDeployMock.Assets['1'] = @($validAssets[0])
  Assert-Throws { & "$repo/scripts/collect-docs.ps1" example/assuan "$fixture/missing" } 'Missing or duplicate'
  $global:DocsDeployMock.Assets['1'] = @($validAssets[0],$validAssets[0],$validAssets[1])
  Assert-Throws { & "$repo/scripts/collect-docs.ps1" example/assuan "$fixture/duplicate-asset" } 'Missing or duplicate'
  $global:DocsDeployMock.Assets['1'] = $validAssets
  $validUrl = $validAssets[0].browser_download_url
  foreach ($url in 'https://attacker.test/asset.zip','https://github.com/other/assuan/releases/download/v0.9.0/docs-v0.9.0.zip') {
    $validAssets[0].browser_download_url = $url
    Assert-Throws { & "$repo/scripts/collect-docs.ps1" example/assuan "$fixture/url-$([Guid]::NewGuid().ToString('N'))" } 'asset host'
  }
  $validAssets[0].browser_download_url = $validUrl
  $global:DocsDeployMock.Releases += $global:DocsDeployMock.Releases[0]
  Assert-Throws { & "$repo/scripts/collect-docs.ps1" example/assuan "$fixture/duplicate-version" } 'Duplicate documentation version'
  $global:DocsDeployMock.Releases = @((New-DocsDeployRelease '0.9.0' 1))
  foreach ($command in 'api','release') {
    $global:DocsDeployMock.Failure = $command
    Assert-Throws { & "$repo/scripts/collect-docs.ps1" example/assuan "$fixture/failed-$command" } '(query|download) failed'
  }
  $global:DocsDeployMock.Failure = ''
  $sidecar = "$fixture/assets/0.9.0/docs-v0.9.0.sha256"
  $validChecksum = [IO.File]::ReadAllText($sidecar)
  Write-DocsDeployFixture $sidecar ('0' * 64 + "`n")
  Assert-Throws { & "$repo/scripts/collect-docs.ps1" example/assuan "$fixture/hash-mismatch" } 'checksum mismatch'
  foreach ($text in $validChecksum.TrimEnd(), $validChecksum.ToUpperInvariant(), ($validChecksum.TrimEnd() + "`r`n")) {
    Write-DocsDeployFixture $sidecar $text
    Assert-Throws { & "$repo/scripts/collect-docs.ps1" example/assuan "$fixture/sidecar-$([Guid]::NewGuid().ToString('N'))" } 'checksum file'
  }
  $badArchive = "$fixture/assets/0.9.0/docs-v0.9.0.zip"
  $zip = [IO.Compression.ZipFile]::Open($badArchive,[IO.Compression.ZipArchiveMode]::Update)
  try { $null = $zip.CreateEntry('0.9.0/../escape') } finally { $zip.Dispose() }
  Write-DocsDeployFixture $sidecar ((Get-FileHash $badArchive).Hash.ToLowerInvariant() + "`n")
  Assert-Throws { & "$repo/scripts/collect-docs.ps1" example/assuan "$fixture/unsafe-archive" } 'bundle entry path'
  Assert-True (-not (Test-Path "$fixture/unsafe-archive/bundles.json"))
  $global:DocsDeployMock.Releases = @($draft,$pre)
  Assert-Throws { & "$repo/scripts/collect-docs.ps1" example/assuan "$fixture/no-stable" } 'No completed stable'
  $workflow = [IO.File]::ReadAllText("$repo/.github/workflows/docs.yml")
  Assert-True ($workflow -match 'workflow_call:' -and $workflow -match 'workflow_dispatch:')
  Assert-True ($workflow -notmatch 'cargo\s+publish')
  Assert-True ($workflow -match 'group: assuan-pages' -and $workflow -match 'cancel-in-progress: false')
} finally {
  $env:GH_HOST = $savedHost
  Remove-Variable -Name DocsDeployMock -Scope Global
}
Write-Host 'Paginated releases, verified bundles, immutable historical docs and redeploy failure handling passed.'
