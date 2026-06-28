//! Wrapper quanh `tauri-plugin-notification` với toggle `Settings.notifications`.
//!
//! Click handler được giải quyết phía frontend bằng cách subscribe `notification://clicked`
//! và backend sẽ emit event đó khi user nhấn (khi system OS notification action có support).
//! Trên Windows/Linux một số môi trường không expose click callback, nên frontend cũng
//! polling-friendly: queue page tự refresh khi nhận state event.

use crate::models::{DownloadItem, Settings};
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

pub fn notify_completed(app: &AppHandle, settings: &Settings, item: &DownloadItem) {
    if !settings.notifications { return; }
    let title = "Tải xong";
    let body = format!("{}  ·  #{}", item.title, item.short_id);
    let _ = app.notification()
        .builder()
        .title(title)
        .body(body)
        .show();
    // BUG FIX: do NOT emit EV_NOTIFICATION_CLICKED here. The frontend treats
    // that event as a real user click and navigates to /queue — emitting it on
    // every completed download made the app jump to the queue page repeatedly
    // during a batch. A real notification click isn't wired (OS-dependent), so
    // we simply don't auto-navigate on completion.
}

pub fn notify_failed(app: &AppHandle, settings: &Settings, item: &DownloadItem, reason: &str) {
    if !settings.notifications { return; }
    let title = "Tải thất bại";
    let body = format!("{}  ·  #{}\n{}", item.title, item.short_id, reason);
    let _ = app.notification()
        .builder()
        .title(title)
        .body(body)
        .show();
}

/// Notify that a watched channel just posted new video(s). `auto` switches the
/// wording between "đang tải" (auto-download) and "bấm để tải" (notify-only).
pub fn notify_new_videos(
    app: &AppHandle,
    settings: &Settings,
    channel: &str,
    count: u32,
    first_title: &str,
    auto: bool,
) {
    if !settings.notifications {
        return;
    }
    let title = if auto {
        format!("🔔 {channel} có {count} video mới — đang tải")
    } else {
        format!("🔔 {channel} có {count} video mới")
    };
    let body = if count == 1 {
        first_title.to_string()
    } else {
        format!("Mới nhất: {first_title}")
    };
    let _ = app.notification().builder().title(title).body(body).show();
}

pub fn dispatch_terminal(app: &AppHandle, settings: &Settings, item: &DownloadItem) {
    use crate::models::DownloadState::*;
    match item.state {
        Completed => notify_completed(app, settings, item),
        Failed => notify_failed(app, settings, item, item.error_message.as_deref().unwrap_or("Lỗi không xác định")),
        _ => {}
    }
}
