//! Queue_Manager với FSM, semaphore concurrency, và retry policy.

use crate::error::{AppError, AppResult};
use crate::events::{
    CompletedEventPayload, FailedEventPayload, ProgressEventPayload, StateEventPayload,
    EV_DOWNLOAD_COMPLETED, EV_DOWNLOAD_FAILED, EV_DOWNLOAD_PROGRESS, EV_DOWNLOAD_STATE,
    EV_QUEUE_UPDATED,
};
use crate::history_store::HistoryStore;
use crate::models::{
    DownloadItem, DownloadMode, DownloadState, HistoryEntry, HistoryStatus, QueueEvent, Settings,
};
use crate::notification;
use crate::settings_store::SettingsStore;
use crate::ytdlp_runner::{MetaEvent, RunOutcome, YtDlpRunner};
use chrono::Utc;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, Semaphore};
use tokio_util::sync::CancellationToken;

/// FSM transition table.
pub fn transition(state: DownloadState, event: QueueEvent) -> AppResult<DownloadState> {
    use DownloadState::*;
    use QueueEvent::*;
    let next = match (state, event) {
        (Queued, Start) => Downloading,
        (Downloading, Pause) => Paused,
        (Paused, Resume) => Downloading,
        (Downloading, Complete) => Completed,
        (Downloading, Fail) => Failed,
        (Queued | Downloading | Paused, Cancel) => Cancelled,
        // Skipped cũng retry được — nút "Vẫn tải video này" trên mục Bỏ qua
        // (force_redownload bỏ --download-archive nên yt-dlp không né nữa).
        (Failed | Cancelled | Skipped, Retry) => Queued,
        (Queued | Downloading, Skip) => Skipped,
        _ => {
            return Err(AppError::IllegalTransition {
                from: format!("{:?}", state),
                event: format!("{:?}", event),
            })
        }
    };
    Ok(next)
}

const RETRY_DELAYS_MS: [u64; 3] = [2000, 5000, 10000];
pub fn next_retry_delay(attempt: u8) -> Option<Duration> {
    RETRY_DELAYS_MS.get(attempt as usize).map(|ms| Duration::from_millis(*ms))
}

/// Xoá các file tạm còn sót lại của một Download_Item: `.part`, `.ytdl`,
/// `.frag`, `.f<id>.*` và file `.temp.*` được yt-dlp/ffmpeg sinh ra trong khi
/// tải/mux. Best-effort; mọi lỗi I/O đều bỏ qua để không làm hỏng flow huỷ.
///
/// Khi `aggressive=true` (gọi từ flow Cancel), xoá luôn cả file đích đã có
/// (output_path) — vì lúc người dùng huỷ giữa chừng, ta không muốn để lại file
/// dở dang/hoàn tất một phần trên đĩa.
pub fn cleanup_partials(item: &DownloadItem) {
    cleanup_partials_inner(item, false);
}

/// Dọn file tạm BỀN BỈ khi huỷ/xoá: chạy trên thread riêng và thử lại nhiều
/// lần cách nhau ngắn. Lý do: tiến trình yt-dlp/ffmpeg vừa bị kill có thể còn
/// giữ khoá file `.part` thêm chốc lát trên Windows → lần xoá đầu trượt, lần
/// sau ăn. Không chặn caller nên KHÔNG ảnh hưởng tốc độ tải; chỉ đụng file tạm
/// đúng của mục này (khớp tên) nên KHÔNG ảnh hưởng video khác.
pub fn cleanup_partials_retry(item: DownloadItem) {
    std::thread::spawn(move || {
        // 10 lần × 600ms ≈ 6s — đủ cho taskkill /T giết cây tiến trình + Windows
        // nhả khoá file, kể cả máy chậm. Thread nền, không chặn gì.
        for i in 0..10 {
            if i > 0 {
                std::thread::sleep(Duration::from_millis(600));
            }
            cleanup_partials_inner(&item, true);
        }
    });
}

/// True nếu tên file là file TẠM của yt-dlp (đang tải dở), KHÔNG phải video
/// hoàn chỉnh. Rất chặt để TUYỆT ĐỐI không xoá nhầm file đã tải xong:
///   - `.part`, `.ytdl`             (đuôi)
///   - `.part-Frag123`, `.part-N`   (mảnh fragment)
///   - `.f137.mp4`, `.f251.webm`    (format trung gian: `.f` + SỐ)
///   - `.temp.mp4`, `.frag`
/// Lưu ý: KHÔNG dùng `contains(".f")` (khớp bừa "clip.for.mp4"); phải là
/// `.f` theo sau bởi CHỮ SỐ.
fn is_ytdlp_temp_file(name: &str) -> bool {
    let l = name.to_lowercase();
    // `.aria2` = file điều khiển của aria2c (song hành file .part khi tải đa luồng).
    if l.ends_with(".part") || l.ends_with(".ytdl") || l.ends_with(".frag")
        || l.ends_with(".aria2") || l.contains(".part-") || l.contains(".temp.")
        || l.contains(".part.aria2")
    {
        return true;
    }
    // `.f<digits>.` — vd ".f137.", ".f251." (format trung gian yt-dlp).
    if let Some(idx) = l.find(".f") {
        let after = &l[idx + 2..];
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() && after[digits.len()..].starts_with('.') {
            return true;
        }
    }
    false
}

/// Dọn file TẠM của một mục (an toàn). `aggressive` (khi huỷ giữa chừng) chỉ
/// bổ sung xoá đúng 1 file output_path NẾU nó cũng là file tạm — KHÔNG bao giờ
/// xoá video hoàn chỉnh, KHÔNG bao giờ xoá file của mục khác.
///
/// SỬA LỖI NGHIÊM TRỌNG (từng xoá cả trăm video): bản cũ ở chế độ aggressive
/// xoá MỌI file cùng tiền tố tên (kênh rap battle cùng tên → mất sạch), và
/// mẫu `.f` khớp bừa. Giờ chỉ đụng file tạm thật sự.
/// Chuẩn hoá tên để so "mềm": bỏ ký tự không phải chữ/số, viết thường. Nhờ vậy
/// khớp được dù dấu `'`, `!`, khoảng trắng bị sanitize khác nhau (đây là lý do
/// trước hay sót file `.part-Frag`).
fn norm_key(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Rút "khoá gốc" của một tên file yt-dlp: bỏ đuôi tạm (`.part`, `.ytdl`,
/// `.aria2`, `.part-Frag*`, `.frag`, `.temp`), bỏ MÃ FORMAT `.f<digits>` và bỏ
/// 1 phần mở rộng, rồi `norm_key`. Nhờ vậy các file của CÙNG một video —
/// `Title.f401.mp4.part` (video), `Title.f251.webm` (audio), `Title.mp4.part`
/// (đã gộp dở), `Title.mp4.part-Frag12` (mảnh) — đều rút về CÙNG một khoá
/// "title". Dùng để so BẰNG NHAU với khoá lấy từ output_path thật của yt-dlp,
/// nên khớp được cả khi tên quá dài bị yt-dlp cắt ngắn mà KHÔNG đụng video khác.
fn base_key(name: &str) -> String {
    let mut s = name.to_string();
    // 1) Bỏ lặp các đuôi tạm ở cuối (có thể chồng: `.webm.part.aria2`).
    loop {
        let l = s.to_lowercase();
        let before = s.len();
        if let Some(i) = l.rfind(".part-") {
            s.truncate(i);
        } else if l.ends_with(".part") || l.ends_with(".ytdl") || l.ends_with(".frag") {
            s.truncate(s.len() - 5);
        } else if l.ends_with(".aria2") {
            s.truncate(s.len() - 6);
        } else if l.ends_with(".temp") {
            s.truncate(s.len() - 5);
        }
        if s.len() == before {
            break;
        }
    }
    // 2) Bỏ 1 phần mở rộng cuối (vd .mp4/.m4a/.webm/.mkv).
    if let Some(i) = s.rfind('.') {
        let ext = &s[i + 1..];
        if !ext.is_empty() && ext.len() <= 4 && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
            s.truncate(i);
        }
    }
    // 3) Bỏ mã format `.f<digits>` ở cuối (vd ".f401", ".f251").
    let l = s.to_lowercase();
    if let Some(i) = l.rfind(".f") {
        let after = &l[i + 2..];
        if !after.is_empty() && after.chars().all(|c| c.is_ascii_digit()) {
            s.truncate(i);
        }
    }
    norm_key(&s)
}

fn cleanup_partials_inner(item: &DownloadItem, aggressive: bool) {
    let folder = &item.request.save_folder;
    let title_key = norm_key(&crate::filename_resolver::sanitize(&item.title));
    // Khoá gốc lấy từ output_path THẬT của yt-dlp (đã rút bỏ .f<id>/đuôi tạm).
    // Đây là "vân tay" chính xác của video này để so BẰNG NHAU, an toàn tuyệt
    // đối với video khác kể cả khi tên bị cắt ngắn.
    let out_base = item
        .output_path
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .map(base_key)
        .filter(|s| s.len() >= 6);

    // aggressive: nếu output_path trỏ tới 1 file TẠM (đuôi .part…), xoá nó.
    if aggressive {
        if let Some(ref p) = item.output_path {
            if let Some(n) = p.file_name().and_then(|s| s.to_str()) {
                if is_ytdlp_temp_file(n) {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
    }

    // Quét CẢ thư mục đích LẪN thư mục tạm ẩn `.bqd-temp` (nơi bản mới để file
    // tạm). So tên "mềm" (norm_key) để bắt hết dù dấu đặc biệt bị đổi.
    // Prefix cần đủ dài (>=6) để không xoá nhầm video khác cùng đầu tên.
    let want = title_key.clone();
    let matches = |name: &str| -> bool {
        if !is_ytdlp_temp_file(name) {
            return false;
        }
        // (a) Khớp theo TITLE (1 chiều, chống xoá nhầm): tên file bắt đầu bằng
        //     nguyên khoá title. An toàn vì đòi khớp đủ cả title.
        if want.len() >= 6 && norm_key(name).starts_with(&want) {
            return true;
        }
        // (b) Khớp "vân tay" BẰNG NHAU với output_path thật: xử lý được file
        //     audio/video trung gian (.f251/.f401) và cả tên bị cắt ngắn, mà
        //     KHÔNG bao giờ trùng video khác (đòi base_key bằng đúng nhau).
        if let Some(ref ob) = out_base {
            if base_key(name) == *ob {
                return true;
            }
        }
        false
    };

    for dir in [folder.clone(), folder.join(".bqd-temp")] {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            if let Ok(name) = entry.file_name().into_string() {
                if matches(&name) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
}

pub struct QueueManager {
    items: RwLock<IndexMap<String, DownloadItem>>,
    cancel_tokens: Mutex<HashMap<String, CancellationToken>>,
    semaphore: Arc<Semaphore>,
    current_cap: Mutex<u8>,
    settings: Arc<SettingsStore>,
    history: Arc<HistoryStore>,
    runner: Arc<YtDlpRunner>,
    app: AppHandle,
    /// Where the queue is persisted so it survives an app restart.
    queue_path: PathBuf,
    /// Snapshot of the last channel group removed via `remove_group`, kept so
    /// an accidental "Xóa cả kênh" can be undone.
    last_removed: Mutex<Vec<DownloadItem>>,
    /// Ensures shutdown() runs once (tray "quit" + window Destroyed both fire it).
    shutdown_done: std::sync::atomic::AtomicBool,
    /// "Tạm dừng tất cả": khi true, run_loop_for KHÔNG khởi động mục mới (mục
    /// đang chờ acquire permit sẽ nhả ra và giữ nguyên Queued). resume_all()
    /// tắt cờ rồi spawn lại. An toàn với FSM: chỉ dùng transition hợp lệ.
    paused_all: std::sync::atomic::AtomicBool,
}

impl QueueManager {
    pub fn new(
        app: AppHandle,
        settings: Arc<SettingsStore>,
        history: Arc<HistoryStore>,
        runner: Arc<YtDlpRunner>,
        queue_path: PathBuf,
    ) -> Arc<Self> {
        let cap = settings.get().max_concurrency.max(1);
        let me = Arc::new(Self {
            items: RwLock::new(IndexMap::new()),
            cancel_tokens: Mutex::new(HashMap::new()),
            semaphore: Arc::new(Semaphore::new(cap as usize)),
            current_cap: Mutex::new(cap),
            settings,
            history,
            runner,
            app,
            queue_path,
            last_removed: Mutex::new(Vec::new()),
            shutdown_done: std::sync::atomic::AtomicBool::new(false),
            paused_all: std::sync::atomic::AtomicBool::new(false),
        });
        // Periodically save the queue so a crash/power loss doesn't lose more
        // than a few seconds of progress.
        let saver = me.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(15)).await;
                saver.prune_terminal();
                saver.persist();
            }
        });
        // Auto-clean leftover junk files (blank/broken) in the download folder
        // every 2 minutes — protecting in-progress downloads. The user never has
        // to delete them by hand.
        let cleaner = me.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(120)).await;
                let folder = cleaner.settings.get().default_folder;
                if folder.as_os_str().is_empty() {
                    continue;
                }
                let protected = cleaner.protected_prefixes();
                // Blocking FS walk on a worker thread so the runtime isn't blocked.
                std::thread::spawn(move || {
                    let _ = crate::commands::clean_junk_in(&folder, &protected);
                });
            }
        });
        me
    }

    /// Sanitized title prefixes of videos currently downloading — used to keep
    /// the junk cleaner from touching an active download's half-written files.
    pub fn protected_prefixes(&self) -> std::collections::HashSet<String> {
        self.items
            .read()
            .unwrap()
            .values()
            .filter(|it| matches!(it.state, DownloadState::Downloading))
            .map(|it| {
                crate::filename_resolver::sanitize(&it.title)
                    .to_lowercase()
                    .chars()
                    .take(25)
                    .collect::<String>()
            })
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Keep memory bounded for heavy users: drop the oldest finished items from
    /// the in-memory queue beyond a cap (they remain in History on disk). Only
    /// terminal items are pruned — active/queued/paused ones are never removed.
    fn prune_terminal(&self) {
        const MAX_TERMINAL: usize = 300;
        let dropped = {
            let mut map = self.items.write().unwrap();
            let mut terminal: Vec<(String, i64)> = map
                .iter()
                .filter(|(_, it)| {
                    matches!(
                        it.state,
                        DownloadState::Completed
                            | DownloadState::Failed
                            | DownloadState::Cancelled
                            | DownloadState::Skipped
                    )
                })
                .map(|(id, it)| (id.clone(), it.finished_at.map(|t| t.timestamp()).unwrap_or(0)))
                .collect();
            if terminal.len() <= MAX_TERMINAL {
                0
            } else {
                terminal.sort_by_key(|(_, ts)| *ts); // oldest first
                let to_drop = terminal.len() - MAX_TERMINAL;
                for (id, _) in terminal.into_iter().take(to_drop) {
                    map.shift_remove(&id);
                }
                to_drop
            }
        };
        if dropped > 0 {
            self.emit_queue_updated();
        }
    }

    /// Write the current queue to disk (atomic: tmp + rename).
    pub fn persist(&self) {
        let snapshot: Vec<DownloadItem> = self.items.read().unwrap().values().cloned().collect();
        let json = match serde_json::to_string(&snapshot) {
            Ok(j) => j,
            Err(_) => return,
        };
        let tmp = self.queue_path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &self.queue_path);
        }
    }

    /// Restore a saved queue on startup. Unfinished items (downloading/queued)
    /// are reset to Queued and resumed; paused/failed are kept for display;
    /// finished ones are dropped (they're in history + the download-archive).
    pub fn restore(self: &Arc<Self>, saved: Vec<DownloadItem>) {
        let mut to_run: Vec<String> = Vec::new();
        {
            let mut map = self.items.write().unwrap();
            for mut it in saved {
                match it.state {
                    DownloadState::Completed
                    | DownloadState::Cancelled
                    | DownloadState::Skipped => continue,
                    DownloadState::Downloading | DownloadState::Queued => {
                        it.state = DownloadState::Queued;
                        it.bot_retries = 0;
                        it.attempt = 0;
                        it.speed_bps = None;
                        it.eta_sec = None;
                        it.error_message = None;
                        to_run.push(it.short_id.clone());
                    }
                    DownloadState::Failed | DownloadState::Paused => {}
                }
                map.insert(it.short_id.clone(), it);
            }
        }
        self.emit_queue_updated();
        for id in to_run {
            let me = self.clone();
            tauri::async_runtime::spawn(async move { me.run_loop_for(id).await; });
        }
    }

    pub fn list(&self) -> Vec<DownloadItem> {
        self.items.read().unwrap().values().cloned().collect()
    }

    /// Remove ALL queue items whose save folder matches `folder` — used to drop
    /// a whole channel the user no longer wants. Cancels any in-flight ones,
    /// wipes their leftover partial files, and drops them from the queue.
    /// Returns how many were removed.
    pub fn remove_group(&self, folder: &std::path::Path) -> usize {
        // Snapshot the group first so the removal can be undone.
        let snapshot: Vec<DownloadItem> = {
            let map = self.items.read().unwrap();
            map.values()
                .filter(|it| it.request.save_folder == folder)
                .cloned()
                .collect()
        };
        let ids: Vec<String> = snapshot.iter().map(|it| it.short_id.clone()).collect();
        *self.last_removed.lock().unwrap() = snapshot;
        // Cancel any active downloads in this group.
        {
            let mut toks = self.cancel_tokens.lock().unwrap();
            for id in &ids {
                if let Some(tok) = toks.remove(id) {
                    tok.cancel();
                }
            }
        }
        // Clean leftover partials + drop from the queue.
        {
            let mut map = self.items.write().unwrap();
            for id in &ids {
                if let Some(it) = map.get(id).cloned() {
                    cleanup_partials_retry(it);
                }
                map.shift_remove(id);
            }
        }
        if !ids.is_empty() {
            self.emit_queue_updated();
            self.persist();
        }
        ids.len()
    }

    /// Restore the last channel group removed with `remove_group` (undo an
    /// accidental "Xóa cả kênh"). Re-queues unfinished items. Returns count.
    pub fn undo_remove_group(self: &Arc<Self>) -> usize {
        let items: Vec<DownloadItem> = std::mem::take(&mut *self.last_removed.lock().unwrap());
        let total = items.len();
        if total == 0 {
            return 0;
        }
        let mut to_run: Vec<String> = Vec::new();
        {
            let mut map = self.items.write().unwrap();
            for mut it in items {
                if matches!(it.state, DownloadState::Downloading | DownloadState::Queued) {
                    it.state = DownloadState::Queued;
                    it.bot_retries = 0;
                    it.attempt = 0;
                    it.speed_bps = None;
                    it.eta_sec = None;
                    it.error_message = None;
                    to_run.push(it.short_id.clone());
                }
                map.insert(it.short_id.clone(), it);
            }
        }
        self.emit_queue_updated();
        self.persist();
        for id in to_run {
            let me = self.clone();
            tauri::async_runtime::spawn(async move { me.run_loop_for(id).await; });
        }
        total
    }

    /// Cancel mọi download đang chạy/dở và xoá file rác. Được gọi khi đóng app.
    pub fn shutdown(&self) {
        // Run once — both the tray "Thoát hẳn" and the window Destroyed event
        // call this; the second call would redundantly re-sweep files.
        if self.shutdown_done.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        // Save the queue first (with current states) so reopening resumes the
        // downloading/queued items.
        self.persist();
        // Kill all running yt-dlp processes via cancel tokens.
        let tokens: Vec<CancellationToken> = {
            let mut map = self.cancel_tokens.lock().unwrap();
            map.drain().map(|(_, t)| t).collect()
        };
        for tok in tokens {
            tok.cancel();
        }
        // Sweep all non-terminal items for partial files.
        let items: Vec<DownloadItem> = {
            let map = self.items.read().unwrap();
            map.values()
                .filter(|i| !matches!(
                    i.state,
                    DownloadState::Completed | DownloadState::Failed | DownloadState::Cancelled | DownloadState::Skipped
                ))
                .cloned()
                .collect()
        };
        for it in items {
            cleanup_partials(&it);
        }
    }

    pub fn get(&self, id: &str) -> Option<DownloadItem> {
        self.items.read().unwrap().get(id).cloned()
    }

    /// Remove an item entirely from the queue (no FSM, no cleanup of files on disk).
    /// Used by UI when user wants to dismiss a row whose file is gone or that
    /// they no longer care about. If the item is still active, also cancel it.
    pub fn remove_item(&self, id: &str) -> AppResult<()> {
        // Cancel any in-flight download first.
        if let Some(tok) = self.cancel_tokens.lock().unwrap().remove(id) {
            tok.cancel();
        }
        let removed = self.items.write().unwrap().shift_remove(id);
        let Some(item) = removed else {
            return Err(AppError::NotFound(id.to_string()));
        };
        // Xoá SẠCH file tải dở của mục vừa bị xoá. Bản cũ KHÔNG dọn ở đây → khi
        // xoá 1 mục đang tải, file `.part`/mảnh bị mồ côi. Bản bền bỉ thử lại
        // vài lần (chờ tiến trình nhả khoá) và chỉ khớp đúng file của mục này.
        cleanup_partials_retry(item);
        self.emit_queue_updated();
        Ok(())
    }

    pub fn enqueue(self: &Arc<Self>, item: DownloadItem) -> AppResult<()> {
        let id = item.short_id.clone();
        self.items.write().unwrap().insert(id.clone(), item.clone());
        self.emit_state(&item);
        self.emit_queue_updated();
        // NOTE: don't persist() here — adding a batch of 200 would write the
        // whole queue 200×. The 15s timer + shutdown cover persistence.
        let me = self.clone();
        tauri::async_runtime::spawn(async move { me.run_loop_for(id).await; });
        Ok(())
    }

    pub fn pause(self: &Arc<Self>, id: &str) -> AppResult<()> {
        self.transition_item(id, QueueEvent::Pause)?;
        if let Some(tok) = self.cancel_tokens.lock().unwrap().remove(id) {
            tok.cancel();
        }
        Ok(())
    }

    pub fn resume(self: &Arc<Self>, id: &str) -> AppResult<()> {
        self.transition_item(id, QueueEvent::Resume)?;
        // Set state back to Queued so run_loop_for picks it again.
        {
            let mut map = self.items.write().unwrap();
            if let Some(item) = map.get_mut(id) {
                item.state = DownloadState::Queued;
            }
        }
        let id_owned = id.to_string();
        let me = self.clone();
        tauri::async_runtime::spawn(async move { me.run_loop_for(id_owned).await; });
        Ok(())
    }

    pub fn cancel(self: &Arc<Self>, id: &str) -> AppResult<()> {
        self.transition_item(id, QueueEvent::Cancel)?;
        let tok = self.cancel_tokens.lock().unwrap().remove(id);
        match tok {
            // Có tiến trình đang chạy: bấm token → run-loop tự kill + dọn
            // file `.part` (nhánh Cancelled trong run_loop_for).
            Some(t) => t.cancel(),
            // Mục chờ/tạm dừng KHÔNG có run-loop sống → không ai dọn hộ;
            // tự dọn file tải dở tại đây (bền bỉ, thread nền).
            None => {
                if let Some(it) = self.get(id) {
                    cleanup_partials_retry(it);
                }
            }
        }
        Ok(())
    }

    /// Hủy TẤT CẢ mục đang chờ/tải/tạm dừng một phát. Tức thời: chỉ chuyển
    /// trạng thái + bấm cancel-token (kill tiến trình + dọn file `.part`
    /// chạy nền, không chặn). Trả về số mục đã hủy.
    pub fn cancel_all(self: &Arc<Self>) -> u32 {
        let ids: Vec<String> = {
            let map = self.items.read().unwrap();
            map.values()
                .filter(|it| {
                    matches!(
                        it.state,
                        DownloadState::Queued | DownloadState::Downloading | DownloadState::Paused
                    )
                })
                .map(|it| it.short_id.clone())
                .collect()
        };
        let mut n = 0u32;
        for id in &ids {
            if self.cancel(id).is_ok() {
                n += 1;
            }
        }
        n
    }

    pub fn retry(self: &Arc<Self>, id: &str) -> AppResult<DownloadItem> {
        self.transition_item(id, QueueEvent::Retry)?;
        {
            let mut map = self.items.write().unwrap();
            if let Some(item) = map.get_mut(id) {
                item.attempt = 0;
                item.bot_retries = 0;
                item.error_message = None;
                item.bytes_downloaded = 0;
                item.bytes_total = None;
                item.speed_bps = None;
                item.eta_sec = None;
                item.finished_at = None;
            }
        }
        let item = self.get(id).ok_or_else(|| AppError::NotFound(id.to_string()))?;
        let id_owned = id.to_string();
        let me = self.clone();
        tauri::async_runtime::spawn(async move { me.run_loop_for(id_owned).await; });
        Ok(item)
    }

    /// Nút "Vẫn tải video này" trên mục Bỏ qua: bật cờ force_redownload (bỏ
    /// kiểm tra danh-sách-đã-tải cho riêng mục này) rồi chạy lại như Retry.
    /// File cũ còn trên máy sẽ được giữ nguyên — bản mới tự thêm ` (1)`.
    pub fn force_download(self: &Arc<Self>, id: &str) -> AppResult<DownloadItem> {
        {
            let mut map = self.items.write().unwrap();
            if let Some(item) = map.get_mut(id) {
                item.request.force_redownload = true;
            }
        }
        self.retry(id)
    }

    /// Thử lại HÀNG LOẠT mọi mục đang Failed — cho nút "Thử lại tất cả video
    /// lỗi" (kịch bản thật: tải cả kênh mấy trăm video dính lỗi hàng loạt vì
    /// thiếu cookie/bị chặn tạm; user thêm cookie xong chỉ cần 1 nút thay vì
    /// bấm tay từng video). Trả về số mục đã re-queue. Mục nào retry lỗi
    /// (state đổi giữa chừng…) thì bỏ qua, không làm hỏng cả loạt.
    /// Semaphore concurrency vẫn giữ nhịp — re-queue 300 mục cũng chỉ chạy
    /// đồng thời đúng số luồng đã cấu hình.
    pub fn retry_all_failed(self: &Arc<Self>) -> usize {
        let ids: Vec<String> = {
            let map = self.items.read().unwrap();
            map.iter()
                .filter(|(_, it)| matches!(it.state, DownloadState::Failed))
                .map(|(id, _)| id.clone())
                .collect()
        };
        let mut n = 0;
        for id in &ids {
            if self.retry(id).is_ok() {
                n += 1;
            }
        }
        if n > 0 {
            self.emit_queue_updated();
        }
        n
    }

    /// "Tạm dừng tất cả": bật cờ (chặn khởi động mục mới) + tạm dừng mọi mục
    /// đang tải. Trả số mục đã tạm dừng.
    pub fn pause_all(self: &Arc<Self>) -> usize {
        self.paused_all.store(true, std::sync::atomic::Ordering::Relaxed);
        let ids: Vec<String> = {
            let map = self.items.read().unwrap();
            map.iter()
                .filter(|(_, it)| matches!(it.state, DownloadState::Downloading))
                .map(|(id, _)| id.clone())
                .collect()
        };
        let mut n = 0;
        for id in &ids {
            if self.pause(id).is_ok() {
                n += 1;
            }
        }
        self.emit_queue_updated();
        n
    }

    /// "Tiếp tục tất cả": tắt cờ + đưa mọi mục Paused/Queued chạy lại. Trả số
    /// mục đã kích hoạt.
    pub fn resume_all(self: &Arc<Self>) -> usize {
        self.paused_all.store(false, std::sync::atomic::Ordering::Relaxed);
        let ids: Vec<String> = {
            let map = self.items.read().unwrap();
            map.iter()
                .filter(|(_, it)| matches!(it.state, DownloadState::Paused | DownloadState::Queued))
                .map(|(id, _)| id.clone())
                .collect()
        };
        let mut n = 0;
        for id in &ids {
            // Paused → đưa về Queued (transition Resume hợp lệ); Queued thì
            // giữ nguyên. Sau đó spawn run_loop_for để chạy.
            {
                let mut map = self.items.write().unwrap();
                if let Some(it) = map.get_mut(id) {
                    it.state = DownloadState::Queued;
                }
            }
            let me = self.clone();
            let id_owned = id.clone();
            tauri::async_runtime::spawn(async move { me.run_loop_for(id_owned).await; });
            n += 1;
        }
        if n > 0 {
            self.emit_queue_updated();
        }
        n
    }

    pub async fn set_concurrency(self: &Arc<Self>, n: u8) {
        let n = n.clamp(1, 100);
        let mut cap = self.current_cap.lock().unwrap();
        if n > *cap {
            self.semaphore.add_permits((n - *cap) as usize);
        } else if n < *cap {
            // Acquire (n - new) permits to "remove" them — acquire_owned + forget.
            let to_remove = (*cap - n) as usize;
            let sem = self.semaphore.clone();
            let _ = tauri::async_runtime::spawn(async move {
                for _ in 0..to_remove {
                    if let Ok(p) = sem.clone().acquire_owned().await {
                        p.forget();
                    }
                }
            });
        }
        *cap = n;
    }

    fn transition_item(&self, id: &str, event: QueueEvent) -> AppResult<()> {
        let mut map = self.items.write().unwrap();
        let item = map.get_mut(id).ok_or_else(|| AppError::NotFound(id.to_string()))?;
        let next = transition(item.state, event)?;
        item.state = next;
        let cloned = item.clone();
        drop(map);
        self.emit_state(&cloned);
        self.emit_queue_updated();
        Ok(())
    }

    fn emit_state(&self, item: &DownloadItem) {
        let payload = StateEventPayload {
            short_id: item.short_id.clone(),
            state: item.state,
            error_message: item.error_message.clone(),
            output_path: item.output_path.clone(),
        };
        let _ = self.app.emit(EV_DOWNLOAD_STATE, payload);
    }

    fn emit_queue_updated(&self) {
        let _ = self.app.emit(EV_QUEUE_UPDATED, crate::events::QueueUpdatedPayload { items: self.list() });
    }

    async fn run_loop_for(self: Arc<Self>, id: String) {
        // Acquire a permit (blocks until concurrency slot is free).
        let permit = match self.semaphore.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => return,
        };

        // "Tạm dừng tất cả" đang bật → không khởi động mục mới. Nhả permit,
        // giữ nguyên trạng thái Queued; resume_all() sẽ spawn lại sau.
        if self.paused_all.load(std::sync::atomic::Ordering::Relaxed) {
            drop(permit);
            return;
        }

        // Mark as Downloading.
        let mut item = match self.get(&id) {
            Some(i) => i,
            None => return,
        };
        if item.state != DownloadState::Queued {
            drop(permit);
            return;
        }
        if let Err(e) = self.transition_item(&id, QueueEvent::Start) {
            log_err(&e);
            drop(permit);
            return;
        }
        item.state = DownloadState::Downloading;

        let cancel = CancellationToken::new();
        self.cancel_tokens.lock().unwrap().insert(id.clone(), cancel.clone());

        // Resume flag: yt-dlp `--continue` if attempt > 0 OR state was previously Paused.
        let resume = item.attempt > 0;

        // Detect CDN-rewritten URLs (Douyin) — title from yt-dlp would be the
        // CDN file ID (gibberish), and we already scraped a real title from
        // the share page. Used by both pre-resolve and meta channel below.
        let url_is_cdn = item.request.url.contains("aweme.snssdk.com")
            || item.request.url.contains("/playwm/")
            || item.request.url.contains("/play/?");

        // Pre-resolve filename: nếu file đích đã tồn tại (do user tải lại
        // cùng video), tự thêm ` (1)`, ` (2)`... để KHÔNG ghi đè file cũ.
        // Chuyển vào yt-dlp qua `output_stem` ⇒ template `-o "<stem>.<ext>"`.
        //
        // Bỏ qua khi:
        //   - title rỗng / "video" mặc định → để yt-dlp tự pick title đẹp.
        //   - URL là CDN (Douyin) → đã có post-rename ở phase Completed.
        let output_stem: Option<String> = if !url_is_cdn
            && !item.title.trim().is_empty()
            && item.title != item.request.url
        {
            let folder = &item.request.save_folder;
            let sanitized = crate::filename_resolver::sanitize(&item.title);
            // Dùng "mp4" (video) hoặc "mp3" (audio) chỉ để check collision —
            // ext thật sẽ được yt-dlp tự đặt từ %(ext)s.
            let ext_for_check = match item.request.mode {
                crate::models::DownloadMode::Audio => "mp3",
                crate::models::DownloadMode::Video => "mp4",
            };
            let candidate = crate::filename_resolver::auto_rename(
                folder, &sanitized, ext_for_check, |p| p.exists(),
            );
            // Lấy file_stem (không có ext) để truyền vào args_builder.
            candidate
                .file_stem()
                .and_then(|s| s.to_str())
                .map(String::from)
        } else {
            None
        };

        let (tx, mut rx) = mpsc::channel::<crate::models::ProgressSnapshot>(64);
        let app = self.app.clone();
        let id_for_progress = id.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(snap) = rx.recv().await {
                let _ = app.emit(EV_DOWNLOAD_PROGRESS, ProgressEventPayload {
                    short_id: id_for_progress.clone(),
                    progress: snap,
                });
            }
        });

        // Live metadata channel: yt-dlp prints title/thumbnail/channel before
        // download starts (via --print before_dl:...). We patch the queue item
        // and re-broadcast EV_QUEUE_UPDATED so UI shows the real preview within
        // 1-2 seconds of starting (critical for batch-added items).
        //
        // EXCEPTION: when the URL got rewritten by url_resolver (Douyin → CDN
        // direct URL), yt-dlp's TITLE is the file name on the CDN (gibberish
        // like `oEPACEIyM8B7...`). In that case the queue item already carries
        // a *better* title scraped from the share page, so we skip TITLE
        // updates from yt-dlp to avoid clobbering it. `url_is_cdn` was
        // computed earlier (above output_stem block) and is reused here.
        let (meta_tx, mut meta_rx) = mpsc::channel::<MetaEvent>(8);
        let me_meta = self.clone();
        let id_for_meta = id.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(ev) = meta_rx.recv().await {
                let mut changed = false;
                {
                    let mut map = me_meta.items.write().unwrap();
                    if let Some(it) = map.get_mut(&id_for_meta) {
                        match ev {
                            MetaEvent::Title(t) => {
                                // Skip when URL is a resolved CDN one — the
                                // pre-filled title from url_resolver is more
                                // accurate than the CDN file ID.
                                if !url_is_cdn && !t.trim().is_empty() && it.title != t {
                                    it.title = t;
                                    changed = true;
                                }
                            }
                            MetaEvent::Thumbnail(u) => {
                                if it.thumbnail.as_deref() != Some(u.as_str()) {
                                    it.thumbnail = Some(u);
                                    changed = true;
                                }
                            }
                            MetaEvent::Channel(c) => {
                                if !url_is_cdn && it.channel.as_deref() != Some(c.as_str()) {
                                    it.channel = Some(c);
                                    changed = true;
                                }
                            }
                        }
                    }
                }
                if changed {
                    me_meta.emit_queue_updated();
                }
            }
        });

        let settings_snapshot: Settings = self.settings.get();
        let outcome = self.runner.run_download(
            &item,
            &settings_snapshot,
            resume,
            cancel.clone(),
            tx,
            meta_tx,
            output_stem.clone(),
        ).await;

        self.cancel_tokens.lock().unwrap().remove(&id);
        drop(permit);

        match outcome {
            Ok(RunOutcome::Completed { output_path, title, thumbnail, channel }) => {
                let mut map = self.items.write().unwrap();
                if let Some(it) = map.get_mut(&id) {
                    it.state = DownloadState::Completed;
                    it.output_path = output_path.clone();
                    it.finished_at = Some(Utc::now());
                    // Same url_is_cdn guard as the meta channel: when the URL
                    // was rewritten by url_resolver to a CDN one, yt-dlp's
                    // resolved title is the CDN file ID, not the real video
                    // title. Keep the pre-filled value in those cases.
                    if !url_is_cdn {
                        if let Some(t) = title {
                            if !t.trim().is_empty() {
                                it.title = t;
                            }
                        }
                        if let Some(ch) = channel {
                            if !ch.trim().is_empty() {
                                it.channel = Some(ch);
                            }
                        }
                    }
                    if let Some(th) = thumbnail {
                        if !th.trim().is_empty() {
                            it.thumbnail = Some(th);
                        }
                    }
                    // Post-rename CDN-specific (Douyin): khi yt-dlp lưu file
                    // với tên CDN ID xấu → đổi sang title đẹp đã scrape được.
                    // Khi trùng, dùng auto_rename để thêm ` (1)`, ` (2)` thay vì
                    // bỏ qua (cũ: chỉ rename khi !target.exists() ⇒ user thấy
                    // tên CDN ID gibberish khi tải lại video Douyin).
                    if url_is_cdn {
                        if let Some(ref cur_path) = it.output_path {
                            let title = it.title.clone();
                            let sanitized = crate::filename_resolver::sanitize(&title);
                            let parent = cur_path.parent().map(|p| p.to_path_buf());
                            let ext = cur_path
                                .extension()
                                .and_then(|s| s.to_str())
                                .unwrap_or("mp4")
                                .to_string();
                            if !sanitized.is_empty() {
                                if let Some(parent) = parent {
                                    let target = crate::filename_resolver::auto_rename(
                                        &parent,
                                        &sanitized,
                                        &ext,
                                        |p| p.exists() && p != cur_path.as_path(),
                                    );
                                    if cur_path != &target {
                                        if std::fs::rename(cur_path, &target).is_ok() {
                                            it.output_path = Some(target);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                let snap = map.get(&id).cloned();
                drop(map);
                if let Some(it) = snap {
                    self.emit_state(&it);
                    self.emit_queue_updated();
                    let _ = self.app.emit(EV_DOWNLOAD_COMPLETED, CompletedEventPayload {
                        short_id: it.short_id.clone(),
                        output_path: it.output_path.clone().unwrap_or_default(),
                        title: it.title.clone(),
                    });
                    let _ = self.history.insert(&to_history(&it, HistoryStatus::Completed, None));
                    notification::notify_completed(&self.app, &settings_snapshot, &it);
                }
            }
            Ok(RunOutcome::Cancelled) => {
                let mut map = self.items.write().unwrap();
                if let Some(it) = map.get_mut(&id) {
                    if it.state != DownloadState::Paused {
                        it.state = DownloadState::Cancelled;
                        it.finished_at = Some(Utc::now());
                    }
                }
                let snap = map.get(&id).cloned();
                drop(map);
                if let Some(it) = snap {
                    // Dọn cache rác khi cancel (đảm bảo state == Cancelled, không phải Paused).
                    if matches!(it.state, DownloadState::Cancelled) {
                        cleanup_partials_retry(it.clone());
                    }
                    self.emit_state(&it);
                    self.emit_queue_updated();
                    // Không lưu Cancelled vào History — user chủ động huỷ thì
                    // đó là hành động tạm thời, không phải kết quả tải. History
                    // chỉ chứa các mục Completed.
                }
            }
            Ok(RunOutcome::Skipped) => {
                // Video đã có trong download-archive → bỏ qua. Đánh dấu Skipped
                // (không lưu History, không báo lỗi). UI hiện "Đã bỏ qua".
                let snap = {
                    let mut map = self.items.write().unwrap();
                    if let Some(it) = map.get_mut(&id) {
                        it.state = DownloadState::Skipped;
                        it.finished_at = Some(Utc::now());
                        it.error_message = None;
                    }
                    map.get(&id).cloned()
                };
                if let Some(it) = snap {
                    self.emit_state(&it);
                    self.emit_queue_updated();
                }
            }
            Ok(RunOutcome::Failed { reason }) => {
                self.clone().handle_failure(id.clone(), reason, settings_snapshot.clone());
            }
            Err(e) => {
                self.clone().handle_failure(id.clone(), format!("{e}"), settings_snapshot.clone());
            }
        }
    }

    fn handle_failure(self: Arc<Self>, id: String, reason: String, settings_snapshot: Settings) {
        // How many times to keep retrying a rate-limited item (each after a long
        // cooldown). 30 × 10 min ≈ 5h — generous so big batches finish unattended.
        const BOT_RETRY_CAP: u8 = 30;
        // 403 trên YouTube (sau khi runner đã thử client dự phòng) = IP/phiên
        // đang bị YouTube đánh dấu, hoặc yt-dlp chưa có fix cho player mới —
        // xử như bot wall: cooldown rồi retry (proxy xoay + yt-dlp có thể đã
        // tự update nightly trong lúc chờ) thay vì fail cứng.
        let is_forbidden_yt = crate::error::is_forbidden_error(&reason) && {
            let map = self.items.read().unwrap();
            map.get(&id)
                .map(|it| crate::args_builder::is_youtube(&it.request.url))
                .unwrap_or(false)
        };
        let is_bot = crate::error::is_bot_error(&reason) || is_forbidden_yt;
        // Thông báo user-facing: tiếng Việt rõ ràng + hướng dẫn làm gì tiếp
        // (raw reason giữ lại ở dòng "Chi tiết kỹ thuật" để chẩn đoán).
        let friendly = crate::error::friendly_reason(&reason);
        if is_bot {
            // YouTube vá kiểu chặn mới ở yt-dlp nightly trong vài giờ-vài ngày
            // → ép check update ngay (throttle 1h) để lần retry chạy binary mới.
            if let Ok(dd) = self.app.path().app_data_dir() {
                crate::ytdlp_update::spawn_forced_update(self.app.clone(), dd);
            }
        }
        // Decide retry vs fail under a short-lived guard.
        let (should_retry, delay) = {
            let mut map = self.items.write().unwrap();
            let it = match map.get_mut(&id) { Some(i) => i, None => return };
            if is_bot {
                // Rate-limited / bot wall: don't give up — wait the configured
                // cooldown and retry (counted separately from `attempt`). The
                // IP cools down (or a proxy rotates in) and it eventually lands.
                if it.bot_retries < BOT_RETRY_CAP {
                    it.bot_retries += 1;
                    let mins = settings_snapshot.rate_limit_cooldown_min.max(1);
                    let now = chrono::Local::now();
                    let retry_at = now + chrono::Duration::minutes(mins as i64);
                    it.error_message = Some(format!(
                        "⏳ YouTube đang giới hạn (lúc {}) — app sẽ TỰ tải lại lúc {} (lần {}). \
                         Không cần làm gì; muốn nhanh hơn thì mở Cài đặt → bấm \"Sửa lỗi tải ngay\".",
                        now.format("%H:%M"),
                        retry_at.format("%H:%M"),
                        it.bot_retries
                    ));
                    it.state = DownloadState::Queued; // show as waiting, not failed
                    let cloned = it.clone();
                    drop(map);
                    self.emit_state(&cloned);
                    self.emit_queue_updated();
                    (true, Duration::from_secs(mins as u64 * 60))
                } else {
                    it.error_message = Some(friendly.clone());
                    (false, Duration::default())
                }
            } else {
                let delay = next_retry_delay(it.attempt);
                it.attempt += 1;
                it.error_message = Some(friendly.clone());
                (delay.is_some(), delay.unwrap_or_default())
            }
        };
        if should_retry {
            // Spawn separate task for the backoff sleep so the parent
            // worker future stays simple and Send.
            let me = self.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(delay).await;
                {
                    let mut map = me.items.write().unwrap();
                    if let Some(it) = map.get_mut(&id) {
                        it.state = DownloadState::Queued;
                    }
                }
                me.emit_queue_updated();
                let me2 = me.clone();
                tauri::async_runtime::spawn(async move { me2.run_loop_for(id).await; });
            });
        } else {
            let snap = {
                let mut map = self.items.write().unwrap();
                if let Some(it) = map.get_mut(&id) {
                    it.state = DownloadState::Failed;
                    it.finished_at = Some(Utc::now());
                }
                map.get(&id).cloned()
            };
            if let Some(it) = snap {
                cleanup_partials(&it);
                self.emit_state(&it);
                self.emit_queue_updated();
                let _ = self.app.emit(EV_DOWNLOAD_FAILED, FailedEventPayload {
                    short_id: it.short_id.clone(),
                    reason: friendly.clone(),
                });
                // Không lưu Failed vào History — chỉ lưu các mục đã tải xong
                // thực sự. Mục Failed vẫn hiện trong "Đang tải" để user thấy
                // lý do và Retry; chỉ khi cancel/refresh queue mới biến mất.
                notification::notify_failed(&self.app, &settings_snapshot, &it, &friendly);
            }
        }
    }
}

fn to_history(item: &DownloadItem, status: HistoryStatus, error: Option<String>) -> HistoryEntry {
    HistoryEntry {
        short_id: item.short_id.clone(),
        url: item.request.url.clone(),
        title: item.title.clone(),
        extractor: item.extractor.clone(),
        format_id: item.request.format_id.clone(),
        mode: match item.request.mode { DownloadMode::Video => DownloadMode::Video, DownloadMode::Audio => DownloadMode::Audio },
        save_folder: item.request.save_folder.clone(),
        output_path: item.output_path.clone(),
        status,
        error,
        finished_at: item.finished_at.unwrap_or_else(Utc::now),
        channel: item.channel.clone(),
        thumbnail: item.thumbnail.clone(),
        edited: false,
        edited_at: None,
    }
}

fn log_err(_e: &AppError) { /* hook for future logging */ }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::DownloadState::*;
    use crate::models::QueueEvent::*;

    #[test]
    fn temp_file_detection_never_matches_completed() {
        // File TẠM → true (được phép dọn).
        assert!(is_ytdlp_temp_file("video.mp4.part"));
        assert!(is_ytdlp_temp_file("video.ytdl"));
        assert!(is_ytdlp_temp_file("video.f137.mp4"));
        assert!(is_ytdlp_temp_file("video.f251.webm"));
        assert!(is_ytdlp_temp_file("GEECHI GOTTI vs JOEY.mp4.part-Frag238"));
        assert!(is_ytdlp_temp_file("clip.temp.mp4"));
        assert!(is_ytdlp_temp_file("seg.frag"));
        assert!(is_ytdlp_temp_file("video.mp4.aria2"), "file dieu khien aria2c");
        assert!(is_ytdlp_temp_file("video.f251.webm.part.aria2"));

        // Video HOÀN CHỈNH → false (TUYỆT ĐỐI không được dọn).
        assert!(!is_ytdlp_temp_file("GEECHI GOTTI vs JOEY LINWOOD Rap Battle BMBL.mp4"));
        assert!(!is_ytdlp_temp_file("video.mp4"));
        assert!(!is_ytdlp_temp_file("song.mp3"));
        assert!(!is_ytdlp_temp_file("clip.for.you.mp4"), "\".f\" trong \"for\" KHÔNG được coi là file tạm");
        assert!(!is_ytdlp_temp_file("My.Final.Cut.mp4"));
        assert!(!is_ytdlp_temp_file("movie.flv"));
    }

    fn mk_item(folder: &std::path::Path, title: &str, output: Option<&str>) -> DownloadItem {
        use crate::models::{ConflictPolicy, DownloadMode, DownloadRequest};
        DownloadItem {
            short_id: "id1".into(),
            request: DownloadRequest {
                url: "https://www.youtube.com/watch?v=abc".into(),
                mode: DownloadMode::Video,
                format_id: None,
                save_folder: folder.to_path_buf(),
                sub_langs: vec![],
                auto_translate_to: None,
                on_conflict: ConflictPolicy::Ask,
                use_aria2c: false,
                playlist_all: false,
                polite: false,
                force_redownload: false,
                max_height: None,
            },
            title: title.into(),
            thumbnail: None,
            channel: None,
            extractor: "youtube".into(),
            state: Cancelled,
            bytes_downloaded: 0,
            bytes_total: None,
            speed_bps: None,
            eta_sec: None,
            attempt: 0,
            bot_retries: 0,
            error_message: None,
            output_path: output.map(|o| folder.join(o)),
            created_at: Utc::now(),
            finished_at: None,
        }
    }

    #[test]
    fn base_key_reduces_all_streams_to_same_title() {
        // Video-only, audio-only, mảnh, đã-gộp-dở → cùng 1 khoá gốc.
        assert_eq!(base_key("They Told Us….f401.mp4.part"), base_key("They Told Us….f251.webm.part"));
        assert_eq!(base_key("Clip.f137.mp4"), base_key("Clip.mp4.part"));
        assert_eq!(base_key("Clip.mp4.part-Frag12"), base_key("Clip.webm.part"));
        // Video khác → khoá khác.
        assert_ne!(base_key("Clip A.f401.mp4.part"), base_key("Clip B.f401.mp4.part"));
    }

    #[test]
    fn cleanup_removes_only_this_items_temp_files() {
        let dir = std::env::temp_dir().join(format!("bqd-cleanup-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0)));
        std::fs::create_dir_all(&dir).unwrap();
        let touch = |name: &str| std::fs::write(dir.join(name), b"x").unwrap();

        // Kịch bản THẬT: yt-dlp tải video-only trước (.f401), rồi audio-only
        // (.f251); huỷ giữa chừng để lại cả hai + mảnh. Title có ký tự "…".
        touch("They Told Us the WRONG Weight….f401.mp4");        // video-only đã xong (temp)
        touch("They Told Us the WRONG Weight….f251.webm.part");  // audio-only đang tải
        touch("They Told Us the WRONG Weight….mp4.part-Frag8");  // mảnh
        touch("They Told Us the WRONG Weight….mp4.part");        // file gộp dở
        // Video HOÀN CHỈNH của mục này → TUYỆT ĐỐI giữ.
        touch("They Told Us the WRONG Weight….mp4");
        // Video KHÁC (cùng vài chữ đầu) → KHÔNG được đụng.
        touch("They Told A Completely Different Story.mp4.part");
        touch("Another Clip.f401.mp4.part");

        // output_path = luồng yt-dlp đang ghi (audio-only), như thực tế.
        let item = mk_item(&dir, "They Told Us the WRONG Weight…",
            Some("They Told Us the WRONG Weight….f251.webm"));
        cleanup_partials_inner(&item, true);

        let exists = |n: &str| dir.join(n).exists();
        assert!(!exists("They Told Us the WRONG Weight….f401.mp4"), "video-only phải bị xoá");
        assert!(!exists("They Told Us the WRONG Weight….f251.webm.part"), "audio-only phải bị xoá");
        assert!(!exists("They Told Us the WRONG Weight….mp4.part-Frag8"), "mảnh phải bị xoá");
        assert!(!exists("They Told Us the WRONG Weight….mp4.part"), "file gộp dở phải bị xoá");
        assert!(exists("They Told Us the WRONG Weight….mp4"), "video hoàn chỉnh phải được GIỮ");
        assert!(exists("They Told A Completely Different Story.mp4.part"), "video KHÁC không được đụng");
        assert!(exists("Another Clip.f401.mp4.part"), "video KHÁC không được đụng");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn norm_key_matches_despite_special_chars() {
        // Tên tập thật + mảnh fragment có dấu ' và ! — so "mềm" phải khớp.
        let title = "Turning Unc's Tv Off With a Different Remote!";
        let frag = "Turning Uncs Tv Off With a Different Remote.mp4.part-Frag176";
        let tkey = norm_key(title);
        assert!(tkey.len() >= 6);
        assert!(norm_key(frag).starts_with(&tkey), "phải khớp dù khác dấu ' !");
        // Video khác đầu tên → KHÔNG khớp (không xoá nhầm).
        assert!(!norm_key("Completely Different Video.mp4.part").starts_with(&tkey));
    }

    #[test]
    fn legal_transitions() {
        assert_eq!(transition(Queued, Start).unwrap(), Downloading);
        assert_eq!(transition(Downloading, Pause).unwrap(), Paused);
        assert_eq!(transition(Paused, Resume).unwrap(), Downloading);
        assert_eq!(transition(Downloading, Complete).unwrap(), Completed);
        assert_eq!(transition(Downloading, Fail).unwrap(), Failed);
        assert_eq!(transition(Failed, Retry).unwrap(), Queued);
        assert_eq!(transition(Cancelled, Retry).unwrap(), Queued);
        // Nút "Vẫn tải video này" cần retry được từ Skipped.
        assert_eq!(transition(Skipped, Retry).unwrap(), Queued);
        assert_eq!(transition(Queued, Cancel).unwrap(), Cancelled);
    }

    #[test]
    fn illegal_transitions() {
        assert!(transition(Completed, Pause).is_err());
        assert!(transition(Completed, Retry).is_err());
        assert!(transition(Skipped, Pause).is_err());
    }

    #[test]
    fn retry_delays() {
        assert_eq!(next_retry_delay(0), Some(Duration::from_millis(2000)));
        assert_eq!(next_retry_delay(1), Some(Duration::from_millis(5000)));
        assert_eq!(next_retry_delay(2), Some(Duration::from_millis(10000)));
        assert!(next_retry_delay(3).is_none());
    }
}
