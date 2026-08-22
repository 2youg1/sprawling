@echo off
rem Raises a city in this folder if there is not one yet, serves it, and
rem opens the WebUI. This window is the city: closing it stops the city.
cd /d "%~dp0"
sprawling.exe up
if errorlevel 1 pause
