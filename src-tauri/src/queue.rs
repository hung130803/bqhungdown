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
use tauri::{AppHandle, Emitter};
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
        (Failed | Cancelled, Retry) => Queued,
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

pub fn cleanup_partials_aggressive(item: &DownloadItem) {
    cleanup_partials_inner(item, true);
}

fn cleanup_partials_inner(item: &DownloadItem, aggressive: bool) {
    let folder = &item.request.save_folder;
    let title_prefix = crate::filename_resolver::sanitize(&item.title);
    // Stem from any pre-resolved output_path so we catch files yt-dlp wrote
    // under a slightly different sanitization than ours (Douyin CDN ids,
    // alternate punctuation, etc.).
    let path_stem = item
        .output_path
        .as_ref()
        .and_then(|p| p.file_stem())
        .and_then(|s| s.to_str())
        .map(String::from);

    // Aggressive: remove the canonical output file too if we know it.
    if aggressive {
        if let Some(ref p) = item.output_path {
            let _ = std::fs::remove_file(p);
        }
    }

    let entries = match std::fs::read_dir(folder) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match entry.file_name().into_string() {
            Ok(s) => s,
            Err(_) => continue,
        };
        let matches_prefix = (!title_prefix.is_empty() && name.starts_with(&title_prefix))
            || path_stem
                .as_deref()
                .map(|s| !s.is_empty() && name.starts_with(s))
                .unwrap_or(false);
        if !matches_prefix {
            continue;
        }
        // Partial markers always cleaned. In aggressive mode we additionally
        // wipe any same-stem file (could be the .mp4 yt-dlp finalized between
        // cancel signal and process exit).
        let is_partial = name.ends_with(".part")
            || name.ends_with(".ytdl")
            || name.contains(".part-")
            || name.contains(".f")
            || name.contains(".temp.")
            || name.ends_with(".frag");
        if is_partial || aggressive {
            let _ = std::fs::remove_file(&path);
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
                if let Some(it) = map.get(id) {
                    cleanup_partials_aggressive(it);
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
        if removed.is_none() {
            return Err(AppError::NotFound(id.to_string()));
        }
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
        if let Some(tok) = self.cancel_tokens.lock().unwrap().remove(id) {
            tok.cancel();
        }
        Ok(())
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
                        cleanup_partials_aggressive(&it);
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
        let is_bot = crate::error::is_bot_error(&reason);
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
                        "⏳ Bị giới hạn lúc {} — tự tải lại lúc {} (lần {})",
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
                    it.error_message = Some(reason.clone());
                    (false, Duration::default())
                }
            } else {
                let delay = next_retry_delay(it.attempt);
                it.attempt += 1;
                it.error_message = Some(reason.clone());
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
                    reason: reason.clone(),
                });
                // Không lưu Failed vào History — chỉ lưu các mục đã tải xong
                // thực sự. Mục Failed vẫn hiện trong "Đang tải" để user thấy
                // lý do và Retry; chỉ khi cancel/refresh queue mới biến mất.
                notification::notify_failed(&self.app, &settings_snapshot, &it, &reason);
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
    fn legal_transitions() {
        assert_eq!(transition(Queued, Start).unwrap(), Downloading);
        assert_eq!(transition(Downloading, Pause).unwrap(), Paused);
        assert_eq!(transition(Paused, Resume).unwrap(), Downloading);
        assert_eq!(transition(Downloading, Complete).unwrap(), Completed);
        assert_eq!(transition(Downloading, Fail).unwrap(), Failed);
        assert_eq!(transition(Failed, Retry).unwrap(), Queued);
        assert_eq!(transition(Cancelled, Retry).unwrap(), Queued);
        assert_eq!(transition(Queued, Cancel).unwrap(), Cancelled);
    }

    #[test]
    fn illegal_transitions() {
        assert!(transition(Completed, Pause).is_err());
        assert!(transition(Skipped, Retry).is_err());
    }

    #[test]
    fn retry_delays() {
        assert_eq!(next_retry_delay(0), Some(Duration::from_millis(2000)));
        assert_eq!(next_retry_delay(1), Some(Duration::from_millis(5000)));
        assert_eq!(next_retry_delay(2), Some(Duration::from_millis(10000)));
        assert!(next_retry_delay(3).is_none());
    }
}
