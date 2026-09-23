# Private link helpers. Markdig comes from the pinned PowerShell installation.
function Get-DocHtmlTags([string] $Text) {
  # Skip comments and complete raw-text elements; never rewrite script strings.
  $pattern = '(?is)<!--.*?-->|<(script|style|textarea)\b[^>]*>.*?</\1\s*>|<[a-z][a-z0-9:-]*\b(?:[^>"'']|"[^"]*"|''[^'']*'')*>'
  foreach ($match in [regex]::Matches($Text, $pattern)) {
    if ($match.Value -match '^(?is)<!--|^<(script|style|textarea)\b') { continue }
    $match
  }
}

function Get-DocAttributes([string] $Text, [string] $Names = 'href|src') {
  foreach ($tag in Get-DocHtmlTags $Text) {
    foreach ($attribute in [regex]::Matches($tag.Value, "(?is)\s(?<name>$Names)\s*=\s*(?:`"(?<value>[^`"]*)`"|'(?<value>[^']*)'|(?<value>[^\s>]+))")) {
      [pscustomobject]@{
        name = $attribute.Groups['name'].Value
        value = [Net.WebUtility]::HtmlDecode($attribute.Groups['value'].Value)
        start = $tag.Index + $attribute.Groups['value'].Index
        length = $attribute.Groups['value'].Length
      }
    }
  }
}

function Set-DocReplacements([string] $Text, [object[]] $Edits) {
  $previous = $Text.Length
  foreach ($edit in @($Edits | Sort-Object start -Descending -Unique)) {
    if ($edit.start -lt 0 -or ($edit.start + $edit.length) -gt $previous) { throw 'Overlapping document link spans.' }
    $Text = $Text.Remove($edit.start, $edit.length).Insert($edit.start, $edit.value)
    $previous = $edit.start
  }
  return $Text
}

function Resolve-DocLink([string] $Url, [string] $Source, [hashtable] $Context, [string] $Format) {
  if ($Url -eq '') { return $Url }
  $repositoryPrefix = $Context.repository + '/blob/develop/'
  $rawPrefix = $Context.repository.Replace('https://github.com/', 'https://raw.githubusercontent.com/') + '/develop/'
  $isRepository = $false
  foreach ($prefix in @($repositoryPrefix, $rawPrefix, ($Context.repository + '/tree/develop/'))) {
    if ($Url.StartsWith($prefix, [StringComparison]::Ordinal)) {
      $Url = $Url.Substring($prefix.Length)
      $isRepository = $true
      break
    }
  }
  if (-not $isRepository -and $Url -match '^(?i)([a-z][a-z0-9+.-]*:|//|/)') {
    if ($Url -match '^(?i)(javascript|vbscript|data):') { throw 'Unsafe document URL scheme.' }
    return $Url
  }
  $parts = [regex]::Match($Url, '^([^?#]*)(.*)$')
  $path = [Uri]::UnescapeDataString($parts.Groups[1].Value)
  $suffix = $parts.Groups[2].Value
  if ($isRepository) { $relative = $path }
  elseif ($path -eq '') { $relative = $Source }
  else {
    $sourceFolder = [IO.Path]::GetDirectoryName((Join-Path $Context.root $Source))
    $absolute = [IO.Path]::GetFullPath((Join-Path $sourceFolder $path))
    $relative = [IO.Path]::GetRelativePath($Context.root, $absolute).Replace('\', '/')
  }
  Assert-DocRelativePath $relative
  if ($Context.map.ContainsKey($relative)) {
    $destination = $Context.map[$relative].destination
    if ($Format -eq 'Html') { $destination = [IO.Path]::ChangeExtension($destination, '.html') }
    return ([Uri]::new($Context.base, "$($Context.version)/$destination")).AbsoluteUri + $suffix
  }
  if ($relative -cin $Context.tagFiles) {
    return "$($Context.repository)/blob/v$($Context.version)/$relative$suffix"
  }
  throw "Document link target is not selected: $relative"
}

function Convert-DocMarkdown([string] $Text, [string] $Source, [hashtable] $Context, [string] $Format) {
  $document = (ConvertFrom-Markdown -InputObject $Text).Tokens
  $edits = [Collections.Generic.List[object]]::new()
  foreach ($node in [Markdig.Syntax.MarkdownObjectExtensions]::Descendants($document)) {
    if ($node -is [Markdig.Syntax.Inlines.LinkInline]) {
      $target = Resolve-DocLink $node.Url $Source $Context $Format
      if ($target -ceq $node.Url) { continue }
      $span = $node.UrlSpan
      if ($null -ne $node.Reference) { $span = $node.Reference.UrlSpan }
      if ($span.Start -lt 0 -or $span.Length -le 0) { throw 'Unsupported Markdown link span.' }
      $edits.Add([pscustomobject]@{start=$span.Start;length=$span.Length;value=$target})
    }
    elseif ($node -is [Markdig.Syntax.Inlines.AutolinkInline]) {
      if ($node.IsEmail) { continue }
      $target = Resolve-DocLink $node.Url $Source $Context $Format
      if ($target -ceq $node.Url) { continue }
      $edits.Add([pscustomobject]@{start=$node.Span.Start;length=$node.Span.Length;value="<$target>"})
    }
    elseif ($node -is [Markdig.Syntax.Inlines.HtmlInline] -or $node -is [Markdig.Syntax.HtmlBlock]) {
      $slice = $Text.Substring($node.Span.Start, $node.Span.Length)
      foreach ($attribute in Get-DocAttributes $slice) {
        $target = Resolve-DocLink $attribute.value $Source $Context $Format
        $edits.Add([pscustomobject]@{
          start=$node.Span.Start + $attribute.start;length=$attribute.length
          value=[Net.WebUtility]::HtmlEncode($target)
        })
      }
    }
  }
  return Set-DocReplacements $Text $edits.ToArray()
}

function Convert-DocApiHtml([string] $Text, [string] $Relative, [hashtable] $Context) {
  $edits = [Collections.Generic.List[object]]::new()
  foreach ($attribute in Get-DocAttributes $Text) {
    $url = $attribute.value
    if ($url.StartsWith($Context.repository + '/blob/develop/', [StringComparison]::Ordinal) -or
        $url.StartsWith($Context.repository + '/tree/develop/', [StringComparison]::Ordinal) -or
        $url.StartsWith($Context.repository.Replace('https://github.com/', 'https://raw.githubusercontent.com/') + '/develop/', [StringComparison]::Ordinal)) {
      $target = Resolve-DocLink $url 'README.md' $Context 'Html'
    } elseif ($url -ceq 'LICENSE.md' -and $Relative -match '^([^/]+)/') {
      $crate = $Matches[1].Replace('_', '-')
      $target = Resolve-DocLink $url "crates/$crate/README.md" $Context 'Html'
    } else { continue }
    $edits.Add([pscustomobject]@{start=$attribute.start;length=$attribute.length;value=[Net.WebUtility]::HtmlEncode($target)})
  }
  return Set-DocReplacements $Text $edits.ToArray()
}
