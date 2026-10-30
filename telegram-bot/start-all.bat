@echo off
REM Bat app BQHungDown (neu chua chay) roi bat bot Telegram (neu chua chay).
REM Dung cho khoi dong cung Windows: shortcut nam trong shell:startup.
cd /d "%~dp0"
set "APP=%LOCALAPPDATA%\BQHungDown\bqhungdown.exe"
if not exist "%APP%" set "APP=%~dp0..\src-tauri\target\release\bqhungdown.exe"
tasklist /FI "IMAGENAME eq bqhungdown.exe" /NH 2>nul | find /I "bqhungdown.exe" >nul
if errorlevel 1 if exist "%APP%" start "" "%APP%"
REM Cho app mo cong dieu khien truoc (bot van tu noi lai neu app cham).
timeout /t 15 /nobreak >nul
REM Moi luc chi 1 bot (2 bot cung token -> Telegram bao loi 409).
tasklist /V /FI "WINDOWTITLE eq BQHungDown bot*" /NH 2>nul | find /I "cmd.exe" >nul
if errorlevel 1 start "BQHungDown bot" /min cmd /c run-bot.bat
