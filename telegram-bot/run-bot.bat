@echo off
REM Chay bot Telegram BQHungDown. Lan dau: pip install -r requirements.txt
cd /d "%~dp0"
if not exist config.json (
    copy config.example.json config.json >nul
    echo Da tao config.json - mo file nay, dien telegram_token roi chay lai.
    pause
    exit /b 1
)
:loop
python bot.py
REM Ma 3 = loi cau hinh (thieu/sai token, JSON hong): DUNG, khong chay lai vo han.
if %errorlevel%==3 (
    echo Loi cau hinh - sua config.json roi chay lai. Xem logs\bot.log
    pause
    exit /b 3
)
REM Ma 4 = da co bot dang chay (thuong do app tu bat): khong mo them ban nua.
if %errorlevel%==4 (
    echo Bot da dang chay roi (app tu bat bot khi mo). Khong can mo them.
    timeout /t 5 >nul
    exit /b 0
)
echo Bot dung (ma %errorlevel%). Tu chay lai sau 5 giay... (Ctrl+C de thoat)
timeout /t 5 /nobreak >nul
goto loop
