//! Bộ nhớ đệm danh sách video của kênh (YouTube Data API), mỗi kênh 1 file
//! `<channel_id>.json` trong app_data_dir/channel_cache.
//!
//! Mục đích: load lại 1 kênh đã xem → chỉ lấy thêm video MỚI (so với lần
//! trước) thay vì lấy lại từ đầu → tiết kiệm quota + gần như tức thì. View của
//! video cũ giữ số lần lấy gần nhất; muốn làm mới hết thì dùng "force_refresh".

use crate::models::ChannelVideo;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Phiên bản SƠ ĐỒ cache. Tăng số này khi CÁCH phân loại/đánh dấu video đổi
/// (vd: v2 sửa nhận diện Shorts theo tab + ngưỡng 90s). Kho lưu bởi bản CŨ có
/// version khác -> coi như MISS -> tự lấy lại 1 lần cho đúng. Nhờ vậy 300 máy
/// nhân viên có kho cũ mis-tag Shorts tự khỏi mà không cần bấm gì.
const CACHE_SCHEMA_VERSION: u32 = 4;

#[derive(Serialize, Deserialize)]
struct CachedChannel {
    /// Unix giây lúc lưu — chưa dùng để hết hạn, để dành cho sau.
    #[serde(default)]
    fetched_at: u64,
    /// Version sơ đồ — thiếu (kho đời đầu) = 0, khác hiện tại -> bỏ, lấy lại.
    #[serde(default)]
    schema_version: u32,
    videos: Vec<ChannelVideo>,
}

pub struct ChannelCache {
    dir: PathBuf,
}

impl ChannelCache {
    pub fn new(dir: PathBuf) -> Self {
        ChannelCache { dir }
    }

    /// Đường dẫn file cache cho `channel_id`. None nếu id không an toàn để
    /// làm tên file (chống path traversal) — chỉ nhận chữ/số/`-`/`_`.
    fn path_for(&self, channel_id: &str) -> Option<PathBuf> {
        if channel_id.is_empty()
            || !channel_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return None;
        }
        Some(self.dir.join(format!("{channel_id}.json")))
    }

    /// Đọc danh sách video đã cache. None nếu chưa có / lỗi đọc / id xấu.
    pub fn load(&self, channel_id: &str) -> Option<Vec<ChannelVideo>> {
        self.load_with_age(channel_id).map(|(v, _)| v)
    }

    /// Như `load` nhưng kèm TUỔI cache (giây từ lúc lưu) — UI hiện
    /// "kho lưu X trước" để user biết dữ liệu cũ mới thế nào.
    pub fn load_with_age(&self, channel_id: &str) -> Option<(Vec<ChannelVideo>, u64)> {
        let path = self.path_for(channel_id)?;
        let text = fs::read_to_string(&path).ok()?;
        let parsed: CachedChannel = serde_json::from_str(&text).ok()?;
        // Kho tạo bởi bản CŨ (version khác) -> bỏ qua để lấy lại 1 lần cho
        // đúng (nhất là phân loại Shorts). Coi như chưa có cache.
        if parsed.schema_version != CACHE_SCHEMA_VERSION {
            return None;
        }
        let age = now_unix().saturating_sub(parsed.fetched_at);
        Some((parsed.videos, age))
    }

    /// Ghi danh sách video xuống cache (atomic: tmp + rename). Lỗi → bỏ qua
    /// im lặng (cache chỉ là tối ưu, không được phép làm hỏng luồng chính).
    pub fn save(&self, channel_id: &str, videos: &[ChannelVideo]) {
        let path = match self.path_for(channel_id) {
            Some(p) => p,
            None => return,
        };
        if fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let data = CachedChannel {
            fetched_at: now_unix(),
            schema_version: CACHE_SCHEMA_VERSION,
            videos: videos.to_vec(),
        };
        let json = match serde_json::to_string(&data) {
            Ok(j) => j,
            Err(_) => return,
        };
        let tmp = path.with_extension("json.tmp");
        if fs::write(&tmp, json.as_bytes()).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
}

/// Khóa cache theo (URL kênh + tab) cho đường "lấy CẢ kênh" bất kể nguồn
/// (API hay yt-dlp) — fnv1a-64 hex, luôn ra tên file an toàn, có prefix
/// `full_` để không đụng khóa `UC…` của cache API tăng dần.
pub fn url_key(url: &str, tab: &str) -> String {
    let s = format!("{}|{}", url.trim().to_lowercase(), tab.trim().to_lowercase());
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("full_{h:016x}")
}

fn now_unix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vid(id: &str) -> ChannelVideo {
        ChannelVideo {
            url: format!("https://www.youtube.com/watch?v={id}"),
            title: id.to_string(),
            duration_sec: Some(100),
            view_count: Some(1),
            upload_date: Some("20240101".into()),
            thumbnail: None,
            is_photo: false,
            is_short: false,
            hashtags: vec![],
        }
    }

    #[test]
    fn save_then_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("cctest_{}", now_unix()));
        let cache = ChannelCache::new(dir.clone());
        cache.save("UCabc123", &[vid("a"), vid("b")]);
        let loaded = cache.load("UCabc123").unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].title, "a");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_missing_returns_none() {
        let dir = std::env::temp_dir().join(format!("cctest_miss_{}", now_unix()));
        let cache = ChannelCache::new(dir);
        assert!(cache.load("UCnope").is_none());
    }

    #[test]
    fn rejects_unsafe_channel_id() {
        let cache = ChannelCache::new(std::env::temp_dir());
        assert!(cache.path_for("../etc/passwd").is_none());
        assert!(cache.path_for("a/b").is_none());
        assert!(cache.path_for("UC-valid_123").is_some());
    }

    #[test]
    fn url_key_on_dinh_va_an_toan() {
        let k1 = url_key("https://youtube.com/@Kênh", "videos");
        // Ổn định (mở lần sau ra cùng file) + không phân biệt hoa thường/khoảng trắng.
        assert_eq!(k1, url_key("  HTTPS://YOUTUBE.COM/@Kênh ", "VIDEOS"));
        // Khác tab / khác URL -> khác khóa.
        assert_ne!(k1, url_key("https://youtube.com/@Kênh", "shorts"));
        assert_ne!(k1, url_key("https://youtube.com/@Khac", "videos"));
        // Tên file hợp lệ với path_for.
        let cache = ChannelCache::new(std::env::temp_dir());
        assert!(cache.path_for(&k1).is_some());
    }

    #[test]
    fn load_with_age_tra_tuoi() {
        let dir = std::env::temp_dir().join(format!("cctest_age_{}", now_unix()));
        let cache = ChannelCache::new(dir.clone());
        cache.save("UCage", &[vid("a")]);
        let (videos, age) = cache.load_with_age("UCage").unwrap();
        assert_eq!(videos.len(), 1);
        assert!(age < 60, "vừa lưu xong tuổi phải ~0, ra {age}");
        let _ = fs::remove_dir_all(&dir);
    }
}
