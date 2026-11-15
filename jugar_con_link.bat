@echo off
rem Prueba en VR desde la PC con Meta Quest Link (cable o Air Link activo en el casco).
rem Es la forma mas rapida de iterar: no hace falta exportar el APK.
start "" "%LOCALAPPDATA%\Programs\Godot\Godot_v4.7.2-stable_win64.exe" --path "%~dp0godot" --xr-mode on
