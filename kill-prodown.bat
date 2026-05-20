@echo off
echo === Kiem tra process BQHungDown / yt-dlp / ffmpeg / aria2c ===
tasklist /FI "IMAGENAME eq bqhungdown.exe" /FI "IMAGENAME eq yt-dlp.exe" /FI "IMAGENAME eq ffmpeg.exe" /FI "IMAGENAME eq aria2c.exe"
echo.
echo === Dang kill tat ca... ===
taskkill /F /IM bqhungdown.exe /T 2>nul
taskkill /F /IM yt-dlp.exe /T 2>nul
taskkill /F /IM ffmpeg.exe /T 2>nul
taskkill /F /IM aria2c.exe /T 2>nul
echo.
echo Hoan tat. Process da bi kill (neu co).
pause
