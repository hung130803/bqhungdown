@echo off
echo === Kiem tra process BQHungDown / yt-dlp / ffmpeg / aria2c ===
REM Moi /FI cung luc la AND -> danh sach luon rong. Kiem tung ten mot.
for %%P in (bqhungdown.exe yt-dlp.exe ffmpeg.exe aria2c.exe) do tasklist /FI "IMAGENAME eq %%P" /NH 2>nul | find /I "%%P"
echo.
echo === Dang kill tat ca... ===
taskkill /F /IM bqhungdown.exe /T 2>nul
taskkill /F /IM yt-dlp.exe /T 2>nul
taskkill /F /IM ffmpeg.exe /T 2>nul
taskkill /F /IM aria2c.exe /T 2>nul
echo.
echo Hoan tat. Process da bi kill (neu co).
pause
