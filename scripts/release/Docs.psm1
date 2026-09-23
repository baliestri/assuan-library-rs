# Public documentation generation. Source selection is explicit and reviewed.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module "$PSScriptRoot/Version.psm1" -Force
. "$PSScriptRoot/DocsLinks.ps1"

function Assert-DocRelativePath([string] $Path) {
  if ($Path -notmatch '\A[a-zA-Z0-9_./-]+\z' -or $Path.StartsWith('/') -or
      $Path -match '(^|/)(\.|\.\.|)(/|$)' -or [IO.Path]::IsPathRooted($Path)) {
    throw 'Invalid document path.'
  }
}

function Get-DocSourcePath([string] $Root, [string] $Relative, [switch] $Optional) {
  Assert-DocRelativePath $Relative
  $current = $Root
  foreach ($part in @('') + $Relative.Split('/')) {
    if ($part -ne '') { $current = Join-Path $current $part }
    if (-not (Test-Path -LiteralPath $current)) {
      if ($Optional) { return $null }
      throw "Missing document input: $Relative"
    }
    if ((Get-Item -LiteralPath $current).Attributes -band [IO.FileAttributes]::ReparsePoint) {
      throw 'Linked document path is not supported.'
    }
  }
  if (-not (Test-Path -LiteralPath $current -PathType Leaf)) { throw 'Expected a document file.' }
  return $current
}

function Get-DocFiles([string] $Root) {
  if ((Get-Item -LiteralPath $Root).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Linked document path is not supported.' }
  foreach ($item in Get-ChildItem -LiteralPath $Root -Force) {
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Linked document path is not supported.' }
    if ($item.PSIsContainer) { Get-DocFiles $item.FullName }
    else { $item.FullName }
  }
}

function Write-DocText([string] $Path, [string] $Text) {
  $null = [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Path))
  [IO.File]::WriteAllText($Path, $Text.Replace("`r`n", "`n").Replace("`r", "`n"), [Text.UTF8Encoding]::new($false))
}

function Test-DocContained([string] $Parent, [string] $Child) {
  $relative = [IO.Path]::GetRelativePath($Parent, $Child).Replace('\', '/')
  return ($relative -ne '..' -and -not $relative.StartsWith('../') -and -not [IO.Path]::IsPathRooted($relative))
}

function Build-VersionDocs {
  <# .SYNOPSIS
  Builds one immutable documentation version from selected public sources.
  .DESCRIPTION
  Requires the PowerShell version pinned in release/config.json and existing
  rustdoc output. Returns version, root and a sorted file/hash inventory.
  Refuses an existing version directory; failed builds remain available for
  inspection and must not be published. Never removes previous output.
  #>
  param([string] $Root, [string] $Version, [Uri] $BaseUrl, [string] $Output, [string] $ApiDirectory)
  $config = Get-Content "$PSScriptRoot/config.json" -Raw | ConvertFrom-Json
  if ($PSVersionTable.PSVersion.ToString() -cne $config.powershell) { throw "Documentation requires PowerShell $($config.powershell)." }
  $Version = ConvertTo-ReleaseVersion $Version
  if (-not $BaseUrl.IsAbsoluteUri -or $BaseUrl.Scheme -cnotin @('http','https') -or
      $BaseUrl.Query -ne '' -or $BaseUrl.Fragment -ne '' -or $BaseUrl.UserInfo -ne '' -or
      -not $BaseUrl.AbsolutePath.EndsWith('/')) { throw 'BaseUrl must be an absolute HTTP(S) directory URL.' }
  $Root = [IO.Path]::GetFullPath($Root)
  $Output = [IO.Path]::GetFullPath($Output)
  $ApiDirectory = [IO.Path]::GetFullPath($ApiDirectory)
  if ((Test-DocContained $Output $Root) -or
      ((Test-DocContained $Root $Output) -and -not (Test-DocContained (Join-Path $Root 'target') $Output)) -or
      (Test-DocContained $Output $ApiDirectory) -or (Test-DocContained $ApiDirectory $Output)) {
    throw 'Documentation output must not overlap sources or API input; use target or a separate directory.'
  }
  # Validate existing output parents up to the common ancestor with Root.
  $common = $Root
  while (-not (Test-DocContained $common $Output)) { $common = [IO.Path]::GetDirectoryName($common) }
  $cursor = $Output
  while ($true) {
    if ((Test-Path -LiteralPath $cursor) -and
        ((Get-Item -LiteralPath $cursor).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Linked document output path is not supported.' }
    if ($cursor -eq $common) { break }
    $cursor = [IO.Path]::GetDirectoryName($cursor)
  }
  $versionRoot = Join-Path $Output $Version
  if (Test-Path -LiteralPath $versionRoot) { throw 'Documentation version output already exists.' }
  if (-not (Test-Path -LiteralPath "$ApiDirectory/assuan_library/index.html" -PathType Leaf)) { throw 'Missing facade API documentation.' }
  $apiFiles = @(Get-DocFiles $ApiDirectory)
  $selection = Get-Content (Get-DocSourcePath $Root 'docs/site/sources.json') -Raw | ConvertFrom-Json
  if ($selection.repository -cne 'https://github.com/baliestri/assuan-library-rs') { throw 'Unexpected documentation repository.' }
  $context = @{
    root=$Root;version=$Version;base=$BaseUrl;repository=$selection.repository
    map=[Collections.Generic.Dictionary[string,object]]::new([StringComparer]::Ordinal)
    tagFiles=@($selection.tagFiles)
  }
  $destinations = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
  $entries = @($selection.sources | ForEach-Object {
    $entry = $_
    Assert-DocRelativePath $entry.destination
    if ($entry.destination -notmatch '\.md$' -or $entry.destination -match '^(api|assets)/' -or
        -not $destinations.Add($entry.destination)) { throw 'Duplicate or invalid document destination.' }
    $source = Get-DocSourcePath $Root $entry.source -Optional:$entry.optional
    if ($null -eq $source) { return }
    if (-not $context.map.TryAdd($entry.source, $entry)) { throw 'Duplicate document source.' }
    $entry
  })
  if (-not $context.map.ContainsKey('README.md') -or $context.map['README.md'].destination -cne 'index.md') { throw 'Documentation requires the root overview.' }
  foreach ($tagFile in $context.tagFiles) { $null = Get-DocSourcePath $Root $tagFile }
  $template = [IO.File]::ReadAllText((Get-DocSourcePath $Root 'docs/site/page.html'))
  $css = [IO.File]::ReadAllText((Get-DocSourcePath $Root 'docs/site/site.css'))
  $versionUrl = [Uri]::new($BaseUrl, "$Version/").AbsoluteUri
  $index = [Collections.Generic.List[string]]::new()
  $optionalIndex = [Collections.Generic.List[string]]::new()
  $full = [Collections.Generic.List[string]]::new()
  $full.Add("# Assuan $Version`n`nGuides and examples. Detailed API reference: ${versionUrl}api/assuan_library/index.html`n")
  foreach ($entry in $entries) {
    $text = [IO.File]::ReadAllText((Get-DocSourcePath $Root $entry.source)).Replace("`r`n", "`n").Replace("`r", "`n")
    if ($entry.source.EndsWith('.rs', [StringComparison]::Ordinal)) {
      $longest = 2
      foreach ($run in [regex]::Matches($text, '`+')) { $longest = [Math]::Max($longest, $run.Length) }
      $fence = '`' * ($longest + 1)
      $text = "# $($entry.title)`n`n${fence}rust`n$text`n$fence`n"
    }
    $markdown = Convert-DocMarkdown $text $entry.source $context 'Markdown'
    $forHtml = Convert-DocMarkdown $text $entry.source $context 'Html'
    $body = (ConvertFrom-Markdown -InputObject $forHtml).Html
    $page = @{
      TITLE=$entry.title;VERSION=$Version;STYLE="${versionUrl}assets/site.css"
      MARKDOWN="$versionUrl$($entry.destination)";LLMS="${versionUrl}llms.txt"
      HOME="${versionUrl}index.html";API="${versionUrl}api/assuan_library/index.html";CONTENT=$body
    }
    # Replace template tokens in one pass so source text cannot introduce tokens.
    $html = [regex]::Replace($template, '\{\{([A-Z]+)\}\}', {
      param($match)
      $key = $match.Groups[1].Value
      if (-not $page.ContainsKey($key)) { throw 'Unknown page template field.' }
      if ($key -eq 'CONTENT') { return $page[$key] }
      return [Net.WebUtility]::HtmlEncode($page[$key])
    })
    Write-DocText (Join-Path $versionRoot $entry.destination) $markdown
    Write-DocText (Join-Path $versionRoot ([IO.Path]::ChangeExtension($entry.destination, '.html'))) $html
    $link = "- [$($entry.title)]($versionUrl$($entry.destination)): $($entry.source)"
    if ($entry.includeInFull) {
      $index.Add($link)
      $full.Add("---`n`n## $($entry.title)`n`nSource: [$($entry.source)]($versionUrl$($entry.destination))`n`n$markdown`n")
    } else { $optionalIndex.Add($link) }
  }
  $llms = "# Assuan $Version`n`n> Asynchronous Assuan clients, servers, transports and bounded protocol primitives for Rust.`n`nUse documentation for this version. These guides cover ownership, security and examples; detailed APIs remain in rustdoc.`n`n## Guides and examples`n`n$($index -join "`n")`n`n## Reference`n`n- [Full guides](${versionUrl}llms-full.txt): Consolidated selected Markdown.`n- [API reference](${versionUrl}api/assuan_library/index.html): Detailed rustdoc (HTML).`n"
  if ($optionalIndex.Count -gt 0) { $llms += "`n## Optional`n`n$($optionalIndex -join "`n")`n" }
  Write-DocText "$versionRoot/llms.txt" $llms
  Write-DocText "$versionRoot/llms-full.txt" ($full -join "`n")
  Write-DocText "$versionRoot/assets/site.css" $css
  foreach ($file in $apiFiles) {
    $relative = [IO.Path]::GetRelativePath($ApiDirectory, $file).Replace('\', '/')
    if ($relative -eq '.lock') { continue }
    $destination = Join-Path "$versionRoot/api" $relative
    if ($relative.EndsWith('.html', [StringComparison]::Ordinal)) {
      $html = Convert-DocApiHtml ([IO.File]::ReadAllText($file)) $relative $context
      Write-DocText $destination $html
    } else {
      $null = [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination))
      [IO.File]::Copy($file, $destination, $false)
    }
  }
  if (-not (Test-Path -LiteralPath "$versionRoot/api/index.html")) {
    $crateLinks = foreach ($crate in $config.crates) {
      $directory = $crate.Replace('-', '_')
      if (Test-Path -LiteralPath "$versionRoot/api/$directory/index.html") {
        "<li><a href=`"$directory/index.html`">$crate</a></li>"
      }
    }
    Write-DocText "$versionRoot/api/index.html" "<!doctype html><html lang=`"en`"><head><meta charset=`"utf-8`"><title>Assuan $Version API</title></head><body><h1>Assuan $Version API</h1><ul>$($crateLinks -join '')</ul><a href=`"../index.html`">Guides</a></body></html>`n"
  }
  Test-DocsLinks $Output $BaseUrl
  [string[]] $paths = @(Get-DocFiles $versionRoot | ForEach-Object { [IO.Path]::GetRelativePath($versionRoot,$_).Replace('\','/') })
  [Array]::Sort($paths, [StringComparer]::Ordinal)
  $inventory = @(foreach ($path in $paths) {
    [pscustomobject]@{path=$path;sha256=(Get-FileHash -LiteralPath (Join-Path $versionRoot $path) -Algorithm SHA256).Hash.ToLowerInvariant()}
  })
  return [pscustomobject]@{version=$Version;root=$versionRoot;files=$inventory}
}

function Test-DocsLinks {
  <# .SYNOPSIS
  Checks static HTML and Markdown links under a generated site directory.
  .DESCRIPTION
  Local files and HTML anchors are checked without fetching external URLs.
  JavaScript-generated navigation and external link availability are outside
  this check. Markdown anchors are checked against the companion HTML page.
  #>
  param([string] $Root, [Uri] $BaseUrl)
  $Root = [IO.Path]::GetFullPath($Root)
  $anchors = @{}
  foreach ($file in Get-DocFiles $Root) {
    $extension = [IO.Path]::GetExtension($file)
    if ($extension -cnotin @('.html','.md','.txt')) { continue }
    $relative = [IO.Path]::GetRelativePath($Root,$file).Replace('\','/')
    $sourceUri = [Uri]::new($BaseUrl, $relative)
    $text = [IO.File]::ReadAllText($file)
    if ($extension -eq '.html') { $html = $text }
    else { $html = (ConvertFrom-Markdown -InputObject $text).Html }
    foreach ($attribute in Get-DocAttributes $html) {
      $url = $attribute.value
      if ($url -eq '') { continue }
      $uri = [Uri]::new($sourceUri, $url)
      if ($uri.Scheme -cnotin @('http','https')) { continue }
      if ($uri.Authority -cne $BaseUrl.Authority -or $uri.Scheme -cne $BaseUrl.Scheme) { continue }
      if (-not $uri.AbsolutePath.StartsWith($BaseUrl.AbsolutePath,[StringComparison]::Ordinal)) { throw "Site link escapes base path: $relative" }
      $sitePath = [Uri]::UnescapeDataString($uri.AbsolutePath.Substring($BaseUrl.AbsolutePath.Length))
      if ($sitePath -eq '' -or $sitePath.EndsWith('/')) { $sitePath += 'index.html' }
      $target = [IO.Path]::GetFullPath((Join-Path $Root $sitePath))
      if (-not (Test-DocContained $Root $target)) { throw 'Site link escapes output path.' }
      if (-not (Test-Path -LiteralPath $target -PathType Leaf)) { throw "Missing site link: $relative -> $sitePath" }
      $rawFragment = $uri.Fragment.TrimStart('#')
      $fragment = [Uri]::UnescapeDataString($rawFragment)
      if ($fragment -eq '' -or $fragment.StartsWith(':~:text=')) { continue }
      if ($target.EndsWith('.md',[StringComparison]::Ordinal)) { $target = [IO.Path]::ChangeExtension($target,'.html') }
      if (-not $target.EndsWith('.html',[StringComparison]::Ordinal)) { continue }
      if (-not $anchors.ContainsKey($target)) {
        $anchors[$target] = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
        foreach ($id in Get-DocAttributes ([IO.File]::ReadAllText($target)) 'id|name') { $null = $anchors[$target].Add($id.value) }
      }
      # Rustdoc highlights source ranges through JavaScript; both endpoint
      # anchors must exist, and only API source pages support this notation.
      $sourceRange = $false
      if ($sitePath -match '/api/src/' -and $fragment -match '\A([0-9]+)-([0-9]+)\z') {
        $firstLine = $Matches[1]
        $lastLine = $Matches[2]
        $sourceRange = ([Numerics.BigInteger]::Parse($firstLine) -le [Numerics.BigInteger]::Parse($lastLine)) -and
          $anchors[$target].Contains($firstLine) -and $anchors[$target].Contains($lastLine)
      }
      if (-not $sourceRange -and -not $anchors[$target].Contains($rawFragment) -and -not $anchors[$target].Contains($fragment)) {
        throw "Missing site anchor: $relative -> $sitePath#$fragment"
      }
    }
  }
}

Export-ModuleMember -Function Build-VersionDocs, Test-DocsLinks
