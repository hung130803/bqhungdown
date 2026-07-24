//! Persistent store for auto-watched channels (the "Theo dõi kênh" feature).
//!
//! A small JSON file (`app_config_dir/watchlist.json`) holding a list of
//! [`WatchedChannel`]. Mirrors the atomic write strategy of `settings_store`
//! (write tmp + rename) so a crash mid-write can't corrupt the list. Kept
//! independent of `AppHandle`: the caller resolves the path and passes it in.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::error::AppResult;
use crate::models::WatchedChannel;

pub struct WatchlistStore {
    inner: RwLock<Vec<WatchedChannel>>,
    path: PathBuf,
}

impl WatchlistStore {
    /// Load the watchlist from `path`. KHÔNG được im lặng trả rỗng khi file có
    /// dữ liệu mà chỉ parse hỏng — vì persist() kế tiếp sẽ GHI ĐÈ rỗng =
    /// MẤT SẠCH kênh của user (bug cập nhật). Chiến lược:
    ///   1. Parse chuẩn -> ok.
    ///   2. Parse hỏng nhưng là JSON mảng -> cứu TỪNG phần tử (bỏ phần tử lỗi),
    ///      đồng thời sao lưu file gốc ra .bak để còn khôi phục.
    ///   3. Hỏng hẳn -> sao lưu ra .corrupt rồi mới coi như rỗng (KHÔNG xoá gốc).
    pub fn load(path: PathBuf) -> Self {
        let inner = Self::read_list(&path);
        Self {
            inner: RwLock::new(inner),
            path,
        }
    }

    fn read_list(path: &Path) -> Vec<WatchedChannel> {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return Vec::new(), // chưa có file = chưa thêm kênh nào
        };
        if text.trim().is_empty() {
            return Vec::new();
        }
        // 1) parse chuẩn
        if let Ok(list) = serde_json::from_str::<Vec<WatchedChannel>>(&text) {
            return list;
        }
        // 2) cứu từng phần tử (file cũ/lệch schema 1 vài kênh)
        if let Ok(vals) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
            let _ = fs::copy(path, backup_path(path, "bak")); // giữ bản gốc để cứu
            return vals
                .into_iter()
                .filter_map(|v| serde_json::from_value::<WatchedChannel>(v).ok())
                .collect();
        }
        // 3) hỏng hẳn: SAO LƯU rồi mới rỗng — tuyệt đối không để mất dữ liệu gốc
        let _ = fs::copy(path, backup_path(path, "corrupt"));
        Vec::new()
    }

    pub fn list(&self) -> Vec<WatchedChannel> {
        self.inner.read().unwrap().clone()
    }

    pub fn get(&self, id: &str) -> Option<WatchedChannel> {
        self.inner.read().unwrap().iter().find(|c| c.id == id).cloned()
    }

    /// True if a channel with the same (normalised) URL is already watched.
    pub fn contains_url(&self, url: &str) -> bool {
        let u = url.trim().trim_end_matches('/').to_lowercase();
        self.inner
            .read()
            .unwrap()
            .iter()
            .any(|c| c.url.trim().trim_end_matches('/').to_lowercase() == u)
    }

    pub fn add(&self, channel: WatchedChannel) -> AppResult<()> {
        {
            let mut guard = self.inner.write().unwrap();
            guard.push(channel);
        }
        self.persist()
    }

    pub fn remove(&self, id: &str) -> AppResult<()> {
        {
            let mut guard = self.inner.write().unwrap();
            guard.retain(|c| c.id != id);
        }
        self.persist()
    }

    /// Mutate one channel by id (if present) and persist. Returns the updated
    /// channel, or `None` when the id no longer exists.
    pub fn update<F>(&self, id: &str, f: F) -> AppResult<Option<WatchedChannel>>
    where
        F: FnOnce(&mut WatchedChannel),
    {
        let updated = {
            let mut guard = self.inner.write().unwrap();
            match guard.iter_mut().find(|c| c.id == id) {
                Some(c) => {
                    f(c);
                    Some(c.clone())
                }
                None => None,
            }
        };
        if updated.is_some() {
            self.persist()?;
        }
        Ok(updated)
    }

    /// Atomic write: serialize → tmp → rename.
    fn persist(&self) -> AppResult<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let snapshot = self.inner.read().unwrap().clone();
        let json = serde_json::to_string_pretty(&snapshot)?;
        let tmp = tmp_sibling(&self.path);
        fs::write(&tmp, json.as_bytes())?;
        if let Err(e) = fs::rename(&tmp, &self.path) {
            let _ = fs::remove_file(&tmp);
            return Err(e.into());
        }
        Ok(())
    }
}

/// `watchlist.json` -> `watchlist.json.<suffix>` (bản sao lưu để cứu dữ liệu).
fn backup_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("watchlist.json"));
    name.push(".");
    name.push(suffix);
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

fn tmp_sibling(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("watchlist.json"));
    name.push(".tmp");
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_file(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("wl_test_{}_{}.json", std::process::id(), tag));
        let _ = fs::remove_file(&p);
        p
    }

    #[test]
    fn file_cu_thieu_truong_moi_van_giu_nguyen_kenh() {
        // watchlist.json kiểu CŨ: chỉ có id/url/title/enabled/addedAt, THIẾU
        // tab/group/sourceMode... (các trường thêm ở bản mới). Trước bản vá,
        // parse hỏng -> load rỗng -> MẤT kênh. Nay phải giữ nguyên.
        let p = tmp_file("old");
        fs::write(
            &p,
            r#"[{"id":"c1","url":"https://youtube.com/@x","title":"X","enabled":true,"addedAt":"2024-01-01T00:00:00Z"}]"#,
        )
        .unwrap();
        let store = WatchlistStore::load(p.clone());
        let list = store.list();
        assert_eq!(list.len(), 1, "kênh cũ phải còn, không bị xoá");
        assert_eq!(list[0].id, "c1");
        assert!(list[0].enabled, "giữ nguyên trạng thái tích ✓");
        assert_eq!(list[0].tab, "all", "thiếu tab -> mặc định 'all'");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn file_hong_han_duoc_sao_luu_khong_mat_goc() {
        let p = tmp_file("corrupt");
        fs::write(&p, "{{{ khong phai json").unwrap();
        let store = WatchlistStore::load(p.clone());
        assert_eq!(store.list().len(), 0);
        assert!(
            backup_path(&p, "corrupt").exists(),
            "file hỏng phải được sao lưu .corrupt để cứu"
        );
        let _ = fs::remove_file(&p);
        let _ = fs::remove_file(backup_path(&p, "corrupt"));
    }

    #[test]
    fn cuu_tung_phan_tu_khi_1_kenh_loi() {
        // 1 phần tử hợp lệ + 1 phần tử rác -> cứu được phần tử hợp lệ.
        let p = tmp_file("partial");
        fs::write(
            &p,
            r#"[{"id":"ok","url":"u","enabled":true,"addedAt":"2024-01-01T00:00:00Z"}, 12345]"#,
        )
        .unwrap();
        let store = WatchlistStore::load(p.clone());
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.list()[0].id, "ok");
        let _ = fs::remove_file(&p);
        let _ = fs::remove_file(backup_path(&p, "bak"));
    }
}
