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

#[derive(Serialize, Deserialize)]
struct CachedChannel {
    /// Unix giây lúc lưu — chưa dùng để hết hạn, để dành cho sau.
    #[serde(default)]
    fetched_at: u64,
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
        let path = self.path_for(channel_id)?;
        let text = fs::read_to_string(&path).ok()?;
        let parsed: CachedChannel = serde_json::from_str(&text).ok()?;
        Some(parsed.videos)
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
}
