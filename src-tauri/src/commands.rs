//! Tauri command handlers. All commands return `Result<T, AppError>`.

use crate::error::{AppError, AppResult};
use crate::events::{ConflictEventPayload, EV_DOWNLOAD_CONFLICT, EV_SETTINGS_CHANGED};
use crate::extractors;
use crate::filename_resolver::{self, ResolveOutcome};
use crate::history_store::HistoryStore;
use crate::models::{
    BootstrapPayload, ChannelInfo, ChannelVideo, ConflictChoice, ConflictPolicy, DownloadItem, DownloadMode, DownloadOptions,
    DownloadRequest, DownloadState, ExtractorInfo, HistoryEntry, Settings, SettingsPatch,
    SubtitleTrack, UrlValidation, VideoMetadata, WatchedChannel,
};
use crate::queue::QueueManager;
use crate::settings_store::SettingsStore;
use crate::watchlist_store::WatchlistStore;
use crate::bookmarks_store::BookmarksStore;
use crate::models::Bookmark;
use crate::short_id;
use crate::sidecar_detect;
use crate::url_validator;
use crate::ytdlp_runner::YtDlpRunner;
use chrono::Utc;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

/// Pending conflict resolutions, keyed by short_id. Frontend sends choice via
/// `resolve_conflict`; queue/runner reads from this map (future use).
pub struct PendingConflicts(pub Mutex<HashMap<String, ConflictChoice>>);

impl Default for PendingConflicts {
    fn default() -> Self {
        Self(Mutex::new(HashMap::new()))
    }
}

// ---------- URL / metadata ----------

#[tauri::command]
pub fn validate_url(url: String) -> AppResult<UrlValidation> {
    Ok(url_validator::validate_url(&url))
}

#[tauri::command]
pub async fn fetch_metadata(
    url: String,
    runner: State<'_, Arc<YtDlpRunner>>,
    settings: State<'_, Arc<SettingsStore>>,
) -> AppResult<VideoMetadata> {
    let s = settings.get();
    // Pre-resolve site-specific quirks (e.g., Douyin → CDN URL via tikwm /
    // share scraping). Returns the original string + extracted metadata when
    // applicable. We use that metadata to short-circuit yt-dlp when possible
    // (Douyin's yt-dlp extractor often errors with "Fresh cookies needed").
    let (resolved, meta) = crate::url_resolver::resolve_with_meta(&url).await;

    // If url_resolver scraped a real title (Douyin share-page path), build the
    // VideoMetadata directly without invoking yt-dlp at all — yt-dlp doesn't
    // know the resolved CDN URL is video and would just error out.
    if let Some(meta) = meta {
        if meta.title.is_some() {
            let extractor = crate::url_validator::resolve_extractor(&url)
                .unwrap_or("generic")
                .to_string();
            return Ok(VideoMetadata {
                url: resolved,
                extractor,
                title: meta.title.unwrap_or_default(),
                channel: meta.channel,
                thumbnail: meta.thumbnail,
                duration_sec: None,
                formats: vec![],
                subtitles: vec![],
                playlist_entries: None,
                playlist_total: None,
            });
        }
    }

    // Fall through: ask yt-dlp like before for non-Douyin URLs.
    runner.fetch_metadata(&resolved, &s).await
}

/// Fetch a flat listing of videos from a channel/user URL. Returns up to
/// `limit` videos (default 0 = no limit). The UI then lets the user filter /
/// pick and enqueue them via `enqueue_batch`.
#[tauri::command]
pub async fn fetch_channel_videos(
    url: String,
    limit: Option<u32>,
    detailed: Option<bool>,
    tab: Option<String>,
    force_refresh: Option<bool>,
    app: AppHandle,
    settings: State<'_, Arc<SettingsStore>>,
) -> AppResult<ChannelFetchResult> {
    let s = settings.get();
    let cap = limit.unwrap_or(0).min(5000);
    let det = detailed.unwrap_or(false);
    let tab_s = tab.unwrap_or_else(|| "videos".into());
    let force = force_refresh.unwrap_or(false);
    let (info, videos) = crate::channel_fetcher::fetch_channel(
        &app, &url, cap, det, &tab_s, &s, force,
    ).await?;
    Ok(ChannelFetchResult { info, videos })
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelFetchResult {
    pub info: ChannelInfo,
    pub videos: Vec<ChannelVideo>,
}

/// Cancel any in-flight `fetch_channel_videos` call. The yt-dlp child(ren)
/// are killed and the awaiting future returns an error so the UI can clear
/// its loading state.
#[tauri::command]
pub fn cancel_channel_fetch() -> AppResult<()> {
    crate::channel_fetcher::cancel();
    Ok(())
}

/// Fetch a thumbnail URL and return it as a `data:image/...;base64,...` URL.
#[tauri::command]
pub async fn fetch_thumbnail_data_url(url: String) -> AppResult<String> {
    let lower = url.to_lowercase();
    let referer = if lower.contains("cdninstagram.com") || lower.contains("fbcdn.net") {
        Some("https://www.instagram.com/")
    } else if lower.contains("tiktokcdn") || lower.contains("byteoversea") {
        Some("https://www.tiktok.com/")
    } else if lower.contains("aweme") || lower.contains("douyin") {
        Some("https://www.douyin.com/")
    } else if lower.contains("hdslb.com") {
        Some("https://www.bilibili.com/")
    } else if lower.contains("bstarstatic.com") {
        Some("https://www.bilibili.tv/")
    } else {
        None
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
        )
        .build()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let mut req = client.get(&url);
    if let Some(r) = referer {
        req = req.header("Referer", r);
    }
    let resp = req.send().await.map_err(|e| AppError::Other(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(AppError::Other(format!("HTTP {}", resp.status())));
    }
    let mime = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .unwrap_or_else(|| "image/jpeg".to_string());
    let bytes = resp.bytes().await.map_err(|e| AppError::Other(e.to_string()))?;
    use data_encoding::BASE64;
    let b64 = BASE64.encode(&bytes);
    Ok(format!("data:{mime};base64,{b64}"))
}

// ---------- Enqueue ----------

pub(crate) fn build_request(url: String, options: DownloadOptions, settings: &Settings) -> DownloadRequest {
    DownloadRequest {
        url,
        mode: options.mode,
        format_id: options.format_id,
        save_folder: options.save_folder,
        sub_langs: options.sub_langs,
        auto_translate_to: options.auto_translate_to,
        on_conflict: options.on_conflict,
        use_aria2c: settings.aria2c_enabled,
        playlist_all: options.playlist_all.unwrap_or(false),
        polite: options.polite.unwrap_or(false),
        force_redownload: false,
    }
}

pub(crate) fn make_item(
    req: DownloadRequest,
    title: Option<String>,
    thumbnail: Option<String>,
    extractor: Option<String>,
    channel: Option<String>,
    taken: &std::collections::HashSet<String>,
) -> DownloadItem {
    let now = Utc::now();
    let ts_ms = now.timestamp_millis();
    let short_id = short_id::generate(&req.url, ts_ms, taken);
    let extractor = extractor.unwrap_or_else(|| {
        url_validator::resolve_extractor(&req.url)
            .unwrap_or("generic")
            .to_string()
    });
    DownloadItem {
        short_id,
        request: req.clone(),
        title: title.unwrap_or_else(|| req.url.clone()),
        thumbnail,
        channel,
        extractor,
        state: DownloadState::Queued,
        bytes_downloaded: 0,
        bytes_total: None,
        speed_bps: None,
        eta_sec: None,
        attempt: 0,
        bot_retries: 0,
        error_message: None,
        output_path: None,
        created_at: now,
        finished_at: None,
    }
}

#[tauri::command]
pub async fn enqueue_download(
    url: String,
    options: DownloadOptions,
    title: Option<String>,
    thumbnail: Option<String>,
    extractor: Option<String>,
    channel: Option<String>,
    queue: State<'_, Arc<QueueManager>>,
    settings: State<'_, Arc<SettingsStore>>,
    history: State<'_, Arc<HistoryStore>>,
) -> AppResult<DownloadItem> {
    // Pre-resolve site-specific quirks (e.g., viralhog watch → embed URL).
    // For Douyin we also get back title/thumbnail/channel scraped from the
    // share page so the queue item shows nice metadata even though we're
    // feeding yt-dlp a raw CDN URL.
    let (url, meta) = crate::url_resolver::resolve_with_meta(&url).await;
    let title = title.or_else(|| meta.as_ref().and_then(|m| m.title.clone()));
    let thumbnail = thumbnail.or_else(|| meta.as_ref().and_then(|m| m.thumbnail.clone()));
    let channel = channel.or_else(|| meta.as_ref().and_then(|m| m.channel.clone()));

    let s = settings.get();
    if !options.save_folder.exists() {
        return Err(AppError::SaveFolderUnavailable(options.save_folder.clone()));
    }
    // Dedup #1: if the same URL is already in queue in a non-terminal state,
    // return it (cùng URL đang tải dở → không tạo bản trùng).
    if let Some(existing) = queue.list().into_iter().find(|it| it.request.url == url && !is_terminal(it.state)) {
        return Ok(existing);
    }
    // NOTE: Dedup #2 (skip when same URL already completed) đã bị bỏ —
    // user muốn tải lại sẽ tự động tạo file mới với tên `(1)`, `(2)`...
    // tránh ghi đè file cũ. Logic auto-rename ở filename_resolver.rs.
    let mut taken = history.known_short_ids().unwrap_or_default();
    for it in queue.list() {
        taken.insert(it.short_id);
    }
    let req = build_request(url, options, &s);
    let item = make_item(req, title, thumbnail, extractor, channel, &taken);
    queue.enqueue(item.clone())?;
    Ok(item)
}

fn is_terminal(s: DownloadState) -> bool {
    matches!(s, DownloadState::Completed | DownloadState::Failed | DownloadState::Cancelled | DownloadState::Skipped)
}

#[tauri::command]
pub async fn enqueue_batch(
    urls: Vec<String>,
    options: DownloadOptions,
    queue: State<'_, Arc<QueueManager>>,
    settings: State<'_, Arc<SettingsStore>>,
    history: State<'_, Arc<HistoryStore>>,
) -> AppResult<Vec<DownloadItem>> {
    let s = settings.get();
    // Make sure the target folder exists (channel downloads use a per-channel
    // subfolder that may not exist yet).
    let _ = std::fs::create_dir_all(&options.save_folder);
    let mut taken = history.known_short_ids().unwrap_or_default();
    let mut existing_active: std::collections::HashSet<String> = std::collections::HashSet::new();
    for it in queue.list() {
        taken.insert(it.short_id.clone());
        if !is_terminal(it.state) {
            existing_active.insert(it.request.url.clone());
        }
    }
    let mut out = Vec::new();
    let mut seen_in_batch: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Resolve every URL with metadata. Sequential is OK — share-page scrape
    // per URL is ~300-800ms, doing 10-20 URLs in a batch takes a few seconds
    // which feels fine since the user already pasted them all at once.
    for orig_url in urls {
        let (url, meta) = crate::url_resolver::resolve_with_meta(&orig_url).await;
        if seen_in_batch.contains(&url) || existing_active.contains(&url) {
            continue;
        }
        seen_in_batch.insert(url.clone());
        let v = url_validator::validate_url(&url);
        if !v.valid && !url_validator::validate_url(&orig_url).valid {
            continue;
        }
        // Use original URL's extractor (e.g. "douyin") so the platform badge /
        // colour stay correct, even when the resolver swapped url to a raw CDN.
        let extractor = url_validator::resolve_extractor(&orig_url)
            .map(|s| s.to_string())
            .or(v.extractor);
        let title = meta.as_ref().and_then(|m| m.title.clone());
        let thumbnail = meta.as_ref().and_then(|m| m.thumbnail.clone());
        let channel = meta.as_ref().and_then(|m| m.channel.clone());

        let req = build_request(url, options.clone(), &s);
        let item = make_item(req, title, thumbnail, extractor, channel, &taken);
        taken.insert(item.short_id.clone());
        queue.enqueue(item.clone())?;
        out.push(item);
    }
    Ok(out)
}

#[tauri::command]
pub async fn enqueue_playlist(
    playlist_url: String,
    selected: Vec<String>,
    options: DownloadOptions,
    all_with_yes_playlist: Option<bool>,
    queue: State<'_, Arc<QueueManager>>,
    settings: State<'_, Arc<SettingsStore>>,
    history: State<'_, Arc<HistoryStore>>,
) -> AppResult<Vec<DownloadItem>> {
    let s = settings.get();
    let mut taken = history.known_short_ids().unwrap_or_default();
    for it in queue.list() {
        taken.insert(it.short_id);
    }
    let mut out = Vec::new();
    if all_with_yes_playlist.unwrap_or(false) {
        // Single item with playlist_all = true; runner expands.
        let playlist_url = crate::url_resolver::resolve(&playlist_url).await;
        let mut opts = options.clone();
        opts.playlist_all = Some(true);
        let req = build_request(playlist_url.clone(), opts, &s);
        let item = make_item(req, None, None, None, None, &taken);
        taken.insert(item.short_id.clone());
        queue.enqueue(item.clone())?;
        out.push(item);
    } else {
        for url in selected {
            let url = crate::url_resolver::resolve(&url).await;
            let req = build_request(url.clone(), options.clone(), &s);
            let item = make_item(
                req,
                None,
                None,
                url_validator::resolve_extractor(&url).map(|s| s.to_string()),
                None,
                &taken,
            );
            taken.insert(item.short_id.clone());
            queue.enqueue(item.clone())?;
            out.push(item);
        }
    }
    Ok(out)
}

// ---------- Queue control ----------

#[tauri::command]
pub fn pause_download(short_id: String, queue: State<Arc<QueueManager>>) -> AppResult<()> {
    queue.pause(&short_id)
}

#[tauri::command]
pub fn resume_download(short_id: String, queue: State<Arc<QueueManager>>) -> AppResult<()> {
    queue.resume(&short_id)
}

#[tauri::command]
pub fn cancel_download(short_id: String, queue: State<Arc<QueueManager>>) -> AppResult<()> {
    queue.cancel(&short_id)
}

#[tauri::command]
pub fn retry_download(
    short_id: String,
    queue: State<Arc<QueueManager>>,
) -> AppResult<DownloadItem> {
    queue.retry(&short_id)
}

/// Nút "Thử lại tất cả video lỗi": re-queue mọi mục Failed một phát.
/// Trả về số mục đã đưa lại vào hàng đợi.
#[tauri::command]
pub fn retry_all_failed(queue: State<Arc<QueueManager>>) -> AppResult<u32> {
    Ok(queue.retry_all_failed() as u32)
}

/// Nút "Tạm dừng tất cả" — dừng mọi mục đang tải + chặn khởi động mục mới.
#[tauri::command]
pub fn pause_all_downloads(queue: State<Arc<QueueManager>>) -> AppResult<u32> {
    Ok(queue.pause_all() as u32)
}

/// Nút "Tiếp tục tất cả" — chạy lại mọi mục đang tạm dừng/chờ.
#[tauri::command]
pub fn resume_all_downloads(queue: State<Arc<QueueManager>>) -> AppResult<u32> {
    Ok(queue.resume_all() as u32)
}

/// Nút "Vẫn tải video này" trên mục Bỏ qua — tải bất chấp danh sách đã-tải.
#[tauri::command]
pub fn force_download(
    short_id: String,
    queue: State<Arc<QueueManager>>,
) -> AppResult<DownloadItem> {
    queue.force_download(&short_id)
}

/// Nút "Kiểm tra proxy": thử kết nối Internet QUA proxy đã nhập và trả về
/// kết quả cho user (sống + độ trễ, hay chết + lý do). Dùng cùng cơ chế
/// normalize (socks5→socks5h) như lúc tải thật, nên kết quả phản ánh đúng
/// việc tải sẽ chạy hay không. Test tới generate_204 của Google (nhẹ, toàn
/// cầu, trả 204 rỗng).
#[tauri::command]
pub async fn test_proxy(proxy: String) -> AppResult<String> {
    let raw = proxy.trim();
    if raw.is_empty() {
        return Err(AppError::Other("Chưa nhập proxy".into()));
    }
    let normalized = crate::args_builder::normalize_proxy(raw);
    let p = reqwest::Proxy::all(&normalized)
        .map_err(|e| AppError::Other(format!("Proxy sai định dạng: {e}")))?;
    let client = reqwest::Client::builder()
        .proxy(p)
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::Other(e.to_string()))?;

    // Hỏi IP + quốc gia LỐI RA qua proxy (ipinfo.io — HTTPS, miễn phí, nhẹ).
    // Một request vừa xác nhận proxy sống, vừa cho biết IP thoát ở nước nào →
    // user biết proxy có mở được video khoá theo vùng hay không.
    let started = std::time::Instant::now();
    match client.get("https://ipinfo.io/json").send().await {
        Ok(resp) => {
            let ms = started.elapsed().as_millis();
            if !resp.status().is_success() {
                let code = resp.status().as_u16();
                return Ok(format!(
                    "⚠️ Proxy có phản hồi nhưng mã lạ ({code}) — có thể vẫn tải được, thử 1 video."
                ));
            }
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let ip = body.get("ip").and_then(|v| v.as_str()).unwrap_or("?");
            let cc = body.get("country").and_then(|v| v.as_str()).unwrap_or("");
            let country = country_vi(cc);
            Ok(format!("✅ Proxy sống ({ms}ms) — IP {ip} · {country}. Tải qua proxy này được."))
        }
        Err(e) => {
            let msg = e.to_string().to_lowercase();
            let hint = if msg.contains("timed out") || msg.contains("timeout") {
                "proxy quá chậm hoặc chết (hết 15s không phản hồi)"
            } else if msg.contains("connect") || msg.contains("refused") || msg.contains("dns") {
                "không kết nối được tới proxy — kiểm tra lại IP/cổng/tài khoản"
            } else {
                "proxy không dùng được"
            };
            Err(AppError::Other(format!("❌ {hint}. ({e})")))
        }
    }
}

/// Đổi mã quốc gia ISO 2 chữ → tên tiếng Việt (kèm cờ) cho các nước hay gặp;
/// nước lạ thì trả về "<Quốc gia CC>" để vẫn đọc được.
fn country_vi(cc: &str) -> String {
    let name = match cc {
        "VN" => "🇻🇳 Việt Nam",
        "SG" => "🇸🇬 Singapore",
        "JP" => "🇯🇵 Nhật Bản",
        "KR" => "🇰🇷 Hàn Quốc",
        "US" => "🇺🇸 Mỹ",
        "HK" => "🇭🇰 Hồng Kông",
        "TW" => "🇹🇼 Đài Loan",
        "CN" => "🇨🇳 Trung Quốc",
        "TH" => "🇹🇭 Thái Lan",
        "MY" => "🇲🇾 Malaysia",
        "ID" => "🇮🇩 Indonesia",
        "PH" => "🇵🇭 Philippines",
        "IN" => "🇮🇳 Ấn Độ",
        "GB" => "🇬🇧 Anh",
        "DE" => "🇩🇪 Đức",
        "FR" => "🇫🇷 Pháp",
        "NL" => "🇳🇱 Hà Lan",
        "RU" => "🇷🇺 Nga",
        "" => return "quốc gia không rõ".to_string(),
        other => return format!("Quốc gia {other}"),
    };
    name.to_string()
}

#[tauri::command]
pub fn list_queue(queue: State<Arc<QueueManager>>) -> AppResult<Vec<DownloadItem>> {
    Ok(queue.list())
}

/// Remove a single queue item (no file deletion). Used when the user dismisses
/// a row whose file is missing on disk, or simply wants to clean up. If the
/// item is still active, this also cancels its download.
#[tauri::command]
pub fn remove_queue_item(short_id: String, queue: State<Arc<QueueManager>>) -> AppResult<()> {
    queue.remove_item(&short_id)
}

/// Remove every queued item saving into `folder` (i.e. a whole channel the user
/// dropped). Cancels active ones + cleans partials. Returns count removed.
#[tauri::command]
pub fn remove_queue_group(folder: String, queue: State<Arc<QueueManager>>) -> AppResult<usize> {
    Ok(queue.remove_group(std::path::Path::new(&folder)))
}

/// Undo the last "Xóa cả kênh" (restore the removed group). Returns count.
#[tauri::command]
pub fn undo_remove_group(queue: State<Arc<QueueManager>>) -> AppResult<usize> {
    Ok(queue.undo_remove_group())
}

/// Cheap existence check used by the UI before opening a file. Returns false
/// for missing/inaccessible paths instead of erroring.
#[tauri::command]
pub fn path_exists(path: String) -> AppResult<bool> {
    Ok(std::path::Path::new(&path).exists())
}

#[tauri::command]
pub fn resolve_conflict(
    short_id: String,
    choice: ConflictChoice,
    pending: State<PendingConflicts>,
) -> AppResult<()> {
    pending.0.lock().insert(short_id, choice);
    Ok(())
}

// ---------- Settings ----------

#[tauri::command]
pub fn get_settings(settings: State<Arc<SettingsStore>>) -> AppResult<Settings> {
    Ok(settings.get())
}

#[tauri::command]
pub async fn update_settings(
    patch: SettingsPatch,
    settings: State<'_, Arc<SettingsStore>>,
    queue: State<'_, Arc<QueueManager>>,
    app: AppHandle,
) -> AppResult<Settings> {
    let new_concurrency = patch.max_concurrency;
    let po_change = patch.po_token_enabled;
    let next = settings.apply_patch(patch)?;
    if let Some(n) = new_concurrency {
        queue.set_concurrency(n).await;
    }
    // Start/stop the PO token provider live when the toggle flips.
    if let Some(enabled) = po_change {
        let po_arc = app
            .state::<Arc<crate::po_token::ProviderProcess>>()
            .inner()
            .clone();
        let yt_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|x| x.to_path_buf()));
        if enabled {
            if let Ok(dd) = app.path().app_data_dir() {
                crate::po_token::enable(app.clone(), dd, yt_dir, po_arc);
            }
        } else {
            crate::po_token::shutdown(&po_arc);
            if let Some(dir) = yt_dir.as_deref() {
                crate::po_token::uninstall_plugin(dir);
            }
        }
    }
    let _ = app.emit(EV_SETTINGS_CHANGED, next.clone());
    Ok(next)
}

/// Kiểm tra YouTube Data API key có dùng được không. Trả Ok(()) → đèn XANH;
/// Err(thông báo) → đèn ĐỎ kèm lý do (key sai / chưa bật API / hết quota).
#[tauri::command]
pub async fn validate_youtube_api_key(key: String) -> AppResult<()> {
    crate::youtube_api::validate_key(&key).await
}

// ---------- Filesystem helpers ----------

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> AppResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |path| {
        let _ = tx.send(path);
    });
    let path = rx.await.map_err(|e| AppError::Other(e.to_string()))?;
    Ok(path
        .and_then(|fp| fp.into_path().ok())
        .map(|p| p.to_string_lossy().to_string()))
}

/// Pick a single file. Used for the cookies.txt picker in Settings.
#[tauri::command]
pub async fn pick_file(app: AppHandle) -> AppResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Cookies / Text", &["txt"])
        .pick_file(move |path| {
            let _ = tx.send(path);
        });
    let path = rx.await.map_err(|e| AppError::Other(e.to_string()))?;
    Ok(path
        .and_then(|fp| fp.into_path().ok())
        .map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
pub fn check_folder_writable(path: String) -> AppResult<bool> {
    let p = PathBuf::from(&path);
    if !p.exists() || !p.is_dir() {
        return Ok(false);
    }
    let probe = p.join(".prodown_write_probe");
    let ok = std::fs::write(&probe, b"x").is_ok();
    let _ = std::fs::remove_file(&probe);
    Ok(ok)
}

#[tauri::command]
pub fn open_in_folder(path: String) -> AppResult<()> {
    // Normalize forward slashes to backslashes on Windows so explorer.exe accepts them.
    #[cfg(target_os = "windows")]
    let path = path.replace('/', "\\");
    let p = PathBuf::from(&path);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // Highlight the file in its folder via `explorer /select,"<path>"`.
        // CRITICAL: rust's Command::arg() wraps args containing spaces in quotes,
        // turning the whole `/select,C:\path\with space\file.mp4` into a single
        // quoted token. Explorer cannot parse that and silently falls back to
        // the user's home/Documents folder. We use `raw_arg` to pass the exact
        // command-line tail Explorer expects, with the path quoted internally.
        if p.is_file() {
            let raw = format!("/select,\"{}\"", p.display());
            std::process::Command::new("explorer")
                .raw_arg(raw)
                .spawn()
                .map_err(AppError::from)?;
            return Ok(());
        }
    }
    let target = if p.is_file() {
        p.parent().map(|x| x.to_path_buf()).unwrap_or(p)
    } else {
        p
    };
    open_path_with_os(&target)
}

/// Open a file with the OS default app (e.g. play a video in the system player).
#[tauri::command]
pub fn open_file(path: String) -> AppResult<()> {
    #[cfg(target_os = "windows")]
    let path = path.replace('/', "\\");
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(AppError::NotFound(path));
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // `explorer <file>` opens an Explorer window, not the default media player.
        // Use `cmd /c start "" "<path>"` so Windows resolves the file association
        // (Movies & TV, VLC, etc.). The empty `""` is the window title argument
        // that `start` requires when the path is quoted.
        let raw = format!("/c start \"\" \"{}\"", p.display());
        std::process::Command::new("cmd")
            .raw_arg(raw)
            // Hide the brief cmd console window. 0x08000000 = CREATE_NO_WINDOW.
            .creation_flags(0x08000000)
            .spawn()
            .map_err(AppError::from)?;
        return Ok(());
    }
    #[cfg(not(target_os = "windows"))]
    {
        open_path_with_os(&p)
    }
}

/// Best-effort: find the actual file on disk for a download whose `output_path`
/// was lost. Searches RECURSIVELY (depth ≤ 3) inside `save_folder` ONLY — we
/// intentionally do not fall back to the parent or `dirs::download_dir()`
/// because that produced false matches against unrelated files in sibling
/// folders (e.g., random files in `Documents` or other Downloads subfolders).
#[tauri::command]
pub fn find_output_file(save_folder: String, title: String) -> AppResult<Option<String>> {
    let folder = PathBuf::from(&save_folder);
    if !folder.is_dir() { return Ok(None); }

    let sanitized = crate::filename_resolver::sanitize(&title).to_lowercase();
    if sanitized.is_empty() { return Ok(None); }

    let exts = ["mp4", "mkv", "webm", "mov", "m4a", "mp3", "opus", "flac", "wav"];
    let mut best: Option<(PathBuf, u64)> = None;

    scan_dir(&folder, &sanitized, &exts, &mut best, 0);

    Ok(best.map(|(p, _)| p.to_string_lossy().to_string()))
}

fn scan_dir(
    dir: &std::path::Path,
    needle: &str,
    exts: &[&str],
    best: &mut Option<(PathBuf, u64)>,
    depth: u32,
) {
    if depth > 3 { return; }
    let entries = match std::fs::read_dir(dir) { Ok(e) => e, Err(_) => return };
    for entry in entries.flatten() {
        let p = entry.path();
        let ft = match entry.file_type() { Ok(t) => t, Err(_) => continue };
        if ft.is_dir() {
            scan_dir(&p, needle, exts, best, depth + 1);
            continue;
        }
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
        let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
        if !exts.contains(&ext.as_str()) { continue; }
        // Match if stem contains the sanitized title (or first 30 chars of it).
        // Use a CHAR boundary, not a byte index — slicing `&needle[..30]` mid
        // codepoint panics on multibyte titles (Korean/Vietnamese/etc.).
        let needle_short: &str = match needle.char_indices().nth(30) {
            Some((i, _)) => &needle[..i],
            None => needle,
        };
        if !stem.contains(needle_short) && !stem.starts_with(needle) { continue; }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if size < 100_000 { continue; } // skip tiny fragments
        if best.as_ref().map(|(_, s)| size > *s).unwrap_or(true) {
            *best = Some((p, size));
        }
    }
}

/// Update a history entry's `output_path` (used to backfill paths for items
/// downloaded before the FINALPATH parser was added).
#[tauri::command]
pub fn update_history_output_path(
    short_id: String,
    output_path: String,
    history: State<Arc<HistoryStore>>,
) -> AppResult<()> {
    let mut entry = match history.get(&short_id)? {
        Some(e) => e,
        None => return Err(AppError::NotFound(short_id)),
    };
    entry.output_path = Some(PathBuf::from(output_path));
    history.insert(&entry)?;
    Ok(())
}

#[tauri::command]
pub fn open_url(url: String) -> AppResult<()> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // Open URL in default browser via `cmd /c start "" "<url>"`.
        let raw = format!("/c start \"\" \"{}\"", url);
        std::process::Command::new("cmd")
            .raw_arg(raw)
            .creation_flags(0x08000000)
            .spawn()
            .map_err(AppError::from)?;
        return Ok(());
    }
    #[cfg(not(target_os = "windows"))]
    {
        open_path_with_os(&PathBuf::from(url))
    }
}

fn open_path_with_os(path: &std::path::Path) -> AppResult<()> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // Use `cmd /c start "" "<path>"` so Windows respects file/folder associations.
        // For directories this opens a new Explorer window rooted at the folder
        // (matching what users expect when clicking "Open folder").
        let raw = format!("/c start \"\" \"{}\"", path.display());
        std::process::Command::new("cmd")
            .raw_arg(raw)
            .creation_flags(0x08000000)
            .spawn()
            .map_err(AppError::from)?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(AppError::from)?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(AppError::from)?;
    }
    Ok(())
}

// ---------- History ----------

#[tauri::command]
pub fn list_history(
    query: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
    history: State<Arc<HistoryStore>>,
) -> AppResult<Vec<HistoryEntry>> {
    history.list(query.as_deref(), limit.unwrap_or(200), offset.unwrap_or(0))
}

/// Mark several history entries as edited (or unedited). Returns rows changed.
#[tauri::command]
pub fn set_history_edited(
    short_ids: Vec<String>,
    edited: bool,
    history: State<Arc<HistoryStore>>,
) -> AppResult<u64> {
    history.set_edited(&short_ids, edited)
}

#[tauri::command]
pub fn delete_history_entry(
    short_id: String,
    delete_file: Option<bool>,
    history: State<Arc<HistoryStore>>,
) -> AppResult<()> {
    let entry = history.get(&short_id)?;
    history.delete(&short_id)?;
    if delete_file.unwrap_or(false) {
        if let Some(e) = entry {
            if let Some(path) = e.output_path {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    Ok(())
}

/// Batch-delete several history entries in one IPC round-trip. Returns the
/// number of rows actually removed (entries already missing are silently
/// ignored). When `delete_files` is true, each row's `output_path` is also
/// unlinked best-effort.
#[tauri::command]
pub fn delete_history_entries(
    short_ids: Vec<String>,
    delete_files: Option<bool>,
    history: State<Arc<HistoryStore>>,
) -> AppResult<u64> {
    let also_delete = delete_files.unwrap_or(false);
    let mut removed: u64 = 0;
    for id in &short_ids {
        let entry = history.get(id).ok().flatten();
        match history.delete(id) {
            Ok(()) => removed += 1,
            Err(crate::error::AppError::NotFound(_)) => continue,
            Err(e) => return Err(e),
        }
        if also_delete {
            if let Some(e) = entry {
                if let Some(path) = e.output_path {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
    Ok(removed)
}

#[tauri::command]
pub fn clear_history(
    delete_files: Option<bool>,
    history: State<Arc<HistoryStore>>,
) -> AppResult<u64> {
    if delete_files.unwrap_or(false) {
        // Snapshot output paths before wiping the table.
        let entries = history.list(None, u32::MAX, 0)?;
        for e in entries {
            if let Some(path) = e.output_path {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    history.clear_all()
}

#[tauri::command]
pub fn redownload_from_history(
    short_id: String,
    queue: State<Arc<QueueManager>>,
    settings: State<Arc<SettingsStore>>,
    history: State<Arc<HistoryStore>>,
) -> AppResult<DownloadItem> {
    let entry = history
        .get(&short_id)?
        .ok_or_else(|| AppError::NotFound(short_id))?;
    let s = settings.get();
    let mut taken = history.known_short_ids().unwrap_or_default();
    for it in queue.list() {
        taken.insert(it.short_id);
    }
    let options = DownloadOptions {
        mode: entry.mode,
        format_id: entry.format_id,
        save_folder: entry.save_folder.clone(),
        sub_langs: vec![],
        auto_translate_to: None,
        on_conflict: ConflictPolicy::Ask,
        playlist_all: None,
        polite: None,
    };
    let req = build_request(entry.url.clone(), options, &s);
    let item = make_item(req, Some(entry.title), entry.thumbnail.clone(), Some(entry.extractor), entry.channel.clone(), &taken);
    queue.enqueue(item.clone())?;
    Ok(item)
}

// ---------- Extractors / subs ----------

#[tauri::command]
pub fn list_extractors() -> AppResult<Vec<ExtractorInfo>> {
    Ok(extractors::list_all()
        .iter()
        .map(|e| ExtractorInfo {
            name: e.name.to_string(),
            host_pattern: e.host_regex.to_string(),
            featured: e.featured,
        })
        .collect())
}

#[tauri::command]
pub async fn get_subtitle_langs(
    url: String,
    runner: State<'_, Arc<YtDlpRunner>>,
    settings: State<'_, Arc<SettingsStore>>,
) -> AppResult<Vec<SubtitleTrack>> {
    let s = settings.get();
    let md = runner.fetch_metadata(&url, &s).await?;
    Ok(md.subtitles)
}

// ---------- Clipboard watcher toggle ----------

#[tauri::command]
pub fn set_clipboard_watcher(
    enabled: bool,
    settings: State<Arc<SettingsStore>>,
) -> AppResult<()> {
    settings.apply_patch(SettingsPatch {
        clipboard_watcher: Some(enabled),
        ..Default::default()
    })?;
    Ok(())
}

// ---------- Auto-watch channels ----------

#[tauri::command]
pub fn list_watched_channels(
    store: State<Arc<WatchlistStore>>,
) -> AppResult<Vec<WatchedChannel>> {
    Ok(store.list())
}

#[tauri::command]
pub async fn add_watched_channel(
    url: String,
    tab: Option<String>,
    app: AppHandle,
    store: State<'_, Arc<WatchlistStore>>,
    queue: State<'_, Arc<QueueManager>>,
    settings: State<'_, Arc<SettingsStore>>,
    history: State<'_, Arc<HistoryStore>>,
) -> AppResult<WatchedChannel> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err(AppError::InvalidUrl);
    }
    // Already watching this URL → return the existing entry (idempotent add).
    if store.contains_url(&url) {
        if let Some(c) = store
            .list()
            .into_iter()
            .find(|c| c.url.trim_end_matches('/').to_lowercase() == url.trim_end_matches('/').to_lowercase())
        {
            return Ok(c);
        }
    }
    let taken: std::collections::HashSet<String> =
        store.list().into_iter().map(|c| c.id).collect();
    let id = short_id::generate(&url, Utc::now().timestamp_millis(), &taken);
    let channel = WatchedChannel {
        id: id.clone(),
        url: url.clone(),
        title: None,
        enabled: true,
        tab: tab.unwrap_or_else(|| "all".into()),
        added_at: Utc::now(),
        last_checked: None,
        last_new_count: None,
        last_error: None,
        channel_id: None,
        auto_download: true,
        pending: vec![],
        seen_ids: vec![],
        dest_dir: None,
        target_name: None,
        group: None,
        source_mode: "new".into(),
        auto_fetch_date: None,
        picked: vec![],
        daily_limit: 1,
        drip_date: None,
        drip_count: 0,
        done_ids: vec![],
    };
    store.add(channel)?;
    // Baseline pass: records current videos as "seen" and enqueues NOTHING
    // (seen_ids was empty), so watching starts from now, not the backlog.
    crate::watcher::check_channel(&app, store.inner(), queue.inner(), settings.inner(), history.inner(), &id).await;
    store.get(&id).ok_or_else(|| AppError::NotFound(id))
}

#[tauri::command]
pub fn remove_watched_channel(
    id: String,
    store: State<Arc<WatchlistStore>>,
) -> AppResult<()> {
    store.remove(&id)
}

#[tauri::command]
pub fn set_watched_enabled(
    id: String,
    enabled: bool,
    store: State<Arc<WatchlistStore>>,
) -> AppResult<Option<WatchedChannel>> {
    store.update(&id, |c| c.enabled = enabled)
}

/// Switch a channel between "tự tải" (auto-download) and "chỉ báo" (notify-only).
#[tauri::command]
pub fn set_watched_auto_download(
    id: String,
    auto: bool,
    store: State<Arc<WatchlistStore>>,
) -> AppResult<Option<WatchedChannel>> {
    store.update(&id, |c| c.auto_download = auto)
}

/// Đặt THƯ MỤC LƯU RIÊNG cho 1 kênh theo dõi (dây chuyền 2 tool: video mới
/// của kênh rơi vào đúng thư mục trung chuyển của kênh — INTEGRATION.md).
/// None/rỗng = quay về thư mục tải mặc định chung.
#[tauri::command]
pub fn set_watched_dest_dir(
    id: String,
    dest_dir: Option<String>,
    store: State<Arc<WatchlistStore>>,
) -> AppResult<Option<WatchedChannel>> {
    let d = dest_dir
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty());
    store.update(&id, |c| c.dest_dir = d.clone())
}

/// Lưu HÀNG CHỜ LÀM của kênh theo dõi — danh sách video user tích chọn từ
/// kho kênh nguồn. Watcher mỗi ngày tự rót tối đa `daily_limit` video từ đầu
/// hàng về thư mục riêng của kênh (INTEGRATION.md).
#[tauri::command]
pub fn set_watched_picked(
    id: String,
    picked: Vec<crate::models::PickedVideo>,
    store: State<Arc<WatchlistStore>>,
) -> AppResult<Option<WatchedChannel>> {
    store.update(&id, |c| c.picked = picked.clone())
}

/// Đặt số video TỰ TẢI tối đa mỗi ngày cho kênh theo dõi (kẹp 1..=3).
#[tauri::command]
pub fn set_watched_daily_limit(
    id: String,
    limit: u32,
    store: State<Arc<WatchlistStore>>,
) -> AppResult<Option<WatchedChannel>> {
    store.update(&id, |c| c.daily_limit = limit.clamp(1, 3))
}

/// Đặt CHẾ ĐỘ NGUỒN của kênh: "new" (chỉ video mới) | "picked" (hàng chờ 🎯)
/// | "auto" (tự vét kho theo view). Đổi sang "auto" thì xóa auto_fetch_date
/// để hôm nay được quét kho ngay, không phải chờ mai.
#[tauri::command]
pub fn set_watched_source_mode(
    id: String,
    mode: String,
    store: State<Arc<WatchlistStore>>,
) -> AppResult<Option<WatchedChannel>> {
    if !matches!(mode.as_str(), "new" | "picked" | "auto") {
        return Err(AppError::Other(format!("chế độ nguồn không hợp lệ: {mode}")));
    }
    store.update(&id, |c| {
        c.source_mode = mode.clone();
        if mode == "auto" {
            c.auto_fetch_date = None;
        }
    })
}

/// Đặt TÊN KÊNH ĐÍCH (kênh TikTok của user) cho kênh theo dõi — video tự
/// về `<watch_root>\<tên>` (resolve_watch_folder). None/rỗng = bỏ.
#[tauri::command]
pub fn set_watched_target(
    id: String,
    target: Option<String>,
    store: State<Arc<WatchlistStore>>,
) -> AppResult<Option<WatchedChannel>> {
    let t = target.map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
    // Tên phải ra được thư mục hợp lệ sau khi làm sạch — báo lỗi sớm cho UI
    // thay vì âm thầm rơi về thư mục mặc định.
    if let Some(name) = &t {
        if crate::watcher::sanitize_folder_name(name).is_none() {
            return Err(AppError::Other(format!("Tên kênh không hợp lệ: {name}")));
        }
    }
    store.update(&id, |c| c.target_name = t.clone())
}

/// Gán nhóm/quốc gia cho kênh theo dõi (trang Theo dõi lọc theo nhãn này).
/// None/rỗng = bỏ nhóm.
#[tauri::command]
pub fn set_watched_group(
    id: String,
    group: Option<String>,
    store: State<Arc<WatchlistStore>>,
) -> AppResult<Option<WatchedChannel>> {
    let g = group.map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
    store.update(&id, |c| c.group = g.clone())
}

/// Manually download one video that was detected in "notify only" mode, then
/// drop it from the channel's pending list.
#[tauri::command]
pub async fn download_pending(
    id: String,
    video_url: String,
    store: State<'_, Arc<WatchlistStore>>,
    queue: State<'_, Arc<QueueManager>>,
    settings: State<'_, Arc<SettingsStore>>,
    history: State<'_, Arc<HistoryStore>>,
) -> AppResult<Option<WatchedChannel>> {
    let channel = store.get(&id).ok_or_else(|| AppError::NotFound(id.clone()))?;
    let det = match channel.pending.iter().find(|d| d.url == video_url).cloned() {
        Some(d) => d,
        None => return Ok(store.get(&id)),
    };
    let s = settings.get();
    // Thư mục của kênh — CÙNG một chỗ quyết định với watcher (dây chuyền,
    // INTEGRATION.md) để tải tay hay tự tải đều về đúng 1 thư mục.
    let folder = crate::watcher::resolve_watch_folder(
        &channel.dest_dir, &channel.target_name, &s.watch_root, &s.default_folder,
    );
    let _ = std::fs::create_dir_all(&folder);
    let options = DownloadOptions {
        mode: DownloadMode::Video,
        format_id: None,
        save_folder: folder,
        sub_langs: vec![],
        auto_translate_to: None,
        on_conflict: ConflictPolicy::Rename,
        playlist_all: None,
        polite: Some(true),
    };
    let mut taken = history.known_short_ids().unwrap_or_default();
    for it in queue.list() {
        taken.insert(it.short_id);
    }
    let req = build_request(det.url.clone(), options, &s);
    let extractor = url_validator::resolve_extractor(&det.url).map(|x| x.to_string());
    let item = make_item(req, Some(det.title.clone()), det.thumbnail.clone(), extractor, channel.title.clone(), &taken);
    queue.enqueue(item)?;
    store.update(&id, |c| c.pending.retain(|d| d.url != video_url))
}

/// Drop a pending detection without downloading it.
#[tauri::command]
pub fn dismiss_pending(
    id: String,
    video_url: String,
    store: State<Arc<WatchlistStore>>,
) -> AppResult<Option<WatchedChannel>> {
    store.update(&id, |c| c.pending.retain(|d| d.url != video_url))
}

/// Manually trigger an immediate check of all enabled watched channels.
#[tauri::command]
pub async fn check_watched_now(
    app: AppHandle,
    store: State<'_, Arc<WatchlistStore>>,
    queue: State<'_, Arc<QueueManager>>,
    settings: State<'_, Arc<SettingsStore>>,
    history: State<'_, Arc<HistoryStore>>,
) -> AppResult<Vec<WatchedChannel>> {
    Ok(crate::watcher::check_all(&app, store.inner(), queue.inner(), settings.inner(), history.inner()).await)
}

// ---------- Junk cleanup ----------

/// True for yt-dlp leftover/broken files: `.part`, `.ytdl`, `.frag`, fragment
/// files like `name.f140.m4a` / `name.f313.webm`, `.temp.` files, and any
/// 0-byte file. These are the "blank icon" junk left by interrupted downloads.
fn is_junk_file(name: &str, size: u64) -> bool {
    let l = name.to_lowercase();
    if l.ends_with(".part")
        || l.ends_with(".ytdl")
        || l.ends_with(".frag")
        || l.contains(".part-")
        || l.contains(".temp.")
    {
        return true;
    }
    // Fragment leftover: a MIDDLE dotted segment shaped like `f<digits>`
    // (e.g. "title.f140.m4a"). Checking only middle segments avoids deleting a
    // real file that merely happens to be named "f150.mp4".
    let segs: Vec<&str> = name.split('.').collect();
    if segs.len() >= 3 {
        for seg in &segs[1..segs.len() - 1] {
            let b = seg.as_bytes();
            if b.len() >= 2
                && (b[0] == b'f' || b[0] == b'F')
                && seg[1..].chars().all(|c| c.is_ascii_digit())
            {
                return true;
            }
        }
    }
    size == 0
}

fn clean_dir(
    dir: &std::path::Path,
    count: &mut u64,
    depth: u32,
    protected: &std::collections::HashSet<String>,
) {
    if depth > 6 {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_dir() {
            clean_dir(&path, count, depth + 1, protected);
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        // Skip files touched in the last 2 minutes — likely an active download.
        if let Ok(modified) = meta.modified() {
            if modified.elapsed().map(|e| e.as_secs() < 120).unwrap_or(false) {
                continue;
            }
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !is_junk_file(&name, meta.len()) {
            continue;
        }
        // Never touch files belonging to a video that's still downloading
        // (its half-written fragments would corrupt the in-progress merge).
        let lname = name.to_lowercase();
        if protected.iter().any(|p| !p.is_empty() && lname.starts_with(p)) {
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            *count += 1;
        }
    }
}

/// Recursively delete leftover/broken download files under `root`, skipping any
/// whose name starts with a `protected` prefix (active downloads). Shared by the
/// manual command and the queue's automatic cleanup.
pub(crate) fn clean_junk_in(root: &std::path::Path, protected: &std::collections::HashSet<String>) -> u64 {
    if !root.is_dir() {
        return 0;
    }
    let mut count = 0u64;
    clean_dir(root, &mut count, 0, protected);
    count
}

/// Recursively delete leftover/broken download files under `folder` (and its
/// per-channel subfolders), protecting files of currently-downloading videos.
#[tauri::command]
pub fn clean_junk_files(folder: String, queue: State<Arc<QueueManager>>) -> AppResult<u64> {
    Ok(clean_junk_in(std::path::Path::new(&folder), &queue.protected_prefixes()))
}

// ---------- Saved bookmarks ----------

#[tauri::command]
pub fn list_bookmarks(store: State<Arc<BookmarksStore>>) -> AppResult<Vec<Bookmark>> {
    Ok(store.list())
}

#[tauri::command]
pub fn add_bookmark(
    url: String,
    note: Option<String>,
    store: State<Arc<BookmarksStore>>,
) -> AppResult<Bookmark> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err(AppError::InvalidUrl);
    }
    let taken: std::collections::HashSet<String> =
        store.list().into_iter().map(|b| b.id).collect();
    let bm = Bookmark {
        id: short_id::generate(&url, Utc::now().timestamp_millis(), &taken),
        url,
        note: note.unwrap_or_default(),
        added_at: Utc::now(),
    };
    store.add(bm.clone())?;
    Ok(bm)
}

#[tauri::command]
pub fn remove_bookmark(id: String, store: State<Arc<BookmarksStore>>) -> AppResult<()> {
    store.remove(&id)
}

#[tauri::command]
pub fn update_bookmark_note(
    id: String,
    note: String,
    store: State<Arc<BookmarksStore>>,
) -> AppResult<()> {
    store.update_note(&id, note)
}

// ---------- JS runtime (Deno) ----------

/// "unknown" | "downloading" | "ready" | "failed" — for the Settings UI.
#[tauri::command]
pub fn deno_status() -> AppResult<String> {
    Ok(crate::js_runtime::status().to_string())
}

/// Re-trigger the Deno download (used by the "Tải lại" button if it failed).
#[tauri::command]
pub fn retry_deno() -> AppResult<()> {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|x| x.to_path_buf()));
    crate::js_runtime::ensure(dir);
    Ok(())
}

// ---------- Nút "Sửa lỗi tải ngay" ----------

/// Chạy NGAY toàn bộ quy trình tự phục hồi mà app vẫn làm ngầm — cho nút
/// "Sửa lỗi tải" ở Settings, mỗi khi YouTube đổi luật user chỉ cần bấm 1 nút:
///   1. Update yt-dlp kênh nightly (bỏ qua throttle 12h) — fix ~90% trường hợp.
///   2. Đảm bảo Deno nằm cạnh yt-dlp (giải challenge JS của YouTube).
///   3. Cài lại plugin PO token + khởi động lại provider (nếu đang bật).
/// Trả về thông báo kết quả để hiện trực tiếp cho user.
#[tauri::command]
pub async fn fix_download_engine(
    app: AppHandle,
    settings: State<'_, Arc<SettingsStore>>,
) -> AppResult<String> {
    let data_dir = app.path().app_data_dir().ok();

    // 1) yt-dlp nightly — bước quan trọng nhất, chờ kết quả thật.
    //    Dịch output tiếng Anh của yt-dlp thành thông báo user hiểu được.
    let update_msg = match crate::ytdlp_update::run_update_now(&app, data_dir.clone()).await {
        Ok(m) => {
            let l = m.to_lowercase();
            if l.contains("up to date") {
                format!("✅ Bộ tải đã là bản mới nhất — nếu vẫn lỗi, YouTube vừa đổi luật mà bản vá chưa ra; thử lại nút này sau vài giờ. ({m})")
            } else if l.contains("updated yt-dlp to") {
                format!("✅ Đã cập nhật bộ tải lên bản mới — bấm Thử lại ở video bị lỗi. ({m})")
            } else {
                m
            }
        }
        Err(e) => format!("⚠️ Không cập nhật được — kiểm tra mạng rồi bấm lại. ({e})"),
    };

    let yt_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|x| x.to_path_buf()));

    // 2) Deno — best-effort chạy nền; ô trạng thái Deno sẵn có trong UI sẽ
    //    tự chuyển "downloading" → "ready".
    crate::js_runtime::ensure(yt_dir.clone());

    // 3) PO token: cài lại plugin + restart provider để chắc chắn đang sống
    //    (enable() tự kill tiến trình cũ trước khi spawn cái mới).
    let po_note = if settings.get().po_token_enabled {
        let po_arc = app
            .state::<Arc<crate::po_token::ProviderProcess>>()
            .inner()
            .clone();
        if let Some(dd) = data_dir {
            crate::po_token::enable(app.clone(), dd, yt_dir, po_arc);
            "đã khởi động lại"
        } else {
            "không tìm thấy thư mục dữ liệu"
        }
    } else {
        "đang tắt trong Cài đặt"
    };

    Ok(format!("yt-dlp: {update_msg}\nPO token: {po_note}"))
}

// ---------- Bootstrap ----------

#[tauri::command]
pub fn app_bootstrap(
    settings: State<Arc<SettingsStore>>,
    queue: State<Arc<QueueManager>>,
) -> AppResult<BootstrapPayload> {
    Ok(BootstrapPayload {
        settings: settings.get(),
        queue: queue.list(),
        ffmpeg_available: true, // sidecar-bundled; runtime check could be wired later
        aria2c_available: sidecar_detect::aria2c_available(),
    })
}

// Conflict event helper used by queue/runner (not a #[tauri::command]).
pub fn emit_conflict(app: &AppHandle, short_id: &str, suggested: &PathBuf, conflicting: &PathBuf) {
    let _ = app.emit(
        EV_DOWNLOAD_CONFLICT,
        ConflictEventPayload {
            short_id: short_id.to_string(),
            suggested_path: suggested.clone(),
            conflicting_path: conflicting.clone(),
        },
    );
}

// Pre-flight conflict resolution helper combining filename_resolver + emit_conflict.
pub fn preflight_conflict(
    app: &AppHandle,
    short_id: &str,
    save_folder: &std::path::Path,
    title: &str,
    ext: &str,
    policy: ConflictPolicy,
) -> ResolveOutcome {
    let exists = |p: &std::path::Path| p.exists();
    let outcome = filename_resolver::resolve(save_folder, title, ext, policy, exists);
    if let ResolveOutcome::AskUser {
        suggested,
        conflicting,
    } = &outcome
    {
        emit_conflict(app, short_id, suggested, conflicting);
    }
    outcome
}
