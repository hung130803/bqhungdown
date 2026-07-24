//! Bộ đếm ƯỚC TÍNH quota YouTube Data API đã tiêu — THEO CHÍNH APP NÀY.
//!
//! LƯU Ý QUAN TRỌNG: YouTube KHÔNG có endpoint trả về "quota còn lại". Google
//! chỉ hiện trong Cloud Console. Nên đây là ƯỚC TÍNH: app tự cộng số đơn vị nó
//! tiêu cho từng key trong ngày. Chính xác NẾU key chỉ dùng bởi app này; nếu
//! key còn dùng nơi khác thì số thực đã tiêu sẽ cao hơn.
//!
//! Reset theo mốc của Google: nửa đêm giờ Thái Bình Dương (xấp xỉ UTC-8 — lệch
//! tối đa ~1h quanh mốc reset do DST, không đáng kể với một ước tính).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// Hạn mức mặc định 1 project YouTube Data API v3 (đơn vị/ngày).
pub const DAILY_QUOTA: u32 = 10_000;

#[derive(Default, Serialize, Deserialize)]
struct Usage {
    /// Ngày (giờ Thái Bình Dương) của số liệu — khác ngày thì reset về 0.
    day: String,
    /// key -> số đơn vị đã tiêu hôm nay.
    used: HashMap<String, u32>,
}

static STATE: OnceLock<Mutex<Usage>> = OnceLock::new();
static PATH: OnceLock<PathBuf> = OnceLock::new();

/// Ngày hiện tại theo giờ Thái Bình Dương (UTC-8 xấp xỉ) — YYYY-MM-DD.
fn pacific_day() -> String {
    let pt = chrono::Utc::now() - chrono::Duration::hours(8);
    pt.format("%Y-%m-%d").to_string()
}

/// Gọi 1 lần lúc khởi động: nạp số liệu đã lưu (nếu cùng ngày), set đường ghi.
pub fn init(path: PathBuf) {
    let mut u: Usage = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if u.day != pacific_day() {
        u = Usage { day: pacific_day(), used: HashMap::new() };
    }
    let _ = PATH.set(path);
    let _ = STATE.set(Mutex::new(u));
}

fn save(u: &Usage) {
    if let Some(p) = PATH.get() {
        if let Ok(json) = serde_json::to_string(u) {
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, json.as_bytes()).is_ok() {
                let _ = std::fs::rename(&tmp, p);
            }
        }
    }
}

/// Cộng `units` đã tiêu cho `key`. Tự reset khi sang ngày mới (giờ TBD).
pub fn add(key: &str, units: u32) {
    let Some(m) = STATE.get() else { return };
    let Ok(mut u) = m.lock() else { return };
    let today = pacific_day();
    if u.day != today {
        u.day = today;
        u.used.clear();
    }
    *u.used.entry(key.to_string()).or_insert(0) += units;
    save(&u);
}

/// Số đơn vị đã tiêu hôm nay cho từng key trong `keys` (đúng thứ tự truyền vào).
/// Kèm ngày số liệu để UI hiển thị "reset lúc…".
pub fn snapshot(keys: &[String]) -> (String, Vec<u32>) {
    let Some(m) = STATE.get() else {
        return (pacific_day(), keys.iter().map(|_| 0).collect());
    };
    let Ok(mut u) = m.lock() else {
        return (pacific_day(), keys.iter().map(|_| 0).collect());
    };
    let today = pacific_day();
    if u.day != today {
        u.day = today;
        u.used.clear();
    }
    let out = keys
        .iter()
        .map(|k| *u.used.get(k.trim()).unwrap_or(&0))
        .collect();
    (u.day.clone(), out)
}
