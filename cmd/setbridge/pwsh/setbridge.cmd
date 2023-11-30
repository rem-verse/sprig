@echo off

setlocal
cd /d "%~dp0"
powershell -executionpolicy bypass ".\setbridge.ps1" %*