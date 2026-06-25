//! YtDlp_Sidecar orchestrator.
//!
//! Spawn `yt-dlp` qua `tauri_plugin_shell::ShellExt::shell().sidecar("yt-dlp")`.
//! - `fetch_metadata(url)` chạy `--dump-single-json` với timeout 30s, parse JSON
//!   sang `VideoMetadata` (xem yt-dlp output: title, channel, thumbnail, duration,
//!   formats, automatic_captions, subtitles, entries, playlist_count).
//! - `run_download(item, cancel_token, progress_tx)` spawn child download, đọc
//!   stdout line-buffered, parse progress, gửi qua mpsc, chờ exit code.

use crate::args_builder::{self, BuildMode};
use crate::error::{AppError, AppResult};
use crate::models::{
    DownloadItem, PlaylistEntry, ProgressSnapshot, QualityFormat, Settings,
    SubtitleTrack, VideoMetadata,
};
use crate::progress_parser;
use serde_json::Value;
use std::time::Duration;
use tauri::AppHandle;
use tauri::Manager;
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const METADATA_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub enum RunOutcome {
    Completed {
        output_path: Option<std::path::PathBuf>,
        title: Option<String>,
        thumbnail: Option<String>,
        channel: Option<String>,
    },
    Cancelled,
    /// yt-dlp skipped the item because it's already in the download-archive
    /// (the "Bỏ qua video đã tải" feature). Not an error, not a real download.
    Skipped,
    Failed { reason: String },
}

/// Sự kiện metadata bắt được từ stdout yt-dlp trước khi download kết thúc, để
/// queue cập nhật item ngay (UI thấy thumbnail/title/channel trong vài giây
/// đầu thay vì phải đợi xong toàn bộ).
#[derive(Debug, Clone)]
pub enum MetaEvent {
    Title(String),
    Thumbnail(String),
    Channel(String),
}

#[derive(Clone)]
pub struct YtDlpRunner {
    app: AppHandle,
    /// Path to the yt-dlp `--download-archive` file (records IDs of finished
    /// downloads so re-runs skip them). `None` if app_data_dir can't resolve.
    archive_path: Option<std::path::PathBuf>,
}

impl YtDlpRunner {
    pub fn new(app: AppHandle) -> Self {
        let archive_path = app
            .path()
            .app_data_dir()
            .ok()
            .map(|d| d.join("download_archive.txt"));
        Self { app, archive_path }
    }

    pub async fn fetch_metadata(&self, url: &str, settings: &Settings) -> AppResult<VideoMetadata> {
        // Pre-resolve site-specific quirks (e.g., viralhog watch → embed URL)
        // before yt-dlp ever sees the URL.
        let resolved = crate::url_resolver::resolve(url).await;
        let url_for_dlp = resolved.as_str();

        // First try: native extractor (or generic if site is in our hint list).
        let force_generic_first = matches!(
            crate::url_validator::resolve_extractor(url_for_dlp),
            Some("viralhog" | "9gag" | "imgur" | "gfycat" | "redgifs" | "coub" | "tumblr" | "newgrounds")
        );

        match self.fetch_metadata_inner(url_for_dlp, force_generic_first, settings).await {
            Ok(md) => Ok(md),
            // Browser cookies couldn't be decrypted (DPAPI) — retry without them.
            Err(AppError::YtDlpFailed(ref msg))
                if args_builder::settings_have_cookies(settings)
                    && crate::error::is_cookie_decrypt_error(msg) =>
            {
                let no_cookies = args_builder::settings_without_cookies(settings);
                self.fetch_metadata_inner(url_for_dlp, force_generic_first, &no_cookies).await
            }
            Err(AppError::YtDlpFailed(msg)) if !force_generic_first && is_unsupported_url(&msg) => {
                // yt-dlp doesn't have a handler for this site; retry with generic
                // extractor — it scans the HTML for `<video>` / og:video.
                self.fetch_metadata_inner(url_for_dlp, true, settings).await
            }
            Err(e) => Err(e),
        }
    }

    async fn fetch_metadata_inner(
        &self,
        url: &str,
        force_generic: bool,
        settings: &Settings,
    ) -> AppResult<VideoMetadata> {
        let mut args: Vec<String> = vec![
            "--no-warnings".into(),
            "--dump-single-json".into(),
            "--flat-playlist".into(),
            "--encoding".into(), "utf-8".into(),
            "--user-agent".into(),
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36".into(),
            "-4".into(),
            "--socket-timeout".into(), "30".into(),
        ];
        // Cookies từ trình duyệt — bắt buộc cho Douyin / Bilibili / IG private /
        // YouTube age-gated. File cookies.txt ưu tiên hơn browser (AppBound
        // encryption issue). Caller retries without cookies on DPAPI failure.
        args_builder::push_cookie_args(&mut args, settings);
        args_builder::push_proxy_args(&mut args, settings);
        if force_generic {
            args.push("--force-generic-extractor".into());
        }
        args.push(url.to_string());
        let _ = args_builder::BuildMode::FetchMetadata; // keep reference

        let cmd = self.app.shell().sidecar("yt-dlp").map_err(|e| AppError::YtDlpFailed(e.to_string()))?.args(args);

        let fut = async {
            let (mut rx, _child) = cmd.spawn().map_err(|e| AppError::YtDlpFailed(e.to_string()))?;
            let mut stdout_buf = String::new();
            let mut stderr_buf = String::new();
            let mut exit_code: Option<i32> = None;
            while let Some(ev) = rx.recv().await {
                match ev {
                    CommandEvent::Stdout(bytes) => stdout_buf.push_str(&String::from_utf8_lossy(&bytes)),
                    CommandEvent::Stderr(bytes) => stderr_buf.push_str(&String::from_utf8_lossy(&bytes)),
                    CommandEvent::Terminated(payload) => { exit_code = payload.code; break; }
                    _ => {}
                }
            }
            if exit_code.unwrap_or(-1) != 0 {
                let last = stderr_buf.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("yt-dlp failed").to_string();
                return Err(AppError::YtDlpFailed(last));
            }
            let value: Value = serde_json::from_str(&stdout_buf)?;
            Ok(parse_metadata(url, value))
        };

        match tokio::time::timeout(METADATA_TIMEOUT, fut).await {
            Ok(r) => r,
            Err(_) => Err(AppError::Timeout),
        }
    }

    pub async fn run_download(
        &self,
        item: &DownloadItem,
        settings: &Settings,
        resume: bool,
        cancel: CancellationToken,
        progress_tx: mpsc::Sender<ProgressSnapshot>,
        meta_tx: mpsc::Sender<MetaEvent>,
        output_stem: Option<String>,
    ) -> AppResult<RunOutcome> {
        // ── TikTok photo posts: yt-dlp only extracts mp3 audio, so we route
        //    these through tikwm + parallel image fetcher and produce a folder
        //    of jpgs named after the post title.
        let url_lower = item.request.url.to_lowercase();
        let looks_like_tiktok = url_lower.contains("tiktok.com");
        if looks_like_tiktok {
            // Fetch tikwm metadata; if it has `images` we treat as photo post.
            if let Some(post) = crate::tiktok_photo::fetch_photo_meta(&item.request.url).await {
                // Optimistically emit the title so UI shows nice name immediately.
                let _ = meta_tx.send(MetaEvent::Title(post.title.clone())).await;
                let _ = progress_tx
                    .send(ProgressSnapshot {
                        bytes_downloaded: 0,
                        bytes_total: None,
                        speed_bps: None,
                        eta_sec: None,
                        percent: None,
                    })
                    .await;
                let folder = crate::tiktok_photo::download_photo_post(&post, &item.request.save_folder).await?;
                return Ok(RunOutcome::Completed {
                    output_path: Some(folder),
                    title: Some(post.title.clone()),
                    thumbnail: post.images.first().cloned(),
                    channel: None,
                });
            }
        }

        // First attempt: native extractor (or generic if URL is in our hint list).
        let outcome = self
            .run_once(item, settings, resume, false, cancel.clone(), progress_tx.clone(), meta_tx.clone(), output_stem.clone())
            .await?;

        if let RunOutcome::Failed { reason } = &outcome {
            // YouTube anti-bot / 429 — if proxies are configured, retry once so
            // run_once rebuilds args and rotates to the next proxy (fresh IP).
            if !settings.proxies.is_empty()
                && crate::error::is_bot_error(reason)
                && !cancel.is_cancelled()
            {
                return self
                    .run_once(item, settings, resume, false, cancel.clone(), progress_tx.clone(), meta_tx.clone(), output_stem.clone())
                    .await;
            }
            // Browser cookies couldn't be decrypted (DPAPI) — retry without them.
            // Public videos don't need cookies, so this recovers transparently.
            if args_builder::settings_have_cookies(settings)
                && crate::error::is_cookie_decrypt_error(reason)
                && !cancel.is_cancelled()
            {
                let no_cookies = args_builder::settings_without_cookies(settings);
                return self
                    .run_once(item, &no_cookies, resume, false, cancel.clone(), progress_tx.clone(), meta_tx.clone(), output_stem.clone())
                    .await;
            }
            // Auto-retry with `--force-generic-extractor` when yt-dlp says it
            // can't handle the site — works for viralhog and any other
            // "self-hosted MP4" page that exposes the URL in HTML <video>.
            if is_unsupported_url(reason) && !cancel.is_cancelled() {
                return self
                    .run_once(item, settings, resume, true, cancel, progress_tx, meta_tx, output_stem)
                    .await;
            }
        }
        Ok(outcome)
    }

    async fn run_once(
        &self,
        item: &DownloadItem,
        settings: &Settings,
        resume: bool,
        force_generic: bool,
        cancel: CancellationToken,
        progress_tx: mpsc::Sender<ProgressSnapshot>,
        meta_tx: mpsc::Sender<MetaEvent>,
        output_stem: Option<String>,
    ) -> AppResult<RunOutcome> {
        let mut args = args_builder::build(
            &item.request,
            settings,
            BuildMode::Download { resume, force_generic, output_stem },
        );
        // "Bỏ qua video đã tải": record finished downloads in an archive file and
        // skip anything already listed. yt-dlp accepts options after the URL, so
        // appending here is fine. Only YouTube/most extractors expose a stable id;
        // ones that don't simply never match and download normally.
        if settings.skip_downloaded {
            if let Some(archive) = &self.archive_path {
                args.push("--download-archive".into());
                args.push(archive.to_string_lossy().to_string());
            }
        }
        let cmd = self.app.shell().sidecar("yt-dlp").map_err(|e| AppError::YtDlpFailed(e.to_string()))?.args(args);

        let (mut rx, child) = cmd.spawn().map_err(|e| AppError::YtDlpFailed(e.to_string()))?;
        // SAFETY: rustc tưởng `child` không cần `mut` vì .kill() lấy &self,
        // nhưng đa số API tauri-plugin-shell version mới yêu cầu mut. Giữ nguyên.

        let mut stderr_tail = String::new();
        let mut last_emit: Option<std::time::Instant> = None;
        let mut output_path: Option<std::path::PathBuf> = None;
        let mut resolved_title: Option<String> = None;
        let mut resolved_thumbnail: Option<String> = None;
        let mut resolved_channel: Option<String> = None;
        let mut cancelled = false;
        let mut archived_skip = false;
        let mut exit_code: Option<i32> = None;
        let started_at = std::time::Instant::now();
        let mut last_activity = std::time::Instant::now();
        const STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);
        const HARD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = child.kill();
                    cancelled = true;
                    break;
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(15)) => {
                    if last_activity.elapsed() > STALL_TIMEOUT {
                        let _ = child.kill();
                        stderr_tail.push_str(&format!("\n[watchdog] no activity for {}s, killed yt-dlp", STALL_TIMEOUT.as_secs()));
                        break;
                    }
                    if started_at.elapsed() > HARD_TIMEOUT {
                        let _ = child.kill();
                        stderr_tail.push_str("\n[watchdog] hard timeout 30 minutes, killed yt-dlp");
                        break;
                    }
                }
                ev = rx.recv() => {
                    let Some(ev) = ev else { break; };
                    last_activity = std::time::Instant::now();
                    match ev {
                        CommandEvent::Stdout(bytes) => {
                            let text = String::from_utf8_lossy(&bytes);
                            for line in text.split_terminator(|c| c == '\n' || c == '\r') {
                                // "Bỏ qua video đã tải": yt-dlp prints this when the
                                // id is already in the download-archive and exits 0.
                                if line.contains("has already been recorded in the archive") {
                                    archived_skip = true;
                                }
                                if let Some(p) = progress_parser::parse_progress(line) {
                                    // Always emit the FIRST progress event immediately so the
                                    // UI bar starts moving without waiting for the throttle window.
                                    let first = last_emit.is_none();
                                    if first || progress_parser::should_emit(&mut last_emit, std::time::Duration::from_millis(250)) {
                                        if first { last_emit = Some(std::time::Instant::now()); }
                                        let _ = progress_tx.send(p).await;
                                    }
                                }
                                if let Some(rest) = line.strip_prefix("[download] Destination: ") {
                                    output_path = Some(std::path::PathBuf::from(rest.trim()));
                                } else if let Some(rest) = line.strip_prefix("[Merger] Merging formats into \"") {
                                    let path = rest.trim_end_matches('"');
                                    output_path = Some(std::path::PathBuf::from(path));
                                } else if let Some(rest) = line.strip_prefix("[ExtractAudio] Destination: ") {
                                    output_path = Some(std::path::PathBuf::from(rest.trim()));
                                } else if let Some(rest) = line.strip_prefix("FINALPATH|") {
                                    // Most reliable: emitted by `--print after_move:...`.
                                    let p = rest.trim();
                                    if !p.is_empty() {
                                        output_path = Some(std::path::PathBuf::from(p));
                                    }
                                } else if let Some(rest) = line.strip_prefix("TITLE|") {
                                    let t = rest.trim();
                                    if !t.is_empty() && t != "NA" {
                                        let owned = t.to_string();
                                        resolved_title = Some(owned.clone());
                                        let _ = meta_tx.send(MetaEvent::Title(owned)).await;
                                    }
                                } else if let Some(rest) = line.strip_prefix("THUMB|") {
                                    let t = rest.trim();
                                    if !t.is_empty() && t != "NA" {
                                        let owned = t.to_string();
                                        resolved_thumbnail = Some(owned.clone());
                                        let _ = meta_tx.send(MetaEvent::Thumbnail(owned)).await;
                                    }
                                } else if let Some(rest) = line.strip_prefix("CHANNEL|") {
                                    let t = rest.trim();
                                    if !t.is_empty() && t != "NA" {
                                        let owned = t.to_string();
                                        resolved_channel = Some(owned.clone());
                                        let _ = meta_tx.send(MetaEvent::Channel(owned)).await;
                                    }
                                } else if let Some(rest) = line.strip_prefix("[download] ") {
                                    // yt-dlp prints "[download] <path> has already been downloaded"
                                    if let Some(path) = rest.strip_suffix(" has already been downloaded") {
                                        output_path = Some(std::path::PathBuf::from(path.trim()));
                                    }
                                }
                            }
                        }
                        CommandEvent::Stderr(bytes) => {
                            let text = String::from_utf8_lossy(&bytes);
                            stderr_tail.push_str(&text);
                            if stderr_tail.len() > 8192 {
                                let drop = stderr_tail.len() - 4096;
                                stderr_tail.drain(..drop);
                            }
                        }
                        CommandEvent::Terminated(payload) => { exit_code = payload.code; break; }
                        _ => {}
                    }
                }
            }
        }

        if cancelled { return Ok(RunOutcome::Cancelled); }
        if archived_skip && output_path.is_none() {
            return Ok(RunOutcome::Skipped);
        }
        match exit_code.unwrap_or(-1) {
            0 => Ok(RunOutcome::Completed {
                output_path,
                title: resolved_title,
                thumbnail: resolved_thumbnail,
                channel: resolved_channel,
            }),
            code => {
                let last = stderr_tail.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("yt-dlp failed").to_string();
                Ok(RunOutcome::Failed { reason: format!("(exit {code}) {last}") })
            }
        }
    }
}

// -- JSON parsing helpers --------------------------------------------------

/// Detect "Unsupported URL: ..." error from yt-dlp stderr — both the bare
/// message and the `ERROR: Unsupported URL` form.
fn is_unsupported_url(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("unsupported url") || lower.contains("no suitable extractor")
}

fn parse_metadata(url: &str, value: Value) -> VideoMetadata {
    let extractor = value.get("extractor").and_then(|v| v.as_str()).unwrap_or("generic").to_string();
    let title = value.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let channel = value.get("channel").and_then(|v| v.as_str()).map(|s| s.to_string())
        .or_else(|| value.get("uploader").and_then(|v| v.as_str()).map(|s| s.to_string()));
    let thumbnail = value.get("thumbnail").and_then(|v| v.as_str()).map(|s| s.to_string());
    let duration_sec = value.get("duration").and_then(|v| v.as_f64()).map(|f| f as u64);

    let mut formats = Vec::new();
    if let Some(arr) = value.get("formats").and_then(|v| v.as_array()) {
        for f in arr {
            if let Some(qf) = parse_format(f) { formats.push(qf); }
        }
    }
    let subtitles = parse_subtitles(&value);
    let (playlist_entries, playlist_total) = parse_playlist(&value);

    VideoMetadata {
        url: url.to_string(), extractor, title, channel, thumbnail, duration_sec,
        formats, subtitles, playlist_entries, playlist_total,
    }
}

fn parse_format(f: &Value) -> Option<QualityFormat> {
    let format_id = f.get("format_id").and_then(|v| v.as_str())?.to_string();
    let ext = f.get("ext").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let resolution = f.get("resolution").and_then(|v| v.as_str()).map(|s| s.to_string());
    let fps = f.get("fps").and_then(|v| v.as_f64()).map(|x| x as f32);
    let vcodec = f.get("vcodec").and_then(|v| v.as_str()).map(|s| s.to_string());
    let acodec = f.get("acodec").and_then(|v| v.as_str()).map(|s| s.to_string());
    let abr = f.get("abr").and_then(|v| v.as_f64()).map(|x| x as f32);
    let vbr = f.get("vbr").and_then(|v| v.as_f64()).map(|x| x as f32);
    let filesize = f.get("filesize").or_else(|| f.get("filesize_approx")).and_then(|v| v.as_u64());
    let height = f.get("height").and_then(|v| v.as_u64()).map(|x| x as u32);
    let width = f.get("width").and_then(|v| v.as_u64()).map(|x| x as u32);
    let tbr = f.get("tbr").and_then(|v| v.as_f64()).map(|x| x as f32);
    let format_note = f.get("format_note").and_then(|v| v.as_str()).map(|s| s.to_string());

    let is_audio_only = vcodec.as_deref() == Some("none") && acodec.as_deref() != Some("none");
    let is_video_only = acodec.as_deref() == Some("none") && vcodec.as_deref() != Some("none");

    Some(QualityFormat {
        format_id, ext, resolution, fps, vcodec, acodec, abr, vbr, filesize,
        is_audio_only, is_video_only, format_note, tbr, height, width,
    })
}

fn parse_subtitles(value: &Value) -> Vec<SubtitleTrack> {
    let mut out = Vec::new();
    if let Some(map) = value.get("subtitles").and_then(|v| v.as_object()) {
        for (lang, _tracks) in map {
            out.push(SubtitleTrack { lang_code: lang.clone(), lang_name: lang.clone(), is_auto: false });
        }
    }
    if let Some(map) = value.get("automatic_captions").and_then(|v| v.as_object()) {
        for (lang, _tracks) in map {
            // Mark autos with "is_auto = true". Avoid duplicating manual track of same lang.
            if !out.iter().any(|t| t.lang_code == *lang) {
                out.push(SubtitleTrack { lang_code: lang.clone(), lang_name: lang.clone(), is_auto: true });
            }
        }
    }
    out
}

fn parse_playlist(value: &Value) -> (Option<Vec<PlaylistEntry>>, Option<u32>) {
    let entries = value.get("entries").and_then(|v| v.as_array());
    if let Some(arr) = entries {
        let parsed: Vec<PlaylistEntry> = arr.iter().filter_map(|e| {
            let url = e.get("url").or_else(|| e.get("webpage_url")).and_then(|v| v.as_str())?.to_string();
            let title = e.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let duration_sec = e.get("duration").and_then(|v| v.as_f64()).map(|f| f as u64);
            Some(PlaylistEntry { url, title, duration_sec })
        }).collect();
        let total = value.get("playlist_count").and_then(|v| v.as_u64()).map(|x| x as u32).or_else(|| Some(parsed.len() as u32));
        (Some(parsed), total)
    } else { (None, None) }
}
