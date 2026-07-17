//! Settings_Store — quản lý cấu hình ứng dụng dạng JSON trên đĩa.
//!
//! Trách nhiệm:
//! * Nạp/lưu `Settings` tại đường dẫn chỉ định (thường là
//!   `tauri::path::app_config_dir()/settings.json`).
//! * Validate (`max_concurrency ∈ 1..=10`).
//! * Khôi phục mặc định khi file hỏng (Req 16.5).
//! * Ghi xuống đĩa nguyên tử (write tmp + rename) để tránh corrupt giữa chừng.
//!
//! Store nhận `PathBuf` ở constructor để giữ độc lập với `AppHandle`; phía
//! caller (`lib.rs`) sẽ resolve `app_config_dir` và truyền vào.

use std::fs;
use std::path::{Path, PathBuf};

use std::sync::RwLock;

use crate::error::{AppError, AppResult};
use crate::models::{Settings, SettingsPatch};

const MAX_CONCURRENCY_MIN: u8 = 1;
const MAX_CONCURRENCY_MAX: u8 = 100;

/// Lưu trữ `Settings` trong bộ nhớ kèm path để persist.
pub struct SettingsStore {
    inner: RwLock<Settings>,
    path: PathBuf,
}

impl SettingsStore {
    /// Nạp cấu hình từ `path`.
    ///
    /// * Nếu file tồn tại và parse thành công → trả `(store, None)`. Giá trị
    ///   `max_concurrency` được clamp về `[1,10]` để bảo vệ trước on-disk
    ///   values lệch khoảng.
    /// * Nếu file tồn tại nhưng parse thất bại → ghi đè bằng `Settings::default()`
    ///   và trả `(store, Some(AppError::ConfigCorrupt))` (Req 16.5).
    /// * Nếu file chưa tồn tại → tạo cha dir, ghi defaults, trả `(store, None)`.
    pub fn load(path: PathBuf) -> (Self, Option<AppError>) {
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(text) => match serde_json::from_str::<Settings>(&text) {
                    Ok(mut settings) => {
                        clamp_max_concurrency(&mut settings);
                        migrate_youtube_keys(&mut settings);
                        migrate_po_token_default(&mut settings);
                        migrate_aria2c_default(&mut settings);
                        let store = Self {
                            inner: RwLock::new(settings),
                            path,
                        };
                        (store, None)
                    }
                    Err(_) => recover_with_defaults(path),
                },
                Err(_) => recover_with_defaults(path),
            }
        } else {
            let settings = Settings::default();
            let store = Self {
                inner: RwLock::new(settings.clone()),
                path,
            };
            // Best-effort initial write; nếu fail vẫn coi như load OK vì
            // in-memory đã có defaults — lần persist tiếp theo sẽ thử lại.
            let _ = store.persist(&settings);
            (store, None)
        }
    }

    /// Trả snapshot clone của `Settings` hiện tại.
    pub fn get(&self) -> Settings {
        self.inner.read().unwrap().clone()
    }

    /// Đường dẫn file đang dùng để persist.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Áp dụng một closure mutate trên `Settings`, validate và persist xuống đĩa.
    ///
    /// Closure nhận `&mut Settings` và trả `AppResult<()>`. Nếu closure trả
    /// `Err`, lock được nhả không đổi gì. Nếu closure trả `Ok` nhưng giá trị
    /// vi phạm ràng buộc (`max_concurrency` ngoài `[1,10]`), state cũng được
    /// roll back và trả `AppError::InvalidSetting`.
    pub fn update_with<F>(&self, f: F) -> AppResult<Settings>
    where
        F: FnOnce(&mut Settings) -> AppResult<()>,
    {
        // Snapshot để rollback nếu validate/persist fail.
        let mut guard = self.inner.write().unwrap();
        let backup = guard.clone();

        if let Err(err) = f(&mut *guard) {
            *guard = backup;
            return Err(err);
        }

        if let Err(err) = validate(&*guard) {
            *guard = backup;
            return Err(err);
        }

        let snapshot = guard.clone();
        // Nhả lock trước khi I/O để ghi đĩa không block reader (path bất biến).
        drop(guard);

        if let Err(err) = self.persist(&snapshot) {
            // Persist fail → rollback in-memory để đồng nhất với đĩa.
            *self.inner.write().unwrap() = backup;
            return Err(err);
        }

        Ok(snapshot)
    }

    /// Áp dụng `SettingsPatch` (chỉ field `Some`) rồi persist.
    pub fn apply_patch(&self, patch: SettingsPatch) -> AppResult<Settings> {
        self.update_with(|s| {
            if let Some(folder) = patch.default_folder {
                s.default_folder = folder;
            }
            if let Some(mc) = patch.max_concurrency {
                if !(MAX_CONCURRENCY_MIN..=MAX_CONCURRENCY_MAX).contains(&mc) {
                    return Err(AppError::InvalidSetting {
                        field: "maxConcurrency".to_string(),
                    });
                }
                s.max_concurrency = mc;
            }
            if let Some(theme) = patch.theme {
                s.theme = theme;
            }
            if let Some(lang) = patch.language {
                s.language = lang;
            }
            if let Some(v) = patch.clipboard_watcher {
                s.clipboard_watcher = v;
            }
            if let Some(v) = patch.notifications {
                s.notifications = v;
            }
            if let Some(v) = patch.aria2c_enabled {
                s.aria2c_enabled = v;
            }
            // cookies_browser: `Some(Some(name))` → set, `Some(None)` → clear,
            // `None` → leave unchanged. Validate against the supported set; bad
            // names are silently ignored (UI prevents them but be defensive).
            if let Some(opt) = patch.cookies_browser {
                match opt {
                    Some(name) => {
                        let name = name.to_lowercase();
                        if crate::models::cookies_browser_is_valid(&name) {
                            s.cookies_browser = Some(name);
                        }
                    }
                    None => s.cookies_browser = None,
                }
            }
            // cookies_file: same Option<Option<String>> semantics.
            if let Some(opt) = patch.cookies_file {
                s.cookies_file = opt;
            }
            if let Some(v) = patch.skip_downloaded {
                s.skip_downloaded = v;
            }
            if let Some(v) = patch.watch_interval_min {
                s.watch_interval_min = v.clamp(1, 1440);
            }
            if let Some(list) = patch.proxies {
                // Trim + drop blank lines so a textarea with trailing newlines
                // doesn't create empty "proxies".
                s.proxies = list
                    .into_iter()
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect();
            }
            if let Some(v) = patch.po_token_enabled {
                s.po_token_enabled = v;
            }
            if let Some(v) = patch.rate_limit_cooldown_min {
                s.rate_limit_cooldown_min = v.clamp(1, 120);
            }
            if let Some(v) = patch.channel_subfolder {
                s.channel_subfolder = v;
            }
            if let Some(v) = patch.minimize_to_tray {
                s.minimize_to_tray = v;
            }
            // youtube_api_key: Option<Option<String>> — trim, blank → None.
            if let Some(opt) = patch.youtube_api_key {
                s.youtube_api_key = opt
                    .map(|k| k.trim().to_string())
                    .filter(|k| !k.is_empty());
            }
            // youtube_api_keys: danh sách → chuẩn hoá (trim, bỏ rỗng, bỏ trùng).
            if let Some(keys) = patch.youtube_api_keys {
                s.youtube_api_keys = normalize_api_keys(keys);
            }
            Ok(())
        })
    }

    /// Ghi `settings` xuống đĩa nguyên tử: serialize → write tmp → rename.
    fn persist(&self, settings: &Settings) -> AppResult<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        let json = serde_json::to_string_pretty(settings)?;

        let tmp_path = tmp_sibling(&self.path);
        fs::write(&tmp_path, json.as_bytes())?;

        // `rename` ghi đè file đích trên cùng volume một cách nguyên tử trên
        // mọi nền tảng được hỗ trợ (Win/macOS/Linux POSIX rename).
        if let Err(err) = fs::rename(&tmp_path, &self.path) {
            // Cố gắng dọn tmp để khỏi rác trên đĩa.
            let _ = fs::remove_file(&tmp_path);
            return Err(err.into());
        }
        Ok(())
    }
}

fn recover_with_defaults(path: PathBuf) -> (SettingsStore, Option<AppError>) {
    let settings = Settings::default();
    let store = SettingsStore {
        inner: RwLock::new(settings.clone()),
        path,
    };
    // Ghi đè để khôi phục file hỏng/không đọc được.
    let _ = store.persist(&settings);
    (store, Some(AppError::ConfigCorrupt))
}

fn clamp_max_concurrency(settings: &mut Settings) {
    if settings.max_concurrency < MAX_CONCURRENCY_MIN {
        settings.max_concurrency = MAX_CONCURRENCY_MIN;
    } else if settings.max_concurrency > MAX_CONCURRENCY_MAX {
        settings.max_concurrency = MAX_CONCURRENCY_MAX;
    }
}

/// Chuẩn hoá danh sách API key: trim, bỏ rỗng, bỏ trùng (giữ thứ tự).
pub(crate) fn normalize_api_keys(keys: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    keys.into_iter()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .filter(|k| seen.insert(k.clone()))
        .collect()
}

/// Chuyển dữ liệu key cũ (1 key trong `youtube_api_key`) sang danh sách mới
/// `youtube_api_keys` để người dùng không mất key đã lưu. Idempotent.
fn migrate_youtube_keys(settings: &mut Settings) {
    if settings.youtube_api_keys.is_empty() {
        if let Some(old) = settings.youtube_api_key.take() {
            let old = old.trim().to_string();
            if !old.is_empty() {
                settings.youtube_api_keys = vec![old];
            }
        }
    }
    settings.youtube_api_keys = normalize_api_keys(std::mem::take(&mut settings.youtube_api_keys));
}

/// Migration một lần: file settings cũ (tạo từ thời PO token mặc định TẮT)
/// lưu sẵn `poTokenEnabled: false` dù user chưa từng đụng vào — bật lại đúng
/// 1 lần vì thiếu PO token là nguồn 403 YouTube số 1. Sau lần này, user tắt
/// thủ công thì flag `po_token_migrated` (đã true) giữ nguyên lựa chọn đó.
fn migrate_po_token_default(settings: &mut Settings) {
    if !settings.po_token_migrated {
        settings.po_token_enabled = true;
        settings.po_token_migrated = true;
    }
}

/// Migration một lần: BẬT LẠI aria2c để khôi phục tốc độ. Bản v0.1.70 tắt
/// aria2c (tưởng nó gây lỗi tải-cả-kênh) — hoá ra lỗi do nguyên nhân khác,
/// còn tắt aria2c làm CHẬM tới 40 lần khi YouTube bóp từng kết nối. Bật lại
/// đúng 1 lần; sau đó user tự tắt thì được tôn trọng.
fn migrate_aria2c_default(settings: &mut Settings) {
    if !settings.aria2c_speed_restored {
        settings.aria2c_enabled = true;
        settings.aria2c_speed_restored = true;
    }
}

fn validate(settings: &Settings) -> AppResult<()> {
    if !(MAX_CONCURRENCY_MIN..=MAX_CONCURRENCY_MAX).contains(&settings.max_concurrency) {
        return Err(AppError::InvalidSetting {
            field: "maxConcurrency".to_string(),
        });
    }
    Ok(())
}

fn tmp_sibling(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("settings.json"));
    name.push(".tmp");
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Language, Theme};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_tmp_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("prodown-settings-{nanos}-{n}"));
        dir.join("settings.json")
    }

    #[test]
    fn load_creates_defaults_when_file_missing() {
        let path = unique_tmp_path();
        let (store, err) = SettingsStore::load(path.clone());
        assert!(err.is_none());
        let s = store.get();
        assert_eq!(s.max_concurrency, 3);
        assert!(path.exists(), "default settings should be written to disk");

        // Cleanup
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn load_reports_config_corrupt_and_overwrites_defaults() {
        let path = unique_tmp_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{ this is not valid json").unwrap();

        let (store, err) = SettingsStore::load(path.clone());
        assert!(matches!(err, Some(AppError::ConfigCorrupt)));
        assert_eq!(store.get().max_concurrency, 3);

        // File đã được ghi đè bằng JSON hợp lệ.
        let raw = fs::read_to_string(&path).unwrap();
        let parsed: Settings = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.max_concurrency, 3);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn load_clamps_max_concurrency_on_disk() {
        let path = unique_tmp_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();

        let mut bad = Settings::default();
        bad.max_concurrency = 200;
        fs::write(&path, serde_json::to_string(&bad).unwrap()).unwrap();

        let (store, err) = SettingsStore::load(path.clone());
        assert!(err.is_none());
        assert_eq!(store.get().max_concurrency, MAX_CONCURRENCY_MAX);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn migrates_old_po_token_off_to_on_once() {
        let path = unique_tmp_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();

        // File cũ: chưa có flag migrated, PO token đang tắt kiểu mặc định cũ.
        let mut old = Settings::default();
        old.po_token_enabled = false;
        old.po_token_migrated = false;
        fs::write(&path, serde_json::to_string(&old).unwrap()).unwrap();

        let (store, err) = SettingsStore::load(path.clone());
        assert!(err.is_none());
        assert!(store.get().po_token_enabled, "migration phải bật PO token");
        assert!(store.get().po_token_migrated);

        // User chủ động tắt sau migration → được tôn trọng ở lần load sau.
        let mut off = store.get();
        off.po_token_enabled = false;
        fs::write(&path, serde_json::to_string(&off).unwrap()).unwrap();
        let (store2, _) = SettingsStore::load(path.clone());
        assert!(!store2.get().po_token_enabled);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn restores_aria2c_speed_once() {
        let path = unique_tmp_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();

        // File bị v0.1.70 tắt aria2c (enabled=false, chưa speed_restored).
        let mut old = Settings::default();
        old.aria2c_enabled = false;
        old.aria2c_speed_restored = false;
        fs::write(&path, serde_json::to_string(&old).unwrap()).unwrap();

        let (store, err) = SettingsStore::load(path.clone());
        assert!(err.is_none());
        assert!(store.get().aria2c_enabled, "migration phải BẬT LẠI aria2c (khôi phục tốc độ)");
        assert!(store.get().aria2c_speed_restored);

        // User chủ động tắt sau đó → được tôn trọng (không bật lại nữa).
        let mut off = store.get();
        off.aria2c_enabled = false;
        fs::write(&path, serde_json::to_string(&off).unwrap()).unwrap();
        let (store2, _) = SettingsStore::load(path.clone());
        assert!(!store2.get().aria2c_enabled, "đã restored → tôn trọng lựa chọn user");

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn apply_patch_persists_changes() {
        let path = unique_tmp_path();
        let (store, _) = SettingsStore::load(path.clone());

        let patch = SettingsPatch {
            max_concurrency: Some(7),
            theme: Some(Theme::Dark),
            language: Some(Language::En),
            clipboard_watcher: Some(false),
            ..Default::default()
        };
        let updated = store.apply_patch(patch).unwrap();
        assert_eq!(updated.max_concurrency, 7);
        assert_eq!(updated.theme, Theme::Dark);
        assert_eq!(updated.language, Language::En);
        assert!(!updated.clipboard_watcher);

        // Đọc lại từ đĩa.
        let raw = fs::read_to_string(&path).unwrap();
        let on_disk: Settings = serde_json::from_str(&raw).unwrap();
        assert_eq!(on_disk.max_concurrency, 7);
        assert_eq!(on_disk.theme, Theme::Dark);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn migrates_single_youtube_key_to_list() {
        let path = unique_tmp_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();

        // File cũ chỉ có 1 key trong `youtube_api_key`.
        let mut old = Settings::default();
        old.youtube_api_key = Some("  AIzaOLD  ".to_string());
        fs::write(&path, serde_json::to_string(&old).unwrap()).unwrap();

        let (store, err) = SettingsStore::load(path.clone());
        assert!(err.is_none());
        let s = store.get();
        assert_eq!(s.youtube_api_keys, vec!["AIzaOLD".to_string()]);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn normalize_keys_trims_dedups_drops_empty() {
        let out = normalize_api_keys(vec![
            " k1 ".into(),
            "".into(),
            "k2".into(),
            "k1".into(),
            "   ".into(),
        ]);
        assert_eq!(out, vec!["k1".to_string(), "k2".to_string()]);
    }

    #[test]
    fn apply_patch_sets_youtube_api_keys() {
        let path = unique_tmp_path();
        let (store, _) = SettingsStore::load(path.clone());
        let updated = store
            .apply_patch(SettingsPatch {
                youtube_api_keys: Some(vec![" a ".into(), "b".into(), "a".into()]),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(updated.youtube_api_keys, vec!["a".to_string(), "b".to_string()]);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn apply_patch_rejects_out_of_range_concurrency() {
        let path = unique_tmp_path();
        let (store, _) = SettingsStore::load(path.clone());

        let before = store.get();
        let err = store
            .apply_patch(SettingsPatch {
                max_concurrency: Some(0),
                ..Default::default()
            })
            .unwrap_err();
        match err {
            AppError::InvalidSetting { field } => assert_eq!(field, "maxConcurrency"),
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(store.get(), before, "rollback expected on validation failure");

        let err = store
            .apply_patch(SettingsPatch {
                max_concurrency: Some(101),
                ..Default::default()
            })
            .unwrap_err();
        assert!(matches!(err, AppError::InvalidSetting { .. }));
        assert_eq!(store.get(), before);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn update_with_rolls_back_on_closure_error() {
        let path = unique_tmp_path();
        let (store, _) = SettingsStore::load(path.clone());
        let before = store.get();

        let err = store
            .update_with(|s| {
                s.max_concurrency = 9;
                Err(AppError::Other("nope".into()))
            })
            .unwrap_err();
        assert!(matches!(err, AppError::Other(_)));
        assert_eq!(store.get(), before);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
