<#
.SYNOPSIS
  Ejecuta la escena en la PC (sin casco) y guarda una captura PNG.

.EXAMPLE
  .\tools\captura.ps1 -Salida C:\temp\vista.png -Vista "0,1.65,3.4,0,-4"
  Vista = x, alto de los ojos, z, giro (grados), inclinación (grados).
#>
param(
    [Parameter(Mandatory = $true)][string]$Salida,
    [string]$Vista = "0,1.65,3.4,0,-4",
    [int]$Esperar = 120,
    [string]$Resolucion = "1600x900",
    [string]$Escena = "",
    [string[]]$Extra = @()
)
$ErrorActionPreference = 'Stop'
$raiz = Split-Path -Parent $PSScriptRoot
$godot = Join-Path $env:LOCALAPPDATA 'Programs\Godot\Godot_v4.7.2-stable_win64_console.exe'
$proyecto = Join-Path $raiz 'godot'
$logs = Join-Path $raiz 'build'
New-Item -ItemType Directory -Force $logs | Out-Null

$argumentos = @('--path', $proyecto, '--xr-mode', 'off', '--resolution', $Resolucion)
if ($Escena) { $argumentos += $Escena }
$argumentos += @('--', "--captura=$Salida", "--vista=$Vista", "--esperar=$Esperar") + $Extra

if (Test-Path $Salida) { Remove-Item $Salida -Force }
$p = Start-Process -FilePath $godot -ArgumentList $argumentos -NoNewWindow -PassThru `
    -RedirectStandardOutput (Join-Path $logs 'captura_out.txt') -RedirectStandardError (Join-Path $logs 'captura_err.txt')
if (-not $p.WaitForExit(180000)) { $p.Kill(); Write-Warning "Tiempo agotado" }
Get-Content (Join-Path $logs 'captura_out.txt') | Where-Object { $_ -notmatch '^\s*$' } | Select-Object -Last 30
$err = Get-Content (Join-Path $logs 'captura_err.txt') | Where-Object { $_ -notmatch '^\s*$' }
if ($err) { "--- stderr ---"; $err | Select-Object -Last 30 }
if (Test-Path $Salida) { "OK captura: $Salida" } else { "SIN captura" }
