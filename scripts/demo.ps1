param([switch]$Keep)
$ErrorActionPreference = 'Stop'
$previousBinary = $env:SOAPBOX_BINARY
try {
  if (-not $env:SOAPBOX_BINARY) {
    $env:SOAPBOX_BINARY = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\.local\bin\soapbox.exe'))
  }
  if (-not (Test-Path -LiteralPath $env:SOAPBOX_BINARY)) {
    throw 'Build Soapbox or set SOAPBOX_BINARY to your compiled executable first.'
  }
  $demoArguments = @((Join-Path $PSScriptRoot 'demo.mjs'))
  if ($Keep) { $demoArguments += '--keep' }
  & node @demoArguments
  if ($LASTEXITCODE -ne 0) { throw "Soapbox demo failed with exit code $LASTEXITCODE" }
} finally {
  $env:SOAPBOX_BINARY = $previousBinary
}
