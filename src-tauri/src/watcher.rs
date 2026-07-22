//! Auto-watch monitor for the "Theo dõi kênh" feature.
//!
//! A background task periodically re-fetches each enabled [`WatchedChannel`],
//! finds videos whose id isn't in the channel's `seen_ids`, and enqueues them.
//!
//! Baseline rule: a channel added with an empty `seen_ids` is treated as a
//! *baseline* on its first check — we record the current videos as "seen" and
//! enqueue NOTHING, so watching never dumps the whole backlog. Subsequent
//! checks enqueue only genuinely new uploads.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tauri::{AppHandle, Emitter};

use crate::history_store::HistoryStore;
use crate::models::{
    ChannelVideo, ConflictPolicy, DetectedVideo, DownloadMode, DownloadOptions, PickedVideo,
    WatchedChannel,
};
use crate::queue::QueueManager;
use crate::settings_store::SettingsStore;
use crate::watchlist_store::WatchlistStore;

/// How many recent videos to inspect per check. Channels rarely post more than
/// this between checks, and it bounds the baseline `seen_ids`.
const CHECK_LIMIT: u32 = 50;
/// Wait before the first sweep so app startup isn't slammed.
const STARTUP_DELAY: Duration = Duration::from_secs(60);

fn video_id_of(url: &str) -> String {
    crate::channel_fetcher::extract_video_id(url).unwrap_or_else(|| url.to_string())
}

/// A fetched video plus its dedup id and publish time (for "đăng X phút trước").
struct Fetched {
    id: String,
    video: ChannelVideo,
    published: Option<String>,
}

/// Re-fetch one watched channel and enqueue any new videos. Returns the number
/// of videos enqueued (0 on baseline / error / nothing new). Always updates the
/// channel's `last_checked` / `last_new_count` / `last_error`.
///
/// Fast path: for an already-baselined YouTube channel whose id we know, we hit
/// the lightweight RSS feed (no bot wall, ~instant) instead of a heavy yt-dlp
/// scrape — so the monitor can run every 1-2 minutes safely. Baseline, non-
/// YouTube channels, or an RSS miss fall back to the yt-dlp path.
pub async fn check_channel(
    app: &AppHandle,
    store: &Arc<WatchlistStore>,
    queue: &Arc<QueueManager>,
    settings_store: &Arc<SettingsStore>,
    history: &Arc<HistoryStore>,
    id: &str,
) -> u32 {
    let channel = match store.get(id) {
        Some(c) => c,
        None => return 0,
    };
    let is_baseline = channel.seen_ids.is_empty();

    // Fast incremental check via RSS (YouTube only, after baseline).
    if !is_baseline {
        if let Some(cid) = channel.channel_id.clone() {
            if let Ok(fetched) = fetch_rss_videos(&cid).await {
                if !fetched.is_empty() {
                    return apply(app, store, queue, settings_store, history, &channel, fetched, None, None, id).await;
                }
            }
        }
    }

    // yt-dlp path: baseline, non-YouTube, or RSS unavailable.
    let settings = settings_store.get();
    let tab = if channel.tab.is_empty() { "all".to_string() } else { channel.tab.clone() };
    match crate::channel_fetcher::fetch_channel(app, &channel.url, CHECK_LIMIT, false, &tab, &settings, false).await {
        Ok((info, videos)) => {
            let fetched: Vec<Fetched> = videos
                .into_iter()
                .map(|v| Fetched {
                    id: video_id_of(&v.url),
                    published: v.upload_date.clone(),
                    video: v,
                })
                .collect();
            let title = if info.title.is_empty() { None } else { Some(info.title) };
            apply(app, store, queue, settings_store, history, &channel, fetched, info.channel_id, title, id).await
        }
        Err(e) => {
            let msg = format!("{e}");
            let _ = store.update(id, |c| {
                c.last_checked = Some(Utc::now());
                c.last_error = Some(msg.clone());
            });
            0
        }
    }
}

/// Diff `fetched` against the channel's `seen_ids`, enqueue new videos (none on
/// baseline), and persist the updated channel. Returns count enqueued.
#[allow(clippy::too_many_arguments)]
async fn apply(
    app: &AppHandle,
    store: &Arc<WatchlistStore>,
    queue: &Arc<QueueManager>,
    settings_store: &Arc<SettingsStore>,
    history: &Arc<HistoryStore>,
    channel: &crate::models::WatchedChannel,
    fetched: Vec<Fetched>,
    channel_id: Option<String>,
    title: Option<String>,
    id: &str,
) -> u32 {
    let is_baseline = channel.seen_ids.is_empty();
    let seen: std::collections::HashSet<&String> = channel.seen_ids.iter().collect();
    // New = fetched videos not seen before (none on baseline).
    let new_fetched: Vec<&Fetched> = if is_baseline {
        Vec::new()
    } else {
        fetched.iter().filter(|f| !seen.contains(&f.id)).collect()
    };
    let new_count = new_fetched.len() as u32;
    let settings = settings_store.get();
    let auto = channel.auto_download;

    // Hạn mức tự tải trong ngày (video mới + hàng chờ đã tích). Giữ số đếm
    // theo ngày local để restart app không reset hạn mức.
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut drip_count = if channel.drip_date.as_deref() == Some(today.as_str()) {
        channel.drip_count
    } else {
        0
    };

    if new_count > 0 {
        let chan_name = channel.title.clone().unwrap_or_else(|| channel.url.clone());
        let first = new_fetched.first().map(|f| f.video.title.clone()).unwrap_or_default();
        crate::notification::notify_new_videos(app, &settings, &chan_name, new_count, &first, auto);

        if auto {
            let vids: Vec<ChannelVideo> = new_fetched.iter().map(|f| f.video.clone()).collect();
            let got = enqueue_new(app, queue, settings_store, history, &channel.title,
                                  &channel.dest_dir, &vids, &settings).await;
            drip_count += got;
        }
    }

    // Hàng chờ làm: nếu hôm nay còn suất, rót tiếp từ danh sách đã tích chọn
    // (video mới đăng vừa enqueue ở trên đã chiếm suất trước). Không rót ở
    // lượt baseline — kênh vừa thêm, đợi vòng quét kế cho ổn định.
    let mut dripped: Vec<PickedVideo> = if is_baseline {
        Vec::new()
    } else {
        plan_drip(channel, drip_count)
    };
    if !dripped.is_empty() {
        let vids: Vec<ChannelVideo> = dripped.iter().map(picked_to_channel_video).collect();
        let got = enqueue_new(app, queue, settings_store, history, &channel.title,
                              &channel.dest_dir, &vids, &settings).await;
        dripped.truncate(got as usize);
        drip_count += got;
    }
    let new_done: Vec<String> = new_fetched
        .iter()
        .filter(|_| auto)
        .map(|f| f.id.clone())
        .chain(dripped.iter().map(|p| p.id.clone()))
        .collect();

    let now = Utc::now();
    // In "notify only" mode, remember the new videos so the UI can list them
    // with a manual download button. Auto mode pushes them to the queue instead.
    let detections: Vec<DetectedVideo> = if !auto {
        new_fetched
            .iter()
            .map(|f| DetectedVideo {
                id: f.id.clone(),
                url: f.video.url.clone(),
                title: f.video.title.clone(),
                thumbnail: f.video.thumbnail.clone(),
                published: f.published.clone(),
                detected_at: now,
            })
            .collect()
    } else {
        Vec::new()
    };

    let _ = store.update(id, |c| {
        for f in &fetched {
            if !c.seen_ids.contains(&f.id) {
                c.seen_ids.push(f.id.clone());
            }
        }
        c.last_checked = Some(now);
        c.last_new_count = Some(new_count);
        c.last_error = None;
        if c.title.is_none() {
            c.title = title.clone();
        }
        if c.channel_id.is_none() {
            c.channel_id = channel_id.clone();
        }
        // Newest detections first; cap to keep the list bounded.
        for d in detections.iter().rev() {
            c.pending.insert(0, d.clone());
        }
        if c.pending.len() > 200 {
            c.pending.truncate(200);
        }
        // Chốt hạn mức ngày + rút video đã rót khỏi hàng chờ, ghi "đã làm".
        c.drip_date = Some(today.clone());
        c.drip_count = drip_count;
        for p in &dripped {
            c.picked.retain(|x| x.id != p.id);
            if !c.seen_ids.contains(&p.id) {
                c.seen_ids.push(p.id.clone());
            }
        }
        for did in &new_done {
            if !c.done_ids.contains(did) {
                c.done_ids.push(did.clone());
            }
        }
    });

    if new_count > 0 || !dripped.is_empty() {
        let _ = app.emit(
            crate::events::EV_WATCH_UPDATED,
            crate::events::WatchUpdatedPayload { channel_id: id.to_string(), new_count },
        );
    }
    new_count
}

/// Hàng chờ làm — chọn video sẽ rót hôm nay: lấy từ ĐẦU hàng `picked` (đúng
/// thứ tự user tích) tối đa `daily_limit - đã_tải_hôm_nay` video, bỏ video
/// đã nằm trong `done_ids` (phòng tích trùng). Hàm thuần để unit-test.
fn plan_drip(channel: &WatchedChannel, drip_count: u32) -> Vec<PickedVideo> {
    let limit = channel.daily_limit.clamp(1, 3);
    if drip_count >= limit || channel.picked.is_empty() {
        return Vec::new();
    }
    let slots = (limit - drip_count) as usize;
    channel
        .picked
        .iter()
        .filter(|p| !channel.done_ids.contains(&p.id))
        .take(slots)
        .cloned()
        .collect()
}

fn picked_to_channel_video(p: &PickedVideo) -> ChannelVideo {
    ChannelVideo {
        url: p.url.clone(),
        title: p.title.clone(),
        duration_sec: None,
        view_count: p.view_count,
        upload_date: None,
        thumbnail: p.thumbnail.clone(),
        is_photo: false,
        is_short: false,
        hashtags: Vec::new(),
    }
}

/// Fetch a YouTube channel's RSS feed (newest ~15 uploads) and parse out the
/// video id / title / thumbnail. Lightweight and not bot-gated.
async fn fetch_rss_videos(channel_id: &str) -> Result<Vec<Fetched>, String> {
    let url = format!("https://www.youtube.com/feeds/videos.xml?channel_id={channel_id}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let body = client
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    Ok(parse_rss(&body))
}

fn parse_rss(xml: &str) -> Vec<Fetched> {
    let mut out = Vec::new();
    for entry in xml.split("<entry>").skip(1) {
        let vid = match between(entry, "<yt:videoId>", "</yt:videoId>") {
            Some(v) if !v.is_empty() => v,
            _ => continue,
        };
        let title = between(entry, "<title>", "</title>").map(|t| unescape_xml(&t)).unwrap_or_default();
        let thumbnail = between(entry, "<media:thumbnail url=\"", "\"");
        let published = between(entry, "<published>", "</published>");
        out.push(Fetched {
            id: vid.clone(),
            published,
            video: ChannelVideo {
                url: format!("https://www.youtube.com/watch?v={vid}"),
                title,
                duration_sec: None,
                view_count: None,
                upload_date: None,
                thumbnail,
                is_short: false,
                is_photo: false,
                hashtags: Vec::new(),
            },
        });
    }
    out
}

fn between(s: &str, start: &str, end: &str) -> Option<String> {
    let i = s.find(start)? + start.len();
    let rest = &s[i..];
    let j = rest.find(end)?;
    Some(rest[..j].to_string())
}

fn unescape_xml(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

/// Build queue items for the new videos and enqueue them. Background downloads
/// use polite mode (sleep between requests) + auto-rename so they never block
/// on a conflict prompt. Returns how many were enqueued.
async fn enqueue_new(
    app: &AppHandle,
    queue: &Arc<QueueManager>,
    _settings_store: &Arc<SettingsStore>,
    history: &Arc<HistoryStore>,
    channel_title: &Option<String>,
    dest_dir: &Option<String>,
    videos: &[ChannelVideo],
    settings: &crate::models::Settings,
) -> u32 {
    if videos.is_empty() {
        return 0;
    }
    let _ = app; // reserved for future per-item resolution
    // Thư mục RIÊNG của kênh theo dõi (dây chuyền: mỗi kênh 1 thư mục —
    // INTEGRATION.md); trống -> thư mục tải mặc định chung như cũ.
    let folder: std::path::PathBuf = dest_dir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| settings.default_folder.clone());

    let mut taken = history.known_short_ids().unwrap_or_default();
    for it in queue.list() {
        taken.insert(it.short_id);
    }

    let mut count = 0u32;
    for v in videos {
        let options = DownloadOptions {
            mode: DownloadMode::Video,
            format_id: None,
            save_folder: folder.clone(),
            sub_langs: vec![],
            auto_translate_to: None,
            on_conflict: ConflictPolicy::Rename,
            playlist_all: None,
            polite: Some(true),
        };
        let req = crate::commands::build_request(v.url.clone(), options, settings);
        let extractor = crate::url_validator::resolve_extractor(&v.url).map(|s| s.to_string());
        let item = crate::commands::make_item(
            req,
            Some(v.title.clone()),
            v.thumbnail.clone(),
            extractor,
            channel_title.clone(),
            &taken,
        );
        taken.insert(item.short_id.clone());
        if queue.enqueue(item).is_ok() {
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chan(picked: Vec<PickedVideo>, daily: u32, done: Vec<String>) -> WatchedChannel {
        WatchedChannel {
            id: "t".into(),
            url: "https://youtube.com/@t".into(),
            title: None,
            enabled: true,
            tab: "all".into(),
            added_at: Utc::now(),
            last_checked: None,
            last_new_count: None,
            last_error: None,
            channel_id: None,
            auto_download: true,
            pending: vec![],
            seen_ids: vec!["base1".into()],
            dest_dir: Some("D:\\TrungChuyen\\K".into()),
            picked,
            daily_limit: daily,
            drip_date: None,
            drip_count: 0,
            done_ids: done,
        }
    }

    fn pv(id: &str) -> PickedVideo {
        PickedVideo {
            id: id.into(),
            url: format!("https://youtube.com/watch?v={id}"),
            title: id.into(),
            view_count: Some(1000),
            thumbnail: None,
        }
    }

    #[test]
    fn drip_lay_dung_han_muc_theo_thu_tu() {
        let c = chan(vec![pv("a"), pv("b"), pv("c")], 2, vec![]);
        let got = plan_drip(&c, 0);
        assert_eq!(got.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn drip_video_moi_da_chiem_suat() {
        let c = chan(vec![pv("a"), pv("b")], 2, vec![]);
        // 1 video mới đăng đã tải hôm nay -> chỉ còn 1 suất cho hàng chờ.
        assert_eq!(plan_drip(&c, 1).len(), 1);
        // đủ hạn mức -> không rót.
        assert!(plan_drip(&c, 2).is_empty());
    }

    #[test]
    fn drip_bo_video_da_lam_va_han_muc_kep() {
        let c = chan(vec![pv("done1"), pv("x")], 1, vec!["done1".into()]);
        let got = plan_drip(&c, 0);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "x");
        // daily_limit ngoài khoảng bị kẹp về 1..=3.
        let c9 = chan((0..9).map(|i| pv(&format!("v{i}"))).collect(), 9, vec![]);
        assert_eq!(plan_drip(&c9, 0).len(), 3);
    }

    #[test]
    fn drip_hang_rong_khong_rot() {
        let c = chan(vec![], 2, vec![]);
        assert!(plan_drip(&c, 0).is_empty());
    }

    /// File watchlist.json ĐỜI CŨ (trước khi có picked/daily_limit/...) phải
    /// đọc được nguyên vẹn với giá trị mặc định — không được vỡ dữ liệu user.
    #[test]
    fn doc_file_watchlist_cu_khong_vo() {
        let old = r#"{
            "id":"abc","url":"https://youtube.com/@k","title":"K",
            "enabled":true,"tab":"all","addedAt":"2026-07-01T00:00:00Z",
            "lastChecked":null,"lastNewCount":null,"lastError":null,
            "channelId":"UCx","autoDownload":true,"pending":[],
            "seenIds":["v1","v2"],"destDir":"D:\\T\\K"
        }"#;
        let c: WatchedChannel = serde_json::from_str(old).expect("file cũ phải đọc được");
        assert!(c.picked.is_empty());
        assert_eq!(c.daily_limit, 1);
        assert_eq!(c.drip_count, 0);
        assert!(c.drip_date.is_none());
        assert!(c.done_ids.is_empty());
        assert_eq!(c.seen_ids.len(), 2);
    }
}

/// Check every enabled channel once, sequentially. Returns the refreshed list.
pub async fn check_all(
    app: &AppHandle,
    store: &Arc<WatchlistStore>,
    queue: &Arc<QueueManager>,
    settings_store: &Arc<SettingsStore>,
    history: &Arc<HistoryStore>,
) -> Vec<crate::models::WatchedChannel> {
    for c in store.list() {
        if c.enabled {
            check_channel(app, store, queue, settings_store, history, &c.id).await;
        }
    }
    store.list()
}

/// Spawn the background monitor loop. Reads the interval fresh each round so a
/// settings change takes effect on the next sweep. Never blocks startup.
pub fn spawn_monitor(
    app: AppHandle,
    store: Arc<WatchlistStore>,
    queue: Arc<QueueManager>,
    settings_store: Arc<SettingsStore>,
    history: Arc<HistoryStore>,
) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        loop {
            let _ = check_all(&app, &store, &queue, &settings_store, &history).await;
            let interval_min = settings_store.get().watch_interval_min.clamp(1, 1440);
            tokio::time::sleep(Duration::from_secs(interval_min as u64 * 60)).await;
        }
    });
}
