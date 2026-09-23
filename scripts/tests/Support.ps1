Set-StrictMode -Version Latest

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
