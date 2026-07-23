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

/// Đối soát `dl_pending` (video đang tải qua dây chuyền) của MỌI kênh với
/// thực tế — gọi trước mỗi lần quét và khi UI reload:
/// - Có trong history Completed  → tải XONG → chuyển sang `done_ids`.
/// - Còn trong hàng đợi (kể cả đang chờ/retry) → giữ nguyên, chờ tiếp.
/// - Không còn ở đâu (đã hủy / lỗi cứng / biến mất) → TRẢ SUẤT: gỡ khỏi
///   `dl_pending` + `seen_ids`, giảm `drip_count` nếu là suất HÔM NAY, và
///   XÓA `auto_fetch_date` để lượt chạy sau được quét kho lấy lại ngay
///   (không phải chờ mai). Không đụng video user tự tải tay.
pub fn reconcile_all(
    store: &Arc<WatchlistStore>,
    queue: &Arc<QueueManager>,
    history: &Arc<HistoryStore>,
) {
    use crate::models::HistoryStatus;
    let channels = store.list();
    if channels.iter().all(|c| c.dl_pending.is_empty()) {
        return; // không có gì đang chờ chốt — khỏi truy vấn queue/history
    }
    // Video đang "sống" trong hàng đợi (chưa kết thúc) — theo video id.
    let live: std::collections::HashSet<String> = queue
        .list()
        .iter()
        .filter(|it| {
            matches!(
                it.state,
                crate::models::DownloadState::Queued
                    | crate::models::DownloadState::Downloading
                    | crate::models::DownloadState::Paused
            )
        })
        .map(|it| video_id_of(&it.request.url))
        .collect();
    // Video đã tải XONG (history chỉ chứa Completed, nhưng lọc cho chắc).
    let done_hist: std::collections::HashSet<String> = history
        .list(None, 2000, 0)
        .unwrap_or_default()
        .iter()
        .filter(|e| e.status == HistoryStatus::Completed)
        .map(|e| video_id_of(&e.url))
        .collect();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    for ch in channels {
        if ch.dl_pending.is_empty() {
            continue;
        }
        let _ = store.update(&ch.id, |c| {
            let pending = std::mem::take(&mut c.dl_pending);
            for vid in pending {
                if done_hist.contains(&vid) {
                    if !c.done_ids.contains(&vid) {
                        c.done_ids.push(vid); // chốt đã làm
                    }
                } else if live.contains(&vid) {
                    c.dl_pending.push(vid); // vẫn đang tải/chờ/retry
                } else {
                    // Hủy/lỗi cứng → trả suất, gỡ seen, cho quét kho lại.
                    c.seen_ids.retain(|s| s != &vid);
                    if c.drip_date.as_deref() == Some(today.as_str()) && c.drip_count > 0 {
                        c.drip_count -= 1;
                    }
                    c.auto_fetch_date = None;
                }
            }
        });
    }
}

/// Làm sạch tên kênh đích thành tên thư mục Windows hợp lệ: thay ký tự cấm
/// `<>:"/\|?*` + ký tự điều khiển bằng khoảng trắng, gộp khoảng trắng thừa,
/// bỏ chấm/khoảng trắng cuối (Windows cấm). Rỗng sau khi lọc -> None.
pub fn sanitize_folder_name(name: &str) -> Option<String> {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || (c as u32) < 0x20 {
                ' '
            } else {
                c
            }
        })
        .collect();
    let mut out = String::new();
    for w in cleaned.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(w);
    }
    let out = out.trim_end_matches(['.', ' ']).to_string();
    if out.is_empty() { None } else { Some(out) }
}

/// Thư mục lưu video của 1 kênh theo dõi — MỘT nơi quyết định duy nhất cho
/// mọi đường tải (video mới / hàng chờ / tự vét / tải tay pending), chốt
/// NGAY LÚC ENQUEUE nên nhiều kênh tải song song không thể lẫn thư mục:
/// 1. `dest_dir` user chọn tay 📁 (ưu tiên — giữ tương thích kênh cũ)
/// 2. `<watch_root>\<target_name đã làm sạch>` khi user gõ tên kênh đích
/// 3. thư mục tải mặc định chung (kèm theo: tool cắt sẽ KHÔNG thấy).
pub fn resolve_watch_folder(
    dest_dir: &Option<String>,
    target_name: &Option<String>,
    watch_root: &Option<String>,
    default_folder: &std::path::Path,
) -> std::path::PathBuf {
    if let Some(d) = dest_dir.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return std::path::PathBuf::from(d);
    }
    if let (Some(root), Some(name)) = (
        watch_root.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        target_name.as_deref().and_then(sanitize_folder_name),
    ) {
        return std::path::Path::new(root).join(name);
    }
    default_folder.to_path_buf()
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
    // Đối soát video ĐANG TẢI dở trước tiên: tải xong → chốt "đã làm";
    // hủy/lỗi cứng → trả lại suất để lấy lại (không coi là đã tải).
    reconcile_all(store, queue, history);

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
    let settings = settings_store.get();
    // Video ĐÃ TẢI (download-archive của yt-dlp) — loại khỏi CẢ "video mới"
    // LẪN "vét kho" để KHÔNG enqueue lại đồ cũ rồi bị yt-dlp "Bỏ qua" (phí
    // suất + đếm nhầm). Chỉ đọc khi bật "bỏ qua video đã tải".
    let archived = if settings.skip_downloaded {
        load_archive_ids(app)
    } else {
        std::collections::HashSet::new()
    };
    // New = fetched videos CHƯA thấy VÀ CHƯA tải (none on baseline).
    let new_fetched: Vec<&Fetched> = if is_baseline {
        Vec::new()
    } else {
        fetched.iter()
            .filter(|f| !seen.contains(&f.id) && !archived.contains(&f.id))
            .collect()
    };
    let new_count = new_fetched.len() as u32;
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
            let folder = resolve_watch_folder(
                &channel.dest_dir, &channel.target_name,
                &settings.watch_root, &settings.default_folder,
            );
            let got = enqueue_new(app, queue, settings_store, history, &channel.title,
                                  folder, channel.max_height, &vids, &settings).await;
            drip_count += got;
        }
    }

    // Còn suất hôm nay thì lấy thêm video theo CHẾ ĐỘ NGUỒN của kênh
    // (video mới đăng vừa enqueue ở trên đã chiếm suất trước). Không rót ở
    // lượt baseline — kênh vừa thêm, đợi vòng quét kế cho ổn định.
    let mut dripped: Vec<PickedVideo> = Vec::new();
    let mut auto_scanned = false;
    // Ghi chú minh bạch cho lần tự vét (video nào + bao nhiêu view).
    let mut pick_note: Option<String> = None;
    // Chế độ "new" = CHỈ video mới, không rót gì thêm. Mọi chế độ khác
    // (auto / picked cũ) = TỰ ĐỘNG: hàng chờ tích ƯU TIÊN trước (bất kể
    // view nhiều/ít, đúng thứ tự tích) → HẾT hàng chờ mới vét video view
    // cao nhất CHƯA làm. Bỏ qua video đã tải (done_ids).
    if !is_baseline && channel.source_mode != "new" {
        let limit = channel.daily_limit.clamp(1, 3);
        // 1. Hàng chờ tích trước (video mới đã chiếm suất ở trên nếu có);
        //    bỏ video đã có trong archive (dùng `archived` đã nạp ở trên).
        dripped = plan_drip(channel, drip_count);
        dripped.retain(|p| !archived.contains(&p.id));
        let after_picked = drip_count + dripped.len() as u32;
        // 2. Còn suất → vét kho view cao nhất (quét TỐI ĐA 1 lần/ngày).
        if after_picked < limit
            && channel.auto_fetch_date.as_deref() != Some(today.as_str())
        {
            auto_scanned = true;
            // Chỉ VIDEO DÀI hoặc SHORTS — không còn "all" trộn lẫn (bỏ theo
            // yêu cầu: vét lẫn shorts+dài là hỏng format cắt).
            let tab = if channel.tab == "shorts" { "shorts".to_string() } else { "videos".to_string() };
            // Loại video đã làm + vừa lấy từ hàng chờ + ĐÃ TẢI (archive).
            let mut done_plus: Vec<String> = channel.done_ids.clone();
            done_plus.extend(channel.dl_pending.clone());
            done_plus.extend(dripped.iter().map(|p| p.id.clone()));
            done_plus.extend(archived.iter().cloned());
            // KHO CẢ KÊNH (quét 1 lần, lưu đĩa, lần sau lấy thẳng) → chọn
            // video NHIỀU VIEW NHẤT của cả kênh chưa làm.
            let videos = vet_pool(app, &channel.url, &tab, &done_plus, &settings).await;
            if !videos.is_empty() {
                let cand = pick_auto_candidates(
                    &videos, &done_plus, (limit - after_picked) as usize, &tab,
                );
                if let Some(top) = cand.first() {
                    pick_note = Some(fmt_pick_note(top));
                }
                dripped.extend(cand);
            }
        }
    }
    if !dripped.is_empty() {
        let vids: Vec<ChannelVideo> = dripped.iter().map(picked_to_channel_video).collect();
        let folder = resolve_watch_folder(
            &channel.dest_dir, &channel.target_name,
            &settings.watch_root, &settings.default_folder,
        );
        let got = enqueue_new(app, queue, settings_store, history, &channel.title,
                              folder, channel.max_height, &vids, &settings).await;
        dripped.truncate(got as usize);
        drip_count += got;
    }
    // KHO CẠN: quét kho hôm nay mà không moi ra được video nào chưa làm
    // (và cũng chẳng có video mới) -> báo user đổi key. Có bài trở lại
    // (video mới hoặc rót được) -> tự gỡ cờ.
    let source_empty_now: Option<bool> = if new_count > 0 || !dripped.is_empty() {
        Some(false)
    } else if auto_scanned {
        Some(true)
    } else {
        None
    };
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
        if auto_scanned {
            c.auto_fetch_date = Some(today.clone());
        }
        if let Some(v) = source_empty_now {
            c.source_empty = v;
        }
        if let Some(n) = &pick_note {
            c.last_pick = Some(n.clone());
        }
        for p in &dripped {
            c.picked.retain(|x| x.id != p.id);
            if !c.seen_ids.contains(&p.id) {
                c.seen_ids.push(p.id.clone());
            }
        }
        // CHƯA chốt "đã làm" — chỉ đánh dấu ĐANG TẢI. Reconcile sẽ chuyển
        // sang done_ids khi tải xong, hoặc trả suất nếu hủy/lỗi.
        for did in &new_done {
            if !c.dl_pending.contains(did) && !c.done_ids.contains(did) {
                c.dl_pending.push(did.clone());
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

/// Bóc id video ĐÃ TẢI từ nội dung file download-archive của yt-dlp — mỗi dòng
/// dạng "<extractor> <id>" (vd "youtube dQw4w9WgXcQ"); lấy token CUỐI làm id.
/// Hàm thuần để unit-test (không đụng đĩa).
fn parse_archive_ids(text: &str) -> std::collections::HashSet<String> {
    text.lines()
        .filter_map(|l| l.split_whitespace().last())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Đọc id video đã tải từ `app_data_dir/download_archive.txt` (nguồn sự thật
/// cho "đã tải trên máy này" — bền qua mọi phiên/đường tải, không chỉ done_ids
/// của kênh). Lỗi/không có file -> tập rỗng.
fn load_archive_ids(app: &AppHandle) -> std::collections::HashSet<String> {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("download_archive.txt"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| parse_archive_ids(&t))
        .unwrap_or_default()
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

/// Tự vét kho 🤖 — chọn `n` video làm hôm nay: bỏ post ảnh + video đã làm
/// (`done_ids`), LỌC THEO LOẠI (`want`: "shorts" = chỉ Shorts; còn lại =
/// CHỈ VIDEO DÀI — kể cả "all", vì vét lẫn Shorts vào kênh cắt là đểu),
/// xếp view CAO NHẤT trước (thiếu số view thì đứng cuối, giữ thứ tự kênh
/// = mới trước), khử trùng id. Hàm thuần để unit-test.
fn pick_auto_candidates(
    videos: &[ChannelVideo],
    done_ids: &[String],
    n: usize,
    want: &str,
) -> Vec<PickedVideo> {
    let mut cands: Vec<(String, &ChannelVideo)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for v in videos {
        if v.is_photo {
            continue;
        }
        if want == "shorts" {
            if !v.is_short {
                continue;
            }
        } else if v.is_short {
            continue;
        }
        let id = video_id_of(&v.url);
        if done_ids.contains(&id) || !seen.insert(id.clone()) {
            continue;
        }
        cands.push((id, v));
    }
    // sort_by ổn định: cùng view (hoặc cùng thiếu view) giữ thứ tự kênh.
    cands.sort_by(|a, b| b.1.view_count.unwrap_or(0).cmp(&a.1.view_count.unwrap_or(0)));
    cands
        .into_iter()
        .take(n)
        .map(|(id, v)| PickedVideo {
            id,
            url: v.url.clone(),
            title: v.title.clone(),
            view_count: v.view_count,
            thumbnail: v.thumbnail.clone(),
        })
        .collect()
}

/// Bao nhiêu ứng viên "nhiều view nhất theo view XẤP XỈ" (trên TOÀN KÊNH) sẽ
/// được probe VIEW THẬT khi quét mới. 80 = đủ rộng để chắc chắn chứa video
/// hit thật của cả kênh, đủ hẹp để probe nhanh (~3 batch × 30).
const VET_PROBE_WINDOW: usize = 80;

/// Cache kho video của kênh trên đĩa (app_data_dir/channel_cache).
fn vet_cache(app: &AppHandle) -> Option<crate::channel_cache::ChannelCache> {
    use tauri::Manager;
    let dir = app.path().app_data_dir().ok()?.join("channel_cache");
    Some(crate::channel_cache::ChannelCache::new(dir))
}

/// KHO VÉT của kênh — QUÉT CẢ KÊNH (kể cả video cũ) 1 LẦN rồi LƯU XUỐNG ĐĨA;
/// những lần sau lấy THẲNG từ kho đã lưu, KHÔNG quét lại (đỡ mất công quét
/// mấy nghìn video mỗi ngày). Chỉ quét mới khi: chưa có kho, hoặc kho đã CẠN
/// (mọi video đều đã làm — `exclude` phủ hết) → quét lại 1 phát để bắt video
/// mới đăng / xác nhận cạn. Lúc quét mới có PROBE VIEW THẬT cho nhóm đầu bảng
/// rồi lưu kèm, nên lần sau xếp hạng bằng view thật mà không probe lại.
async fn vet_pool(
    app: &AppHandle,
    url: &str,
    tab: &str,
    exclude: &[String],
    settings: &crate::models::Settings,
) -> Vec<ChannelVideo> {
    let cache = vet_cache(app);
    let key = crate::channel_cache::url_key(url, tab);
    // 1. Còn video CHƯA làm trong kho đã lưu → dùng lại, khỏi quét mạng.
    if let Some(c) = &cache {
        if let Some(vids) = c.load(&key) {
            let has_unused = vids
                .iter()
                .any(|v| !v.is_photo && !exclude.contains(&video_id_of(&v.url)));
            if has_unused {
                return vids;
            }
        }
    }
    // 2. Kho trống/cạn → QUÉT CẢ KÊNH 1 lần (limit 0, flat = 1 lần gọi).
    let mut videos = match crate::channel_fetcher::fetch_channel(
        app, url, 0, false, tab, settings, false,
    )
    .await
    {
        Ok((_i, v)) => v,
        // Quét lỗi (mạng/bot) → dùng tạm kho cũ nếu có, đừng mất dữ liệu.
        Err(_) => return cache.and_then(|c| c.load(&key)).unwrap_or_default(),
    };
    // 3. XẾP HẠNG NHANH, ÍT REQUEST (chống bot + nhanh cho 100+ kênh):
    //    YouTube ĐÃ trả sẵn view XẤP XỈ khi quét cả kênh → dùng luôn để xếp
    //    "nhiều view nhất", KHÔNG đo lại từng video (mỗi lần đo là 1 request,
    //    100 kênh × 80 video = quá nhiều → chậm + dễ bị chặn). CHỈ khi kênh
    //    KHÔNG có view nào (hiếm) mới đo 1 cửa sổ để có cái mà xếp.
    let has_views = videos.iter().any(|v| v.view_count.is_some());
    if !has_views {
        let top = pick_auto_candidates(&videos, &[], VET_PROBE_WINDOW, tab);
        let as_cv: Vec<ChannelVideo> = top.iter().map(picked_to_channel_video).collect();
        if let Ok(probed) = crate::channel_fetcher::probe_views(app, as_cv, settings).await {
            let exact: std::collections::HashMap<String, u64> = probed
                .iter()
                .filter_map(|v| v.view_count.map(|n| (v.url.clone(), n)))
                .collect();
            for v in videos.iter_mut() {
                if let Some(n) = exact.get(&v.url) {
                    v.view_count = Some(*n);
                }
            }
        }
    }
    // 4. Lưu kho để lần sau lấy thẳng khỏi quét lại.
    if let Some(c) = &cache {
        c.save(&key, &videos);
    }
    videos
}

/// Số view gọn cho người đọc: 12500 → "12,5N", 2100000 → "2,1Tr".
fn fmt_views_vi(v: u64) -> String {
    if v >= 1_000_000 {
        format!("{:.1}Tr view", v as f64 / 1_000_000.0).replace('.', ",")
    } else if v >= 1_000 {
        format!("{:.1}N view", v as f64 / 1_000.0).replace('.', ",")
    } else {
        format!("{v} view")
    }
}

/// Ghi chú minh bạch cho lần TỰ VÉT: "🔥 tự lấy: <tên> · 12,5N view".
fn fmt_pick_note(p: &PickedVideo) -> String {
    match p.view_count {
        Some(v) => format!("🔥 tự lấy: {} · {}", p.title, fmt_views_vi(v)),
        None => format!("🔥 tự lấy: {}", p.title),
    }
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
    folder: std::path::PathBuf,
    max_height: Option<u32>,
    videos: &[ChannelVideo],
    settings: &crate::models::Settings,
) -> u32 {
    if videos.is_empty() {
        return 0;
    }
    let _ = app; // reserved for future per-item resolution
    // Thư mục đã được resolve_watch_folder chốt cho KÊNH NÀY — tạo sẵn
    // (best-effort) để yt-dlp không vấp thư mục thiếu khi tải song song.
    let _ = std::fs::create_dir_all(&folder);

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
            max_height,
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

    #[test]
    fn parse_archive_lay_dung_id_va_bo_dong_rong() {
        // yt-dlp ghi "<extractor> <id>"; lấy token cuối. Dòng rỗng/khoảng
        // trắng bỏ qua. Trùng gộp về set.
        let text = "youtube aaa\nyoutube bbb\n\n   \nyoutube aaa\n";
        let got = parse_archive_ids(text);
        assert_eq!(got.len(), 2);
        assert!(got.contains("aaa") && got.contains("bbb"));
    }

    #[test]
    fn vet_loai_video_da_co_trong_archive() {
        // done_plus chứa "hi" (đã tải — mô phỏng id lấy từ download-archive)
        // -> pick KHÔNG chọn lại nó dù nhiều view nhất. Gốc rễ bug "vét đồ cũ".
        let videos = vec![cv("hi", Some(9000), false),
                          cv("new1", Some(50), false),
                          cv("new2", Some(10), false)];
        let got = pick_auto_candidates(&videos, &["hi".to_string()], 2, "all");
        let ids: Vec<&str> = got.iter().map(|p| p.id.as_str()).collect();
        assert!(!ids.contains(&"hi"), "KHÔNG được chọn lại video đã tải (hi)");
        assert_eq!(ids, vec!["new1", "new2"], "chọn video CHƯA tải theo view");
    }

    #[test]
    fn vet_video_dai_khong_ron_shorts() {
        // want="videos" -> KHÔNG chọn video is_short (đã đánh dấu lúc fetch),
        // dù short nhiều view hơn. Fix ca "đặt Video dài mà lẫn Shorts".
        let mut sh = cv("short_hot", Some(9999), false); sh.is_short = true;
        let vids = vec![sh, cv("dai1", Some(100), false), cv("dai2", Some(50), false)];
        let got = pick_auto_candidates(&vids, &[], 5, "videos");
        let ids: Vec<&str> = got.iter().map(|p| p.id.as_str()).collect();
        assert!(!ids.contains(&"short_hot"), "Video dài KHÔNG được rót Shorts");
        assert_eq!(ids, vec!["dai1", "dai2"]);
        // want="shorts" thì ngược lại: CHỈ lấy short
        let mut sh2 = cv("short_hot", Some(9999), false); sh2.is_short = true;
        let got2 = pick_auto_candidates(
            &[sh2, cv("dai1", Some(100), false)], &[], 5, "shorts");
        assert_eq!(got2.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
                   vec!["short_hot"]);
    }

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
            target_name: None,
            max_height: None,
            group: None,
            source_mode: "picked".into(),
            auto_fetch_date: None,
            picked,
            daily_limit: daily,
            drip_date: None,
            drip_count: 0,
            done_ids: done,
            dl_pending: vec![],
            source_empty: false,
            last_pick: None,
        }
    }

    /// pick_auto_candidates PHẢI xếp theo view giảm dần: khi có view thật thì
    /// lấy đúng video nhiều view nhất (nền cho vét sau khi probe view thật).
    #[test]
    fn pick_auto_lay_dung_video_nhieu_view_nhat() {
        let mut lo = cv("lo", Some(1_000), false);
        let mut mid = cv("mid", Some(500_000), false);
        let hi = cv("hi", Some(9_000_000), false);
        lo.title = "lo".into();
        mid.title = "mid".into();
        let got = pick_auto_candidates(&[lo, mid, hi], &[], 1, "all");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "hi", "phải lấy video NHIỀU VIEW NHẤT, không phải video giữa/đầu");
    }

    #[test]
    fn fmt_views_gon_kieu_viet() {
        assert_eq!(fmt_views_vi(950), "950 view");
        assert_eq!(fmt_views_vi(12_500), "12,5N view");
        assert_eq!(fmt_views_vi(2_100_000), "2,1Tr view");
    }

    fn cv(id: &str, views: Option<u64>, photo: bool) -> ChannelVideo {
        ChannelVideo {
            url: format!("https://www.youtube.com/watch?v={id}"),
            title: id.into(),
            duration_sec: None,
            view_count: views,
            upload_date: None,
            thumbnail: None,
            is_photo: photo,
            is_short: false,
            hashtags: Vec::new(),
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

    /// Tăng hạn mức GIỮA NGÀY (1 → 2 sau khi đã tải 1) phải mở lại quét kho
    /// ngay — lần ▶ kế tiếp tải nốt phần chênh, không phải chờ sang mai.
    /// Giảm hạn mức thì giữ nguyên cờ (không có gì để tải thêm).
    #[test]
    fn tang_han_muc_giua_ngay_mo_lai_quet_kho() {
        let mut c = chan(vec![], 1, vec![]);
        c.auto_fetch_date = Some("2026-07-23".into());
        crate::commands::apply_daily_limit(&mut c, 2);
        assert_eq!(c.daily_limit, 2);
        assert!(c.auto_fetch_date.is_none(), "tăng hạn mức phải xóa cờ quét-hôm-nay");
        // Giảm: giữ cờ.
        c.auto_fetch_date = Some("2026-07-23".into());
        crate::commands::apply_daily_limit(&mut c, 1);
        assert_eq!(c.daily_limit, 1);
        assert!(c.auto_fetch_date.is_some(), "giảm hạn mức không cần quét lại");
        // Kẹp 1..=3.
        crate::commands::apply_daily_limit(&mut c, 99);
        assert_eq!(c.daily_limit, 3);
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

    #[test]
    fn thu_muc_uu_tien_dung_thu_tu_va_khong_lan() {
        let def = std::path::PathBuf::from("D:\\Down");
        // 1. dest_dir tay thắng tất
        let f = resolve_watch_folder(
            &Some("E:\\Tay".into()), &Some("Kênh A".into()),
            &Some("D:\\TC".into()), &def,
        );
        assert_eq!(f, std::path::PathBuf::from("E:\\Tay"));
        // 2. gốc + tên kênh đích
        let f = resolve_watch_folder(&None, &Some("Kênh A".into()), &Some("D:\\TC".into()), &def);
        assert_eq!(f, std::path::PathBuf::from("D:\\TC").join("Kênh A"));
        // 3. thiếu gốc -> mặc định; thiếu tên -> mặc định
        assert_eq!(resolve_watch_folder(&None, &Some("K".into()), &None, &def), def);
        assert_eq!(resolve_watch_folder(&None, &None, &Some("D:\\TC".into()), &def), def);
        // 2 kênh đích khác nhau -> 2 thư mục khác nhau (tải song song không lẫn)
        let a = resolve_watch_folder(&None, &Some("Kênh A".into()), &Some("D:\\TC".into()), &def);
        let b = resolve_watch_folder(&None, &Some("Kênh B".into()), &Some("D:\\TC".into()), &def);
        assert_ne!(a, b);
    }

    #[test]
    fn ten_thu_muc_duoc_lam_sach_ky_tu_cam() {
        assert_eq!(sanitize_folder_name("kênh: mỹ/1 *hot*?"), Some("kênh mỹ 1 hot".into()));
        assert_eq!(sanitize_folder_name("Kênh A."), Some("Kênh A".into()));
        assert_eq!(sanitize_folder_name("  @user.tiktok  "), Some("@user.tiktok".into()));
        assert_eq!(sanitize_folder_name("<>:\\|?*"), None);
        assert_eq!(sanitize_folder_name("   "), None);
        // Tên sạch qua resolve không nổ đường dẫn
        let f = resolve_watch_folder(
            &None, &Some("kênh: mỹ/1".into()), &Some("D:\\TC".into()),
            std::path::Path::new("D:\\Down"),
        );
        assert_eq!(f, std::path::PathBuf::from("D:\\TC").join("kênh mỹ 1"));
    }

    fn cvs(id: &str, views: Option<u64>, short: bool) -> ChannelVideo {
        let mut v = cv(id, views, false);
        v.is_short = short;
        v
    }

    #[test]
    fn auto_chon_view_cao_nhat_chua_lam() {
        let videos = vec![
            cv("moi", Some(100), false),      // view thấp
            cv("hot", Some(9_000), false),    // view cao nhất -> chọn trước
            cv("dalam", Some(50_000), true),  // post ảnh -> bỏ
            cv("done1", Some(99_999), false), // đã làm -> bỏ
            cv("kha", Some(5_000), false),
        ];
        let got = pick_auto_candidates(&videos, &["done1".to_string()], 2, "all");
        assert_eq!(
            got.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["hot", "kha"]
        );
    }

    #[test]
    fn auto_mac_dinh_chi_vet_video_dai_bo_shorts() {
        let videos = vec![
            cvs("short_hot", Some(900_000), true), // Shorts view khủng -> vẫn bỏ
            cvs("dai_1", Some(50_000), false),
            cvs("dai_2", Some(80_000), false),
        ];
        // "all" lẫn "videos" đều CHỈ lấy video dài, xếp theo view.
        for want in ["all", "videos"] {
            let got = pick_auto_candidates(&videos, &[], 5, want);
            assert_eq!(
                got.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
                ["dai_2", "dai_1"],
                "want={want}"
            );
        }
        // Kênh chuyên Shorts thì ngược lại: chỉ lấy Shorts.
        let got = pick_auto_candidates(&videos, &[], 5, "shorts");
        assert_eq!(got.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["short_hot"]);
    }

    #[test]
    fn auto_thieu_view_giu_thu_tu_kenh_va_khu_trung() {
        // Không có số view (yt-dlp flat đôi khi thiếu) -> giữ thứ tự kênh
        // (video mới đứng trước); id trùng chỉ lấy 1 lần.
        let videos = vec![
            cv("a", None, false),
            cv("a", None, false),
            cv("b", None, false),
        ];
        let got = pick_auto_candidates(&videos, &[], 5, "videos");
        assert_eq!(got.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
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
        // Kênh cũ mặc định "new" — GIỮ NGUYÊN hành vi trước giờ (chỉ video
        // mới), không tự dưng đi vét kho của user.
        assert_eq!(c.source_mode, "new");
        assert!(c.group.is_none());
        assert!(c.auto_fetch_date.is_none());
        assert!(c.target_name.is_none());
        assert!(!c.source_empty);
        assert!(c.dl_pending.is_empty());
    }

    /// Logic đối soát dl_pending (tách riêng để test không cần queue/history
    /// thật): áp cùng quy tắc với reconcile_pending.
    fn reconcile_logic(
        c: &mut WatchedChannel,
        live: &[&str],
        done_hist: &[&str],
        today: &str,
    ) {
        let pending = std::mem::take(&mut c.dl_pending);
        for vid in pending {
            if done_hist.contains(&vid.as_str()) {
                if !c.done_ids.contains(&vid) {
                    c.done_ids.push(vid);
                }
            } else if live.contains(&vid.as_str()) {
                c.dl_pending.push(vid);
            } else {
                c.seen_ids.retain(|s| s != &vid);
                if c.drip_date.as_deref() == Some(today) && c.drip_count > 0 {
                    c.drip_count -= 1;
                }
                c.auto_fetch_date = None;
            }
        }
    }

    #[test]
    fn reconcile_tai_xong_thi_chot_da_lam() {
        let mut c = chan(vec![], 1, vec![]);
        c.dl_pending = vec!["v1".into()];
        c.seen_ids = vec!["v1".into()];
        c.drip_date = Some("2026-07-23".into());
        c.drip_count = 1;
        reconcile_logic(&mut c, &[], &["v1"], "2026-07-23");
        assert!(c.dl_pending.is_empty());
        assert!(c.done_ids.contains(&"v1".to_string()));
        assert_eq!(c.drip_count, 1, "tải xong thì GIỮ suất");
    }

    #[test]
    fn reconcile_dang_tai_thi_giu_nguyen() {
        let mut c = chan(vec![], 1, vec![]);
        c.dl_pending = vec!["v1".into()];
        c.drip_date = Some("2026-07-23".into());
        c.drip_count = 1;
        reconcile_logic(&mut c, &["v1"], &[], "2026-07-23");
        assert_eq!(c.dl_pending, vec!["v1".to_string()], "đang tải → còn chờ");
        assert!(c.done_ids.is_empty());
        assert_eq!(c.drip_count, 1);
    }

    #[test]
    fn reconcile_huy_loi_thi_tra_suat() {
        let mut c = chan(vec![], 1, vec![]);
        c.dl_pending = vec!["v1".into()];
        c.seen_ids = vec!["base1".into(), "v1".into()];
        c.drip_date = Some("2026-07-23".into());
        c.drip_count = 1;
        c.auto_fetch_date = Some("2026-07-23".into());
        // v1 không còn trong queue, chưa Completed → hủy/lỗi.
        reconcile_logic(&mut c, &[], &[], "2026-07-23");
        assert!(c.dl_pending.is_empty());
        assert!(!c.done_ids.contains(&"v1".to_string()), "KHÔNG được coi là đã làm");
        assert!(!c.seen_ids.contains(&"v1".to_string()), "gỡ seen để lấy lại");
        assert_eq!(c.drip_count, 0, "TRẢ lại suất hôm nay");
        // Fix bug "hủy xong bấm ▶ không tải được": phải cho quét kho lại ngay.
        assert!(c.auto_fetch_date.is_none(), "xóa cờ quét để ▶ lấy lại được ngay");
    }
}

/// ➕ TẢI THÊM 1 video cho kênh NGAY HÔM NAY, vượt hạn mức ngày (user chủ
/// động bấm): hàng chờ tích trước → hết thì vét kho video view cao nhất
/// chưa làm. CỘNG THÊM vào bộ đếm hôm nay (không hoàn suất như trước —
/// fix bug "tải thêm mà vẫn hiện 1 video"). Trả về số video đã xếp tải.
pub async fn force_one_more(
    app: &AppHandle,
    store: &Arc<WatchlistStore>,
    queue: &Arc<QueueManager>,
    settings_store: &Arc<SettingsStore>,
    history: &Arc<HistoryStore>,
    id: &str,
) -> u32 {
    reconcile_all(store, queue, history);
    let channel = match store.get(id) {
        Some(c) => c,
        None => return 0,
    };
    let settings = settings_store.get();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    // 1. Ưu tiên hàng chờ user đã tích (bỏ video đã làm / đang tải).
    let mut cand: Option<PickedVideo> = channel
        .picked
        .iter()
        .find(|p| !channel.done_ids.contains(&p.id) && !channel.dl_pending.contains(&p.id))
        .cloned();
    // 2. Hết hàng chờ → quét kho lấy video view cao nhất chưa làm (bỏ qua
    //    giới hạn "1 lần quét/ngày" vì đây là lệnh tay).
    if cand.is_none() {
        let tab = if channel.tab == "shorts" { "shorts".to_string() } else { "videos".to_string() };
        let mut done_plus = channel.done_ids.clone();
        done_plus.extend(channel.dl_pending.clone());
        // KHO CẢ KÊNH (đã lưu → lấy thẳng; cạn → quét lại 1 phát).
        let videos = vet_pool(app, &channel.url, &tab, &done_plus, &settings).await;
        cand = pick_auto_candidates(&videos, &done_plus, 1, &tab).into_iter().next();
    }
    let Some(p) = cand else {
        // Không còn gì để lấy — kho cạn thật.
        let _ = store.update(id, |c| c.source_empty = true);
        return 0;
    };

    let folder = resolve_watch_folder(
        &channel.dest_dir, &channel.target_name,
        &settings.watch_root, &settings.default_folder,
    );
    let got = enqueue_new(
        app, queue, settings_store, history, &channel.title,
        folder, channel.max_height, &[picked_to_channel_video(&p)], &settings,
    ).await;
    if got > 0 {
        let _ = store.update(id, |c| {
            if c.drip_date.as_deref() != Some(today.as_str()) {
                c.drip_date = Some(today.clone());
                c.drip_count = 0;
            }
            c.drip_count += got;
            c.picked.retain(|x| x.id != p.id);
            if !c.seen_ids.contains(&p.id) {
                c.seen_ids.push(p.id.clone());
            }
            if !c.dl_pending.contains(&p.id) {
                c.dl_pending.push(p.id.clone());
            }
            c.source_empty = false;
            c.last_pick = Some(fmt_pick_note(&p));
        });
        let _ = app.emit(
            crate::events::EV_WATCH_UPDATED,
            crate::events::WatchUpdatedPayload { channel_id: id.to_string(), new_count: got },
        );
    }
    got
}

/// Bao nhiêu kênh được QUÉT NGUỒN cùng lúc khi bấm ▶ Chạy tất cả. 3 =
/// nhanh hơn hẳn chạy nối đuôi mà vẫn nhẹ, không làm YouTube nghi bot.
/// (Việc TẢI video vẫn theo hạn mức tải song song riêng của hàng đợi.)
const CHECK_CONCURRENCY: usize = 3;

/// Quét MỌI kênh đang bật — chạy SONG SONG tối đa `CHECK_CONCURRENCY` kênh
/// một lúc (trước đây nối đuôi từng kênh → kênh sau phải đợi kênh trước
/// quét xong mới chạy, rất lâu với nhiều kênh). Trả về danh sách đã cập nhật.
pub async fn check_all(
    app: &AppHandle,
    store: &Arc<WatchlistStore>,
    queue: &Arc<QueueManager>,
    settings_store: &Arc<SettingsStore>,
    history: &Arc<HistoryStore>,
) -> Vec<crate::models::WatchedChannel> {
    use tokio::sync::Semaphore;
    use tokio::task::JoinSet;

    let ids: Vec<String> = store
        .list()
        .into_iter()
        .filter(|c| c.enabled)
        .map(|c| c.id)
        .collect();
    let sem = Arc::new(Semaphore::new(CHECK_CONCURRENCY));
    let mut set: JoinSet<()> = JoinSet::new();
    for id in ids {
        let app = app.clone();
        let store = store.clone();
        let queue = queue.clone();
        let settings_store = settings_store.clone();
        let history = history.clone();
        let sem = sem.clone();
        set.spawn(async move {
            let _permit = sem.acquire_owned().await.ok();
            check_channel(&app, &store, &queue, &settings_store, &history, &id).await;
        });
    }
    while set.join_next().await.is_some() {}
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
            // Mặc định TẮT: kênh chỉ tải khi user CHÍNH TAY bấm ▶ Chạy tất
            // cả / ▶ từng kênh. Bật watch_auto_enabled mới tự quét nền.
            if settings_store.get().watch_auto_enabled {
                let _ = check_all(&app, &store, &queue, &settings_store, &history).await;
            }
            let interval_min = settings_store.get().watch_interval_min.clamp(1, 1440);
            tokio::time::sleep(Duration::from_secs(interval_min as u64 * 60)).await;
        }
    });
}
