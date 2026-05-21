//! Fetch a flat listing of videos from a channel/user URL using yt-dlp.
//!
//! Strategy:
//! - Run yt-dlp with `--flat-playlist --dump-single-json` to get the entry
//!   list very fast (no per-video network round-trips).
//! - For YouTube channels we resolve to the `/videos` tab so we don't
//!   accidentally include Shorts/Streams unless the user wants them.
//! - We deliberately keep this synchronous-feeling: the call returns a list
//!   the UI can immediately render with checkboxes; downloads are deferred
//!   to `enqueue_batch`.

use crate::error::{AppError, AppResult};
use crate::models::{ChannelInfo, ChannelVideo, Settings};
use serde_json::Value;
use std::time::Duration;
use tauri::AppHandle;
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

const TIMEOUT: Duration = Duration::from_secs(120);

/// Normalise a YouTube channel URL so we always hit the Videos tab.
/// Examples:
///   `https://www.youtube.com/@MrBeast`        → `.../@MrBeast/videos`
///   `https://www.youtube.com/channel/UCxxx`   → `.../channel/UCxxx/videos`
///   `https://www.youtube.com/c/PewDiePie`     → `.../c/PewDiePie/videos`
///   `https://www.youtube.com/watch?v=abc`     → `.../@MrBeast/videos` (extract from /watch URL via yt-dlp metadata is too slow; we leave it untouched and let yt-dlp choose)
/// Also rewrite TikTok video URLs back to the user profile so "Tải kênh"
/// works when the user pastes a single video URL by mistake.
/// Pass anything else through unchanged so playlists / Douyin URLs
/// reach yt-dlp untouched.
fn normalise_channel_url(raw: &str) -> String {
    let lower = raw.to_lowercase();

    // TikTok: collapse `/@user/video/<id>` → `/@user`. Even if the user
    // already gave us `/@user`, leave it as-is.
    if lower.contains("tiktok.com/@") {
        if let Some(idx) = raw.find("/video/") {
            return raw[..idx].to_string();
        }
        return raw.to_string();
    }

    if !lower.contains("youtube.com") {
        return raw.to_string();
    }
    if lower.ends_with("/videos")
        || lower.ends_with("/shorts")
        || lower.ends_with("/streams")
        || lower.contains("/playlist?list=")
        || lower.contains("/watch?")
    {
        return raw.to_string();
    }
    let trimmed = raw.trim_end_matches('/');
    format!("{trimmed}/videos")
}

/// Fetch up to `limit` recent videos from a channel/user URL.
///
/// `limit` is honoured server-side via `--playlist-end <N>` so we don't have
/// to wait for yt-dlp to enumerate the full channel (some YouTube channels
/// have 10k+ videos).
pub async fn fetch_channel(
    app: &AppHandle,
    url: &str,
    limit: u32,
    settings: &Settings,
) -> AppResult<(ChannelInfo, Vec<ChannelVideo>)> {
    let resolved = normalise_channel_url(url);
    let mut args: Vec<String> = vec![
        "--no-warnings".into(),
        "--dump-single-json".into(),
        "--flat-playlist".into(),
        "--encoding".into(),
        "utf-8".into(),
        "-4".into(),
        "--socket-timeout".into(),
        "30".into(),
        "--user-agent".into(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36"
            .into(),
        "--playlist-end".into(),
        limit.to_string(),
        // Ask the YouTube tab extractor to include approximate upload dates
        // alongside the flat listing. Without this flag yt-dlp omits the
        // `timestamp` / `upload_date` fields for channel videos which would
        // make the date filter useless.
        "--extractor-args".into(),
        "youtubetab:approximate_date".into(),
    ];
    // Cookies — same priority as fetch_metadata.
    if let Some(file) = settings.cookies_file.as_deref() {
        if !file.is_empty() {
            args.push("--cookies".into());
            args.push(file.to_string());
        }
    } else if let Some(browser) = settings.cookies_browser.as_deref() {
        if !browser.is_empty() {
            args.push("--cookies-from-browser".into());
            args.push(browser.to_string());
        }
    }
    args.push(resolved.clone());

    let cmd = app
        .shell()
        .sidecar("yt-dlp")
        .map_err(|e| AppError::YtDlpFailed(e.to_string()))?
        .args(args);

    let fut = async {
        let (mut rx, _child) = cmd
            .spawn()
            .map_err(|e| AppError::YtDlpFailed(e.to_string()))?;
        let mut stdout_buf = String::new();
        let mut stderr_buf = String::new();
        let mut exit_code: Option<i32> = None;
        while let Some(ev) = rx.recv().await {
            match ev {
                CommandEvent::Stdout(bytes) => stdout_buf.push_str(&String::from_utf8_lossy(&bytes)),
                CommandEvent::Stderr(bytes) => stderr_buf.push_str(&String::from_utf8_lossy(&bytes)),
                CommandEvent::Terminated(payload) => {
                    exit_code = payload.code;
                    break;
                }
                _ => {}
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
        Ok(parse_channel(&resolved, value))
    };

    match tokio::time::timeout(TIMEOUT, fut).await {
        Ok(r) => r,
        Err(_) => Err(AppError::Timeout),
    }
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
    let title = e
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let duration_sec = e
        .get("duration")
        .and_then(|v| v.as_f64())
        .map(|f| f as u64);
    let view_count = e.get("view_count").and_then(|v| v.as_u64());
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
    Some(ChannelVideo {
        url,
        title,
        duration_sec,
        view_count,
        upload_date,
        thumbnail,
    })
}
