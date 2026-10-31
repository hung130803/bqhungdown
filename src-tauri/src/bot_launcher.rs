//! TỰ BẬT BOT TELEGRAM khi mở app.
//!
//! Bot (`telegram-bot/`, Python) chạy lần đầu sẽ ghi `<config>/telegram_bot.json`
//! (thư mục bot + pythonw.exe). Từ đó mỗi lần mở app, app bật bot chạy NGẦM
//! (không cửa sổ) và CANH SUỐT ĐỜI APP: cứ `CHECK_EVERY` xem khoá
//! `<bot>/logs/bot.lock` — còn ai giữ khoá = bot đang sống (dù do app, do
//! run-bot.bat hay do lần mở app trước bật); khoá trống = bot đã chết → bật lại.
//!
//! LỖI CŨ (31/10/2026): thấy "đã có bot" (mã 4) là app THÔI canh luôn → bot
//! đó văng sau (máy ngủ dậy, phiên terminal đóng…) thì không ai bật lại.
//! Mã 3 = lỗi cấu hình (thiếu/sai token) → thử lại thưa (5 phút), chờ người dùng sửa.
//! Tắt tự bật: sửa `"enabled": false` trong telegram_bot.json.
//!
//! Tắt app KHÔNG tắt bot — bot còn chạy để báo "app đã tắt" lên Telegram.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Mã thoát của bot: đã có bản khác chạy / lỗi cấu hình (xem bqbot/launcher.py, config.py).
const EXIT_ALREADY_RUNNING: i32 = 4;
const EXIT_CONFIG_ERROR: i32 = 3;
const RESTART_DELAY: Duration = Duration::from_secs(15);
/// Văng liên tục (vd máy mất mạng lúc khởi động bot) thì nghỉ lâu hơn.
const MAX_RESTART_DELAY: Duration = Duration::from_secs(300);
/// Nhịp kiểm tra bot còn sống (đọc khoá file — gần như không tốn gì).
const CHECK_EVERY: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct BotRegistration {
    #[serde(default = "yes")]
    pub enabled: bool,
    pub dir: PathBuf,
    pub python: PathBuf,
    #[serde(default = "default_script")]
    pub script: String,
}

fn yes() -> bool {
    true
}
fn default_script() -> String {
    "bot.py".into()
}

/// Đọc đăng ký; None nếu chưa có / tắt / thiếu file thật.
pub fn read_registration(config_dir: &Path) -> Option<BotRegistration> {
    let text = std::fs::read_to_string(config_dir.join("telegram_bot.json")).ok()?;
    let reg: BotRegistration = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    if !reg.enabled || !reg.python.is_file() || !reg.dir.join(&reg.script).is_file() {
        return None;
    }
    Some(reg)
}

/// Chờ bao lâu rồi xét lại sau khi bot (do app bật) thoát. App KHÔNG bao giờ
/// thôi canh — chỉ giãn nhịp.
fn next_delay(exit_code: Option<i32>, prev: Duration, ran_for: Duration) -> Duration {
    match exit_code {
        // Có bot khác đang chạy → quay về canh khoá theo nhịp thường.
        Some(EXIT_ALREADY_RUNNING) => CHECK_EVERY,
        // Sai/thiếu token: thử thưa, người dùng sửa config xong là tự lên.
        Some(EXIT_CONFIG_ERROR) => MAX_RESTART_DELAY,
        _ => {
            // Chạy ổn được lâu rồi mới văng → bật lại nhanh; văng ngay → giãn dần.
            if ran_for > Duration::from_secs(600) {
                RESTART_DELAY
            } else {
                (prev * 2).clamp(RESTART_DELAY, MAX_RESTART_DELAY)
            }
        }
    }
}

/// Bot còn sống không? Bot (Python) khoá byte đầu `logs/bot.lock` suốt đời
/// tiến trình. Thử khoá cả file: được = không ai giữ = bot đã chết (nhả ngay).
fn bot_alive(reg: &BotRegistration) -> bool {
    let path = reg.dir.join("logs").join("bot.lock");
    let Ok(f) = std::fs::OpenOptions::new().read(true).write(true).open(&path) else {
        return false; // chưa từng chạy / đã dọn
    };
    match f.try_lock() {
        Ok(()) => {
            let _ = f.unlock();
            false
        }
        Err(std::fs::TryLockError::WouldBlock) => true,
        Err(_) => false,
    }
}

fn spawn_once(reg: &BotRegistration) -> std::io::Result<std::process::Child> {
    let mut cmd = std::process::Command::new(&reg.python);
    cmd.arg(&reg.script).current_dir(&reg.dir);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: chạy ngầm, không bật cửa sổ đen.
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn()
}

/// Gọi 1 lần lúc mở app; canh tới khi app tắt.
pub fn spawn_supervisor(config_dir: PathBuf) {
    std::thread::spawn(move || {
        let mut delay = RESTART_DELAY;
        loop {
            // Đọc lại mỗi vòng: người dùng tắt "enabled" / dời thư mục bot /
            // bot đăng ký lần đầu sau khi app đã mở — đều theo được.
            let Some(reg) = read_registration(&config_dir) else {
                std::thread::sleep(CHECK_EVERY);
                continue;
            };
            if bot_alive(&reg) {
                delay = RESTART_DELAY;
                std::thread::sleep(CHECK_EVERY);
                continue;
            }
            let started = std::time::Instant::now();
            let code = match spawn_once(&reg) {
                Ok(mut child) => child.wait().ok().and_then(|s| s.code()),
                Err(e) => {
                    eprintln!("[bot] không bật được bot: {e}");
                    None
                }
            };
            delay = next_delay(code, delay, started.elapsed());
            std::thread::sleep(delay);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_requires_real_files_and_enabled() {
        let dir = std::env::temp_dir().join(format!("bqd-botreg-{}", std::process::id()));
        let bot = dir.join("bot");
        std::fs::create_dir_all(&bot).unwrap();
        std::fs::write(bot.join("bot.py"), "").unwrap();
        let py = dir.join("pythonw.exe");
        std::fs::write(&py, "").unwrap();
        let write = |enabled: bool| {
            let j = serde_json::json!({ "enabled": enabled, "dir": bot, "python": py });
            std::fs::write(dir.join("telegram_bot.json"), format!("\u{feff}{j}")).unwrap();
        };
        write(true);
        assert_eq!(read_registration(&dir).unwrap().script, "bot.py");
        write(false);
        assert!(read_registration(&dir).is_none(), "người dùng tắt thì không bật");
        write(true);
        std::fs::remove_file(bot.join("bot.py")).unwrap();
        assert!(read_registration(&dir).is_none(), "dời/xoá thư mục bot thì thôi");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restart_policy_never_gives_up() {
        let s = Duration::from_secs(1);
        assert_eq!(next_delay(Some(EXIT_ALREADY_RUNNING), RESTART_DELAY, s), CHECK_EVERY,
                   "đã có bot → vẫn canh tiếp theo nhịp thường");
        assert_eq!(next_delay(Some(EXIT_CONFIG_ERROR), RESTART_DELAY, s), MAX_RESTART_DELAY,
                   "sai cấu hình → thử thưa, không bỏ hẳn");
        assert_eq!(next_delay(Some(1), RESTART_DELAY, s), RESTART_DELAY * 2, "văng ngay → giãn dần");
        assert_eq!(next_delay(Some(1), MAX_RESTART_DELAY, s), MAX_RESTART_DELAY);
        assert_eq!(next_delay(None, MAX_RESTART_DELAY, Duration::from_secs(3600)), RESTART_DELAY,
                   "chạy lâu rồi mới văng → bật lại nhanh");
    }

    /// Khoá của bot Python (msvcrt.locking 1 byte) phải làm `bot_alive` trả true.
    #[test]
    fn detects_python_bot_lock() {
        let dir = std::env::temp_dir().join(format!("bqd-botlock-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("logs")).unwrap();
        let reg = BotRegistration { enabled: true, dir: dir.clone(), python: dir.clone(), script: "bot.py".into() };
        assert!(!bot_alive(&reg), "chưa có file khoá → bot chưa chạy");
        let lock = dir.join("logs").join("bot.lock");
        std::fs::write(&lock, "").unwrap();
        assert!(!bot_alive(&reg), "có file nhưng không ai giữ → bot đã chết");
        let script = "import msvcrt,sys,time\nf=open(sys.argv[1],'a+');f.seek(0);msvcrt.locking(f.fileno(),msvcrt.LK_NBLCK,1)\nprint('LOCKED',flush=True);time.sleep(8)\n";
        let py = std::process::Command::new("python").args(["-c", script]).arg(&lock)
            .stdout(std::process::Stdio::piped()).spawn();
        let Ok(mut child) = py else { return }; // máy không có Python → bỏ qua phần này
        let mut out = child.stdout.take().unwrap();
        let mut buf = [0u8; 6];
        use std::io::Read;
        if out.read_exact(&mut buf).is_ok() && &buf == b"LOCKED" {
            assert!(bot_alive(&reg), "bot Python đang giữ khoá → còn sống");
        }
        let _ = child.kill();
        let _ = child.wait();
        assert!(!bot_alive(&reg), "bot tắt → khoá trống");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
