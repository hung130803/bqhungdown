//! MỞ CÙNG WINDOWS (dây chuyền #3, 04/11/2026): lối tắt `BQHungDown.lnk` trong
//! thư mục Startup của user, chạy exe với `--minimized` (app lên thẳng khay,
//! không bật cửa sổ). Công tắc `Settings.mo_cung_windows` mặc định TẮT.
//!
//! Mở lần 2 sẽ đụng khoá 1-phiên (`instance_lock`) và bật hộp thoại — vì vậy
//! bot `/openapp` kiểm tra tiến trình app trước khi mở (launcher.app_dang_chay).

use std::path::{Path, PathBuf};

/// Tham số dòng lệnh: mở app ẩn cửa sổ, chỉ hiện ở khay.
pub const ARG_MINIMIZED: &str = "--minimized";
const TEN_LOI_TAT: &str = "BQHungDown.lnk";

pub fn mo_thu_nho() -> bool {
    std::env::args().any(|a| a == ARG_MINIMIZED)
}

/// `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\BQHungDown.lnk`
pub fn duong_dan_loi_tat() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join(r"Microsoft\Windows\Start Menu\Programs\Startup").join(TEN_LOI_TAT))
}

/// Chuỗi trong nháy đơn của PowerShell: `'` nhân đôi.
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Script PowerShell tạo lối tắt (WScript.Shell) — hàm thuần để test.
pub fn script_tao(lnk: &Path, exe: &Path) -> String {
    let dir = exe.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut({});$s.TargetPath={};$s.Arguments={};$s.WorkingDirectory={};$s.WindowStyle=7;$s.Save()",
        ps_quote(&lnk.to_string_lossy()),
        ps_quote(&exe.to_string_lossy()),
        ps_quote(ARG_MINIMIZED),
        ps_quote(&dir),
    )
}

/// Tạo lối tắt tới `exe` tại `lnk` (ghi đè). Chạy PowerShell không cửa sổ.
pub fn tao(lnk: &Path, exe: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        if let Some(p) = lnk.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script_tao(lnk, exe)])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() && lnk.exists() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (lnk, exe);
        Err("chỉ hỗ trợ Windows".into())
    }
}

static LOI: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Lỗi tạo lối tắt lần gần nhất (None = ổn) — bot báo qua `/api/daychuyen`.
pub fn loi_gan_nhat() -> Option<String> {
    LOI.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn dat_loi(v: Option<String>) {
    *LOI.lock().unwrap_or_else(|e| e.into_inner()) = v;
}

/// Đồng bộ lối tắt theo công tắc: bật → tạo/cập nhật (trỏ exe đang chạy), tắt
/// → xoá đúng file lối tắt của app. Lỗi ghi log + giữ lại cho bot báo, không
/// ảnh hưởng việc chính.
pub fn dong_bo(bat: bool) {
    let Some(lnk) = duong_dan_loi_tat() else {
        if bat {
            dat_loi(Some("không tìm thấy thư mục Startup (%APPDATA%)".into()));
        }
        return;
    };
    dat_loi(None);
    if bat {
        match std::env::current_exe() {
            Ok(exe) => {
                if let Err(e) = tao(&lnk, &exe) {
                    eprintln!("[startup] không tạo được lối tắt mở cùng Windows: {e}");
                    dat_loi(Some(e));
                }
            }
            Err(e) => {
                eprintln!("[startup] không biết đường dẫn exe: {e}");
                dat_loi(Some(format!("không biết đường dẫn exe: {e}")));
            }
        }
    } else if lnk.exists() {
        if let Err(e) = std::fs::remove_file(&lnk) {
            eprintln!("[startup] không xoá được lối tắt: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loi_tao_loi_tat_giu_cho_bot_bao_va_xoa_khi_on() {
        dat_loi(Some("access denied".into()));
        assert_eq!(loi_gan_nhat().as_deref(), Some("access denied"));
        dat_loi(None);
        assert_eq!(loi_gan_nhat(), None);
    }

    #[test]
    fn script_tao_loi_tat_co_minimized_va_thoat_nhay_don() {
        let s = script_tao(Path::new(r"C:\U\Startup\BQHungDown.lnk"), Path::new(r"C:\Anh's App\bqhungdown.exe"));
        assert!(s.contains(r"$s.TargetPath='C:\Anh''s App\bqhungdown.exe'"), "{s}");
        assert!(s.contains("$s.Arguments='--minimized'"));
        assert!(s.contains(r"$s.WorkingDirectory='C:\Anh''s App'"));
        let lnk = duong_dan_loi_tat().unwrap();
        assert!(lnk.ends_with(r"Start Menu\Programs\Startup\BQHungDown.lnk"));
    }

    #[cfg(windows)]
    #[test]
    fn tao_loi_tat_that_trong_thu_muc_tam() {
        let d = std::env::temp_dir().join(format!("bqd-startup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let lnk = d.join("BQHungDown.lnk");
        let exe = std::env::current_exe().unwrap();
        tao(&lnk, &exe).expect("tạo lối tắt");
        assert!(std::fs::metadata(&lnk).unwrap().len() > 0);
        std::fs::remove_dir_all(d).ok();
    }
}
