@echo off
rem Compila Rust, exporta el APK, lo instala en el Meta Quest 3 (USB) y lo abre.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\exportar_quest.ps1" -Instalar -Ejecutar
pause
