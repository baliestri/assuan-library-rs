# Deterministic documentation bundles and immutable site assembly.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module "$PSScriptRoot/Version.psm1" -Force
$script:Workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$script:BundleLimit = 1GB

function Resolve-BundlePath([string] $Path, [switch] $Write) {
  $full = [IO.Path]::GetFullPath($Path)
  $relative = [IO.Path]::GetRelativePath($script:Workspace, $full).Replace('\','/')
  if ($relative -eq '.' -or $relative -eq '..' -or $relative.StartsWith('../') -or [IO.Path]::IsPathRooted($relative)) {
    throw 'Bundle path must stay inside the workspace.'
  }
  if ($Write -and -not $relative.StartsWith('target/', [StringComparison]::Ordinal)) {
    throw 'Generated bundle paths must be inside workspace target/.'
  }
  $current = $script:Workspace
  foreach ($part in @('') + $relative.Split('/')) {
    if ($part -ne '') { $current = Join-Path $current $part }
    if ((Test-Path -LiteralPath $current) -and
        ((Get-Item -LiteralPath $current).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
      throw 'Linked bundle paths are not supported.'
    }
  }
  return $full
}

function Get-BundleFiles([string] $Root) {
  foreach ($item in Get-ChildItem -LiteralPath $Root -Force) {
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Linked bundle paths are not supported.' }
    if ($item.PSIsContainer) { Get-BundleFiles $item.FullName }
    else { $item.FullName }
  }
}

function Assert-BundleEntries([object[]] $Entries, [string] $Version) {
  $paths = [Collections.Generic.Dictionary[string,string]]::new([StringComparer]::OrdinalIgnoreCase)
  $files = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
  [long] $total = 0
  foreach ($entry in $Entries) {
    $name = [string]$entry.FullName
    if (-not $name.StartsWith("$Version/", [StringComparison]::Ordinal) -or
        $name -match '[\x00-\x1f\\:*?"<>|]' -or $name.EndsWith('/')) { throw 'Invalid bundle entry path.' }
    $segments = $name.Split('/')
    for ($index = 0; $index -lt $segments.Count; $index++) {
      $segment = $segments[$index]
      if ($segment -eq '' -or $segment -in @('.','..') -or $segment -match '[ .]$' -or
          $segment -match '^(?i:CON|PRN|AUX|NUL|COM[0-9¹²³]|LPT[0-9¹²³])(?:\.|$)') {
        throw 'Invalid bundle entry path.'
      }
      $prefix = $segments[0..$index] -join '/'
      $leaf = $index -eq ($segments.Count - 1)
      if ($paths.ContainsKey($prefix)) {
        if ($paths[$prefix] -cne $prefix -or $leaf -or $files.Contains($prefix)) { throw 'Duplicate or conflicting bundle path.' }
      } else { $paths.Add($prefix, $prefix) }
      if ($leaf) { $null = $files.Add($prefix) }
    }
    $unixType = ($entry.ExternalAttributes -shr 16) -band 0xF000
    if ($unixType -notin @(0,0x8000) -or ($entry.ExternalAttributes -band 0x410) -ne 0) {
      throw 'Non-regular bundle entries are not supported.'
    }
    if ($entry.Length -lt 0 -or $entry.Length -gt ($script:BundleLimit - $total)) { throw 'Bundle exceeds the 1 GiB extraction budget.' }
    $total += $entry.Length
  }
  foreach ($required in 'index.html','llms.txt','llms-full.txt') {
    if (-not $files.Contains("$Version/$required")) { throw "Missing required bundle file: $required" }
    if ($paths["$Version/$required"] -cne "$Version/$required") { throw 'Invalid bundle file casing.' }
  }
}

function New-DocsBundle {
  <# .SYNOPSIS
  Creates a deterministic ZIP containing exactly one documentation version.
  .DESCRIPTION
  Derives the stable version from the input directory name. Output must be a
  new path under workspace target/. Returns archive and individual file hashes.
  Files are stored without compression to avoid runtime-dependent compressor
  output. Timestamps and attributes are fixed. Failed temporary output is kept
  for inspection; existing archives are never replaced.
  #>
  param([string] $VersionRoot, [string] $ArchivePath)
  $VersionRoot = Resolve-BundlePath $VersionRoot
  $ArchivePath = Resolve-BundlePath $ArchivePath -Write
  if (-not (Test-Path -LiteralPath $VersionRoot -PathType Container)) { throw 'Missing bundle source directory.' }
  if (Test-Path -LiteralPath $ArchivePath) { throw 'Bundle archive already exists.' }
  $version = ConvertTo-ReleaseVersion ([IO.Path]::GetFileName($VersionRoot.TrimEnd([IO.Path]::DirectorySeparatorChar)))
  $relativeOutput = [IO.Path]::GetRelativePath($VersionRoot,$ArchivePath).Replace('\','/')
  if ($relativeOutput -ne '..' -and -not $relativeOutput.StartsWith('../')) { throw 'Bundle output overlaps its source.' }
  [string[]] $files = @(Get-BundleFiles $VersionRoot)
  [Array]::Sort($files, [StringComparer]::Ordinal)
  $entries = @(foreach ($file in $files) {
    [pscustomobject]@{FullName="$version/$([IO.Path]::GetRelativePath($VersionRoot,$file).Replace('\','/'))";Length=([IO.FileInfo]::new($file)).Length;ExternalAttributes=0}
  })
  Assert-BundleEntries $entries $version
  $null = [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($ArchivePath))
  $temporary = "$ArchivePath.partial-$([Guid]::NewGuid().ToString('N'))"
  $stream = [IO.File]::Open($temporary,[IO.FileMode]::CreateNew,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
  $zip = [IO.Compression.ZipArchive]::new($stream,[IO.Compression.ZipArchiveMode]::Create,$true)
  $inventory = [Collections.Generic.List[object]]::new()
  try {
    foreach ($index in 0..($entries.Count - 1)) {
      $entry = $zip.CreateEntry($entries[$index].FullName,[IO.Compression.CompressionLevel]::NoCompression)
      $entry.LastWriteTime = [DateTimeOffset]::new(1980,1,1,0,0,0,[TimeSpan]::Zero)
      $entry.ExternalAttributes = 0
      $inputStream = [IO.File]::Open($files[$index],[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
      $outputStream = $entry.Open()
      $digest = [Security.Cryptography.IncrementalHash]::CreateHash([Security.Cryptography.HashAlgorithmName]::SHA256)
      try {
        if ($inputStream.Length -ne $entries[$index].Length) { throw 'Bundle input changed during packaging.' }
        $buffer = [byte[]]::new(65536)
        [long] $copied = 0
        while (($count = $inputStream.Read($buffer,0,$buffer.Length)) -gt 0) {
          if ($count -gt ($entries[$index].Length - $copied)) { throw 'Bundle input changed during packaging.' }
          $digest.AppendData($buffer,0,$count)
          $outputStream.Write($buffer,0,$count)
          $copied += $count
        }
        if ($copied -ne $entries[$index].Length) { throw 'Bundle input changed during packaging.' }
        $hash = [Convert]::ToHexString($digest.GetHashAndReset()).ToLowerInvariant()
      } finally { $digest.Dispose(); $outputStream.Dispose(); $inputStream.Dispose() }
      $inventory.Add([pscustomobject]@{path=$entries[$index].FullName.Substring($version.Length+1);sha256=$hash})
    }
  } finally { $zip.Dispose(); $stream.Dispose() }
  $hash = (Get-FileHash -LiteralPath $temporary -Algorithm SHA256).Hash.ToLowerInvariant()
  [IO.File]::Move($temporary,$ArchivePath,$false)
  return [pscustomobject]@{sha256=$hash;files=$inventory.ToArray()}
}

function Expand-VerifiedBundle {
  <# .SYNOPSIS
  Validates an entire ZIP before extracting its regular files into a new path.
  .DESCRIPTION
  Checks archive hash, version prefix, duplicate/colliding paths, entry types
  and a 1 GiB uncompressed budget. Uses the same open archive for hashing and
  extraction. A failed extraction stays in sibling staging; Destination is
  promoted only after success. Output is restricted to workspace target/.
  #>
  param([string] $ArchivePath, [string] $ExpectedHash, [string] $Destination, [string] $Version)
  $Version = ConvertTo-ReleaseVersion $Version
  $ArchivePath = Resolve-BundlePath $ArchivePath
  $Destination = Resolve-BundlePath $Destination -Write
  if (-not (Test-Path -LiteralPath $ArchivePath -PathType Leaf)) { throw 'Missing bundle archive.' }
  if (Test-Path -LiteralPath $Destination) { throw 'Bundle extraction destination already exists.' }
  if ($ExpectedHash -notmatch '\A[0-9a-fA-F]{64}\z') { throw 'Invalid bundle checksum.' }
  $stream = [IO.File]::Open($ArchivePath,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
  try {
    $actual = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream))
    if ($actual -ine $ExpectedHash) { throw 'Bundle checksum mismatch.' }
    $stream.Position = 0
    $zip = [IO.Compression.ZipArchive]::new($stream,[IO.Compression.ZipArchiveMode]::Read,$true)
    try {
      Assert-BundleEntries @($zip.Entries) $Version
      $staging = "$Destination.partial-$([Guid]::NewGuid().ToString('N'))"
      $null = [IO.Directory]::CreateDirectory($staging)
      [long] $written = 0
      $buffer = [byte[]]::new(65536)
      foreach ($entry in $zip.Entries) {
        $target = Join-Path $staging $entry.FullName
        $null = [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target))
        $inputStream = $entry.Open()
        $outputStream = [IO.File]::Open($target,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
        try {
          [long] $entryBytes = 0
          while (($count = $inputStream.Read($buffer,0,$buffer.Length)) -gt 0) {
            if ($count -gt ($entry.Length - $entryBytes) -or $count -gt ($script:BundleLimit - $written)) {
              throw 'Bundle extraction exceeds declared length or budget.'
            }
            $outputStream.Write($buffer,0,$count)
            $entryBytes += $count
            $written += $count
          }
          if ($entryBytes -ne $entry.Length) { throw 'Truncated bundle entry.' }
        } finally { $outputStream.Dispose(); $inputStream.Dispose() }
      }
    } finally { $zip.Dispose() }
  } finally { $stream.Dispose() }
  [IO.Directory]::Move($staging,$Destination)
  return Join-Path $Destination $Version
}

function Merge-DocsVersions {
  <# .SYNOPSIS
  Assembles all supplied historical bundles and a latest-version landing page.
  .DESCRIPTION
  Requires the complete bundle inventory; never reads or modifies an existing
  site. Copies root LLM files from the numerically newest stable version.
  Returns latest and versions. Existing output is rejected; failed builds stay
  in staging without promoting an incomplete site. No network requests occur.
  #>
  param([object[]] $Bundles, [string] $Output, [Uri] $BaseUrl)
  $Output = Resolve-BundlePath $Output -Write
  if (Test-Path -LiteralPath $Output) { throw 'Assembled documentation output already exists.' }
  if ($Bundles.Count -eq 0) { throw 'At least one documentation bundle is required.' }
  if (-not $BaseUrl.IsAbsoluteUri -or $BaseUrl.Scheme -cnotin @('http','https') -or
      $BaseUrl.Query -ne '' -or $BaseUrl.Fragment -ne '' -or $BaseUrl.UserInfo -ne '' -or
      -not $BaseUrl.AbsolutePath.EndsWith('/')) { throw 'Invalid documentation base URL.' }
  $byVersion = @{}
  $versions = [Collections.Generic.List[string]]::new()
  foreach ($bundle in $Bundles) {
    $version = ConvertTo-ReleaseVersion $bundle.version
    if ($byVersion.ContainsKey($version)) { throw 'Duplicate documentation version.' }
    $archive = Resolve-BundlePath $bundle.archive
    if (-not (Test-Path -LiteralPath $archive -PathType Leaf)) { throw 'Missing historical bundle.' }
    $byVersion[$version] = $bundle
    $position = 0
    while ($position -lt $versions.Count -and (Compare-ReleaseVersion $versions[$position] $version) -gt 0) { $position++ }
    $versions.Insert($position,$version)
  }
  $staging = "$Output.partial-$([Guid]::NewGuid().ToString('N'))"
  $null = [IO.Directory]::CreateDirectory($staging)
  foreach ($version in $versions) {
    $bundle = $byVersion[$version]
    $unpack = "$Output.unpack-$([Guid]::NewGuid().ToString('N'))"
    $versionRoot = Expand-VerifiedBundle $bundle.archive $bundle.sha256 $unpack $version
    $destination = Join-Path $staging $version
    # Both paths are validated under target/ before moving a directory.
    $null = Resolve-BundlePath $versionRoot -Write
    $null = Resolve-BundlePath $destination -Write
    [IO.Directory]::Move($versionRoot,$destination)
  }
  $latest = $versions[0]
  foreach ($name in 'llms.txt','llms-full.txt') {
    [IO.File]::Copy((Join-Path "$staging/$latest" $name),(Join-Path $staging $name),$false)
  }
  $items = foreach ($version in $versions) {
    $href = [Net.WebUtility]::HtmlEncode([Uri]::new($BaseUrl,"$version/index.html").AbsoluteUri)
    "<li><a href=`"$href`">$version</a></li>"
  }
  $latestUrl = [Net.WebUtility]::HtmlEncode([Uri]::new($BaseUrl,"$latest/index.html").AbsoluteUri)
  $html = "<!doctype html><html lang=`"en`"><head><meta charset=`"utf-8`"><meta name=`"viewport`" content=`"width=device-width, initial-scale=1`"><title>Assuan documentation</title></head><body><main><h1>Assuan documentation</h1><p><a href=`"$latestUrl`">Latest stable: $latest</a></p><nav aria-label=`"Documentation versions`"><h2>Versions</h2><ul>$($items -join '')</ul></nav><p><a href=`"llms.txt`">LLM index</a> · <a href=`"llms-full.txt`">Full guides</a></p></main></body></html>`n"
  $result = [pscustomobject][ordered]@{latest=$latest;versions=$versions.ToArray()}
  [IO.File]::WriteAllText("$staging/index.html",$html,[Text.UTF8Encoding]::new($false))
  $json = ($result | ConvertTo-Json -Depth 4).Replace("`r`n","`n") + "`n"
  [IO.File]::WriteAllText("$staging/versions.json",$json,[Text.UTF8Encoding]::new($false))
  [IO.Directory]::Move($staging,$Output)
  return $result
}

Export-ModuleMember -Function New-DocsBundle, Expand-VerifiedBundle, Merge-DocsVersions
