Import-Module "$PSScriptRoot/../release/Archive.psm1" -Force
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$fixture = Join-Path $repo "target/release-tests/archive-$([Guid]::NewGuid().ToString('N'))"
$null = [IO.Directory]::CreateDirectory($fixture)
function Write-ArchiveFixture([string] $Path, [string] $Text) {
  $null = [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Path))
  [IO.File]::WriteAllText($Path, $Text, [Text.UTF8Encoding]::new($false))
}
function New-InvalidZip([string] $Name, [string[]] $Entries, [int] $Attributes = 0) {
  $path = Join-Path $fixture "$Name.zip"
  $stream = [IO.File]::Open($path, [IO.FileMode]::CreateNew)
  $zip = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create, $true)
  try {
    foreach ($name in $Entries) {
      $entry = $zip.CreateEntry($name)
      $entry.ExternalAttributes = $Attributes
      $writer = [IO.StreamWriter]::new($entry.Open())
      try { $writer.Write('fixture') } finally { $writer.Dispose() }
    }
  } finally { $zip.Dispose(); $stream.Dispose() }
  return $path
}
$bundles = @()
foreach ($version in '0.9.0','0.10.0') {
  $root = "$fixture/source/$version"
  Write-ArchiveFixture "$root/index.html" "<h1>Release $version</h1>"
  Write-ArchiveFixture "$root/llms.txt" "# $version`nhttps://example.test/project/$version/index.html`n"
  Write-ArchiveFixture "$root/llms-full.txt" "# Full $version`n"
  Write-ArchiveFixture "$root/assets/data.bin" "`0binary`r`n"
  $path = "$fixture/$version.zip"
  $bundle = New-DocsBundle $root $path
  $again = New-DocsBundle $root "$fixture/$version-again.zip"
  Assert-Equal $bundle.sha256 $again.sha256
  Assert-Equal ($bundle.files | ConvertTo-Json -Compress) ($again.files | ConvertTo-Json -Compress)
  Assert-Throws { New-DocsBundle $root $path } 'exists'
  $bundles += [pscustomobject]@{version=$version;archive=$path;sha256=$bundle.sha256}
}
$oldHash = (Get-FileHash "$fixture/source/0.9.0/llms.txt").Hash
$site = Merge-DocsVersions $bundles "$fixture/site" ([Uri]'https://example.test/project/')
Assert-Equal $site.latest '0.10.0'
Assert-Equal (Get-FileHash "$fixture/site/0.9.0/llms.txt").Hash $oldHash
Assert-Equal (Get-FileHash "$fixture/site/llms.txt").Hash (Get-FileHash "$fixture/source/0.10.0/llms.txt").Hash
$reverse = Merge-DocsVersions @($bundles[1],$bundles[0]) "$fixture/reverse" ([Uri]'https://example.test/project/')
Assert-Equal $reverse.latest '0.10.0'
Assert-Equal (Get-FileHash "$fixture/site/versions.json").Hash (Get-FileHash "$fixture/reverse/versions.json").Hash
foreach ($bundle in $bundles) {
  foreach ($file in Get-ChildItem "$fixture/source/$($bundle.version)" -Recurse -File) {
    $relative = [IO.Path]::GetRelativePath("$fixture/source", $file.FullName)
    Assert-Equal (Get-FileHash $file.FullName).Hash (Get-FileHash (Join-Path "$fixture/site" $relative)).Hash
  }
}
Assert-Throws { Merge-DocsVersions $bundles "$fixture/site" ([Uri]'https://example.test/project/') } 'exists'
Assert-Throws { Merge-DocsVersions @($bundles[0],$bundles[0]) "$fixture/duplicate-version" ([Uri]'https://example.test/project/') } 'Duplicate'
Assert-Throws { Expand-VerifiedBundle $bundles[0].archive ('0' * 64) "$fixture/hash-failure" '0.9.0' } 'checksum'
Assert-True (-not (Test-Path "$fixture/hash-failure"))
$missing = [pscustomobject]@{version='0.11.0';archive="$fixture/absent.zip";sha256=('0' * 64)}
Assert-Throws { Merge-DocsVersions @($bundles[0],$missing) "$fixture/missing" ([Uri]'https://example.test/project/') } 'Missing'
Assert-True (-not (Test-Path "$fixture/missing"))

$badPaths = @('../escape', '/absolute', 'C:/drive', '//host/share', '0.9.0/../escape',
  '0.9.0/a\b', '0.9.0/NUL', '0.9.0/trailing.', '0.9.0/a:b', "0.9.0/null`0byte", '0.10.0/wrong-version')
$index = 0
foreach ($badPath in $badPaths) {
  $path = New-InvalidZip "bad-$index" @('0.9.0/safe.txt',$badPath)
  $hash = (Get-FileHash $path).Hash
  Assert-Throws { Expand-VerifiedBundle $path $hash "$fixture/bad-$index" '0.9.0' } 'bundle'
  Assert-True (-not (Test-Path "$fixture/bad-$index"))
  $index++
}
foreach ($names in @(@('0.9.0/a','0.9.0/a'), @('0.9.0/A','0.9.0/a'),
    @('0.9.0/Dir/a','0.9.0/dir/b'), @('0.9.0/file','0.9.0/file/child'))) {
  $path = New-InvalidZip "collision-$index" $names
  Assert-Throws { Expand-VerifiedBundle $path (Get-FileHash $path).Hash "$fixture/collision-$index" '0.9.0' } 'bundle'
  $index++
}
$symlink = New-InvalidZip 'symlink' @('0.9.0/link') -1577123840
Assert-Throws { Expand-VerifiedBundle $symlink (Get-FileHash $symlink).Hash "$fixture/symlink" '0.9.0' } 'bundle'
$bad = [pscustomobject]@{version='0.8.0';archive=$symlink;sha256=(Get-FileHash $symlink).Hash}
Assert-Throws { Merge-DocsVersions @($bundles[0],$bad) "$fixture/interrupted" ([Uri]'https://example.test/project/') } 'bundle'
Assert-True (-not (Test-Path "$fixture/interrupted"))
Assert-Equal (Get-FileHash "$fixture/site/0.9.0/llms.txt").Hash $oldHash

# A forged central-directory size must be rejected before extraction, without
# allocating or expanding a gigabyte fixture.
$oversized = New-InvalidZip 'oversized' @('0.9.0/large.bin')
$bytes = [IO.File]::ReadAllBytes($oversized)
$central = -1
for ($offset = 0; $offset -le $bytes.Length - 28; $offset++) {
  if ([BitConverter]::ToUInt32($bytes,$offset) -eq 0x02014b50) { $central = $offset; break }
}
Assert-True ($central -ge 0)
[BitConverter]::GetBytes([uint32]1073741825).CopyTo($bytes,$central + 24)
[IO.File]::WriteAllBytes($oversized,$bytes)
Assert-Throws { Expand-VerifiedBundle $oversized (Get-FileHash $oversized).Hash "$fixture/oversized" '0.9.0' } 'budget'
Assert-True (-not (Test-Path "$fixture/oversized"))

# Exercise the CLI's relative-manifest archive resolution from another cwd.
$manifest = "$fixture/bundles.json"
$cliBundles = @($bundles | ForEach-Object { [pscustomobject]@{version=$_.version;archive=[IO.Path]::GetFileName($_.archive);sha256=$_.sha256} })
Write-ArchiveFixture $manifest (ConvertTo-Json -InputObject $cliBundles)
Push-Location $fixture
try {
  $json = & pwsh -NoProfile -File "$repo/scripts/assemble-docs.ps1" -Manifest $manifest -Output "$fixture/cli" -BaseUrl https://example.test/project/
  Assert-Equal $LASTEXITCODE 0
  Assert-Equal (($json | ConvertFrom-Json).latest) '0.10.0'
} finally { Pop-Location }
Write-Host 'Deterministic bundles, historical hashes, numeric latest and invalid-archive rejection passed.'
