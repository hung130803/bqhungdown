@echo off
setlocal enabledelayedexpansion

echo === BQHungDown Cache Cleanup ===
echo.
set /p folder="Nhap duong dan folder can don rac (Enter de bo qua): "
if "%folder%"=="" goto :end

if not exist "%folder%" (
    echo Folder khong ton tai: %folder%
    goto :end
)

echo.
echo Dang quet rac trong %folder% ...

set count=0
for %%E in (part ytdl frag temp) do (
    for %%F in ("%folder%\*.%%E" "%folder%\*.%%E.*") do (
        if exist "%%~F" (
            echo   Xoa: %%~nxF
            del /F /Q "%%~F" 2>nul
            set /a count+=1
        )
    )
)

rem yt-dlp temp files like "video.f137.mp4.part", "video.temp.mp4"
for %%F in ("%folder%\*.f*.part") do (
    if exist "%%~F" (
        echo   Xoa: %%~nxF
        del /F /Q "%%~F" 2>nul
        set /a count+=1
    )
)
for %%F in ("%folder%\*.temp.*") do (
    if exist "%%~F" (
        echo   Xoa: %%~nxF
        del /F /Q "%%~F" 2>nul
        set /a count+=1
    )
)

echo.
echo Da xoa !count! file rac.

:end
echo.
pause
endlocal
