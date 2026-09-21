# Run from the workspace root. Use a short alias for a portable GnuPG install;
# only the temporary agents created by the Rust fixture are started or stopped.
[CmdletBinding()]
param([switch] $Workspace)

$ErrorActionPreference = 'Stop'
$originalConf = $env:ASSUAN_GPGCONF
$originalConnector = $env:ASSUAN_GPG_CONNECT_AGENT
$originalRequired = $env:ASSUAN_GNUPG_REQUIRED
$drive = $null
try {
  $conf = if ($originalConf) { $originalConf } else { 'gpgconf.exe' }
  $connector = if ($originalConnector) { $originalConnector } else { 'gpg-connect-agent.exe' }
  $confPath = (Get-Command $conf -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
  $connectorPath = (Get-Command $connector -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
  $bin = Split-Path $confPath -Parent
  if ((Split-Path $bin -Leaf) -ne 'bin' -or
      (Split-Path $connectorPath -Parent) -ne $bin) {
    throw 'Select gpgconf and gpg-connect-agent from the same GnuPG bin directory.'
  }
  $installation = Split-Path $bin -Parent
  foreach ($letter in 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z') {
    $candidate = "${letter}:"
    if (Test-Path "$candidate\") { continue }
    & subst.exe $candidate $installation
    if ($LASTEXITCODE -eq 0) {
      $drive = $candidate
      break
    }
  }
  if (-not $drive) { throw 'No drive letter is available for the temporary GnuPG alias.' }
  $env:ASSUAN_GPGCONF = "$drive\bin\gpgconf.exe"
  $env:ASSUAN_GPG_CONNECT_AGENT = "$drive\bin\gpg-connect-agent.exe"
  $env:ASSUAN_GNUPG_REQUIRED = '1'
  if ($Workspace) {
    & cargo test --workspace
  } else {
    & cargo test -p assuan-library --test interop_agent --test interop_server -- --nocapture
  }
  if ($LASTEXITCODE -ne 0) { throw 'GnuPG interoperability tests failed.' }
}
finally {
  $env:ASSUAN_GPGCONF = $originalConf
  $env:ASSUAN_GPG_CONNECT_AGENT = $originalConnector
  $env:ASSUAN_GNUPG_REQUIRED = $originalRequired
  if ($drive) {
    & subst.exe $drive /D
    if ($LASTEXITCODE -ne 0) { Write-Error 'Could not remove the temporary GnuPG drive alias.' }
  }
}
