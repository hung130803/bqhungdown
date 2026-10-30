@echo off
REM Chay bot Telegram BQHungDown. Lan dau: pip install -r requirements.txt
cd /d "%~dp0"
if not exist config.json (
    copy config.example.json config.json >nul
    echo Da tao config.json — mo file nay, dien telegram_token roi chay lai.
    pause
    exit /b 1
)
:loop
python bot.py
echo Bot dung (ma %errorlevel%). Tu chay lai sau 5 giay... (Ctrl+C de thoat)
timeout /t 5 /nobreak >nul
goto loop
