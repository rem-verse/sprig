@echo off

setlocal
cd /d "%~dp0"
powershell -executionpolicy bypass ".\getbridgetype.ps1" %*