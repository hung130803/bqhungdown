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

static COOKIE_COPY_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Deletes a per-download cookies copy when dropped.
pub(crate) struct TempCookieCopy(pub(crate) Option<std::path::PathBuf>);
impl Drop for TempCookieCopy {
    fn drop(&mut self) {
        if let Some(p) = &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Copy the user's cookies.txt to a unique temp file. yt-dlp REWRITES the
/// `--cookies` file when it exits (to persist refreshed cookies); with several
/// concurrent downloads sharing one file, those writes race and corrupt it
/// ("does not look like a Netscape format cookies file") or lock it on Windows
/// ("[Errno 13] Permission denied"). A per-download copy removes the sharing.
pub(crate) fn copy_cookies(src: &str) -> Option<std::path::PathBuf> {
    if src.is_empty() || !std::path::Path::new(src).is_file() {
        return None;
    }
    let n = COOKIE_COPY_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dst = std::env::temp_dir().join(format!("bqh_cookies_{}_{}.txt", std::process::id(), n));
    std::fs::copy(src, &dst).ok().map(|_| dst)
}

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

        match self.fetch_metadata_inner(url_for_dlp, force_generic_first, false, settings).await {
            Ok(md) => Ok(md),
            // Browser cookies couldn't be decrypted (DPAPI) — retry without them.
            Err(AppError::YtDlpFailed(ref msg))
                if args_builder::settings_have_cookies(settings)
                    && crate::error::is_cookie_decrypt_error(msg) =>
            {
                let no_cookies = args_builder::settings_without_cookies(settings);
                self.fetch_metadata_inner(url_for_dlp, force_generic_first, false, &no_cookies).await
            }
            // YouTube anti-bot / 403 khi lấy metadata — thử lại bằng client
            // dự phòng (mweb + PO token thường xuyên qua được), giống hệt
            // cơ chế của run_download.
            Err(AppError::YtDlpFailed(ref msg))
                if args_builder::is_youtube(url_for_dlp)
                    && (crate::error::is_bot_error(msg)
                        || crate::error::is_forbidden_error(msg)
                        || crate::error::is_format_error(msg)) =>
            {
                self.fetch_metadata_inner(url_for_dlp, force_generic_first, true, settings).await
            }
            Err(AppError::YtDlpFailed(msg)) if !force_generic_first && is_unsupported_url(&msg) => {
                // yt-dlp doesn't have a handler for this site; retry with generic
                // extractor — it scans the HTML for `<video>` / og:video.
                self.fetch_metadata_inner(url_for_dlp, true, false, settings).await
            }
            Err(e) => Err(e),
        }
    }

    async fn fetch_metadata_inner(
        &self,
        url: &str,
        force_generic: bool,
        fallback_clients: bool,
        settings: &Settings,
    ) -> AppResult<VideoMetadata> {
        let mut args: Vec<String> = vec![
            "--no-warnings".into(),
            "--dump-single-json".into(),
            "--flat-playlist".into(),
            "--encoding".into(), "utf-8".into(),
            "-4".into(),
            "--socket-timeout".into(), "30".into(),
        ];
        // UA cứng chỉ cho site ngoài YouTube — với YouTube, yt-dlp tự gửi UA
        // khớp từng player client; ép UA lệch fingerprint là nguồn 403/bot-check.
        if !args_builder::is_youtube(url) {
            args.push("--user-agent".into());
            args.push("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36".into());
        }
        // Cookies từ trình duyệt — bắt buộc cho Douyin / Bilibili / IG private /
        // YouTube age-gated. File cookies.txt ưu tiên hơn browser (AppBound
        // encryption issue). Caller retries without cookies on DPAPI failure.
        args_builder::push_cookie_args(&mut args, settings);
        args_builder::push_proxy_args(&mut args, settings);
        args_builder::push_bilibili_headers(&mut args, url);
        if force_generic {
            args.push("--force-generic-extractor".into());
        }
        // Retry sau bot/403: kéo metadata qua client dự phòng — mweb có PO
        // token vẫn sống khi client mặc định bị chặn theo IP.
        if fallback_clients {
            args.push("--extractor-args".into());
            args.push("youtube:player_client=default,tv,mweb,web_safari".into());
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
                let last = crate::error::best_error_line(&stderr_buf);
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
            .run_once(item, settings, resume, false, false, cancel.clone(), progress_tx.clone(), meta_tx.clone(), output_stem.clone())
            .await?;

        if let RunOutcome::Failed { reason } = &outcome {
            // YouTube anti-bot ("Sign in to confirm…" / 429): client mặc định
            // bị chặn theo IP, nhưng client dự phòng (nhất là mweb + PO token
            // từ bgutil) thường vẫn XUYÊN QUA được ngay — kiểm chứng thực tế
            // 2026-07 trên IP đang bị flag: default chết, mweb tải full speed.
            // → retry ngay với safe_retry (đổi client + networking dè dặt),
            // proxy (nếu có) cũng tự xoay vì args được build lại. Chỉ khi lần
            // này cũng fail mới rơi xuống cooldown của queue.
            if crate::error::is_bot_error(reason) && !cancel.is_cancelled() {
                return self
                    .run_once(item, settings, resume, false, true, cancel.clone(), progress_tx.clone(), meta_tx.clone(), output_stem.clone())
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
                    .run_once(item, &no_cookies, resume, false, false, cancel.clone(), progress_tx.clone(), meta_tx.clone(), output_stem.clone())
                    .await;
            }
            // "Requested format is not available" (YouTube SABR hiding URLs)
            // hoặc HTTP 403 (googlevideo từ chối URL đã extract — thiếu PO
            // token / lệch IP / player đổi) — retry kéo format từ client khác
            // (tv/mweb/web_safari) + networking dè dặt; proxy cũng tự xoay
            // sang con kế tiếp vì args được build lại.
            if (crate::error::is_format_error(reason)
                || crate::error::is_forbidden_error(reason))
                && !cancel.is_cancelled()
            {
                return self
                    .run_once(item, settings, resume, false, true, cancel, progress_tx, meta_tx, output_stem)
                    .await;
            }
            // "Video unavailable" trên YouTube: khi tải cả kênh dồn dập,
            // YouTube soft-block IP bằng cách NÓI DỐI là video không tồn tại
            // (kiểm chứng thực tế 2026-07: cùng video, lúc batch báo
            // unavailable, thử riêng lẻ thì sống nguyên). Quy trình:
            //   1. Thử lại bằng client dự phòng (mweb/tv) — nhiều khi qua luôn.
            //   2. Vẫn "unavailable" → hỏi oembed (endpoint công khai, không
            //      bị bot-gate): video còn sống → gắn SOFT_BLOCK_MARKER để
            //      queue cooldown rồi TỰ tải lại; chết thật → fail hẳn với
            //      thông báo "video đã bị xoá" (lúc này mới đúng).
            if crate::error::is_unavailable_error(reason)
                && args_builder::is_youtube(&item.request.url)
                && !cancel.is_cancelled()
            {
                let second = self
                    .run_once(item, settings, resume, false, true, cancel.clone(), progress_tx.clone(), meta_tx.clone(), output_stem.clone())
                    .await?;
                if let RunOutcome::Failed { reason: r2 } = &second {
                    if crate::error::is_unavailable_error(r2)
                        && youtube_video_exists(&item.request.url).await
                    {
                        return Ok(RunOutcome::Failed {
                            reason: format!("{} {r2}", crate::error::SOFT_BLOCK_MARKER),
                        });
                    }
                }
                return Ok(second);
            }
            // Auto-retry with `--force-generic-extractor` when yt-dlp says it
            // can't handle the site — works for viralhog and any other
            // "self-hosted MP4" page that exposes the URL in HTML <video>.
            if is_unsupported_url(reason) && !cancel.is_cancelled() {
                return self
                    .run_once(item, settings, resume, true, false, cancel, progress_tx, meta_tx, output_stem)
                    .await;
            }
        }
        Ok(outcome)
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_once(
        &self,
        item: &DownloadItem,
        settings: &Settings,
        resume: bool,
        force_generic: bool,
        force_clients: bool,
        cancel: CancellationToken,
        progress_tx: mpsc::Sender<ProgressSnapshot>,
        meta_tx: mpsc::Sender<MetaEvent>,
        output_stem: Option<String>,
    ) -> AppResult<RunOutcome> {
        // Give this download its own cookies copy so concurrent downloads don't
        // race on the shared cookies file (yt-dlp rewrites it on exit). The
        // guard deletes the copy when run_once returns.
        let _cookie_guard;
        let settings_copy;
        let settings: &Settings = match settings.cookies_file.as_deref() {
            Some(f) if !f.is_empty() => match copy_cookies(f) {
                Some(tmp) => {
                    let mut s = settings.clone();
                    s.cookies_file = Some(tmp.to_string_lossy().into_owned());
                    _cookie_guard = TempCookieCopy(Some(tmp));
                    settings_copy = s;
                    &settings_copy
                }
                None => settings,
            },
            _ => settings,
        };

        // safe_retry (= force_clients): args_builder thêm player client dự
        // phòng cho YouTube + hạ -N, bỏ aria2c — combo chống 403.
        let mut args = args_builder::build(
            &item.request,
            settings,
            BuildMode::Download {
                resume,
                force_generic,
                output_stem,
                safe_retry: force_clients,
                // MỖI LƯỢT TẢI MỘT THƯ MỤC TẠM RIÊNG (`.bqd-temp/<short_id>`).
                // Bản cũ ghi mảnh thẳng vào thư mục kênh — nơi bộ dọn rác, lượt
                // tải khác và luồng dọn-sau-huỷ cùng thò tay vào → mảnh bị xoá
                // giữa lúc đang tải → "[Errno 2] ... .part-Frag4" + hàng chục
                // file rời rạc. Nhãn dùng `short_id` nên BỀN qua khởi động lại
                // (resume vẫn thấy đúng mảnh cũ của chính nó).
                temp_tag: Some(item.short_id.clone()),
            },
        );

        // "Bỏ qua video đã tải": record finished downloads in an archive file and
        // skip anything already listed. yt-dlp accepts options after the URL, so
        // appending here is fine. Only YouTube/most extractors expose a stable id;
        // ones that don't simply never match and download normally.
        // force_redownload (nút "Vẫn tải video này" trên mục Bỏ qua): bỏ hẳn
        // --download-archive cho riêng lần chạy này để yt-dlp không né.
        if settings.skip_downloaded && !item.request.force_redownload {
            if let Some(archive) = &self.archive_path {
                args.push("--download-archive".into());
                args.push(archive.to_string_lossy().to_string());
            }
        }

        // Bilibili cần cookie `buvid3` để qua tường lửa risk-control (thiếu →
        // HTTP 412). yt-dlp không tự lấy buvid, nên app tự xin từ trang chủ
        // (qua proxy nếu có) rồi truyền vào. Chỉ thêm khi user CHƯA cấu hình
        // file cookies (nếu có cookies premium thì đã gồm buvid + SESSDATA).
        if args_builder::is_bilibili(&item.request.url)
            && !args_builder::settings_have_cookies(settings)
        {
            let proxy = args_builder::next_proxy(settings);
            if let Some(buvid) = fetch_bilibili_buvid(&item.request.url, &proxy).await {
                args.push("--add-header".into());
                args.push(format!("Cookie:buvid3={buvid}"));
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
        // 240s (không phải 90s): yt-dlp IM LẶNG hợp lệ khá lâu ở nhiều pha —
        // trích format / tạo PO token (JS runtime nặng) / ffmpeg GHÉP video+
        // audio file lớn (yt-dlp không in gì suốt lúc ghép) / YouTube bóp băng
        // thông lúc chạy nhiều. 90s giết oan hàng loạt. 240s vẫn để HARD_TIMEOUT
        // 30 phút bắt treo thật, và stall thật giờ được tự retry (xem queue).
        const STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(240);
        const HARD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

        let child_pid = child.pid();
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    // Giết CẢ CÂY tiến trình, không chỉ yt-dlp: trên Windows
                    // child.kill() KHÔNG giết ffmpeg con nó spawn (đang ghép
                    // file) → ffmpeg "mồ côi" giữ khoá file .part VĨNH VIỄN,
                    // user không xoá nổi kể cả trong Explorer (bug thực tế).
                    kill_process_tree(child_pid);
                    let _ = child.kill();
                    cancelled = true;
                    // Đợi tiến trình yt-dlp/ffmpeg chết HẲN trước khi trả về:
                    // trên Windows tiến trình vừa kill còn giữ khoá file `.part`
                    // thêm chốc lát → nếu dọn ngay sẽ "file đang được dùng" và
                    // xoá hụt. Chờ tối đa 5s cho sự kiện Terminated (hoặc kênh
                    // đóng) rồi mới thoát vòng lặp.
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        async {
                            while let Some(ev) = rx.recv().await {
                                if matches!(ev, CommandEvent::Terminated(_)) { break; }
                            }
                        },
                    ).await;
                    break;
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(15)) => {
                    if last_activity.elapsed() > STALL_TIMEOUT {
                        kill_process_tree(child_pid);
                        let _ = child.kill();
                        stderr_tail.push_str(&format!("\n[watchdog] no activity for {}s, killed yt-dlp", STALL_TIMEOUT.as_secs()));
                        break;
                    }
                    if started_at.elapsed() > HARD_TIMEOUT {
                        kill_process_tree(child_pid);
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
                let last = crate::error::best_error_line(&stderr_tail);
                Ok(RunOutcome::Failed { reason: format!("(exit {code}) {last}") })
            }
        }
    }
}

/// Video YouTube còn sống thật không? Hỏi oembed — endpoint công khai, nhẹ,
/// KHÔNG bị anti-bot gate như trang watch: 200 = video tồn tại (đang bị
/// soft-block thôi); 4xx = đã xoá/riêng tư thật. Lỗi mạng → coi như không
/// xác nhận được (trả false, giữ nguyên thông báo unavailable).
async fn youtube_video_exists(url: &str) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    match client
        .get("https://www.youtube.com/oembed")
        .query(&[("url", url), ("format", "json")])
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Cache buvid3 theo domain (tv/com) để KHÔNG xin mới mỗi lần tải. Dùng chung
/// một "thiết bị" ổn định trông tự nhiên hơn (đỡ bị Bilibili đánh dấu bot) +
/// giảm số request. Làm mới sau 30 phút.
static BILI_BUVID_CACHE: std::sync::Mutex<Option<(bool, String, u64)>> =
    std::sync::Mutex::new(None);

fn now_secs_u64() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Giết CẢ CÂY tiến trình (yt-dlp + mọi con nó spawn: ffmpeg ghép file,
/// aria2c...). Windows: `taskkill /T` — `child.kill()` thường chỉ giết đúng 1
/// tiến trình, để lại ffmpeg mồ côi giữ khoá file `.part` khiến cleanup xoá
/// hụt và user cũng không xoá tay nổi. Chờ taskkill xong (`status()`, ~50-150ms)
/// để chắc chắn cây chết trước khi bước dọn file chạy.
fn kill_process_tree(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = pid; // Unix: kill() của tauri-shell đủ (SIGKILL nhóm mặc định).
    }
}

/// Quét & giết tiến trình tải "MỒ CÔI" còn sót từ phiên app trước — xảy ra khi
/// app crash / bị kill / Windows tắt đột ngột giữa lúc đang tải: yt-dlp/ffmpeg/
/// aria2c mất cha vẫn chạy vô chủ hàng giờ, vừa chiếm mạng vừa giữ khoá file
/// `.part` khiến user KHÔNG XOÁ NỔI (bug thực tế: 2 yt-dlp mồ côi giữ file
/// trong D:\... đến khi bị kill tay).
///
/// An toàn: chỉ giết đúng 3 TÊN tiến trình (yt-dlp/ffmpeg/aria2c) VÀ đường dẫn
/// chứa "bqhungdown" — không bao giờ đụng chính app hay phần mềm khác của user.
/// Gọi đúng 1 lần lúc khởi động, TRƯỚC khi queue restore spawn tiến trình mới
/// (chạy đồng bộ để không có cửa sổ đua giết nhầm tiến trình mới).
pub fn kill_orphan_downloaders() {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let script = "Get-CimInstance Win32_Process | Where-Object { \
            ($_.Name -eq 'yt-dlp.exe' -or $_.Name -eq 'ffmpeg.exe' -or $_.Name -eq 'aria2c.exe') \
            -and $_.ExecutablePath -like '*bqhungdown*' } \
            | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }";
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
}

/// Lấy cookie `buvid3` từ Bilibili để yt-dlp qua tường lửa 412 khi tải.
/// Đúng domain: bilibili.tv (bản quốc tế) vs bilibili.com. Đi qua proxy nếu
/// có (bilibili.tv chặn theo vùng). Có cache 30 phút. Best-effort.
async fn fetch_bilibili_buvid(url: &str, proxy: &Option<String>) -> Option<String> {
    let is_tv = url.to_lowercase().contains("bilibili.tv");
    // Trả từ cache nếu cùng domain và còn hạn (30 phút).
    if let Ok(g) = BILI_BUVID_CACHE.lock() {
        if let Some((cached_tv, ref val, ts)) = *g {
            if cached_tv == is_tv && now_secs_u64().saturating_sub(ts) < 1800 {
                return Some(val.clone());
            }
        }
    }
    let home = if is_tv {
        "https://www.bilibili.tv/en"
    } else {
        "https://www.bilibili.com/"
    };
    let mut b = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36",
        );
    if let Some(px) = proxy {
        if let Ok(p) = reqwest::Proxy::all(px) {
            b = b.proxy(p);
        }
    }
    let client = b.build().ok()?;
    let resp = client.get(home).send().await.ok()?;
    for v in resp.headers().get_all(reqwest::header::SET_COOKIE).iter() {
        if let Ok(s) = v.to_str() {
            if let Some(rest) = s.strip_prefix("buvid3=") {
                if let Some(val) = rest.split(';').next() {
                    if !val.is_empty() {
                        // Lưu cache để lần tải sau dùng lại cùng buvid.
                        if let Ok(mut g) = BILI_BUVID_CACHE.lock() {
                            *g = Some((is_tv, val.to_string(), now_secs_u64()));
                        }
                        return Some(val.to_string());
                    }
                }
            }
        }
    }
    None
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
