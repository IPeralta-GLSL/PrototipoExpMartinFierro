<#
.SYNOPSIS
  Launch the headless ComfyUI used by tools\assetgen (Z-Image-Turbo image generation).

.DESCRIPTION
  Runs the ComfyUIStudio portable install (E:\ComfyUI\ComfyUIStudio\App, ComfyUI 0.22.0,
  embedded Python 3.11 + torch cu128) WITHOUT modifying it:
    * python -B                      : never writes .pyc files into the install
    * --base-directory comfy_runtime : models/custom_nodes/input/output/temp/user all live in
                                       tools\assetgen\comfy_runtime (the install's own "models"
                                       folder is a broken symlink to an unreachable network share)
    * --extra-model-paths-config     : model files are read from E:\KRITA\ComfyUI\models
    * --database-url sqlite:///:memory: : no comfyui.db written into the install's user folder
    * --disable-all-custom-nodes --whitelist-custom-nodes ComfyUI-GGUF : only the GGUF loader
      is loaded, from comfy_runtime\custom_nodes\ComfyUI-GGUF (a copy of the newer July-2026
      build shipped with Krita's ComfyUI; the build bundled with Studio predates Qwen3 support)
    * --disable-api-nodes, --disable-auto-launch, no --windows-standalone-build (no updater)
  Note: the install's own ComfyUI\extra_model_paths.yaml is always read by main.py; it only adds
  non-existent C:\ComfyUI\models_3d_extra paths, which are harmless.

  zimage_generate.py builds exactly the same command line, and both use the same PID file,
  so a server started here is reused (and can be stopped) by zimage_generate.py.

.PARAMETER Port
  Listen port (default 8190). The server always listens on 127.0.0.1 only.

.PARAMETER Background
  Start detached (hidden window). Output goes to comfy_runtime\logs\comfyui_<timestamp>.log,
  the PID is written to comfy_runtime\comfy_server.pid, and the script waits until the HTTP
  API answers. Without this switch the server runs in this console (Ctrl+C stops it).

.PARAMETER Stop
  Stop the server listening on -Port (only if it is this ComfyUI main.py), then exit.

.PARAMETER ExtraArgs
  Additional ComfyUI arguments, e.g. -ExtraArgs '--highvram'

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File tools\assetgen\start_comfy.ps1 -Background
  powershell -ExecutionPolicy Bypass -File tools\assetgen\start_comfy.ps1 -Stop
#>
param(
    [int]$Port = 8190,
    [switch]$Background,
    [switch]$Stop,
    [string[]]$ExtraArgs = @(),
    [int]$StartTimeoutSec = 300
)

$ErrorActionPreference = 'Stop'

$AssetGen  = $PSScriptRoot
$Runtime   = Join-Path $AssetGen 'comfy_runtime'
$StudioApp = 'E:\ComfyUI\ComfyUIStudio\App'
$Python    = Join-Path $StudioApp 'python_embeded\python.exe'
$MainPy    = Join-Path $StudioApp 'ComfyUI\main.py'
$Yaml      = Join-Path $AssetGen 'comfy_extra_model_paths.yaml'
$PidFile   = Join-Path $Runtime 'comfy_server.pid'
$BaseUrl   = "http://127.0.0.1:$Port"

function Get-ListenerPid([int]$p) {
    $c = Get-NetTCPConnection -State Listen -LocalPort $p -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($c) { return [int]$c.OwningProcess }
    return $null
}

function Test-IsOurComfy([int]$procId) {
    $proc = Get-CimInstance Win32_Process -Filter "ProcessId=$procId" -ErrorAction SilentlyContinue
    if (-not $proc -or -not $proc.CommandLine) { return $false }
    return ($proc.CommandLine -like '*ComfyUI\main.py*') -and ($proc.CommandLine -like '*comfy_runtime*')
}

function Test-ApiUp {
    try {
        $null = Invoke-RestMethod -Uri "$BaseUrl/system_stats" -TimeoutSec 3
        return $true
    } catch { return $false }
}

# ----------------------------------------------------------------------------- stop
if ($Stop) {
    $listener = Get-ListenerPid $Port
    if (-not $listener) {
        Write-Host "Nothing is listening on port $Port."
        if (Test-Path $PidFile) { Remove-Item $PidFile -Force }
        exit 0
    }
    if (-not (Test-IsOurComfy $listener)) {
        Write-Host "Port $Port is used by PID $listener, which is not this ComfyUI (main.py + comfy_runtime). Not touching it."
        exit 1
    }
    Write-Host "Stopping ComfyUI PID $listener on port $Port ..."
    try { $null = Invoke-RestMethod -Method Post -Uri "$BaseUrl/interrupt" -TimeoutSec 3 } catch { }
    & taskkill.exe /PID $listener /T /F | Out-Null
    for ($i = 0; $i -lt 40 -and (Get-ListenerPid $Port); $i++) { Start-Sleep -Milliseconds 250 }
    if (Test-Path $PidFile) { Remove-Item $PidFile -Force }
    if (Get-ListenerPid $Port) { Write-Host "Port $Port is still busy."; exit 1 }
    Write-Host "Stopped. Port $Port is free."
    exit 0
}

# ----------------------------------------------------------------------------- checks
foreach ($f in @($Python, $MainPy, $Yaml)) {
    if (-not (Test-Path $f)) { throw "Missing: $f" }
}
foreach ($d in 'custom_nodes', 'input', 'output', 'temp', 'user', 'models', 'logs') {
    New-Item -ItemType Directory -Force -Path (Join-Path $Runtime $d) | Out-Null
}
if (-not (Test-Path (Join-Path $Runtime 'custom_nodes\ComfyUI-GGUF\nodes.py'))) {
    throw "ComfyUI-GGUF is missing from $Runtime\custom_nodes (copy it from E:\KRITA\ComfyUI\ComfyUI\custom_nodes\ComfyUI-GGUF)."
}

$listener = Get-ListenerPid $Port
if ($listener) {
    if ((Test-IsOurComfy $listener) -and (Test-ApiUp)) {
        Write-Host "ComfyUI is already running on $BaseUrl (PID $listener). Reusing it."
        exit 0
    }
    throw "Port $Port is already in use by PID $listener."
}

# ----------------------------------------------------------------------------- command line
$comfyArgs = @(
    '-B', '-s', '-W', 'ignore::FutureWarning',
    $MainPy,
    '--listen', '127.0.0.1',
    '--port', "$Port",
    '--base-directory', $Runtime,
    '--extra-model-paths-config', $Yaml,
    '--disable-all-custom-nodes',
    '--whitelist-custom-nodes', 'ComfyUI-GGUF',
    '--disable-api-nodes',
    '--disable-auto-launch',
    '--database-url', 'sqlite:///:memory:',
    '--preview-method', 'none',
    '--log-stdout'
) + $ExtraArgs

function Format-Arg([string]$a) {
    if ($a -match '[\s"]') { return '"' + ($a -replace '"', '\"') + '"' }
    return $a
}
$argLine = ($comfyArgs | ForEach-Object { Format-Arg $_ }) -join ' '
Write-Host "Launch: `"$Python`" $argLine"

if (-not $Background) {
    # Foreground: logs stream to this console, Ctrl+C stops the server.
    & $Python @comfyArgs
    exit $LASTEXITCODE
}

# ----------------------------------------------------------------------------- background
$stamp  = Get-Date -Format 'yyyyMMdd_HHmmss'
$logOut = Join-Path $Runtime "logs\comfyui_$stamp.log"
$logErr = Join-Path $Runtime "logs\comfyui_$stamp.err.log"
$proc = Start-Process -FilePath $Python -ArgumentList $argLine -WorkingDirectory $Runtime `
    -RedirectStandardOutput $logOut -RedirectStandardError $logErr -WindowStyle Hidden -PassThru
Set-Content -Path $PidFile -Value $proc.Id -Encoding ascii
Write-Host "Started PID $($proc.Id); log: $logOut"

$deadline = (Get-Date).AddSeconds($StartTimeoutSec)
while ((Get-Date) -lt $deadline) {
    if ($proc.HasExited) {
        Write-Host "ComfyUI exited early with code $($proc.ExitCode). Last log lines:"
        if (Test-Path $logOut) { Get-Content $logOut -Tail 40 }
        if (Test-Path $logErr) { Get-Content $logErr -Tail 40 }
        if (Test-Path $PidFile) { Remove-Item $PidFile -Force }
        exit 1
    }
    if (Test-ApiUp) {
        Write-Host "ComfyUI is ready on $BaseUrl"
        exit 0
    }
    Start-Sleep -Seconds 1
}
Write-Host "Timed out after $StartTimeoutSec s waiting for $BaseUrl (process still running, PID $($proc.Id))."
exit 1
