<#
.SYNOPSIS
  Exporta el APK para Meta Quest 3 y (opcionalmente) lo instala y lo abre en el casco.

.EXAMPLE
  .\tools\exportar_quest.ps1 -Instalar -Ejecutar        # todo: Rust + APK + instalar + abrir
  .\tools\exportar_quest.ps1 -SinRust -Instalar         # sin recompilar Rust
  .\tools\exportar_quest.ps1 -Logcat                    # ver el log de Godot del casco
#>
param(
    [switch]$SinRust,
    [switch]$Instalar,
    [switch]$Ejecutar,
    [switch]$Logcat
)

$ErrorActionPreference = 'Stop'
$raiz = Split-Path -Parent $PSScriptRoot
$godot = Join-Path $env:LOCALAPPDATA 'Programs\Godot\Godot_v4.7.2-stable_win64_console.exe'
$proyecto = Join-Path $raiz 'godot'
$salida = Join-Path $raiz 'build'
$apk = Join-Path $salida 'MartinFierroVR.apk'
$paquete = 'com.prototipojuego.martinfierro'
$adb = Join-Path $env:LOCALAPPDATA 'Android\Sdk\platform-tools\adb.exe'
New-Item -ItemType Directory -Force $salida | Out-Null

# Entorno para Gradle y la firma de debug (el keystore estándar de Android).
if (-not $env:JAVA_HOME) { $env:JAVA_HOME = 'C:\Android\jdk-17' }
if (-not $env:ANDROID_HOME) { $env:ANDROID_HOME = Join-Path $env:LOCALAPPDATA 'Android\Sdk' }
$env:GODOT_ANDROID_KEYSTORE_DEBUG_PATH = Join-Path $env:USERPROFILE '.android\debug.keystore'
$env:GODOT_ANDROID_KEYSTORE_DEBUG_USER = 'androiddebugkey'
$env:GODOT_ANDROID_KEYSTORE_DEBUG_PASSWORD = 'android'

function Ejecutar([string]$exe, [string[]]$argumentos, [switch]$PermitirFallo) {
    Write-Host ">> $(Split-Path -Leaf $exe) $($argumentos -join ' ')" -ForegroundColor Cyan
    $previo = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    & $exe @argumentos 2>&1 | ForEach-Object { "$_" }
    $codigo = $LASTEXITCODE
    $ErrorActionPreference = $previo
    if ($codigo -ne 0 -and -not $PermitirFallo) { throw "Falló ($codigo): $exe" }
}

if (-not ($Logcat -and -not $Instalar -and -not $Ejecutar)) {
    if (-not $SinRust) { & (Join-Path $PSScriptRoot 'build_rust.ps1') }

    Ejecutar $godot @('--headless', '--path', $proyecto, '--import') -PermitirFallo

    $argumentos = @('--headless', '--path', $proyecto)
    $version = Join-Path $proyecto 'android\.build_version'
    if (-not (Test-Path $version) -or ((Get-Content $version -Raw).Trim() -ne '4.7.2.stable')) {
        Write-Host "Instalando la plantilla de Android (Gradle)..." -ForegroundColor Yellow
        $argumentos += '--install-android-build-template'
    }
    if (Test-Path $apk) { Remove-Item $apk -Force }
    $argumentos += @('--export-debug', 'Meta Quest 3', $apk)
    Ejecutar $godot $argumentos -PermitirFallo
    if (-not (Test-Path $apk)) { throw "No se generó el APK (revisá los errores de Gradle arriba)." }
    $mb = [math]::Round((Get-Item $apk).Length / 1MB, 1)
    Write-Host "APK: $apk ($mb MB)" -ForegroundColor Green
}

if ($Instalar) {
    Ejecutar $adb @('install', '-r', $apk)
}
if ($Ejecutar) {
    Ejecutar $adb @('shell', 'am', 'force-stop', $paquete) -PermitirFallo
    Ejecutar $adb @('logcat', '-c') -PermitirFallo
    Ejecutar $adb @('shell', 'am', 'start', '-n', "$paquete/com.godot.game.GodotAppLauncher")
    Write-Host "Ponete el casco: la app se abrió en el Quest." -ForegroundColor Green
}
if ($Logcat) {
    Ejecutar $adb @('logcat', '-d', '-s', 'godot:V') -PermitirFallo
}
