//! TỰ BẬT BOT TELEGRAM khi mở app.
//!
//! Bot (`telegram-bot/`, Python) chạy lần đầu sẽ ghi `<config>/telegram_bot.json`
//! (thư mục bot + pythonw.exe). Từ đó mỗi lần mở app, app bật bot chạy NGẦM
//! (không cửa sổ) và canh: bot văng thì bật lại sau 15 giây.
//!
//! Bot tự giữ khoá "chỉ 1 bản" — đã có bot chạy (bật tay bằng run-bot.bat)
//! thì bản app bật thoát ngay với mã 4 → app thôi, không bật lại vòng vòng.
//! Mã 3 = lỗi cấu hình (thiếu/sai token) → cũng thôi, chờ người dùng sửa.
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

/// Quyết định sau khi bot thoát: Some(chờ bao lâu rồi bật lại) / None = thôi.
fn next_delay(exit_code: Option<i32>, prev: Duration, ran_for: Duration) -> Option<Duration> {
    match exit_code {
        Some(EXIT_ALREADY_RUNNING) | Some(EXIT_CONFIG_ERROR) => None,
        _ => {
            // Chạy ổn được lâu rồi mới văng → bật lại nhanh; văng ngay → giãn dần.
            if ran_for > Duration::from_secs(600) {
                Some(RESTART_DELAY)
            } else {
                Some((prev * 2).clamp(RESTART_DELAY, MAX_RESTART_DELAY))
            }
        }
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

/// Gọi 1 lần lúc mở app. Không có đăng ký → không làm gì.
pub fn spawn_supervisor(config_dir: PathBuf) {
    std::thread::spawn(move || {
        let mut delay = RESTART_DELAY;
        loop {
            // Đọc lại mỗi vòng: người dùng tắt "enabled" / dời thư mục bot thì theo.
            let Some(reg) = read_registration(&config_dir) else { return };
            let started = std::time::Instant::now();
            let code = match spawn_once(&reg) {
                Ok(mut child) => child.wait().ok().and_then(|s| s.code()),
                Err(e) => {
                    eprintln!("[bot] không bật được bot: {e}");
                    None
                }
            };
            match next_delay(code, delay, started.elapsed()) {
                Some(d) => {
                    delay = d;
                    std::thread::sleep(d);
                }
                None => return,
            }
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
    fn restart_policy() {
        let s = Duration::from_secs(1);
        assert_eq!(next_delay(Some(EXIT_ALREADY_RUNNING), RESTART_DELAY, s), None, "đã có bot → thôi");
        assert_eq!(next_delay(Some(EXIT_CONFIG_ERROR), RESTART_DELAY, s), None, "sai cấu hình → thôi");
        assert_eq!(next_delay(Some(1), RESTART_DELAY, s), Some(RESTART_DELAY * 2), "văng ngay → giãn dần");
        assert_eq!(next_delay(Some(1), MAX_RESTART_DELAY, s), Some(MAX_RESTART_DELAY));
        assert_eq!(next_delay(None, MAX_RESTART_DELAY, Duration::from_secs(3600)), Some(RESTART_DELAY),
                   "chạy lâu rồi mới văng → bật lại nhanh");
    }
}
