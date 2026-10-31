//! CỔNG ĐIỀU KHIỂN CỤC BỘ cho bot Telegram (`telegram-bot/`).
//!
//! Bot chạy cùng máy, gọi HTTP tới `127.0.0.1:<port>` kèm
//! `Authorization: Bearer <token>`. Cổng + khoá nằm ở
//! `<app_config_dir>/control_api.json` (tự sinh lần đầu; bot tự đọc file này).
//!
//! NGUYÊN TẮC: KHÔNG viết lại nghiệp vụ — mọi việc gọi thẳng đúng các lệnh
//! Tauri mà giao diện đang dùng (`commands::*`), nên bấm trên bot = bấm trên
//! máy, cùng kiểm tra, cùng luật live/chống trùng/thư mục trung chuyển.
//!
//! An toàn:
//! - Chỉ bind 127.0.0.1 (máy khác trong mạng không chạm tới được).
//! - Thiếu/sai khoá → 401. So khoá thời gian hằng.
//! - Không trả header CORS → trang web lạ trong trình duyệt không đọc được.
//! - Thân request tối đa 6 MB (đủ cho file cookies.txt).

use crate::bookmarks_store::BookmarksStore;
use crate::history_store::HistoryStore;
use crate::models::{
    ConflictPolicy, DownloadItem, DownloadMode, DownloadOptions, DownloadState, SettingsPatch,
    SiteCookie, WatchedChannel,
};
use crate::queue::QueueManager;
use crate::settings_store::SettingsStore;
use crate::watchlist_store::WatchlistStore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const DEFAULT_PORT: u16 = 47321;
const MAX_HEAD: usize = 64 * 1024;
const MAX_BODY: usize = 6 * 1024 * 1024;

/// Các ô cookie theo trang mà giao diện có (`SITE_COOKIE_SLOTS` bên TS) kèm
/// tên miền hợp lệ — file cookie phải có ít nhất 1 cookie thuộc các miền này.
pub const COOKIE_SITES: &[(&str, &[&str])] = &[
    ("youtube", &["youtube.com", "google.com"]),
    ("tiktok", &["tiktok.com"]),
    ("douyin", &["douyin.com"]),
    ("facebook", &["facebook.com"]),
    ("instagram", &["instagram.com"]),
    ("bilibili", &["bilibili.com", "bilibili.tv"]),
    ("twitter", &["x.com", "twitter.com"]),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub token: String,
}

fn default_true() -> bool {
    true
}
fn default_port() -> u16 {
    DEFAULT_PORT
}

/// Đọc (hoặc tạo) `control_api.json`. Khoá rỗng/quá ngắn → sinh khoá mới.
pub fn load_or_create_config(dir: &Path) -> ControlConfig {
    let path = dir.join("control_api.json");
    let mut cfg = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<ControlConfig>(&t).ok())
        .unwrap_or(ControlConfig { enabled: true, port: DEFAULT_PORT, token: String::new() });
    if cfg.token.len() < 32 {
        cfg.token = new_token();
        let _ = std::fs::create_dir_all(dir);
        if let Ok(text) = serde_json::to_string_pretty(&cfg) {
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }
    cfg
}

fn new_token() -> String {
    // Khoá chỉ dùng trong máy: trộn thời gian ns + pid + địa chỉ ngăn xếp +
    // id luồng qua blake3. Đủ để tiến trình/trang web khác không đoán được.
    let marker = 0u8;
    let seed = format!(
        "{:?}|{}|{:p}|{:?}|{:?}",
        std::time::SystemTime::now(),
        std::process::id(),
        &marker,
        std::thread::current().id(),
        std::time::Instant::now(),
    );
    let h = blake3::hash(seed.as_bytes());
    data_encoding::HEXLOWER.encode(&h.as_bytes()[..24])
}

fn same_token(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Khởi chạy server nền. Lỗi bind (cổng bận) chỉ ghi log — app vẫn chạy.
pub fn spawn(app: AppHandle, cfg: ControlConfig) {
    if !cfg.enabled {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], cfg.port));
        // Cổng bận (vd tiến trình cũ chưa nhả) → THỬ LẠI mỗi 5 giây thay vì bỏ
        // cuộc: bỏ cuộc là app chạy mà bot không bao giờ điều khiển được.
        let listener = loop {
            match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => break l,
                Err(e) => {
                    eprintln!("[control-api] chưa mở được cổng {addr}: {e} — thử lại sau 5s");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        };
        no_inherit(&listener);
        let token = Arc::new(cfg.token);
        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(x) => x,
                Err(_) => {
                    // Lỗi lặp lại (hết handle…) không được thành vòng quay 100% CPU.
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
            };
            no_inherit(&stream);
            if !peer.ip().is_loopback() {
                continue;
            }
            let app = app.clone();
            let token = token.clone();
            tauri::async_runtime::spawn(async move {
                // Dài hơn mọi việc có thể chạy qua cổng (sửa bộ tải ~10 phút)
                // để bot không nhận "app tắt" oan khi server tự cắt.
                let _ = tokio::time::timeout(
                    Duration::from_secs(1200),
                    serve_conn(stream, app, token),
                )
                .await;
            });
        }
    });
}

/// Cấm tiến trình con KẾ THỪA socket của cổng điều khiển.
///
/// LỖI THẬT 31/10/2026: socket của tokio trên Windows để mặc định "cho kế
/// thừa" → bot do app bật (bot_launcher) giữ luôn socket nghe 47321. App đóng
/// rồi mà cổng vẫn bị bot giữ → app mở lại không bind được, bot hỏi thì treo
/// ("App phản hồi quá lâu"). Bỏ cờ HANDLE_FLAG_INHERIT ngay sau khi tạo socket.
#[cfg(windows)]
fn no_inherit<S: std::os::windows::io::AsRawSocket>(sock: &S) {
    #[link(name = "kernel32")]
    extern "system" {
        fn SetHandleInformation(handle: isize, mask: u32, flags: u32) -> i32;
    }
    const HANDLE_FLAG_INHERIT: u32 = 0x1;
    // SAFETY: chỉ đổi cờ kế thừa của socket mình đang sở hữu; lỗi thì bỏ qua.
    unsafe {
        SetHandleInformation(sock.as_raw_socket() as isize, HANDLE_FLAG_INHERIT, 0);
    }
}

#[cfg(not(windows))]
fn no_inherit<S>(_sock: &S) {}

struct Request {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn q(&self, key: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
    fn json(&self) -> Result<Value, String> {
        if self.body.is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_slice(&self.body).map_err(|e| format!("JSON không hợp lệ: {e}"))
    }
}

async fn serve_conn(mut stream: tokio::net::TcpStream, app: AppHandle, token: Arc<String>) {
    let req = match read_request(&mut stream, &token).await {
        Ok(r) => r,
        Err((code, msg)) => {
            let _ = write_response(&mut stream, code, &json!({ "ok": false, "error": msg })).await;
            return;
        }
    };
    let (status, body) = match route(&app, &req).await {
        Ok(v) => (200, v),
        Err(ApiError(code, msg)) => (code, json!({ "ok": false, "error": msg })),
    };
    let _ = write_response(&mut stream, status, &body).await;
}

fn is_authorized(auth: Option<&str>, token: &str) -> bool {
    auth.and_then(|h| h.strip_prefix("Bearer "))
        .map(|t| same_token(t.trim(), token))
        .unwrap_or(false)
}

/// Đọc 1 request. Kiểm khoá ngay sau phần header — sai khoá thì KHÔNG đọc
/// thân (tới 6 MB) của kẻ lạ.
async fn read_request(stream: &mut tokio::net::TcpStream, token: &str) -> Result<Request, (u16, String)> {
    read_request_inner(stream, token).await
}

async fn read_request_inner(stream: &mut tokio::net::TcpStream, token: &str) -> Result<Request, (u16, String)> {
    let bad = |m: &str| (400u16, m.to_string());
    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    let mut tmp = [0u8; 8192];
    let head_end = loop {
        if let Some(i) = find_subslice(&buf, b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return Err(bad("Header quá lớn"));
        }
        let n = stream.read(&mut tmp).await.map_err(|e| bad(&e.to_string()))?;
        if n == 0 {
            return Err(bad("Kết nối đóng sớm"));
        }
        buf.extend_from_slice(&tmp[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("").to_uppercase();
    let target = parts.next().unwrap_or("/").to_string();
    let mut content_len = 0usize;
    let mut auth = None;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim().to_ascii_lowercase();
            let v = v.trim();
            if k == "content-length" {
                content_len = v.parse().map_err(|_| bad("Content-Length sai"))?;
            } else if k == "authorization" {
                auth = Some(v.to_string());
            }
        }
    }
    if !is_authorized(auth.as_deref(), token) {
        return Err((401, "Sai hoặc thiếu khoá điều khiển".into()));
    }
    if content_len > MAX_BODY {
        return Err(bad("Dữ liệu gửi lên quá lớn (tối đa 6 MB)"));
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < content_len {
        let n = stream.read(&mut tmp).await.map_err(|e| bad(&e.to_string()))?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(content_len);
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), parse_query(q)),
        None => (target, Vec::new()),
    };
    Ok(Request { method, path, query, body })
}

fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn parse_query(q: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(q.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

async fn write_response(
    stream: &mut tokio::net::TcpStream,
    status: u16,
    body: &Value,
) -> std::io::Result<()> {
    let text = body.to_string();
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        text.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(text.as_bytes()).await?;
    stream.flush().await
}

struct ApiError(u16, String);

impl From<crate::error::AppError> for ApiError {
    fn from(e: crate::error::AppError) -> Self {
        ApiError(409, e.to_string())
    }
}

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(400, msg.into())
}

type ApiResult = Result<Value, ApiError>;

async fn route(app: &AppHandle, req: &Request) -> ApiResult {
    let segs: Vec<&str> = req.path.trim_matches('/').split('/').collect();
    match (req.method.as_str(), segs.as_slice()) {
        ("GET", ["api", "ping"]) => Ok(json!({ "ok": true, "version": app.package_info().version.to_string() })),
        ("POST", ["api", "app", "quit"]) => Ok(quit_app(app)),
        ("GET", ["api", "status"]) => Ok(status(app)),
        ("GET", ["api", "queue"]) => Ok(queue_list(app, req)),
        ("POST", ["api", "queue", id, action]) => queue_action(app, id, action),
        ("POST", ["api", "bulk", action]) => bulk_action(app, action),
        ("POST", ["api", "enqueue"]) => enqueue(app, req.json().map_err(bad)?).await,
        ("GET", ["api", "watch"]) => Ok(watch_list(app, req)),
        ("POST", ["api", "watch", "check"]) => watch_check_all(app, req.json().map_err(bad)?),
        ("POST", ["api", "watch", "add"]) => watch_add(app, req.json().map_err(bad)?).await,
        ("POST", ["api", "watch", id, action]) => watch_action(app, id, action, req.json().map_err(bad)?).await,
        ("GET", ["api", "history"]) => history(app, req),
        ("GET", ["api", "cookies"]) => Ok(cookies_status(app)),
        ("POST", ["api", "cookies", site]) => cookies_set(app, site, req.json().map_err(bad)?).await,
        ("POST", ["api", "settings"]) => settings_patch(app, req.json().map_err(bad)?).await,
        ("POST", ["api", "engine", "fix"]) => {
            let msg = crate::commands::fix_download_engine(app.clone(), app.state()).await?;
            Ok(json!({ "ok": true, "message": msg }))
        }
        ("GET", ["api", "bookmarks"]) => {
            let list = app.state::<Arc<BookmarksStore>>().list();
            Ok(json!({ "ok": true, "items": list }))
        }
        _ => Err(ApiError(404, format!("Không có lệnh {} {}", req.method, req.path))),
    }
}

/// Đóng app từ xa — y như "Thoát hẳn" ở khay. Trả lời TRƯỚC rồi mới thoát
/// (thoát ngay thì bot nhận "mất kết nối" thay vì "đã đóng"). Báo số lượt tải
/// đang dở để bot nói rõ (hàng đợi được lưu, mở lại app là tải tiếp).
fn quit_app(app: &AppHandle) -> Value {
    let active = app
        .state::<Arc<QueueManager>>()
        .list()
        .iter()
        .filter(|it| matches!(it.state, DownloadState::Downloading | DownloadState::Queued))
        .count();
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(700)).await;
        crate::quit_gracefully(&app2);
    });
    json!({ "ok": true, "quitting": true, "activeDownloads": active })
}

fn state_name(s: DownloadState) -> &'static str {
    match s {
        DownloadState::Queued => "queued",
        DownloadState::Downloading => "downloading",
        DownloadState::Paused => "paused",
        DownloadState::Completed => "completed",
        DownloadState::Failed => "failed",
        DownloadState::Cancelled => "cancelled",
        DownloadState::Skipped => "skipped",
    }
}

fn item_json(it: &DownloadItem) -> Value {
    let percent = it.bytes_total.filter(|t| *t > 0).map(|t| {
        ((it.bytes_downloaded as f64 / t as f64) * 100.0).clamp(0.0, 100.0)
    });
    json!({
        "shortId": it.short_id,
        "title": it.title,
        "url": it.request.url,
        "extractor": it.extractor,
        "channel": it.channel,
        "state": state_name(it.state),
        "percent": percent,
        "bytesDownloaded": it.bytes_downloaded,
        "bytesTotal": it.bytes_total,
        "speedBps": it.speed_bps,
        "etaSec": it.eta_sec,
        "attempt": it.attempt,
        "botRetries": it.bot_retries,
        "error": it.error_message,
        "saveFolder": it.request.save_folder,
        "outputPath": it.output_path,
        "createdAt": it.created_at,
        "finishedAt": it.finished_at,
    })
}

fn status(app: &AppHandle) -> Value {
    let queue = app.state::<Arc<QueueManager>>();
    let items = queue.list();
    let mut counts = std::collections::BTreeMap::<&str, u32>::new();
    for it in &items {
        *counts.entry(state_name(it.state)).or_default() += 1;
    }
    let active: Vec<Value> = items
        .iter()
        .filter(|it| it.state == DownloadState::Downloading)
        .map(item_json)
        .collect();
    let speed: f64 = items
        .iter()
        .filter(|it| it.state == DownloadState::Downloading)
        .filter_map(|it| it.speed_bps)
        .sum();
    let s = app.state::<Arc<SettingsStore>>().get();
    let channels = app.state::<Arc<WatchlistStore>>().list();
    let ch_err = channels.iter().filter(|c| c.enabled && c.last_error.is_some()).count();
    json!({
        "ok": true,
        "version": app.package_info().version.to_string(),
        "counts": counts,
        "total": items.len(),
        "active": active,
        "speedBps": speed,
        "settings": {
            "maxConcurrency": s.max_concurrency,
            "watchAutoEnabled": s.watch_auto_enabled,
            "watchIntervalMin": s.watch_interval_min,
            "defaultFolder": s.default_folder,
            "watchRoot": s.watch_root,
            "proxies": s.proxies.len(),
            // ffmpeg app đang dùng — bot mượn để nén video gửi về điện thoại.
            "ffmpegPath": crate::sidecar_detect::ffmpeg_path(),
            "notifications": s.notifications,
        },
        "watch": {
            "channels": channels.len(),
            "enabled": channels.iter().filter(|c| c.enabled).count(),
            "withError": ch_err,
            "groups": s.watch_groups,
        },
    })
}

fn queue_list(app: &AppHandle, req: &Request) -> Value {
    let queue = app.state::<Arc<QueueManager>>();
    let filter = req.q("state").unwrap_or("all");
    let limit: usize = req.q("limit").and_then(|v| v.parse().ok()).unwrap_or(200).min(5000);
    let mut items: Vec<DownloadItem> = queue
        .list()
        .into_iter()
        .filter(|it| filter == "all" || state_name(it.state) == filter
            || (filter == "active" && matches!(it.state, DownloadState::Queued | DownloadState::Downloading | DownloadState::Paused)))
        .collect();
    // Mới nhất trước.
    items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let total = items.len();
    let list: Vec<Value> = items.iter().take(limit).map(item_json).collect();
    json!({ "ok": true, "total": total, "items": list })
}

fn queue_action(app: &AppHandle, id: &str, action: &str) -> ApiResult {
    let queue = app.state::<Arc<QueueManager>>();
    if queue.get(id).is_none() {
        return Err(ApiError(404, format!("Không thấy mục {id} trong hàng đợi")));
    }
    match action {
        "pause" => queue.pause(id)?,
        "resume" => queue.resume(id)?,
        "retry" => {
            queue.retry(id)?;
        }
        "force" => {
            queue.force_download(id)?;
        }
        "remove" => queue.remove_item(id)?,
        "cancel" => crate::commands::cancel_download(
            id.to_string(),
            app.clone(),
            app.state(),
            app.state(),
            app.state(),
        )?,
        _ => return Err(bad(format!("Không có thao tác '{action}'"))),
    }
    let it = queue.get(id);
    Ok(json!({ "ok": true, "item": it.as_ref().map(item_json) }))
}

fn bulk_action(app: &AppHandle, action: &str) -> ApiResult {
    let queue = app.state::<Arc<QueueManager>>();
    let n = match action {
        "pause_all" => queue.pause_all() as u32,
        "resume_all" => queue.resume_all() as u32,
        "retry_failed" => queue.retry_all_failed() as u32,
        "cancel_all" => crate::commands::cancel_all_downloads(app.clone(), app.state(), app.state(), app.state(), app.state())?,
        _ => return Err(bad(format!("Không có thao tác '{action}'"))),
    };
    Ok(json!({ "ok": true, "count": n }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnqueueBody {
    urls: Vec<String>,
    #[serde(default)]
    folder: Option<String>,
    #[serde(default)]
    audio: bool,
}

async fn enqueue(app: &AppHandle, body: Value) -> ApiResult {
    let body: EnqueueBody = serde_json::from_value(body).map_err(|e| bad(e.to_string()))?;
    let urls: Vec<String> = body
        .urls
        .into_iter()
        .map(|u| u.trim().to_string())
        .filter(|u| crate::url_validator::validate_url(u).valid)
        .collect();
    if urls.is_empty() {
        return Err(bad("Không có link hợp lệ (cần http/https của trang được hỗ trợ)"));
    }
    let s = app.state::<Arc<SettingsStore>>().get();
    let folder: PathBuf = match body.folder.filter(|f| !f.trim().is_empty()) {
        Some(f) => PathBuf::from(f.trim()),
        None => s.default_folder.clone(),
    };
    let polite = urls.len() > 15;
    let options = DownloadOptions {
        mode: if body.audio { DownloadMode::Audio } else { DownloadMode::Video },
        format_id: None,
        save_folder: folder.clone(),
        sub_langs: Vec::new(),
        auto_translate_to: None,
        on_conflict: ConflictPolicy::Rename,
        playlist_all: None,
        polite: Some(polite),
        max_height: None,
    };
    let items = crate::commands::enqueue_batch(urls, options, app.state(), app.state(), app.state()).await?;
    let list: Vec<Value> = items.iter().map(item_json).collect();
    Ok(json!({ "ok": true, "added": list.len(), "folder": folder, "items": list }))
}

fn channel_json(c: &WatchedChannel) -> Value {
    json!({
        "id": c.id,
        "url": c.url,
        "title": c.title,
        "enabled": c.enabled,
        "autoDownload": c.auto_download,
        "group": c.group,
        "targetName": c.target_name,
        "destDir": c.dest_dir,
        "tab": c.tab,
        "sourceMode": c.source_mode,
        "dailyLimit": c.daily_limit,
        "dripDate": c.drip_date,
        "dripCount": c.drip_count,
        "downloading": c.dl_pending.len(),
        "picked": c.picked.len(),
        "done": c.done_ids.len(),
        "lastChecked": c.last_checked,
        "lastNewCount": c.last_new_count,
        "lastError": c.last_error,
        "sourceEmpty": c.source_empty,
        "lastPick": c.last_pick,
    })
}

fn watch_list(app: &AppHandle, req: &Request) -> Value {
    let channels = app.state::<Arc<WatchlistStore>>().list();
    let group = req.q("group");
    let only_err = req.q("errors") == Some("1");
    let list: Vec<Value> = channels
        .iter()
        .filter(|c| match group {
            None | Some("*") => true,
            Some(g) => c.group.as_deref().unwrap_or("") == g,
        })
        .filter(|c| !only_err || c.last_error.is_some())
        .map(channel_json)
        .collect();
    json!({ "ok": true, "total": list.len(), "items": list })
}

/// Quét kênh chạy NỀN (có thể vài phút với hàng trăm kênh) — trả lời ngay để
/// bot không phải giữ kết nối; tiến độ xem qua /api/watch + hàng đợi.
fn watch_check_all(app: &AppHandle, body: Value) -> ApiResult {
    let group = body.get("group").and_then(|g| g.as_str()).map(|s| s.to_string());
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let r = crate::commands::check_watched_now(
            app2.clone(),
            app2.state(),
            app2.state(),
            app2.state(),
            app2.state(),
            group,
        )
        .await;
        if let Err(e) = r {
            eprintln!("[control-api] quét kênh lỗi: {e}");
        }
    });
    Ok(json!({ "ok": true, "started": true }))
}

#[derive(Deserialize)]
struct AddBody {
    url: String,
    #[serde(default)]
    tab: Option<String>,
}

async fn watch_add(app: &AppHandle, body: Value) -> ApiResult {
    let body: AddBody = serde_json::from_value(body).map_err(|e| bad(e.to_string()))?;
    if !crate::url_validator::validate_url(body.url.trim()).valid {
        return Err(bad("Link kênh không hợp lệ"));
    }
    let c = crate::commands::add_watched_channel(
        body.url.trim().to_string(),
        body.tab.or(Some("all".into())),
        app.clone(),
        app.state(),
        app.state(),
        app.state(),
        app.state(),
    )
    .await?;
    Ok(json!({ "ok": true, "channel": channel_json(&c) }))
}

async fn watch_action(app: &AppHandle, id: &str, action: &str, body: Value) -> ApiResult {
    let store = app.state::<Arc<WatchlistStore>>();
    if store.get(id).is_none() {
        return Err(ApiError(404, format!("Không thấy kênh {id}")));
    }
    match action {
        "enabled" => {
            let on = body.get("enabled").and_then(|v| v.as_bool()).ok_or_else(|| bad("Thiếu 'enabled'"))?;
            let c = crate::commands::set_watched_enabled(id.to_string(), on, app.state())?;
            Ok(json!({ "ok": true, "channel": c.as_ref().map(channel_json) }))
        }
        // CHỜ kết quả rồi trả về (bot chạy việc này ở luồng việc-dài, không
        // chặn ai). Trước đây trả "started" ngay → kênh hết video / không có
        // video mới thì bot im lặng, người dùng tưởng nút không chạy.
        "check" | "more" => {
            let count = if action == "more" {
                crate::commands::download_more_today(id.to_string(), app.clone(), app.state(), app.state(), app.state(), app.state()).await?
            } else {
                crate::commands::check_watched_one(id.to_string(), app.clone(), app.state(), app.state(), app.state(), app.state()).await?
            };
            let c = store.get(id);
            Ok(json!({ "ok": true, "count": count, "channel": c.as_ref().map(channel_json) }))
        }
        _ => Err(bad(format!("Không có thao tác '{action}'"))),
    }
}

fn history(app: &AppHandle, req: &Request) -> ApiResult {
    let limit: u32 = req.q("limit").and_then(|v| v.parse().ok()).unwrap_or(20).min(500);
    let query = req.q("q").map(|s| s.to_string());
    let list = app.state::<Arc<HistoryStore>>().list(query.as_deref(), limit, 0)?;
    Ok(json!({ "ok": true, "items": list }))
}

// ───────────────────────── Cookie ─────────────────────────

/// Kết quả kiểm tra 1 file cookie Netscape.
#[derive(Debug, Default, PartialEq)]
pub struct CookieCheck {
    pub total: usize,
    pub matching: usize,
    pub expired: usize,
    /// Hạn sớm nhất (epoch giây) của cookie CÓ hạn thuộc đúng trang; None = chỉ
    /// toàn cookie phiên.
    pub earliest_expiry: Option<i64>,
}

/// Đọc file cookies.txt dạng Netscape (7 cột tách tab), đếm cookie thuộc đúng
/// trang. Hàm THUẦN để unit-test.
pub fn inspect_cookie_text(text: &str, domains: &[&str], now: i64) -> CookieCheck {
    let mut out = CookieCheck::default();
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        // `#HttpOnly_.youtube.com` là cookie thật (yt-dlp hiểu), không phải chú thích.
        let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 7 {
            continue;
        }
        out.total += 1;
        let host = cols[0].trim_start_matches('.').to_ascii_lowercase();
        let ok = domains.iter().any(|d| host == *d || host.ends_with(&format!(".{d}")));
        if !ok {
            continue;
        }
        out.matching += 1;
        let exp: i64 = cols[4].trim().parse().unwrap_or(0);
        if exp > 0 {
            if exp < now {
                out.expired += 1;
            } else {
                out.earliest_expiry = Some(out.earliest_expiry.map_or(exp, |e| e.min(exp)));
            }
        }
    }
    out
}

fn site_domains(site: &str) -> Option<&'static [&'static str]> {
    COOKIE_SITES.iter().find(|(k, _)| *k == site).map(|(_, d)| *d)
}

fn cookie_dir(app: &AppHandle) -> Result<PathBuf, ApiError> {
    app.path()
        .app_config_dir()
        .map(|d| d.join("cookies"))
        .map_err(|e| ApiError(500, e.to_string()))
}

fn cookies_status(app: &AppHandle) -> Value {
    let s = app.state::<Arc<SettingsStore>>().get();
    let now = chrono::Utc::now().timestamp();
    let sites: Vec<Value> = COOKIE_SITES
        .iter()
        .map(|(key, domains)| {
            let slot = s.site_cookies.get(*key);
            let file = slot.and_then(|c| c.file.clone()).filter(|f| !f.is_empty());
            let browser = slot.and_then(|c| c.browser.clone()).filter(|b| !b.is_empty());
            let (exists, check, modified) = match file.as_deref() {
                Some(f) => {
                    let p = Path::new(f);
                    let text = std::fs::read_to_string(p).ok();
                    let modified = std::fs::metadata(p)
                        .and_then(|m| m.modified())
                        .ok()
                        .map(chrono::DateTime::<chrono::Utc>::from);
                    let check = text.as_deref().map(|t| inspect_cookie_text(t, domains, now));
                    (text.is_some(), check, modified)
                }
                None => (false, None, None),
            };
            json!({
                "site": key,
                "label": crate::args_builder::cookie_site_label(key),
                "file": file,
                "browser": browser,
                "exists": exists,
                "cookies": check.as_ref().map(|c| c.matching),
                "expired": check.as_ref().map(|c| c.expired),
                "earliestExpiry": check.as_ref().and_then(|c| c.earliest_expiry),
                "modified": modified,
            })
        })
        .collect();
    json!({
        "ok": true,
        "sites": sites,
        "shared": { "file": s.cookies_file, "browser": s.cookies_browser },
    })
}

async fn cookies_set(app: &AppHandle, site: &str, body: Value) -> ApiResult {
    let domains = site_domains(site).ok_or_else(|| {
        bad(format!(
            "Không có ô cookie '{site}'. Dùng: {}",
            COOKIE_SITES.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(", ")
        ))
    })?;
    let content = body
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad("Thiếu nội dung file cookie ('content')"))?;
    let now = chrono::Utc::now().timestamp();
    let check = inspect_cookie_text(content, domains, now);
    if check.matching == 0 {
        return Err(bad(format!(
            "File không có cookie nào của {} (cần file cookies.txt dạng Netscape, xuất từ trình duyệt đang đăng nhập {}).",
            crate::args_builder::cookie_site_label(site),
            domains.join(" / ")
        )));
    }
    if check.matching == check.expired {
        return Err(bad("Mọi cookie của trang này trong file đã HẾT HẠN — xuất lại file mới."));
    }
    let dir = cookie_dir(app)?;
    std::fs::create_dir_all(&dir).map_err(|e| ApiError(500, e.to_string()))?;
    let path = dir.join(format!("{site}.txt"));
    // Giữ bản cũ 1 đời để lỡ file mới hỏng còn quay lại được.
    if path.exists() {
        let _ = std::fs::copy(&path, dir.join(format!("{site}.prev.txt")));
    }
    let tmp = dir.join(format!("{site}.txt.tmp"));
    std::fs::write(&tmp, content).map_err(|e| ApiError(500, e.to_string()))?;
    std::fs::rename(&tmp, &path).map_err(|e| ApiError(500, e.to_string()))?;

    let settings = app.state::<Arc<SettingsStore>>();
    let mut map = settings.get().site_cookies.clone();
    map.insert(
        site.to_string(),
        SiteCookie { file: Some(path.to_string_lossy().into_owned()), browser: None },
    );
    let patch = SettingsPatch { site_cookies: Some(map), ..Default::default() };
    crate::commands::update_settings(patch, app.state(), app.state(), app.clone()).await?;
    Ok(json!({
        "ok": true,
        "site": site,
        "file": path,
        "cookies": check.matching,
        "expired": check.expired,
        "earliestExpiry": check.earliest_expiry,
    }))
}

/// Chỉ cho bot đổi vài cài đặt vận hành (không đụng đường dẫn cookie/thư mục
/// tuỳ ý qua lối này — cookie có lệnh riêng có kiểm tra).
async fn settings_patch(app: &AppHandle, body: Value) -> ApiResult {
    let mut patch = SettingsPatch::default();
    if let Some(v) = body.get("watchAutoEnabled").and_then(|v| v.as_bool()) {
        patch.watch_auto_enabled = Some(v);
    }
    if let Some(v) = body.get("maxConcurrency").and_then(|v| v.as_u64()) {
        patch.max_concurrency = Some(v.clamp(1, 32) as u8);
    }
    if let Some(v) = body.get("watchIntervalMin").and_then(|v| v.as_u64()) {
        patch.watch_interval_min = Some(v.clamp(1, 1440) as u32);
    }
    if let Some(v) = body.get("notifications").and_then(|v| v.as_bool()) {
        patch.notifications = Some(v);
    }
    if patch == SettingsPatch::default() {
        return Err(bad("Không có cài đặt nào được phép đổi (watchAutoEnabled, maxConcurrency, watchIntervalMin, notifications)"));
    }
    let s = crate::commands::update_settings(patch, app.state(), app.state(), app.clone()).await?;
    Ok(json!({
        "ok": true,
        "settings": {
            "maxConcurrency": s.max_concurrency,
            "watchAutoEnabled": s.watch_auto_enabled,
            "watchIntervalMin": s.watch_interval_min,
            "notifications": s.notifications,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const YT: &[&str] = &["youtube.com", "google.com"];

    #[test]
    fn cookie_file_counts_only_matching_site() {
        let text = "# Netscape HTTP Cookie File\n\
            .youtube.com\tTRUE\t/\tTRUE\t2000000000\tSID\tabc\n\
            #HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t1900000000\tHSID\tx\n\
            .tiktok.com\tTRUE\t/\tTRUE\t2000000000\tsid\ty\n\
            .google.com\tTRUE\t/\tTRUE\t100\tOLD\tz\n";
        let c = inspect_cookie_text(text, YT, 1_000_000_000);
        assert_eq!(c.total, 4);
        assert_eq!(c.matching, 3);
        assert_eq!(c.expired, 1);
        assert_eq!(c.earliest_expiry, Some(1_900_000_000));
    }

    #[test]
    fn cookie_file_rejects_lookalike_domain() {
        let text = "evilyoutube.com\tTRUE\t/\tTRUE\t0\tSID\tabc\n";
        assert_eq!(inspect_cookie_text(text, YT, 0).matching, 0);
    }

    #[test]
    fn token_compare_and_generation() {
        let a = new_token();
        assert_eq!(a.len(), 48);
        assert!(same_token(&a, &a.clone()));
        assert!(!same_token(&a, &new_token()));
        assert!(!same_token("abc", "abcd"));
    }

    #[test]
    fn reads_http_request_with_body_split_across_packets() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let client = tokio::spawn(async move {
                let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
                let body = r#"{"urls":["https://youtu.be/a"]}"#;
                let head = format!(
                    "POST /api/enqueue?x=1 HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer abc\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                );
                s.write_all(head.as_bytes()).await.unwrap();
                s.write_all(&body.as_bytes()[..5]).await.unwrap();
                s.flush().await.unwrap();
                tokio::time::sleep(Duration::from_millis(20)).await;
                s.write_all(&body.as_bytes()[5..]).await.unwrap();
                s
            });
            let (mut srv, _) = listener.accept().await.unwrap();
            let req = read_request(&mut srv, "abc").await.unwrap();
            let _keep = client.await.unwrap();
            assert_eq!(req.method, "POST");
            assert_eq!(req.path, "/api/enqueue");
            assert_eq!(req.q("x"), Some("1"));
            assert_eq!(req.json().unwrap()["urls"][0], "https://youtu.be/a");
        });
    }

    #[test]
    fn wrong_token_is_rejected_before_reading_body() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let client = tokio::spawn(async move {
                let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
                // Hứa gửi 5 MB nhưng không gửi gì — server không được chờ thân.
                s.write_all(b"POST /api/enqueue HTTP/1.1\r\nAuthorization: Bearer sai\r\nContent-Length: 5000000\r\n\r\n")
                    .await
                    .unwrap();
                s
            });
            let (mut srv, _) = listener.accept().await.unwrap();
            let r = tokio::time::timeout(Duration::from_secs(2), read_request(&mut srv, "dung")).await;
            let _keep = client.await.unwrap();
            match r {
                Ok(Err((401, _))) => {}
                other => panic!("phải trả 401 ngay, được: {:?}", other.map(|x| x.map(|_| ()))),
            }
        });
    }

    #[cfg(windows)]
    #[test]
    fn listener_socket_is_not_inherited_by_children() {
        use std::os::windows::io::AsRawSocket;
        #[link(name = "kernel32")]
        extern "system" {
            fn GetHandleInformation(handle: isize, flags: *mut u32) -> i32;
        }
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            no_inherit(&l);
            let mut flags = 0u32;
            let ok = unsafe { GetHandleInformation(l.as_raw_socket() as isize, &mut flags) };
            assert_ne!(ok, 0);
            assert_eq!(flags & 0x1, 0, "socket cổng điều khiển không được cho tiến trình con kế thừa");
        });
    }

    #[test]
    fn query_parsing_decodes() {
        let q = parse_query("state=failed&q=a%20b");
        assert_eq!(q[0], ("state".into(), "failed".into()));
        assert_eq!(q[1], ("q".into(), "a b".into()));
    }
}
