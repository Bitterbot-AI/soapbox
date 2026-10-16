# Installs a pinned portable NVIDIA runtime and a small embeddings model.
[CmdletBinding()]
param([switch]$KeepRuntime)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$runtime = Join-Path $root '.local/ollama'
$archive = Join-Path $runtime 'runtime.zip'
$binary = Join-Path $runtime 'ollama.exe'
$digest = '1b226e2802dbb772b5fc32a58f103ca1804ef7501331012de126ab22f67475ef'
$sha = '5134ce7e3bc8e94d0494721c799ec293b429450d0c52461b0ee3647dd3bb4bb7'
New-Item -ItemType Directory -Force -Path $runtime | Out-Null
if (!(Test-Path -LiteralPath $binary)) {
    if (!(Test-Path -LiteralPath $archive)) {
        Write-Host 'Downloading Ollama 0.40.5 portable runtime (approximately 1.5 GB).'
        Invoke-WebRequest 'https://github.com/ollama/ollama/releases/download/v0.40.5/ollama-windows-amd64.zip' -OutFile $archive -TimeoutSec 900
    }
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLower() -ne $sha) { throw 'Ollama archive checksum mismatch' }
    Expand-Archive -LiteralPath $archive -DestinationPath $runtime -Force
}
$settings = @{
    OLLAMA_HOST='127.0.0.1:11434'; OLLAMA_MODELS=(Join-Path $root '.local/models')
    OLLAMA_NO_CLOUD='1'; OLLAMA_MAX_LOADED_MODELS='1'; OLLAMA_NUM_PARALLEL='1'; OLLAMA_MAX_QUEUE='8'
}
$previous = @{}
foreach ($key in $settings.Keys) {
    $previous[$key] = [Environment]::GetEnvironmentVariable($key,'Process')
    [Environment]::SetEnvironmentVariable($key,$settings[$key],'Process')
}
$owned = $null
$completed = $false
try {
    try { $version = Invoke-RestMethod 'http://127.0.0.1:11434/api/version' -TimeoutSec 3 } catch { $version = $null }
    if (!$version) {
        $owned = Start-Process -FilePath $binary -ArgumentList 'serve' -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $runtime 'server.log') -RedirectStandardOutput (Join-Path $runtime 'stdout.log')
        for ($attempt=0; $attempt -lt 40; $attempt++) {
            if ($owned.HasExited) { throw 'Ollama exited; inspect .local/ollama/server.log' }
            try { $version = Invoke-RestMethod 'http://127.0.0.1:11434/api/version' -TimeoutSec 2; break } catch { Start-Sleep -Milliseconds 500 }
        }
    }
    if ($version.version -ne '0.40.5') { throw 'The pilot requires Ollama 0.40.5. Stop other runtimes before setup.' }
    $tags = Invoke-RestMethod 'http://127.0.0.1:11434/api/tags' -TimeoutSec 5
    $installed = $tags.models | Where-Object name -eq 'all-minilm:latest'
    if (!$installed) {
        Invoke-RestMethod 'http://127.0.0.1:11434/api/pull' -Method Post -ContentType 'application/json' -Body '{"model":"all-minilm:latest","stream":false}' -TimeoutSec 300 | Out-Null
        $tags = Invoke-RestMethod 'http://127.0.0.1:11434/api/tags' -TimeoutSec 5
    }
    $model = $tags.models | Where-Object name -eq 'all-minilm:latest'
    if ($model.digest -ne $digest) { throw 'Model tag differs from the reviewed digest; do not qualify this model automatically.' }
    $config = Join-Path $root '.local/worker-gpu.json'
    if (!(Test-Path -LiteralPath $config)) {
        Copy-Item -LiteralPath (Join-Path $root 'examples/worker-gpu.json') -Destination $config
    }
    Write-Host "GPU model installed and pinned. Worker configuration: $config"
    $completed = $true
    if ($KeepRuntime -and $owned) {
        $owned.Id | Set-Content -LiteralPath (Join-Path $root '.local/ollama.pid')
        Write-Host "Runtime started by setup: PID $($owned.Id). Stop it after the pilot."
    }
} finally {
    if ($owned -and (!$KeepRuntime -or !$completed) -and !$owned.HasExited) { Stop-Process -Id $owned.Id }
    foreach ($key in $settings.Keys) { [Environment]::SetEnvironmentVariable($key,$previous[$key],'Process') }
}
