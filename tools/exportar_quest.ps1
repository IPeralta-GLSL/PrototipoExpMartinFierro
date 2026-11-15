<#
.SYNOPSIS
  Exporta el APK para Meta Quest 3 y (opcionalmente) lo instala y lo abre en el casco.

.EXAMPLE
  .\tools\exportar_quest.ps1 -Instalar -Ejecutar        # todo: Rust + APK + instalar + abrir
  .\tools\exportar_quest.ps1 -SinRust -Instalar         # sin recompilar Rust
  .\tools\exportar_quest.ps1 -Logcat                    # ver el log de Godot del casco
  .\tools\exportar_quest.ps1 -Wifi                      # ADB por Wi-Fi (una vez, con el USB puesto)

  ADB por Wi-Fi: con el casco enchufado por USB, -Wifi lo pasa a modo TCP (puerto 5555), se
  conecta a su IP y la guarda en build\quest_wifi.txt. Después ya no hace falta el cable: el
  script se reconecta solo a esa IP. Si el casco se reinicia, el modo TCP se pierde: enchufalo
  una vez y volvé a correr -Wifi.
#>
param(
    [switch]$SinRust,
    [switch]$Instalar,
    [switch]$Ejecutar,
    [switch]$Logcat,
    [switch]$Wifi
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

$ipGuardada = Join-Path $salida 'quest_wifi.txt'

function Dispositivos {
    # adb escribe avisos en stderr (p. ej. al iniciar el daemon): no son errores.
    $ErrorActionPreference = 'Continue'
    & $adb devices 2>$null | Select-Object -Skip 1 | Where-Object { $_ -match '\sdevice$' } |
        ForEach-Object { ($_ -split '\s+')[0] }
}

# ADB por Wi-Fi: modo TCP desde el USB, conectar y guardar la IP.
function Configurar-Wifi {
    # adb escribe avisos en stderr (p. ej. al iniciar el daemon): no son errores.
    $ErrorActionPreference = 'Continue'
    $usb = @(Dispositivos | Where-Object { $_ -notmatch ':' })
    if ($usb.Count -eq 0) { throw "Enchufá el casco por USB (y aceptá la depuración) para activar ADB por Wi-Fi." }
    $serie = $usb[0]
    $linea = & $adb -s $serie shell ip -f inet addr show wlan0 2>$null | Select-String 'inet (\d+\.\d+\.\d+\.\d+)'
    if (-not $linea) { throw "El casco no tiene Wi-Fi conectado." }
    $ip = $linea.Matches[0].Groups[1].Value
    & $adb -s $serie tcpip 5555 | Out-Host
    Start-Sleep 3
    & $adb connect "${ip}:5555" | Out-Host
    Set-Content -Path $ipGuardada -Value "${ip}:5555" -Encoding ascii
    Write-Host "ADB por Wi-Fi listo: ${ip}:5555 (ya podés desenchufar el cable)." -ForegroundColor Green
}

# Elige el casco: por USB si está enchufado (más rápido), si no por Wi-Fi; si no
# aparece ninguno, intenta reconectar a la IP guardada.
function Elegir-Casco {
    # adb escribe avisos en stderr (p. ej. al iniciar el daemon): no son errores.
    $ErrorActionPreference = 'Continue'
    $lista = @(Dispositivos)
    if ($lista.Count -eq 0 -and (Test-Path $ipGuardada)) {
        $destino = (Get-Content $ipGuardada -Raw).Trim()
        Write-Host ">> adb connect $destino" -ForegroundColor Cyan
        & $adb connect $destino | Out-Host
        Start-Sleep 1
        $lista = @(Dispositivos)
    }
    if ($lista.Count -eq 0) { throw "No hay ningún casco conectado (ni por USB ni por Wi-Fi). Probá con -Wifi enchufado por USB." }
    $usb = @($lista | Where-Object { $_ -notmatch ':' })
    $elegido = if ($usb.Count -gt 0) { $usb[0] } else { $lista[0] }
    Write-Host "Casco: $elegido" -ForegroundColor Green
    return $elegido
}

function Ejecutar([string]$exe, [string[]]$argumentos, [switch]$PermitirFallo) {
    Write-Host ">> $(Split-Path -Leaf $exe) $($argumentos -join ' ')" -ForegroundColor Cyan
    $previo = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    & $exe @argumentos 2>&1 | ForEach-Object { "$_" }
    $codigo = $LASTEXITCODE
    $ErrorActionPreference = $previo
    if ($codigo -ne 0 -and -not $PermitirFallo) { throw "Falló ($codigo): $exe" }
}

if ($Wifi) {
    Configurar-Wifi
    if (-not ($Instalar -or $Ejecutar -or $Logcat)) { return }
}

$soloAdb = ($Wifi -or $Logcat) -and -not $Instalar -and -not $Ejecutar
if (-not $soloAdb) {
    if (-not $SinRust) { & (Join-Path $PSScriptRoot 'build_rust.ps1') }

    Ejecutar $godot @('--headless', '--path', $proyecto, '--import') -PermitirFallo

    $argumentos = @('--headless', '--path', $proyecto)
    $version = Join-Path $proyecto 'android\.build_version'
    if (-not (Test-Path $version) -or ((Get-Content $version -Raw).Trim() -ne '4.7.2.stable')) {
        Write-Host "Instalando la plantilla de Android (Gradle)..." -ForegroundColor Yellow
        $argumentos += '--install-android-build-template'
    }
    if (Test-Path $apk) { Remove-Item $apk -Force }
    $argumentos += @('--export-debug', "`"Meta Quest 3`"", "`"$apk`"")
    Write-Host ">> godot $($argumentos -join ' ')" -ForegroundColor Cyan
    # Godot a veces queda colgado al cerrarse después de exportar: si el APK ya
    # está completo (sin cambios hace 20 s) y el proceso sigue, se lo cierra.
    $registro = Join-Path $salida 'exportar_godot.txt'
    $p = Start-Process -FilePath $godot -ArgumentList $argumentos -PassThru -NoNewWindow `
        -RedirectStandardOutput $registro -RedirectStandardError "$registro.err"
    $estable = 0; $previo = -1
    while (-not $p.HasExited) {
        Start-Sleep 5
        if (Test-Path $apk) {
            $tam = (Get-Item $apk).Length
            if ($tam -eq $previo) { $estable += 5 } else { $estable = 0; $previo = $tam }
            if ($estable -ge 20) { Write-Host "Godot no terminó solo: se cierra (el APK ya está)." -ForegroundColor Yellow; Stop-Process -Id $p.Id -Force; break }
        }
    }
    Get-Content $registro, "$registro.err" -ErrorAction SilentlyContinue | Select-String -Pattern 'ERROR|FAILURE|What went wrong|BUILD' | ForEach-Object { "$_" }
    if (-not (Test-Path $apk)) { throw "No se generó el APK (revisá $registro)." }
    $mb = [math]::Round((Get-Item $apk).Length / 1MB, 1)
    Write-Host "APK: $apk ($mb MB)" -ForegroundColor Green
}

if ($Instalar -or $Ejecutar -or $Logcat) { $casco = Elegir-Casco }
if ($Instalar) {
    Ejecutar $adb @('-s', $casco, 'install', '-r', $apk)
}
if ($Ejecutar) {
    Ejecutar $adb @('-s', $casco, 'shell', 'am', 'force-stop', $paquete) -PermitirFallo
    Ejecutar $adb @('-s', $casco, 'logcat', '-c') -PermitirFallo
    Ejecutar $adb @('-s', $casco, 'shell', 'am', 'start', '-n', "$paquete/com.godot.game.GodotAppLauncher")
    Write-Host "Ponete el casco: la app se abrió en el Quest." -ForegroundColor Green
}
if ($Logcat) {
    Ejecutar $adb @('-s', $casco, 'logcat', '-d', '-s', 'godot:V') -PermitirFallo
}
