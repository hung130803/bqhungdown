@echo off
REM ====================================================================
REM  release.bat <version>
REM  Vi du:  release.bat 0.1.2
REM ====================================================================

if "%~1"=="" (
    echo.
    echo Cu phap:  release.bat ^<version^>
    echo Vi du:    release.bat 0.1.2
    echo.
    exit /b 1
)

set "VER=%~1"

echo.
echo ============================================================
echo  Release BQHungDown v%VER%
echo ============================================================
echo.

REM ── [1/5] Bump 3 file version ───────────────────────────────────
echo [1/5] Bump version den %VER%...
node scripts/bump-version.mjs %VER%
if errorlevel 1 (
    echo [LOI] Bump version that bai
    exit /b 1
)

REM ── [2/5] Fetch yt-dlp moi nhat ─────────────────────────────────
echo.
echo [2/5] Fetch yt-dlp moi nhat...
call npm run update:ytdlp
if errorlevel 1 (
    echo [CANH BAO] Fetch yt-dlp that bai. Action GitHub se tu fetch khi build.
)

REM ── [3/5] Git commit ────────────────────────────────────────────
echo.
echo [3/5] Git commit...
git add -A
git commit -m "v%VER%"
echo    OK (bo qua neu khong co thay doi).

REM ── [4/5] Tag ───────────────────────────────────────────────────
echo.
echo [4/5] Tao tag v%VER%...
git tag v%VER%
if errorlevel 1 (
    echo [LOI] Tag co the da ton tai.
    exit /b 1
)

REM ── [5/5] Push ──────────────────────────────────────────────────
echo.
echo [5/5] Push len GitHub...
git push
if errorlevel 1 (
    echo [LOI] Push code that bai.
    exit /b 1
)
git push origin v%VER%
if errorlevel 1 (
    echo [LOI] Push tag that bai.
    exit /b 1
)

echo.
echo ============================================================
echo  Xong! GitHub Action dang build v%VER%.
echo  Theo doi tai: https://github.com/hung130803/bqhungdown/actions
echo  Sau ~10 phut, user se thay banner "Co ban moi" trong app.
echo ============================================================
echo.
