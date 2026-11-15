<#
.SYNOPSIS
  Compila la extensión Rust (gdext) para PC (editor) y para Meta Quest 3 (Android arm64)
  y copia las bibliotecas a godot/bin.

.EXAMPLE
  .\tools\build_rust.ps1            # PC debug + Quest release
  .\tools\build_rust.ps1 -SoloPC    # solo la DLL del editor
  .\tools\build_rust.ps1 -SoloQuest # solo la .so del Quest
  .\tools\build_rust.ps1 -Release   # además, DLL release de PC
#>
param(
    [switch]$SoloPC,
    [switch]$SoloQuest,
    [switch]$Release,
    [switch]$Tests
)

$ErrorActionPreference = 'Stop'
$raiz = Split-Path -Parent $PSScriptRoot
$rust = Join-Path $raiz 'rust'
$bin = Join-Path $raiz 'godot\bin'
New-Item -ItemType Directory -Force $bin | Out-Null

$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (-not (Test-Path (Join-Path $cargoBin 'cargo.exe'))) { throw "No se encontró cargo en $cargoBin. Instalá Rust con rustup." }
if ($env:Path -notlike "*$cargoBin*") { $env:Path = "$cargoBin;$env:Path" }

function Invocar-Cargo([string[]]$argumentos) {
    Write-Host ">> cargo $($argumentos -join ' ')" -ForegroundColor Cyan
    # cargo escribe su progreso por stderr; en PowerShell 5.1 eso no debe
    # contarse como error: se decide solo por el código de salida.
    $previo = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    & cargo @argumentos 2>&1 | ForEach-Object { "$_" }
    $codigo = $LASTEXITCODE
    $ErrorActionPreference = $previo
    if ($codigo -ne 0) { throw "cargo falló ($codigo): cargo $($argumentos -join ' ')" }
}

function Copiar([string]$origen, [string]$destino) {
    if (-not (Test-Path $origen)) { throw "No existe $origen" }
    try {
        Copy-Item $origen $destino -Force
    } catch {
        # El editor de Godot puede tener la DLL abierta: se copia con otro nombre
        # y Godot la recarga al volver a enfocar la ventana (reloadable = true).
        Write-Warning "No se pudo sobrescribir $destino (¿editor abierto?): $($_.Exception.Message)"
        throw
    }
    $mb = [math]::Round((Get-Item $destino).Length / 1MB, 2)
    Write-Host "   -> $destino ($mb MB)" -ForegroundColor Green
}

Push-Location $rust
try {
    if ($Tests) { Invocar-Cargo @('test', '--lib') }

    if (-not $SoloQuest) {
        Invocar-Cargo @('build')
        Copiar (Join-Path $rust 'target\debug\martin_fierro.dll') (Join-Path $bin 'martin_fierro.windows.debug.x86_64.dll')
        # Símbolos de depuración (Godot los busca junto a la DLL para la recarga en caliente).
        $pdb = Join-Path $rust 'target\debug\martin_fierro.pdb'
        if (Test-Path $pdb) { Copy-Item $pdb (Join-Path $bin 'martin_fierro.pdb') -Force -ErrorAction SilentlyContinue }
        if ($Release) {
            Invocar-Cargo @('build', '--release')
            Copiar (Join-Path $rust 'target\release\martin_fierro.dll') (Join-Path $bin 'martin_fierro.windows.release.x86_64.dll')
        }
    }

    if (-not $SoloPC) {
        $sdk = $env:ANDROID_HOME
        if (-not $sdk) { $sdk = Join-Path $env:LOCALAPPDATA 'Android\Sdk' }
        $ndk = Get-ChildItem (Join-Path $sdk 'ndk') -Directory -ErrorAction SilentlyContinue |
            Sort-Object { [version]($_.Name -replace '[^0-9.]', '') } -Descending | Select-Object -First 1
        if (-not $ndk) { throw "No se encontró el Android NDK en $sdk\ndk" }
        $tc = Join-Path $ndk.FullName 'toolchains\llvm\prebuilt\windows-x86_64\bin'
        $env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER = Join-Path $tc 'aarch64-linux-android29-clang.cmd'
        $env:AR_aarch64_linux_android = Join-Path $tc 'llvm-ar.exe'
        $env:CC_aarch64_linux_android = $env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER
        Write-Host "NDK: $($ndk.FullName)" -ForegroundColor DarkGray

        $instalados = & rustup target list --installed
        if ($instalados -notcontains 'aarch64-linux-android') { & rustup target add aarch64-linux-android }

        Invocar-Cargo @('build', '--release', '--target', 'aarch64-linux-android')
        Copiar (Join-Path $rust 'target\aarch64-linux-android\release\libmartin_fierro.so') (Join-Path $bin 'libmartin_fierro.android.arm64.so')
    }
}
finally {
    Pop-Location
}
Write-Host "Listo." -ForegroundColor Green
