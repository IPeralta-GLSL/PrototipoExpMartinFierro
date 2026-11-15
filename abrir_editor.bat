@echo off
rem Abre el proyecto en el editor de Godot 4.7.2
start "" "%LOCALAPPDATA%\Programs\Godot\Godot_v4.7.2-stable_win64.exe" --path "%~dp0godot" -e
