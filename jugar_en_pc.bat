@echo off
rem Prueba la escena en la PC sin casco: WASD para caminar, boton derecho del mouse para mirar,
rem Shift para correr y G para rasguear la guitarra.
start "" "%LOCALAPPDATA%\Programs\Godot\Godot_v4.7.2-stable_win64.exe" --path "%~dp0godot" --xr-mode off -- --escritorio
