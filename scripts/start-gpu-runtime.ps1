$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$binary = Join-Path $root '.local/ollama/ollama.exe'
if (!(Test-Path -LiteralPath $binary)) { throw 'Run scripts/setup-gpu.ps1 first.' }
$settings = @{
    OLLAMA_HOST='127.0.0.1:11434'; OLLAMA_MODELS=(Join-Path $root '.local/models')
    OLLAMA_NO_CLOUD='1'; OLLAMA_MAX_LOADED_MODELS='1'; OLLAMA_NUM_PARALLEL='1'; OLLAMA_MAX_QUEUE='8'
}
$previous = @{}
foreach ($key in $settings.Keys) {
    $previous[$key] = [Environment]::GetEnvironmentVariable($key,'Process')
    [Environment]::SetEnvironmentVariable($key,$settings[$key],'Process')
}
try { & $binary serve } finally {
    foreach ($key in $settings.Keys) { [Environment]::SetEnvironmentVariable($key,$previous[$key],'Process') }
}
