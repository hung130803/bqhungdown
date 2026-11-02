# Cho bot Telegram tu chay khi bat may (dat loi tat vao thu muc Startup cua Windows).
#   Bat:  powershell -ExecutionPolicy Bypass -File tu_chay_khi_bat_may.ps1
#   Tat:  powershell -ExecutionPolicy Bypass -File tu_chay_khi_bat_may.ps1 -Tat
# Bot chay ngam (pythonw), nhe ~40MB; app chua mo thi bam "Mo app" tren Telegram.
# App mo sau se thay bot dang chay (khoa logs\bot.lock) va khong bat them ban thu 2.
param([switch]$Tat)

$lnk = Join-Path ([Environment]::GetFolderPath('Startup')) 'BQHungDown - bot Telegram.lnk'
if ($Tat) {
    Remove-Item $lnk -ErrorAction SilentlyContinue
    Write-Host "Da TAT tu chay bot khi bat may."
    return
}
$reg = Get-Content (Join-Path $env:APPDATA 'com.prodown.app\telegram_bot.json') -Raw | ConvertFrom-Json
$sh = New-Object -ComObject WScript.Shell
$s = $sh.CreateShortcut($lnk)
$s.TargetPath = $reg.python
$s.Arguments = $reg.script
$s.WorkingDirectory = $reg.dir
$s.Description = 'Bot Telegram dieu khien BQHungDown'
$s.Save()
Write-Host "Da BAT: bot tu chay khi bat may -> $lnk"
