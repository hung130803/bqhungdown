//! Fetch a flat listing of videos from a channel/user URL using yt-dlp.
//!
//! Strategy:
//! - Run yt-dlp with `--flat-playlist --dump-single-json` to get the entry
//!   list very fast (no per-video network round-trips).
//! - For YouTube channels we resolve to the `/videos` tab so we don't
//!   accidentally include Shorts/Streams unless the user wants them.
//! - When the user requests "view chính xác", we follow the flat fetch with
//!   a parallel probe (8 concurrent yt-dlp processes) that pulls
//!   view_count + upload_date per video. ~10x faster than `--no-flat-playlist`.
//! - Cancellation: a global `FETCH_GENERATION` atomic; every call increments
//!   it and remembers its own generation. `cancel()` bumps the counter, which
//!   causes in-flight fetches to kill their yt-dlp child(ren) and bail out.

use crate::error::{AppError, AppResult};
use crate::models::{ChannelInfo, ChannelVideo, Settings};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;
use tauri::Manager;
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

const TIMEOUT: Duration = Duration::from_secs(900);
/// Concurrency for batched probes. Each task runs one yt-dlp process that
/// hands off ~30 URLs at once, so the *total* number of yt-dlp processes
/// in flight is `PROBE_CONCURRENCY * 1`. 4 batches × 30 URLs = 120 URLs in
/// flight, which is well under YouTube's anti-bot threshold.
const PROBE_CONCURRENCY: usize = 4;
/// How many URLs to feed a single yt-dlp invocation. Larger = less per-batch
/// overhead; smaller = quicker first-result. 30 hits the sweet spot for
/// YouTube where each video adds ~0.4-0.8s of work.
const BATCH_SIZE: usize = 30;

/// Monotonic generation counter. Each `fetch_channel` call gets a unique
/// generation; `cancel()` bumps it. In-flight fetches compare against their
/// stored generation periodically and abort when it no longer matches.
static FETCH_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Cancel any in-flight `fetch_channel` call. Idempotent: safe to call when
/// no fetch is running.
pub fn cancel() {
    FETCH_GENERATION.fetch_add(1, Ordering::SeqCst);
}

/// Translate a raw yt-dlp stderr line into a short, actionable Vietnamese
/// message. Most importantly, detect YouTube's anti-bot wall so the user knows
/// it's a temporary block (and that updating yt-dlp / retrying usually helps),
/// not an empty channel.
fn friendly_fetch_error(raw: &str) -> String {
    let l = raw.to_lowercase();
    if crate::error::is_cookie_decrypt_error(raw) {
        return "Không đọc được cookie từ trình duyệt (Windows mã hoá DPAPI). \
                Vào Cài đặt → tắt \"Lấy cookies từ trình duyệt\" (để trống). \
                Video YouTube công khai không cần cookie."
            .into();
    }
    if l.contains("sign in to confirm")
        || l.contains("not a bot")
        || l.contains("confirm you")
    {
        return "YouTube đang chặn (yêu cầu xác minh \"không phải robot\"). \
                App sẽ tự cập nhật yt-dlp ở nền — hãy đợi 1-2 phút rồi thử lại. \
                Nếu vẫn lỗi, thử lại sau ít phút hoặc đổi mạng/IP."
            .into();
    }
    if l.contains("http error 429") || l.contains("too many requests") {
        return "YouTube tạm chặn vì tải quá nhiều trong thời gian ngắn (lỗi 429). \
                Hãy đợi vài phút rồi thử lại, hoặc giảm số kênh tải cùng lúc."
            .into();
    }
    if l.contains("this channel does not have")
        || l.contains("does not exist")
        || l.contains("not found")
    {
        return "Không tìm thấy video trên kênh (kênh trống, sai link, hoặc tab không có video).".into();
    }
    if l.contains("private") || l.contains("members-only") {
        return "Kênh/nội dung này ở chế độ riêng tư hoặc chỉ dành cho thành viên.".into();
    }
    // Fallback: keep the original yt-dlp message so power users can read it.
    format!("Không lấy được danh sách video: {raw}")
}

/// Load the set of video IDs already recorded in the yt-dlp download-archive
/// (same file used by the download path). Each line is `<extractor> <id>`; we
/// key on the bare id so it matches `extract_video_id` of a channel entry.
/// Returns an empty set on any error (archive missing on first run, etc.).
fn load_archive_ids(app: &AppHandle) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    if let Ok(dir) = app.path().app_data_dir() {
        if let Ok(text) = std::fs::read_to_string(dir.join("download_archive.txt")) {
            for line in text.lines() {
                if let Some(id) = line.split_whitespace().nth(1) {
                    set.insert(id.to_string());
                }
            }
        }
    }
    set
}

/// Normalise a raw channel/user URL so we can append the correct tab suffix.
/// `tab` is one of: "videos", "shorts", "streams", or empty/other (leave as-is).
fn normalise_channel_url(raw: &str, tab: &str) -> String {
    let lower = raw.to_lowercase();

    // TikTok: collapse `/@user/video/<id>` → `/@user`. Add `/video` tab suffix.
    if lower.contains("tiktok.com/@") {
        let base = if let Some(idx) = raw.find("/video/") {
            raw[..idx].to_string()
        } else {
            raw.trim_end_matches('/').to_string()
        };
        let suffix = if tab == "videos" || tab == "all" { "/video" } else { "" };
        return format!("{base}{suffix}");
    }

    // Facebook: add `/videos` tab suffix.
    if lower.contains("facebook.com/") && !lower.contains("/photo") && !lower.contains("/watch") {
        let base = raw.trim_end_matches('/').to_string();
        return if base.to_lowercase().ends_with("/videos") { base } else { format!("{base}/videos") };
    }

    // Instagram: pass through as-is (no public video tab URL).
    if lower.contains("instagram.com/") {
        return raw.trim_end_matches('/').to_string();
    }

    // Reddit: pass through as-is.
    if lower.contains("reddit.com/") {
        return raw.trim_end_matches('/').to_string();
    }

    if !lower.contains("youtube.com") {
        return raw.to_string();
    }
    // Strip any trailing `/videos`, `/shorts`, `/streams`, `/featured` that
    // might be on the input URL so we can re-append the requested tab cleanly.
    let mut base = raw.trim_end_matches('/').to_string();
    for suffix in ["/videos", "/shorts", "/streams", "/featured"] {
        if base.to_lowercase().ends_with(suffix) {
            base.truncate(base.len() - suffix.len());
            break;
        }
    }
    if base.contains("/playlist?list=") || base.contains("/watch?") {
        return base;
    }
    let suffix = match tab {
        "shorts" => "/shorts",
        "streams" => "/streams",
        "videos" => "/videos",
        _ => "/videos",
    };
    format!("{base}{suffix}")
}

/// Fetch up to `limit` recent videos from a channel/user URL.
///
/// `tab` lets the caller choose which YouTube channel tab to scrape:
///   - "videos"  → long-form (default)
///   - "shorts"  → Shorts only
///   - "streams" → Live/streams only
///   - "all"     → fetch /videos + /shorts (+ /streams when present) and merge
///                  the entries, dropping duplicates by URL.
/// `limit = 0` → fetch all videos. `detailed = true` → also probe view_count.
pub async fn fetch_channel(
    app: &AppHandle,
    url: &str,
    limit: u32,
    detailed: bool,
    tab: &str,
    settings: &Settings,
) -> AppResult<(ChannelInfo, Vec<ChannelVideo>)> {
    let my_gen = FETCH_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

    // Douyin user URLs (`douyin.com/user/<sec_uid>`) — yt-dlp doesn't expose
    // an extractor for these. Use tikwm `/api/user/posts` instead. Each
    // tikwm page returns ~30 posts; we paginate until we hit `limit` or run
    // out of cursors.
    let lower_url = url.to_lowercase();
    if lower_url.contains("douyin.com/user/") {
        return Err(AppError::YtDlpFailed(
            "Douyin chặn quá chặt nên không thể tự lấy danh sách kênh được. \
             Cách tải: mở Douyin trong trình duyệt, sao chép link từng video bạn muốn tải, \
             rồi dán vào tab \"Hàng loạt\" (mỗi link 1 dòng). App sẽ tải hết bằng tikwm proxy."
                .into(),
        ));
    }

    // Đường tắt: nếu là kênh YouTube VÀ người dùng đã nhập API key → dùng
    // YouTube Data API. Lấy view/thời lượng/ngày/hashtag chính xác cho cả kênh
    // trong vài giây (thay vì dò yt-dlp từng video 1-2 tiếng). Lỗi API (key sai,
    // hết quota, link lạ) → rơi xuống đường yt-dlp như cũ, không chặn người dùng.
    let lower = url.to_lowercase();
    let is_youtube = lower.contains("youtube.com");
    if is_youtube {
        if let Some(key) = settings
            .youtube_api_key
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
        {
            match crate::youtube_api::fetch_channel(url, key, limit).await {
                Ok((info, videos)) => {
                    return finalize_listing(app, info, videos, settings);
                }
                Err(e) => {
                    eprintln!("YouTube Data API thất bại, quay lại yt-dlp: {e}");
                }
            }
        }
    }
    let tabs: Vec<&str> = if is_youtube && tab == "all" {
        vec!["videos", "shorts"]
    } else {
        vec![tab]
    };

    let mut info: Option<ChannelInfo> = None;
    let mut videos: Vec<ChannelVideo> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    // Keep the last real yt-dlp error so we can surface *why* nothing came back
    // (bot wall vs. private channel vs. network) instead of a generic message.
    let mut last_err: Option<String> = None;

    for t in tabs {
        let resolved = normalise_channel_url(url, t);
        let fut = run_flat_fetch(app, &resolved, limit, settings, my_gen);
        match tokio::time::timeout(TIMEOUT, fut).await {
            Ok(Ok((nfo, vids))) => {
                if info.is_none() {
                    info = Some(nfo);
                }
                let mark_short = t == "shorts";
                for mut v in vids {
                    if mark_short {
                        v.is_short = true;
                    }
                    if seen.insert(v.url.clone()) {
                        videos.push(v);
                    }
                }
            }
            Ok(Err(AppError::YtDlpFailed(msg))) => last_err = Some(msg),
            Ok(Err(e)) => last_err = Some(e.to_string()),
            Err(_) => last_err = Some("Hết thời gian chờ (timeout)".into()),
        }
        if FETCH_GENERATION.load(Ordering::SeqCst) != my_gen {
            return Err(AppError::YtDlpFailed("Đã huỷ".into()));
        }
    }

    let mut info = match info {
        Some(i) => i,
        None => {
            let raw = last_err.unwrap_or_else(|| "Không có video nào trên kênh".into());
            return Err(AppError::YtDlpFailed(friendly_fetch_error(&raw)));
        }
    };

    // Sort descending by upload_date so newest videos come first.
    videos.sort_by(|a, b| b.upload_date.cmp(&a.upload_date));

    // Deduplicate across /videos + /shorts tabs.
    videos = videos.into_iter().filter(|v| seen.remove(&v.url)).collect();

    // "Bỏ qua video đã tải" — hide entries already in the download-archive so
    // the user doesn't re-pick videos they've downloaded (and likely deleted
    // after re-uploading). Done before the slow detail probe so we don't waste
    // work on hidden videos. Matches by bare video id.
    if settings.skip_downloaded {
        let archived = load_archive_ids(app);
        if !archived.is_empty() {
            let before = videos.len();
            videos.retain(|v| match extract_video_id(&v.url) {
                Some(id) => !archived.contains(&id),
                None => true, // can't determine id → keep (don't hide blindly)
            });
            info.hidden_downloaded = Some((before - videos.len()) as u32);
        }
    }

    // Step 2: probe details only when user explicitly opts in via "detailed".
    // Default mode trusts the flat-playlist response — yt-dlp's
    // `youtubetab:approximate_date` extractor arg already gives most channels
    // a usable upload date in flat mode, so we skip the slow per-video probe.
    if detailed && !videos.is_empty() {
        videos = enrich_in_parallel(app, videos, settings, my_gen, detailed).await?;
    }

    Ok((info, videos))
}

/// Hậu xử lý danh sách lấy từ YouTube Data API: sắp xếp mới-nhất-trước + ẩn
/// video đã tải (nếu bật "Bỏ qua video đã tải"). API đã có sẵn metadata đầy đủ
/// nên KHÔNG cần probe chi tiết → đây là toàn bộ phần còn lại trước khi trả về.
fn finalize_listing(
    app: &AppHandle,
    mut info: ChannelInfo,
    mut videos: Vec<ChannelVideo>,
    settings: &Settings,
) -> AppResult<(ChannelInfo, Vec<ChannelVideo>)> {
    videos.sort_by(|a, b| b.upload_date.cmp(&a.upload_date));

    if settings.skip_downloaded {
        let archived = load_archive_ids(app);
        if !archived.is_empty() {
            let before = videos.len();
            videos.retain(|v| match extract_video_id(&v.url) {
                Some(id) => !archived.contains(&id),
                None => true,
            });
            info.hidden_downloaded = Some((before - videos.len()) as u32);
        }
    }

    Ok((info, videos))
}

/// Fetch a Douyin user's video listing via tikwm. Pages are 30 entries each;
/// we keep requesting with `cursor` until we hit `limit` or `hasMore=false`.
/// `limit = 0` means "all pages".
#[allow(dead_code)] // tikwm /user/posts is Cloudflare-protected; kept as
                    // reference for if we get a working endpoint later.
async fn fetch_douyin_channel(
    url: &str,
    limit: u32,
    my_gen: u64,
) -> AppResult<(ChannelInfo, Vec<ChannelVideo>)> {
    use serde_json::Value;

    let sec_uid = match url.split("/user/").nth(1) {
        Some(rest) => rest
            .split(|c| c == '?' || c == '&' || c == '#')
            .next()
            .unwrap_or("")
            .to_string(),
        None => return Err(AppError::YtDlpFailed("Không nhận diện được Douyin user".into())),
    };
    if sec_uid.is_empty() {
        return Err(AppError::YtDlpFailed("Douyin URL không có sec_uid".into()));
    }

    let endpoints = [
        "https://www.tikwm.com/api/user/posts",
        "https://tikwm.com/api/user/posts",
        "https://api.tikwm.com/api/user/posts",
    ];

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
        )
        .build()
        .map_err(|e| AppError::YtDlpFailed(e.to_string()))?;

    let mut info = ChannelInfo {
        url: url.to_string(),
        title: String::new(),
        thumbnail: None,
        video_count: None,
        extractor: "douyin".into(),
        hidden_downloaded: None,
        channel_id: None,
    };
    let mut videos: Vec<ChannelVideo> = Vec::new();
    let mut cursor: i64 = 0;
    let cap = if limit == 0 { u32::MAX } else { limit };

    'pages: loop {
        if FETCH_GENERATION.load(Ordering::SeqCst) != my_gen {
            return Err(AppError::YtDlpFailed("Đã huỷ".into()));
        }
        let mut page_ok = false;
        for endpoint in endpoints {
            let resp = client
                .get(endpoint)
                .query(&[
                    ("unique_id", sec_uid.as_str()),
                    ("count", "30"),
                    ("cursor", &cursor.to_string()),
                ])
                .send()
                .await;
            let resp = match resp {
                Ok(r) if r.status().is_success() => r,
                _ => continue,
            };
            let json: Value = match resp.json().await {
                Ok(j) => j,
                Err(_) => continue,
            };
            if json.get("code").and_then(|v| v.as_i64()) != Some(0) {
                continue;
            }
            let data = match json.get("data") {
                Some(d) => d,
                None => continue,
            };
            // First page: pull profile info.
            if videos.is_empty() {
                if let Some(author) = data.get("author") {
                    info.title = author
                        .get("nickname")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    info.thumbnail = author
                        .get("avatar")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                }
            }
            if let Some(arr) = data.get("videos").and_then(|v| v.as_array()) {
                for v in arr {
                    if let Some(cv) = parse_tikwm_entry(v) {
                        videos.push(cv);
                        if videos.len() as u32 >= cap {
                            break 'pages;
                        }
                    }
                }
            }
            // tikwm uses `cursor` for next page; some endpoints return
            // hasMore=false at end.
            let has_more = data
                .get("hasMore")
                .and_then(|v| v.as_bool())
                .or_else(|| {
                    data.get("has_more").and_then(|v| match v {
                        Value::Bool(b) => Some(*b),
                        Value::Number(n) => n.as_i64().map(|i| i != 0),
                        _ => None,
                    })
                })
                .unwrap_or(false);
            cursor = data
                .get("cursor")
                .and_then(|v| v.as_i64())
                .unwrap_or(cursor + 30);
            page_ok = true;
            if !has_more {
                break 'pages;
            }
            break; // success — go to next page (don't try other endpoints)
        }
        if !page_ok {
            // All endpoints failed for this cursor — bail with whatever we have.
            if videos.is_empty() {
                return Err(AppError::YtDlpFailed(
                    "TikWM không phản hồi (Cloudflare chặn / mạng)".into(),
                ));
            }
            break;
        }
    }

    info.video_count = Some(videos.len() as u32);
    Ok((info, videos))
}

#[allow(dead_code)]
fn parse_tikwm_entry(v: &serde_json::Value) -> Option<ChannelVideo> {
    let id = v.get("video_id").and_then(|x| x.as_str()).map(String::from)
        .or_else(|| v.get("aweme_id").and_then(|x| x.as_str()).map(String::from))?;
    // tikwm return play link sometimes — but for queueing we want the canonical
    // douyin watch URL so url_resolver can re-fetch the latest CDN link later.
    let url = format!("https://www.douyin.com/video/{id}");
    let title = v
        .get("title")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let duration_sec = v.get("duration").and_then(|x| x.as_u64());
    let view_count = v
        .get("play_count")
        .and_then(|x| x.as_u64())
        .or_else(|| v.get("playCount").and_then(|x| x.as_u64()));
    let upload_date = v
        .get("create_time")
        .and_then(|x| x.as_i64())
        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
        .map(|dt| dt.format("%Y%m%d").to_string());
    let thumbnail = v
        .get("cover")
        .and_then(|x| x.as_str())
        .map(String::from)
        .or_else(|| v.get("origin_cover").and_then(|x| x.as_str()).map(String::from));
    // Douyin photo posts: `images: [...]` instead of video.
    let is_photo = v
        .get("images")
        .and_then(|x| x.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    Some(ChannelVideo {
        url,
        title,
        duration_sec,
        view_count,
        upload_date,
        thumbnail,
        is_short: false,
        is_photo,
        hashtags: Vec::new(),
    })
}


/// Fetch a channel's flat listing, retrying without cookies if the first
/// attempt fails because browser cookies couldn't be decrypted (DPAPI). Public
/// channels don't need cookies, so this keeps listing working even when the
/// user has a broken "cookies from browser" setting.
async fn run_flat_fetch(
    app: &AppHandle,
    resolved: &str,
    limit: u32,
    settings: &Settings,
    my_gen: u64,
) -> AppResult<(ChannelInfo, Vec<ChannelVideo>)> {
    let res = run_flat_fetch_attempt(app, resolved, limit, settings, my_gen).await;
    if let Err(AppError::YtDlpFailed(ref msg)) = res {
        if crate::args_builder::settings_have_cookies(settings)
            && crate::error::is_cookie_decrypt_error(msg)
        {
            let no_cookies = crate::args_builder::settings_without_cookies(settings);
            return run_flat_fetch_attempt(app, resolved, limit, &no_cookies, my_gen).await;
        }
    }
    res
}

async fn run_flat_fetch_attempt(
    app: &AppHandle,
    resolved: &str,
    limit: u32,
    settings: &Settings,
    my_gen: u64,
) -> AppResult<(ChannelInfo, Vec<ChannelVideo>)> {
    let mut args: Vec<String> = vec![
        "--no-warnings".into(),
        "--dump-single-json".into(),
        "--encoding".into(),
        "utf-8".into(),
        "-4".into(),
        "--socket-timeout".into(),
        "30".into(),
        "--user-agent".into(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36"
            .into(),
        "--flat-playlist".into(),
        // Bảo YouTube tab extractor trả thêm field xấp xỉ trong flat mode:
        // - approximate_date: ngày upload (đỡ phải probe per-video)
        // - approximate_view_count: view dạng round-off ("1.2M views")
        // Newer yt-dlp recognises both; older versions ignore safely.
        "--extractor-args".into(),
        "youtubetab:approximate_date,approximate_view_count".into(),
    ];
    if limit > 0 {
        args.push("--playlist-end".into());
        args.push(limit.to_string());
    }
    crate::args_builder::push_cookie_args(&mut args, settings);
    crate::args_builder::push_proxy_args(&mut args, settings);
    args.push(resolved.to_string());

    let cmd = app
        .shell()
        .sidecar("yt-dlp")
        .map_err(|e| AppError::YtDlpFailed(e.to_string()))?
        .args(args);

    let (mut rx, child) = cmd
        .spawn()
        .map_err(|e| AppError::YtDlpFailed(e.to_string()))?;
    let mut stdout_buf = String::new();
    let mut stderr_buf = String::new();
    let mut exit_code: Option<i32> = None;

    loop {
        // Poll with a short timeout so we can periodically check for
        // cancellation even when yt-dlp is silent (e.g., waiting on a slow
        // server response).
        match tokio::time::timeout(Duration::from_millis(250), rx.recv()).await {
            Ok(Some(ev)) => match ev {
                CommandEvent::Stdout(bytes) => {
                    stdout_buf.push_str(&String::from_utf8_lossy(&bytes))
                }
                CommandEvent::Stderr(bytes) => {
                    stderr_buf.push_str(&String::from_utf8_lossy(&bytes))
                }
                CommandEvent::Terminated(payload) => {
                    exit_code = payload.code;
                    break;
                }
                _ => {}
            },
            Ok(None) => break,
            Err(_) => {
                // Timed out — check for cancellation.
                if FETCH_GENERATION.load(Ordering::SeqCst) != my_gen {
                    let _ = child.kill();
                    return Err(AppError::YtDlpFailed("Đã huỷ".into()));
                }
            }
        }
    }

    if exit_code.unwrap_or(-1) != 0 {
        let last = stderr_buf
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("yt-dlp failed")
            .to_string();
        return Err(AppError::YtDlpFailed(last));
    }
    let value: Value = serde_json::from_str(&stdout_buf)?;
    Ok(parse_channel(resolved, value))
}

/// Run `PROBE_CONCURRENCY` yt-dlp processes in parallel — one per video URL —
/// to harvest view_count + upload_date. Returns the augmented list. If the
/// fetch is cancelled mid-flight, drops the in-progress tasks and returns
/// the partial list (UI shows whatever managed to come back).
/// Pull a video id out of a YouTube/TikTok/Douyin URL. Returns None for
/// hosts we don't recognise.
pub(crate) fn extract_video_id(url: &str) -> Option<String> {
    // YouTube `?v=<id>`
    if let Some(idx) = url.find("?v=").or_else(|| url.find("&v=")) {
        let rest = &url[idx + 3..];
        let id: String = rest.chars().take_while(|c| *c != '&').collect();
        if !id.is_empty() {
            return Some(id);
        }
    }
    // YouTube `/shorts/<id>`, `/embed/<id>`, TikTok `/video/<id>`, etc.
    for marker in ["/shorts/", "/embed/", "/video/", "/v/"] {
        if let Some(idx) = url.find(marker) {
            let rest = &url[idx + marker.len()..];
            let id: String = rest.chars().take_while(|c| !"/?#".contains(*c)).collect();
            if !id.is_empty() {
                return Some(id);
            }
        }
    }
    None
}


/// via `--print`. Way faster + more reliable than 200 separate spawns —
/// each yt-dlp startup costs ~2-3s on Windows so batching cuts huge channels
/// down from minutes to seconds.
async fn enrich_in_parallel(
    app: &AppHandle,
    videos: Vec<ChannelVideo>,
    settings: &Settings,
    my_gen: u64,
    detailed: bool,
) -> AppResult<Vec<ChannelVideo>> {
    use std::collections::HashMap;
    use tokio::sync::Semaphore;
    use tokio::task::JoinSet;

    // Pick which entries actually need probing. In fast mode we skip entries
    // already carrying date+view to save time; in detailed mode we always
    // probe to make sure view_count is exact.
    let mut to_probe: Vec<(usize, String)> = Vec::new();
    for (i, v) in videos.iter().enumerate() {
        if detailed {
            if v.view_count.is_some() && v.upload_date.is_some() {
                continue;
            }
        } else if v.upload_date.is_some() && v.view_count.is_some() {
            continue;
        }
        to_probe.push((i, v.url.clone()));
    }
    if to_probe.is_empty() {
        return Ok(videos);
    }

    let url_to_idx: HashMap<String, usize> = to_probe
        .iter()
        .map(|(i, u)| {
            // Trích id từ URL — YouTube watch?v=<id> hoặc /shorts/<id>.
            let id = extract_video_id(u).unwrap_or_else(|| u.clone());
            (id, *i)
        })
        .collect();

    let sem = Arc::new(Semaphore::new(PROBE_CONCURRENCY));
    let mut set: JoinSet<HashMap<String, (Option<u64>, Option<String>)>> = JoinSet::new();

    for chunk in to_probe.chunks(BATCH_SIZE) {
        let urls: Vec<String> = chunk.iter().map(|(_, u)| u.clone()).collect();
        let app = app.clone();
        let s = settings.clone();
        let sem = sem.clone();
        set.spawn(async move {
            let _permit = sem.acquire_owned().await.ok();
            if FETCH_GENERATION.load(Ordering::SeqCst) != my_gen {
                return HashMap::new();
            }
            probe_batch(&app, &urls, &s, my_gen, detailed).await.unwrap_or_default()
        });
    }

    let mut updated = videos;
    while let Some(joined) = set.join_next().await {
        if FETCH_GENERATION.load(Ordering::SeqCst) != my_gen {
            set.shutdown().await;
            return Ok(updated);
        }
        if let Ok(map) = joined {
            for (url, (view, date)) in map {
                if let Some(&i) = url_to_idx.get(&url) {
                    if let Some(v) = updated.get_mut(i) {
                        if view.is_some() {
                            v.view_count = view;
                        }
                        if date.is_some() {
                            v.upload_date = date;
                        }
                    }
                }
            }
        }
    }
    Ok(updated)
}

/// Single yt-dlp process that probes many URLs at once. Output format per
/// line: `<url>|<view_count>|<upload_date>`. Missing values come back as
/// "NA" (yt-dlp default for missing fields with --print).
async fn probe_batch(
    app: &AppHandle,
    urls: &[String],
    settings: &Settings,
    my_gen: u64,
    want_view: bool,
) -> AppResult<std::collections::HashMap<String, (Option<u64>, Option<String>)>> {
    let res = probe_batch_attempt(app, urls, settings, my_gen, want_view).await;
    if let Err(AppError::YtDlpFailed(ref msg)) = res {
        if crate::args_builder::settings_have_cookies(settings)
            && crate::error::is_cookie_decrypt_error(msg)
        {
            let no_cookies = crate::args_builder::settings_without_cookies(settings);
            return probe_batch_attempt(app, urls, &no_cookies, my_gen, want_view).await;
        }
    }
    res
}

async fn probe_batch_attempt(
    app: &AppHandle,
    urls: &[String],
    settings: &Settings,
    my_gen: u64,
    want_view: bool,
) -> AppResult<std::collections::HashMap<String, (Option<u64>, Option<String>)>> {
    use std::collections::HashMap;

    let print_tpl = if want_view {
        "%(id)s|%(view_count)s|%(upload_date)s"
    } else {
        "%(id)s|_|%(upload_date)s"
    };
    // Note: dùng %(id)s để parse map key — Shorts URL trả /shorts/<id>
    // còn flat-playlist URL trả /watch?v=<id>, dùng id thì khớp cả 2.

    let mut args: Vec<String> = vec![
        "--no-warnings".into(),
        "--ignore-errors".into(),
        "--skip-download".into(),
        "--no-playlist".into(),
        "-4".into(),
        "--socket-timeout".into(),
        "20".into(),
        "--user-agent".into(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36"
            .into(),
        "--print".into(),
        print_tpl.into(),
    ];
    // Per-probe cookies copy — 4 probes run concurrently and yt-dlp rewrites the
    // cookies file on exit; sharing one file corrupts/locks it. See copy_cookies.
    let _ck_guard;
    let settings_copy;
    let settings: &Settings = match settings.cookies_file.as_deref() {
        Some(f) if !f.is_empty() => match crate::ytdlp_runner::copy_cookies(f) {
            Some(tmp) => {
                let mut s = settings.clone();
                s.cookies_file = Some(tmp.to_string_lossy().into_owned());
                _ck_guard = crate::ytdlp_runner::TempCookieCopy(Some(tmp));
                settings_copy = s;
                &settings_copy
            }
            None => settings,
        },
        _ => settings,
    };
    crate::args_builder::push_cookie_args(&mut args, settings);
    crate::args_builder::push_proxy_args(&mut args, settings);
    for u in urls {
        args.push(u.clone());
    }

    let cmd = app
        .shell()
        .sidecar("yt-dlp")
        .map_err(|e| AppError::YtDlpFailed(e.to_string()))?
        .args(args);
    let (mut rx, child) = cmd
        .spawn()
        .map_err(|e| AppError::YtDlpFailed(e.to_string()))?;
    let mut stdout = String::new();
    loop {
        match tokio::time::timeout(Duration::from_millis(250), rx.recv()).await {
            Ok(Some(ev)) => match ev {
                CommandEvent::Stdout(b) => stdout.push_str(&String::from_utf8_lossy(&b)),
                CommandEvent::Terminated(_) => break,
                _ => {}
            },
            Ok(None) => break,
            Err(_) => {
                if FETCH_GENERATION.load(Ordering::SeqCst) != my_gen {
                    let _ = child.kill();
                    return Ok(HashMap::new());
                }
            }
        }
    }
    let mut out = HashMap::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, '|');
        let url = match parts.next() {
            Some(u) => u.trim().to_string(),
            None => continue,
        };
        if url.is_empty() {
            continue;
        }
        let view = parts
            .next()
            .and_then(|s| s.trim().parse::<u64>().ok());
        let date = parts.next().and_then(|s| {
            let s = s.trim();
            if s.is_empty() || s == "NA" || s == "None" || s.len() != 8 {
                None
            } else {
                Some(s.to_string())
            }
        });
        out.insert(url, (view, date));
    }
    Ok(out)
}

/// Parse the `--flat-playlist --dump-single-json` output into our channel
/// info + a flat video list.
fn parse_channel(source_url: &str, value: Value) -> (ChannelInfo, Vec<ChannelVideo>) {
    let info = ChannelInfo {
        url: source_url.to_string(),
        title: value
            .get("channel")
            .or_else(|| value.get("uploader"))
            .or_else(|| value.get("title"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        thumbnail: value
            .get("thumbnails")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.last())
            .and_then(|t| t.get("url"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| value.get("thumbnail").and_then(|v| v.as_str()).map(String::from)),
        video_count: value
            .get("playlist_count")
            .and_then(|v| v.as_u64())
            .map(|x| x as u32),
        extractor: value
            .get("extractor")
            .and_then(|v| v.as_str())
            .unwrap_or("generic")
            .to_string(),
        hidden_downloaded: None,
        channel_id: value
            .get("channel_id")
            .or_else(|| value.get("uploader_id"))
            .and_then(|v| v.as_str())
            .filter(|s| s.starts_with("UC"))
            .map(String::from),
    };

    let mut videos = Vec::new();
    if let Some(entries) = value.get("entries").and_then(|v| v.as_array()) {
        for e in entries {
            if let Some(v) = parse_entry(e) {
                videos.push(v);
            }
        }
    }
    (info, videos)
}

fn parse_entry(e: &Value) -> Option<ChannelVideo> {
    let url = e
        .get("url")
        .or_else(|| e.get("webpage_url"))
        .and_then(|v| v.as_str())?
        .to_string();
    // TikTok photo posts: URL có dạng `tiktok.com/@user/photo/<id>`. Cũng
    // bắt cả ie_key=TikTokPhoto và webpage_url với /photo/.
    let url_lower = url.to_lowercase();
    let is_photo = url_lower.contains("/photo/")
        || e.get("ie_key")
            .and_then(|v| v.as_str())
            .map(|s| s.eq_ignore_ascii_case("tiktokphoto"))
            .unwrap_or(false);
    let title = e
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let duration_sec = e
        .get("duration")
        .and_then(|v| v.as_f64())
        .map(|f| f as u64);
    let view_count = e
        .get("view_count")
        .and_then(|v| v.as_u64())
        .or_else(|| e.get("approximate_view_count").and_then(|v| v.as_u64()));
    // upload_date — yt-dlp `--flat-playlist` thường KHÔNG trả `upload_date`
    // trực tiếp, mà chỉ có `timestamp` (Unix epoch). Convert sang YYYYMMDD
    // để frontend hiển thị/filter đồng nhất.
    let upload_date = e
        .get("upload_date")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| {
            e.get("timestamp")
                .and_then(|v| v.as_i64())
                .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                .map(|dt| dt.format("%Y%m%d").to_string())
        });
    let thumbnail = e
        .get("thumbnails")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.last())
        .and_then(|t| t.get("url"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| e.get("thumbnail").and_then(|v| v.as_str()).map(String::from));
    // TikTok photo posts có thumbnail chứa "photomode" trong path. URL của
    // entry vẫn dạng /video/<id> nên k phân biệt được — phải check thumbnail
    // (path `/tos-alisg-i-photomode-sg/...` hoặc `tplv-photomode-image`).
    let is_photo = is_photo
        || thumbnail
            .as_deref()
            .map(|t| t.contains("photomode"))
            .unwrap_or(false);
    Some(ChannelVideo {
        url,
        title,
        duration_sec,
        view_count,
        upload_date,
        thumbnail,
        is_short: false,
        is_photo,
        hashtags: Vec::new(),
    })
}
