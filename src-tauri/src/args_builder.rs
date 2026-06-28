use crate::models::{DownloadMode, DownloadRequest, Settings};

/// Một số site (viralhog, gfycat, redgifs, imgur, 9gag, các trang
/// embed video tự host…) không có extractor riêng trong yt-dlp nhưng có thẻ
/// `<video>` hoặc og:video trong HTML. Cờ `--force-generic-extractor` bảo
/// yt-dlp dùng generic extractor để scan HTML và tìm URL media trực tiếp.
const FORCE_GENERIC_EXTRACTORS: &[&str] = &[
    "viralhog",
    "9gag",
    "imgur",
    "gfycat",
    "redgifs",
    "coub",
    "tumblr",
    "newgrounds",
    "viralhog",
];

fn should_force_generic(url: &str) -> bool {
    let extractor = match crate::url_validator::resolve_extractor(url) {
        Some(e) => e,
        None => return false,
    };
    FORCE_GENERIC_EXTRACTORS.contains(&extractor)
}

use std::sync::atomic::{AtomicUsize, Ordering};

/// Round-robin cursor over the configured proxy list. Each yt-dlp invocation
/// advances it, so requests spread across proxies; a bot error advances it
/// again on retry → a fresh IP.
static PROXY_CURSOR: AtomicUsize = AtomicUsize::new(0);

/// Pick the next proxy round-robin, or `None` when no proxies are configured.
pub fn next_proxy(settings: &Settings) -> Option<String> {
    if settings.proxies.is_empty() {
        return None;
    }
    let i = PROXY_CURSOR.fetch_add(1, Ordering::Relaxed);
    settings.proxies.get(i % settings.proxies.len()).map(|p| normalize_proxy(p))
}

/// Accept the common proxy formats users paste and turn them into the
/// `scheme://[user:pass@]host:port` form yt-dlp expects:
///   - `http://user:pass@ip:port`      → unchanged (already has a scheme)
///   - `ip:port:user:pass`             → `http://user:pass@ip:port`
///   - `ip:port`                       → `http://ip:port`
///   - `user:pass@ip:port`             → `http://user:pass@ip:port`
pub fn normalize_proxy(raw: &str) -> String {
    let p = raw.trim();
    if p.is_empty() || p.contains("://") {
        return p.to_string();
    }
    if p.contains('@') {
        return format!("http://{p}");
    }
    let parts: Vec<&str> = p.split(':').collect();
    match parts.len() {
        2 => format!("http://{}:{}", parts[0], parts[1]),
        4 => format!("http://{}:{}@{}:{}", parts[2], parts[3], parts[0], parts[1]),
        _ => format!("http://{p}"),
    }
}

/// Push `--proxy <url>` for the next proxy in rotation (no-op if none set).
pub fn push_proxy_args(args: &mut Vec<String>, settings: &Settings) {
    if let Some(p) = next_proxy(settings) {
        args.push("--proxy".into());
        args.push(p);
    }
}

/// Push `--cookies <file>` or `--cookies-from-browser <browser>` based on
/// settings. File takes priority over browser (browser cookies hit AppBound/
/// DPAPI decryption failures on modern Windows Chrome/Edge). Shared by every
/// yt-dlp call site so cookie behaviour is consistent.
pub fn push_cookie_args(args: &mut Vec<String>, settings: &Settings) {
    if let Some(file) = settings.cookies_file.as_deref() {
        if !file.is_empty() {
            args.push("--cookies".into());
            args.push(file.to_string());
            return;
        }
    }
    if let Some(browser) = settings.cookies_browser.as_deref() {
        if !browser.is_empty() {
            args.push("--cookies-from-browser".into());
            args.push(browser.to_string());
        }
    }
}

/// True when any cookie source is configured.
pub fn settings_have_cookies(s: &Settings) -> bool {
    s.cookies_file.as_deref().map(|f| !f.is_empty()).unwrap_or(false)
        || s.cookies_browser.as_deref().map(|b| !b.is_empty()).unwrap_or(false)
}

/// Clone settings with all cookie sources cleared — used to retry a call after
/// a cookie-decryption failure (DPAPI).
pub fn settings_without_cookies(s: &Settings) -> Settings {
    let mut c = s.clone();
    c.cookies_file = None;
    c.cookies_browser = None;
    c
}

/// Mode hint cho fetch_metadata vs run_download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildMode {
    /// `yt-dlp --dump-single-json <url>` cho fetch metadata.
    FetchMetadata,
    /// Run download (video hoặc audio).
    Download {
        resume: bool,
        /// Force `--force-generic-extractor` (used as automatic retry when the
        /// native extractor returned "Unsupported URL").
        force_generic: bool,
        /// Optional pre-resolved output filename (sanitized, with optional
        /// `(N)` suffix when collision detected). When `None`, args_builder
        /// uses the default `%(title)s.%(ext)s` template.
        output_stem: Option<String>,
    },
}

/// Build argument vector cho `yt-dlp` từ một `DownloadRequest`.
/// Pure function: không IO, không hidden state.
pub fn build(req: &DownloadRequest, settings: &Settings, mode: BuildMode) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();

    // Common flags
    args.push("--no-warnings".into());
    args.push("--encoding".into());
    args.push("utf-8".into());
    args.push("--no-mtime".into());
    args.push("--retries".into());
    args.push("0".into()); // queue mgr drives retry
    args.push("--socket-timeout".into());
    args.push("30".into());

    // User-agent để bỏ qua bot detection.
    // Dùng default của yt-dlp (thay đổi theo phiên bản, khó bị block hơn).
    args.push("--user-agent".into());
    args.push("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36".into());

    // Site-specific request headers. Critical for Douyin CDN URLs: when we
    // resolve `https://www.douyin.com/...` to `https://...aweme.snssdk.com/...`
    // (via TikWM / share-page scrape), the CDN throttles requests that arrive
    // without a `Referer: https://www.douyin.com/` header. Send it explicitly
    // so download speed isn't capped at a few hundred KB/s.
    let url_lower = req.url.to_lowercase();
    if url_lower.contains("aweme.snssdk.com")
        || url_lower.contains("douyin.com")
        || url_lower.contains("/playwm/")
        || url_lower.contains("/play/?")
    {
        args.push("--add-header".into());
        args.push("Referer:https://www.douyin.com/".into());
    }

    // Cookies từ browser (Settings → "Lấy cookies từ trình duyệt") — bắt buộc
    // cho Douyin / Bilibili / video YouTube giới hạn tuổi v.v.
    // Ưu tiên file cookies.txt > browser khi cả 2 cùng set, vì AppBound
    // encryption của Edge/Chrome trên Windows làm browser-based fail.
    push_cookie_args(&mut args, settings);
    push_proxy_args(&mut args, settings);

    // Sites without a native yt-dlp extractor but with direct media in HTML
    // (viralhog, 9gag, imgur, redgifs…) → force the generic extractor so it
    // scans the HTML for `<video>` / `og:video` / etc. Caller can also force
    // it on retry via `BuildMode::Download { force_generic: true, .. }`.
    let force_generic_caller = matches!(&mode, BuildMode::Download { force_generic: true, .. });
    if force_generic_caller || should_force_generic(&req.url) {
        args.push("--force-generic-extractor".into());
    }

    // NOTE: We deliberately do NOT pin `--extractor-args player_client=...`.
    // Tested 2026-06 against a fresh yt-dlp: forcing `tv`/`web_safari`/`mweb`
    // returned ZERO formats, while yt-dlp's own `default` client set returned
    // the full ladder up to 2160p/4K. The maintainers tune `default` per
    // release to be the best working combo, so the real anti-bot fix is simply
    // keeping yt-dlp current — see `ytdlp_update`. Pinning a client here makes
    // things worse, not better.

    // NOTE: We previously tried `--cookies-from-browser edge` to bypass YouTube
    // anti-bot, but Edge/Chrome on Windows now use AppBound encryption that yt-dlp
    // can't decrypt (DPAPI error). Skip cookies by default; user can manually
    // export cookies.txt and configure later if needed for restricted videos.

    match mode {
        BuildMode::FetchMetadata => {
            args.push("--dump-single-json".into());
            // For playlists, default behavior: include entries metadata only.
            args.push("--flat-playlist".into());
            // No -o, no -N
            args.push(req.url.clone());
            return args;
        }
        BuildMode::Download { resume, force_generic: _, output_stem } => {
            // Output template & path. When the caller pre-resolved a stem
            // (collision-safe `<title> (N)`), we use that literal stem so
            // yt-dlp writes to the unique filename. Otherwise fall back to
            // the standard `%(title)s` template that yt-dlp expands itself.
            args.push("-o".into());
            let folder = req.save_folder.to_string_lossy();
            if let Some(stem) = output_stem {
                args.push(format!("{folder}/{stem}.%(ext)s"));
            } else {
                args.push(format!("{folder}/%(title)s.%(ext)s"));
            }

            // Progress: rely on yt-dlp's default `[download] x.x% of ...` lines
            // (parsed by progress_parser::parse_fallback). Custom --progress-template
            // is brittle across yt-dlp versions and silently drops output, so we
            // intentionally do NOT use it.
            // CRITICAL: when --print is used, yt-dlp implicitly enables quiet mode
            // and silences progress lines. Force them back with --no-quiet --progress.
            args.push("--newline".into());
            args.push("--no-colors".into());
            args.push("--no-quiet".into());
            args.push("--progress".into());

            // Print resolved title BEFORE download starts so the UI can replace
            // the URL placeholder with a human-readable name.
            args.push("--print".into());
            args.push("before_dl:TITLE|%(title)s".into());

            // Print thumbnail URL and uploader/channel BEFORE download so the
            // UI can show a real preview within ~1-2s of starting (especially
            // important for batch-added items where we don't pre-fetch metadata).
            args.push("--print".into());
            args.push("before_dl:THUMB|%(thumbnail)s".into());
            args.push("--print".into());
            args.push("before_dl:CHANNEL|%(channel,uploader,creator)s".into());

            // Print the FINAL output path AFTER all post-processing finishes.
            // This is the most reliable way to know where the video ended up,
            // independent of whether yt-dlp went through Merger / ExtractAudio /
            // FixupM4a etc. The line looks like: "FINALPATH|C:\path\to\video.mp4"
            args.push("--print".into());
            args.push("after_move:FINALPATH|%(filepath,_filename)s".into());

            // Multi-connection (concurrent fragment downloads).
            // -N 32 + http-chunk-size lớn = nhiều stream + ít overhead per chunk.
            // YouTube CDN cho phép tới ~32 connection per IP, vượt qua sẽ bị
            // throttle. 32 là sweet spot.
            args.push("-N".into());
            args.push("32".into());
            // Bigger HTTP chunk → giảm số request, mỗi request lấy được nhiều
            // hơn → tốc độ ổn định hơn (thay vì lúc nhanh lúc chậm do TCP slow-start).
            args.push("--http-chunk-size".into());
            args.push("10485760".into()); // 10 MiB / chunk

            // Anti-throttle: YouTube sometimes deliberately throttles a download
            // to a crawl (~tens of KB/s). If the rate drops below this, yt-dlp
            // re-extracts fresh URLs and resumes at full speed. Big speed win on
            // throttled videos, no quality change.
            args.push("--throttled-rate".into());
            args.push("100K".into());

            // Polite mode — random sleep between requests so we don't trip
            // YouTube/TikTok rate limiting when batch-downloading a channel.
            // Caller turns this on for "Tải kênh" flows.
            if req.polite {
                args.push("--sleep-interval".into());
                args.push("2".into());
                args.push("--max-sleep-interval".into());
                args.push("5".into());
                args.push("--sleep-requests".into());
                args.push("1".into());
            }

            // Aria2c — true multi-stream accelerator. Khi user bật ở Settings,
            // dùng -x 32 -s 32 split=32 cho tốc độ max.
            if req.use_aria2c {
                args.push("--downloader".into());
                let aria_bin = crate::sidecar_detect::aria2c_path()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "aria2c".to_string());
                args.push(aria_bin);
                args.push("--downloader-args".into());
                args.push(
                    "aria2c:-x 32 -s 32 -k 1M --max-connection-per-server=32 \
--split=32 --min-split-size=1M --piece-length=1M --lowest-speed-limit=1K \
--console-log-level=notice --summary-interval=1 --enable-color=false"
                        .into(),
                );
            }

            // Format selection — luôn ưu tiên format CÓ audio. Cấu trúc fallback:
            // 1. {fmt}+bestaudio  → format video-only + audio tốt nhất (YouTube DASH).
            // 2. {fmt}            → format đã có audio (progressive như itag 18).
            // 3. best/bestvideo+bestaudio → fallback tổng — chống nắm chắc 99%.
            //
            // Đặt --merge-output-format mp4 + ffmpeg sẵn → luôn ra file mp4 phát mọi player.
            match req.mode {
                DownloadMode::Video => {
                    if let Some(fmt) = &req.format_id {
                        // FORMAT FALLBACK đảm bảo CÓ AUDIO 100%:
                        //   1. {fmt}+bestaudio  → DASH video-only + best audio
                        //   2. best             → progressive format có audio (last resort)
                        // Cố ý KHÔNG fallback xuống `{fmt}` đơn lẻ vì format
                        // chất lượng thấp (144p, 240p, 360p...) trên YouTube
                        // thường là video-only — fallback đó sẽ ra file mất tiếng.
                        // `best` ở cuối luôn cho format có audio.
                        args.push("-f".into());
                        args.push(format!("{fmt}+bestaudio/best"));
                    } else {
                        // Best quality available với audio đảm bảo. yt-dlp tự pick.
                        args.push("-f".into());
                        args.push("bv*+ba/b".into());
                        args.push("-S".into());
                        args.push("res,fps,vcodec:h264,acodec:m4a,tbr".into());
                    }
                    args.push("--merge-output-format".into());
                    args.push("mp4".into());
                }
                DownloadMode::Audio => {
                    args.push("-x".into());
                    args.push("--audio-format".into());
                    args.push("mp3".into());
                    args.push("--audio-quality".into());
                    args.push("0".into());
                }
            }

            // Subtitles (Req 11)
            if !req.sub_langs.is_empty() {
                args.push("--write-subs".into());
                args.push("--sub-langs".into());
                args.push(req.sub_langs.join(","));
                args.push("--convert-subs".into());
                args.push("srt".into());
            }
            if let Some(target) = &req.auto_translate_to {
                args.push("--write-auto-subs".into());
                args.push("--sub-langs".into());
                args.push(target.clone());
                args.push("--convert-subs".into());
                args.push("srt".into());
            }

            // Playlist (Req 9)
            if req.playlist_all {
                args.push("--yes-playlist".into());
            } else {
                args.push("--no-playlist".into());
            }

            // Conflict policy
            match req.on_conflict {
                crate::models::ConflictPolicy::Overwrite => {
                    args.push("--force-overwrites".into());
                }
                _ => {
                    args.push("--no-overwrites".into());
                }
            }

            if resume {
                args.push("--continue".into());
            }

            // Final URL
            args.push(req.url.clone());

            // Settings hook (currently no extra flag, but future use)
            let _ = settings;

            args
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ConflictPolicy, DownloadMode, DownloadRequest, Settings};
    use std::path::PathBuf;

    fn req() -> DownloadRequest {
        DownloadRequest {
            url: "https://www.youtube.com/watch?v=abc".into(),
            mode: DownloadMode::Video,
            format_id: None,
            save_folder: PathBuf::from("C:/Users/me/Downloads"),
            sub_langs: vec![],
            auto_translate_to: None,
            on_conflict: ConflictPolicy::Ask,
            use_aria2c: false,
            playlist_all: false,
            polite: false,
        }
    }

    #[test]
    fn fetch_metadata_args() {
        let s = Settings::default();
        let args = build(&req(), &s, BuildMode::FetchMetadata);
        assert!(args.contains(&"--dump-single-json".to_string()));
        assert!(args.contains(&"--flat-playlist".to_string()));
        assert_eq!(
            args.last().unwrap(),
            &"https://www.youtube.com/watch?v=abc".to_string()
        );
    }

    #[test]
    fn video_best_default() {
        let s = Settings::default();
        let args = build(&req(), &s, BuildMode::Download { resume: false, force_generic: false, output_stem: None });
        let joined = args.join(" ");
        assert!(joined.contains("-f bv*+ba/b"));
        assert!(joined.contains("-N 32"));
        assert!(joined.contains("%(title)s.%(ext)s"));
        assert!(!joined.contains("--continue"));
    }

    #[test]
    fn audio_mode_emits_extract_audio() {
        let mut r = req();
        r.mode = DownloadMode::Audio;
        let args = build(&r, &Settings::default(), BuildMode::Download { resume: false, force_generic: false, output_stem: None });
        let joined = args.join(" ");
        assert!(joined.contains("-x --audio-format mp3 --audio-quality 0"));
    }

    #[test]
    fn aria2c_when_enabled() {
        let mut r = req();
        r.use_aria2c = true;
        let args = build(&r, &Settings::default(), BuildMode::Download { resume: false, force_generic: false, output_stem: None });
        let joined = args.join(" ");
        assert!(joined.contains("--downloader aria2c"));
        assert!(joined.contains("aria2c:-x 32 -s 32 -k 1M"));
    }

    #[test]
    fn normalize_proxy_formats() {
        // ip:port:user:pass (the format MuaProxy etc. hand out)
        assert_eq!(
            normalize_proxy("103.45.235.203:37223:sp07v2-37223:FCUHX"),
            "http://sp07v2-37223:FCUHX@103.45.235.203:37223"
        );
        // ip:port
        assert_eq!(normalize_proxy("1.2.3.4:8000"), "http://1.2.3.4:8000");
        // user:pass@ip:port
        assert_eq!(normalize_proxy("u:p@1.2.3.4:8000"), "http://u:p@1.2.3.4:8000");
        // already has scheme → unchanged
        assert_eq!(normalize_proxy("socks5://1.2.3.4:1080"), "socks5://1.2.3.4:1080");
        assert_eq!(
            normalize_proxy("http://u:p@1.2.3.4:8000"),
            "http://u:p@1.2.3.4:8000"
        );
    }

    #[test]
    fn resume_appends_continue() {
        let r = req();
        let args = build(&r, &Settings::default(), BuildMode::Download { resume: true, force_generic: false, output_stem: None });
        assert!(args.contains(&"--continue".to_string()));
    }
}
