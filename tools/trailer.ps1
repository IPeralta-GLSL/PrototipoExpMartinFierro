<#
.SYNOPSIS
  Renderiza el teaser de la escena (Movie Maker de Godot, cuadro a cuadro) y lo
  codifica a MP4 (H.264 + AAC) con ffmpeg.

.EXAMPLE
  .\tools\trailer.ps1
  .\tools\trailer.ps1 -Resolucion 2560x1440 -Fps 60
  .\tools\trailer.ps1 -SinCarteles -SinCreditos -Salida build\teaser_limpio.mp4
#>
param(
    [string]$Salida = "",
    [int]$Fps = 30,
    [string]$Resolucion = "1920x1080",
    [switch]$SinCarteles,
    [switch]$SinCreditos,
    [switch]$ConservarCuadros
)
$ErrorActionPreference = 'Stop'
$raiz = Split-Path -Parent $PSScriptRoot
if (-not $Salida) { $Salida = Join-Path $raiz 'build\teaser_martin_fierro.mp4' }
elseif (-not [IO.Path]::IsPathRooted($Salida)) { $Salida = Join-Path $raiz $Salida }
$godot = Join-Path $env:LOCALAPPDATA 'Programs\Godot\Godot_v4.7.2-stable_win64_console.exe'
$proyecto = Join-Path $raiz 'godot'
$cuadros = Join-Path $raiz 'build\trailer_cuadros'

$ffmpeg = (Get-Command ffmpeg -ErrorAction SilentlyContinue).Source
if (-not $ffmpeg) {
    $ffmpeg = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Recurse -Filter ffmpeg.exe -ErrorAction SilentlyContinue |
        Select-Object -First 1 -ExpandProperty FullName
}
if (-not $ffmpeg) { throw "No se encontró ffmpeg." }

if (Test-Path $cuadros) { Remove-Item $cuadros -Recurse -Force }
New-Item -ItemType Directory -Force $cuadros | Out-Null

Write-Host "Renderizando cuadros ($Resolucion a $Fps fps)..." -ForegroundColor Cyan
$t0 = Get-Date
# La ventana no puede ser más grande que la pantalla: se fija el viewport a la
# resolución pedida con un override.cfg temporal (modo de estiramiento "viewport").
$ancho, $alto = $Resolucion -split 'x'
$override = Join-Path $proyecto 'override.cfg'
@"
[display]

window/size/viewport_width=$ancho
window/size/viewport_height=$alto
window/stretch/mode="viewport"
window/size/always_on_top=true
"@ | Out-File -Encoding ascii $override
Write-Host "No minimices la ventana de Godot mientras renderiza: si no se ve, deja de dibujar." -ForegroundColor Yellow
$opciones = @('--escritorio')
if ($SinCarteles) { $opciones += '--sin-carteles' }
if ($SinCreditos) { $opciones += '--sin-creditos' }
$previo = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
try {
    & $godot --path $proyecto --xr-mode off --fixed-fps $Fps `
        --write-movie (Join-Path $cuadros 'cuadro.png') 'res://escenas/trailer.tscn' -- @opciones 2>&1 |
        ForEach-Object { "$_" } | Where-Object { $_ -match 'ERROR|SCRIPT|Movie|movie' } | Select-Object -First 20
}
finally {
    $ErrorActionPreference = $previo
    Remove-Item $override -Force -ErrorAction SilentlyContinue
}
$n = (Get-ChildItem $cuadros -Filter 'cuadro*.png').Count
Write-Host ("{0} cuadros en {1:N0} s" -f $n, ((Get-Date) - $t0).TotalSeconds)
if ($n -lt 10) { throw "Godot no generó cuadros." }
# Control: si la ventana se minimizó, Godot repite el último cuadro (imagen congelada).
$muestra = Get-ChildItem $cuadros -Filter 'cuadro*.png' | Sort-Object Name | Select-Object -Last 240 | Select-Object -SkipLast 40
$unicos = @($muestra | ForEach-Object { (Get-FileHash $_.FullName -Algorithm MD5).Hash } | Select-Object -Unique).Count
if ($muestra.Count -gt 0 -and $unicos -lt ($muestra.Count / 4)) {
    Write-Warning "Hay muchos cuadros repetidos al final ($unicos distintos de $($muestra.Count)): la imagen se congeló. Volvé a renderizar sin minimizar la ventana."
}

$audio = Join-Path $cuadros 'cuadro.wav'
$entrada = Join-Path $cuadros 'cuadro%08d.png'
$argumentos = @('-y', '-loglevel', 'error', '-framerate', "$Fps", '-i', $entrada)
if (Test-Path $audio) { $argumentos += @('-i', $audio) }
$argumentos += @('-c:v', 'libx264', '-preset', 'slow', '-crf', '17', '-pix_fmt', 'yuv420p', '-movflags', '+faststart')
if (Test-Path $audio) { $argumentos += @('-c:a', 'aac', '-b:a', '192k', '-shortest') }
$argumentos += $Salida
Write-Host "Codificando MP4..." -ForegroundColor Cyan
& $ffmpeg @argumentos
if ($LASTEXITCODE -ne 0) { throw "ffmpeg falló ($LASTEXITCODE)" }
if (-not $ConservarCuadros) { Remove-Item $cuadros -Recurse -Force }
$mb = [math]::Round((Get-Item $Salida).Length / 1MB, 1)
Write-Host "Teaser: $Salida ($mb MB)" -ForegroundColor Green
