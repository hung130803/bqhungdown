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

/// True for any YouTube URL (watch/shorts/live/music/youtu.be).
pub fn is_youtube(url: &str) -> bool {
    let l = url.to_lowercase();
    l.contains("youtube.com") || l.contains("youtu.be")
}

/// True cho mọi URL Bilibili (com/tv/b23).
pub fn is_bilibili(url: &str) -> bool {
    let l = url.to_lowercase();
    l.contains("bilibili.com") || l.contains("bilibili.tv") || l.contains("b23.tv")
}

/// Push `Origin` + `Referer` headers for Bilibili URLs — bắt buộc để tránh
/// HTTP 412 "Precondition Failed" mà Bilibili trả khi request thiếu 2 header
/// này (fix đã kiểm chứng, xem yt-dlp#12013). Chọn đúng domain: bilibili.tv
/// (bản quốc tế / BiliIntl) vs bilibili.com. No-op cho site khác.
pub fn push_bilibili_headers(args: &mut Vec<String>, url: &str) {
    let l = url.to_lowercase();
    if !l.contains("bilibili.com") && !l.contains("bilibili.tv") && !l.contains("b23.tv") {
        return;
    }
    let base = if l.contains("bilibili.tv") {
        "https://www.bilibili.tv"
    } else {
        "https://www.bilibili.com"
    };
    args.push("--add-header".into());
    args.push(format!("Origin:{base}"));
    args.push("--add-header".into());
    args.push(format!("Referer:{base}/"));
}

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
///
/// CRITICAL cho các site bị nhà mạng chặn DNS (vd bilibili.tv ở VN): ép giải
/// tên miền Ở PROXY chứ không ở máy user, nếu không proxy vô dụng vì máy vẫn
/// hỏi DNS nhà mạng (đã bị đầu độc → 127.0.0.1). HTTP proxy vốn đã giải tên ở
/// proxy; riêng `socks5://` mặc định giải tên tại máy → nâng lên `socks5h://`
/// (và `socks4://` → `socks4a://`) để giải tên tại proxy.
pub fn normalize_proxy(raw: &str) -> String {
    let p = raw.trim();
    if p.is_empty() {
        return p.to_string();
    }
    if p.contains("://") {
        return upgrade_socks_remote_dns(p);
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

/// Nâng scheme SOCKS lên biến thể giải-tên-tại-proxy (giữ nguyên phần còn lại).
fn upgrade_socks_remote_dns(p: &str) -> String {
    let lower = p.to_lowercase();
    if let Some(rest) = lower.strip_prefix("socks5://") {
        format!("socks5h://{}", &p[p.len() - rest.len()..])
    } else if let Some(rest) = lower.strip_prefix("socks4://") {
        format!("socks4a://{}", &p[p.len() - rest.len()..])
    } else {
        p.to_string()
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
        /// Retry sau lỗi 403 / "format not available": kéo format từ thêm
        /// player client (tv/mweb/web_safari) + networking dè dặt (ít
        /// connection, không aria2c) — googlevideo 403 hàng loạt khi thấy
        /// pattern tải song song hung hãn mà không có PO token hợp lệ.
        safe_retry: bool,
    },
}

/// NGÂN SÁCH KẾT NỐI tổng cho cả máy (mọi video đang tải cộng lại). YouTube
/// bóp băng thông theo IP khi thấy quá nhiều kết nối song song, nên tổng phải
/// giữ ở mức khoẻ thay vì nhân lên theo số luồng.
const CONN_BUDGET: u32 = 48;

/// Số KẾT NỐI cho MỖI video, tự co giãn theo số video tải song song:
/// `budget / số_luồng`, kẹp trong [MIN, max_per_item].
///
/// LÝ DO (lỗi thật đã gặp): trước đây cố định `-N 32`; user đặt 50 luồng →
/// 50 × 32 = 1.600 kết nối cùng lúc → YouTube bóp IP → MỌI video rùa bò.
/// Nay: 1 luồng = 32 kết nối (nhanh tối đa cho 1 video), 3 luồng = 16,
/// 10 luồng = 4, 50 luồng = 2 → tổng luôn ~48, không tự bắn vào chân.
///
/// Hàm THUẦN để unit-test.
pub(crate) fn conns_per_item(concurrency: u8, max_per_item: u32) -> u32 {
    const MIN_PER_ITEM: u32 = 2;
    let conc = (concurrency as u32).max(1);
    (CONN_BUDGET / conc).clamp(MIN_PER_ITEM, max_per_item)
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

    // IPv4 only. googlevideo URL bị khoá theo IP lúc extract; máy dual-stack
    // có thể extract qua IPv6 rồi tải qua IPv4 (happy-eyeballs) → 403 giữa
    // chừng. Ép -4 cho cả extract + download đi cùng một đường.
    args.push("-4".into());

    let yt = is_youtube(&req.url);

    // User-agent: CHỈ set cho site ngoài YouTube. Với YouTube, yt-dlp tự gửi
    // UA khớp với từng player client (web/tv/mweb…) — ép một UA Chrome cứng
    // làm lệch fingerprint (UA nói Chrome X, innertube context nói khác) và là
    // một nguyên nhân 403/bot-check kinh điển. UA cứng cũng lỗi thời dần theo
    // thời gian, càng dễ bị đánh dấu bot.
    if !yt {
        args.push("--user-agent".into());
        args.push("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36".into());
    }

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

    // Bilibili: Origin + Referer để tránh HTTP 412.
    push_bilibili_headers(&mut args, &req.url);

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
        BuildMode::Download { resume, force_generic: _, output_stem, safe_retry } => {
            // Retry sau 403/format-error trên YouTube: default client bị SABR
            // giấu URL hoặc googlevideo từ chối URL đã extract → kéo format từ
            // các client còn phục vụ URL tải trực tiếp.
            if safe_retry && yt {
                args.push("--extractor-args".into());
                args.push("youtube:player_client=default,tv,mweb,web_safari".into());
            }
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
            // Riêng lần retry sau 403: hạ xuống -N 4, bỏ chunk-size — pattern
            // tải song song dồn dập trên URL thiếu PO token là chính thứ làm
            // googlevideo trả 403 tiếp.
            if safe_retry {
                args.push("-N".into());
                args.push("4".into());
            } else {
                // CO GIÃN theo số luồng (xem conns_per_item): 1 luồng = 32,
                // nhiều luồng = ít kết nối/video để TỔNG không vượt ngân sách
                // → không bị YouTube bóp IP khi chạy hàng loạt kênh.
                let n = conns_per_item(settings.max_concurrency, 32);
                args.push("-N".into());
                args.push(n.to_string());
                // Bigger HTTP chunk → giảm số request, mỗi request lấy được nhiều
                // hơn → tốc độ ổn định hơn (thay vì lúc nhanh lúc chậm do TCP slow-start).
                args.push("--http-chunk-size".into());
                args.push("10485760".into()); // 10 MiB / chunk
            }

            // NOTE: removed `--throttled-rate` — on a rate-limited IP it makes
            // yt-dlp re-extract URLs in a loop (download stuck at 0 B) instead
            // of just downloading slowly. Net effect was slower, not faster.

            // Polite mode — a light random sleep BETWEEN videos so big channel
            // batches don't trip rate limits too fast. Kept light (1-2s) for
            // speed: the heavy old values (2-5s + 1s per request) made many
            // small Shorts crawl. The rate-limit auto-retry is the safety net.
            if req.polite {
                args.push("--sleep-interval".into());
                args.push("1".into());
                args.push("--max-sleep-interval".into());
                args.push("2".into());
            }

            // Aria2c — vũ khí tốc độ chính cho YouTube: tải https thường của
            // yt-dlp là MỘT luồng duy nhất (-N chỉ tác dụng với video chia
            // mảnh), nên khi YouTube bóp từng kết nối (~2MB/s lúc IP nóng)
            // là chịu chết. aria2c xé file thành 16 khúc / 16 kết nối song
            // song — đo thực tế 2026-07: ~30MB/s vs ~9MB/s một luồng.
            //
            // LƯU Ý SỐNG CÒN: aria2c chỉ cho phép TỐI ĐA -x 16 kết nối/server.
            // Trước đây truyền -x 32 → aria2c chết ngay exit 28 ("Possible
            // Values: 1-16") → tính năng này CHƯA TỪNG chạy được.
            //
            // Bỏ qua ở lần safe_retry: aria2c gửi UA/header riêng và không
            // refresh được URL hết hạn — nguồn 403 nổi tiếng trên googlevideo.
            if req.use_aria2c && !safe_retry {
                args.push("--downloader".into());
                let aria_bin = crate::sidecar_detect::aria2c_path()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "aria2c".to_string());
                args.push(aria_bin);
                args.push("--downloader-args".into());
                // -x/-s CO GIÃN theo số luồng (aria2c cho tối đa 16/server):
                // 1 luồng = 16 kết nối (nhanh nhất cho 1 video), nhiều luồng =
                // ít hơn để tổng không vượt ngân sách → tránh bị bóp IP.
                let ax = conns_per_item(settings.max_concurrency, 16);
                args.push(format!(
                    "aria2c:-x {ax} -s {ax} -k 1M --min-split-size=1M \
--lowest-speed-limit=1K --console-log-level=notice --summary-interval=1 \
--enable-color=false"
                ));
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
                        // Best quality available với audio đảm bảo, GIỚI HẠN theo
                        // `max_height` (mặc định 1080). 4K to gấp ~5.5 lần 1080p
                        // nên vớ 4K mặc định làm tải hàng loạt chậm hẳn. Fallback
                        // cuối `bv*+ba/b` không kèm điều kiện → nếu kênh không có
                        // mức <=cap vẫn tải được (không bao giờ fail vì cap).
                        // Kênh theo dõi đặt mức riêng thì ưu tiên; không thì mức chung.
                        let mh = req.max_height.unwrap_or(settings.max_height);
                        args.push("-f".into());
                        if mh > 0 {
                            args.push(format!(
                                "bv*[height<={mh}]+ba/b[height<={mh}]/bv*+ba/b"
                            ));
                        } else {
                            args.push("bv*+ba/b".into());
                        }
                    }
                    // Sort thứ tự chọn stream — áp cho CẢ 2 nhánh trên (nhánh
                    // format_id vẫn cần nó để chọn ĐÚNG track audio đem ghép):
                    //   lang      → ƯU TIÊN TIẾNG GỐC của video. YouTube giờ tự
                    //               lồng tiếng AI sang nhiều thứ tiếng; thiếu
                    //               'lang' thì bestaudio so bitrate và hay vớ
                    //               phải bản lồng tiếng nước ngoài.
                    //   quality   → track YouTube tự đánh giá tốt hơn khi cùng tiếng.
                    //   acodec:m4a→ âm thanh AAC để ghép vào mp4 phát được mọi
                    //               máy. Opus nhét trong mp4 bị CÂM trên nhiều
                    //               player Windows/TV — chính là lỗi "video mất
                    //               tiếng" dù file có audio thật.
                    // KHÔNG ép `proto` (DASH) nữa: ép DASH biến video thành 1
                    // file liền → `-N 32` không xé song song được → CHẬM. Để
                    // yt-dlp tự chọn; video HLS (nhiều mảnh) + `-N 32` tải 32
                    // mảnh song song = nhanh hơn nhiều (user xác nhận).
                    args.push("-S".into());
                    args.push("lang,quality,res,fps,vcodec:h264,acodec:m4a,tbr".into());
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

    #[test]
    fn ket_noi_co_gian_theo_so_luong_giu_tong_khoe() {
        // 1 luồng -> tối đa (nhanh nhất cho 1 video)
        assert_eq!(conns_per_item(1, 32), 32);
        assert_eq!(conns_per_item(1, 16), 16);   // aria2c cap 16
        // 3 luồng -> 48/3 = 16
        assert_eq!(conns_per_item(3, 32), 16);
        // 6 luồng -> 8
        assert_eq!(conns_per_item(6, 32), 8);
        // 10 luồng -> 4
        assert_eq!(conns_per_item(10, 32), 4);
        // 50 luồng (ca của user) -> sàn 2, KHÔNG còn 32 => tổng 100 thay vì 1600
        assert_eq!(conns_per_item(50, 32), 2);
        // 0 (dữ liệu lỗi) coi như 1 luồng, không chia cho 0
        assert_eq!(conns_per_item(0, 32), 32);
        // TỔNG kết nối luôn trong ngân sách hợp lý ở mọi mức luồng thường dùng
        for c in 1u8..=10 {
            let total = conns_per_item(c, 32) * c as u32;
            assert!(total <= CONN_BUDGET, "luồng {c} -> tổng {total} vượt ngân sách");
        }
    }

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
            force_redownload: false,
            max_height: None,
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
        // Mặc định max_height = 1080 → format bị giới hạn <=1080, có fallback.
        let s = Settings::default();
        let args = build(&req(), &s, BuildMode::Download { resume: false, force_generic: false, output_stem: None, safe_retry: false });
        let joined = args.join(" ");
        assert!(joined.contains("bv*[height<=1080]+ba/b[height<=1080]/bv*+ba/b"));
        // -N CO GIÃN theo số luồng: mặc định max_concurrency=3 -> 48/3 = 16.
        // (Trước đây cố định 32 -> nhiều luồng là bão kết nối, bị bóp IP.)
        assert_eq!(s.max_concurrency, 3, "mặc định luồng đổi thì sửa kỳ vọng -N");
        assert!(joined.contains("-N 16"), "args: {joined}");
        assert!(joined.contains("%(title)s.%(ext)s"));
        assert!(!joined.contains("--continue"));
    }

    #[test]
    fn max_height_zero_means_unlimited() {
        // 0 = không giới hạn → dùng bv*+ba/b như cũ (vớ 4K nếu có).
        let mut s = Settings::default();
        s.max_height = 0;
        let args = build(&req(), &s, BuildMode::Download { resume: false, force_generic: false, output_stem: None, safe_retry: false });
        let joined = args.join(" ");
        assert!(joined.contains("-f bv*+ba/b"));
        assert!(!joined.contains("height<="));
    }

    #[test]
    fn max_height_1440_caps_format() {
        let mut s = Settings::default();
        s.max_height = 1440;
        let args = build(&req(), &s, BuildMode::Download { resume: false, force_generic: false, output_stem: None, safe_retry: false });
        let joined = args.join(" ");
        assert!(joined.contains("bv*[height<=1440]+ba/b[height<=1440]/bv*+ba/b"));
    }

    #[test]
    fn audio_mode_emits_extract_audio() {
        let mut r = req();
        r.mode = DownloadMode::Audio;
        let args = build(&r, &Settings::default(), BuildMode::Download { resume: false, force_generic: false, output_stem: None, safe_retry: false });
        let joined = args.join(" ");
        assert!(joined.contains("-x --audio-format mp3 --audio-quality 0"));
    }

    #[test]
    fn aria2c_when_enabled() {
        let mut r = req();
        r.use_aria2c = true;
        let args = build(&r, &Settings::default(), BuildMode::Download { resume: false, force_generic: false, output_stem: None, safe_retry: false });
        let joined = args.join(" ");
        assert!(joined.contains("--downloader aria2c"));
        // aria2c chỉ nhận -x tối đa 16 — truyền 32 là chết ngay exit 28.
        assert!(joined.contains("aria2c:-x 16 -s 16 -k 1M"));
        assert!(!joined.contains("-x 32"));
    }

    #[test]
    fn video_sort_prefers_original_lang_and_m4a_in_both_branches() {
        const SORT: &str = "lang,quality,res,fps,vcodec:h264,acodec:m4a,tbr";
        // Nhánh mặc định ("Tốt nhất")
        let args = build(
            &req(),
            &Settings::default(),
            BuildMode::Download { resume: false, force_generic: false, output_stem: None, safe_retry: false },
        );
        let j = args.join(" ");
        assert!(j.contains(&format!("-S {SORT}")), "nhánh mặc định thiếu sort: {j}");
        // Nhánh user tự chọn chất lượng — TRƯỚC ĐÂY thiếu -S nên bestaudio vớ
        // bản lồng tiếng nước ngoài / opus-câm-trong-mp4.
        let mut r = req();
        r.format_id = Some("137".into());
        let args = build(
            &r,
            &Settings::default(),
            BuildMode::Download { resume: false, force_generic: false, output_stem: None, safe_retry: false },
        );
        let j = args.join(" ");
        assert!(j.contains("137+bestaudio/best"));
        assert!(j.contains(&format!("-S {SORT}")), "nhánh format_id thiếu sort: {j}");
    }

    #[test]
    fn safe_retry_uses_fallback_clients_and_conservative_networking() {
        let mut r = req();
        r.use_aria2c = true;
        let args = build(
            &r,
            &Settings::default(),
            BuildMode::Download { resume: false, force_generic: false, output_stem: None, safe_retry: true },
        );
        let joined = args.join(" ");
        assert!(joined.contains("youtube:player_client=default,tv,mweb,web_safari"));
        assert!(joined.contains("-N 4"));
        assert!(!joined.contains("--http-chunk-size"));
        assert!(!joined.contains("--downloader")); // aria2c bị bỏ qua khi safe_retry
    }

    #[test]
    fn youtube_never_gets_hardcoded_user_agent() {
        let args = build(
            &req(),
            &Settings::default(),
            BuildMode::Download { resume: false, force_generic: false, output_stem: None, safe_retry: false },
        );
        assert!(!args.contains(&"--user-agent".to_string()));
        assert!(args.contains(&"-4".to_string()));
    }

    #[test]
    fn non_youtube_keeps_user_agent() {
        let mut r = req();
        r.url = "https://www.bilibili.com/video/BV1xx411c7mD".into();
        let args = build(
            &r,
            &Settings::default(),
            BuildMode::Download { resume: false, force_generic: false, output_stem: None, safe_retry: false },
        );
        assert!(args.contains(&"--user-agent".to_string()));
    }

    #[test]
    fn bilibili_headers_pick_right_domain() {
        let mut a = vec![];
        push_bilibili_headers(&mut a, "https://www.bilibili.com/video/BV1xx");
        let j = a.join(" ");
        assert!(j.contains("Origin:https://www.bilibili.com"));
        assert!(j.contains("Referer:https://www.bilibili.com/"));

        let mut b = vec![];
        push_bilibili_headers(&mut b, "https://www.bilibili.tv/en/play/12345");
        let j = b.join(" ");
        assert!(j.contains("Origin:https://www.bilibili.tv"), "tv phải dùng domain .tv: {j}");

        // Site khác → không thêm gì.
        let mut c = vec![];
        push_bilibili_headers(&mut c, "https://www.youtube.com/watch?v=x");
        assert!(c.is_empty());
    }

    #[test]
    fn bilibili_download_args_include_headers() {
        let mut r = req();
        r.url = "https://www.bilibili.com/video/BV1C8411i7we".into();
        let args = build(&r, &Settings::default(), BuildMode::Download { resume: false, force_generic: false, output_stem: None, safe_retry: false });
        let j = args.join(" ");
        assert!(j.contains("Origin:https://www.bilibili.com"));
        assert!(j.contains("Referer:https://www.bilibili.com/"));
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
        // socks5 → socks5h (giải tên tại proxy, vượt chặn DNS nhà mạng)
        assert_eq!(normalize_proxy("socks5://1.2.3.4:1080"), "socks5h://1.2.3.4:1080");
        assert_eq!(
            normalize_proxy("socks5://user:pass@1.2.3.4:1080"),
            "socks5h://user:pass@1.2.3.4:1080"
        );
        // socks5h giữ nguyên; http giữ nguyên
        assert_eq!(normalize_proxy("socks5h://1.2.3.4:1080"), "socks5h://1.2.3.4:1080");
        assert_eq!(normalize_proxy("http://1.2.3.4:8000"), "http://1.2.3.4:8000");
        assert_eq!(
            normalize_proxy("http://u:p@1.2.3.4:8000"),
            "http://u:p@1.2.3.4:8000"
        );
    }

    #[test]
    fn resume_appends_continue() {
        let r = req();
        let args = build(&r, &Settings::default(), BuildMode::Download { resume: true, force_generic: false, output_stem: None, safe_retry: false });
        assert!(args.contains(&"--continue".to_string()));
    }
}
