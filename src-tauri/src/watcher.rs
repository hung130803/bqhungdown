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
use tauri::AppHandle;

use crate::history_store::HistoryStore;
use crate::models::{ChannelVideo, ConflictPolicy, DownloadMode, DownloadOptions};
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

/// Re-fetch one watched channel and enqueue any new videos. Returns the number
/// of videos enqueued (0 on baseline / error / nothing new). Always updates the
/// channel's `last_checked` / `last_new_count` / `last_error`.
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
    let settings = settings_store.get();
    let tab = if channel.tab.is_empty() { "all".to_string() } else { channel.tab.clone() };

    match crate::channel_fetcher::fetch_channel(app, &channel.url, CHECK_LIMIT, false, &tab, &settings).await {
        Ok((info, videos)) => {
            let fetched_ids: Vec<String> = videos.iter().map(|v| video_id_of(&v.url)).collect();
            let is_baseline = channel.seen_ids.is_empty();
            let seen: std::collections::HashSet<&String> = channel.seen_ids.iter().collect();

            // On baseline we enqueue nothing — just record current videos.
            let new_videos: Vec<ChannelVideo> = if is_baseline {
                Vec::new()
            } else {
                videos
                    .iter()
                    .filter(|v| !seen.contains(&video_id_of(&v.url)))
                    .cloned()
                    .collect()
            };

            let enq = enqueue_new(app, queue, settings_store, history, &channel.title, &new_videos, &settings).await;

            let title = if info.title.is_empty() { None } else { Some(info.title) };
            let _ = store.update(id, |c| {
                for fid in &fetched_ids {
                    if !c.seen_ids.contains(fid) {
                        c.seen_ids.push(fid.clone());
                    }
                }
                c.last_checked = Some(Utc::now());
                c.last_new_count = Some(enq);
                c.last_error = None;
                if c.title.is_none() {
                    c.title = title.clone();
                }
            });
            enq
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

/// Build queue items for the new videos and enqueue them. Background downloads
/// use polite mode (sleep between requests) + auto-rename so they never block
/// on a conflict prompt. Returns how many were enqueued.
async fn enqueue_new(
    app: &AppHandle,
    queue: &Arc<QueueManager>,
    _settings_store: &Arc<SettingsStore>,
    history: &Arc<HistoryStore>,
    channel_title: &Option<String>,
    videos: &[ChannelVideo],
    settings: &crate::models::Settings,
) -> u32 {
    if videos.is_empty() {
        return 0;
    }
    let _ = app; // reserved for future per-item resolution
    let folder = settings.default_folder.clone();

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
            let interval_min = settings_store.get().watch_interval_min.clamp(5, 1440);
            tokio::time::sleep(Duration::from_secs(interval_min as u64 * 60)).await;
        }
    });
}
