# Run from the Rust workspace. Generate Cargo.lock first; all checks reuse it.
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$features = @('client', 'server', 'macros', 'sexpr')
foreach ($mask in 0..15) {
  $enabled = @()
  foreach ($bit in 0..3) {
    if (($mask -band (1 -shl $bit)) -ne 0) { $enabled += $features[$bit] }
  }
  Write-Host "Checking facade features: [$($enabled -join ',')]"
  $cargoArgs = @('check', '--locked', '-p', 'assuan-library', '--lib', '--no-default-features')
  if ($enabled.Count -gt 0) { $cargoArgs += @('--features', ($enabled -join ',')) }
  & cargo @cargoArgs
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
foreach ($crate in 'assuan-protocol', 'assuan-sexpr') {
  & cargo check --locked -p $crate --lib --no-default-features
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  & cargo tree --locked -p $crate --edges normal --no-default-features
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
