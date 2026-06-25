//! Event name constants + typed payloads cho Tauri emit/listen.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::models::{DownloadItem, DownloadState, ProgressSnapshot};

pub const EV_DOWNLOAD_PROGRESS: &str    = "download://progress";
pub const EV_DOWNLOAD_STATE: &str       = "download://state";
pub const EV_DOWNLOAD_CONFLICT: &str    = "download://conflict";
pub const EV_DOWNLOAD_COMPLETED: &str   = "download://completed";
pub const EV_DOWNLOAD_FAILED: &str      = "download://failed";
pub const EV_CLIPBOARD_DETECTED: &str   = "clipboard://detected";
pub const EV_NOTIFICATION_CLICKED: &str = "notification://clicked";
pub const EV_SETTINGS_CHANGED: &str     = "settings://changed";
pub const EV_QUEUE_UPDATED: &str        = "queue://updated";
pub const EV_WATCH_UPDATED: &str        = "watch://updated";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEventPayload { pub short_id: String, pub progress: ProgressSnapshot }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateEventPayload {
    pub short_id: String,
    pub state: DownloadState,
    pub error_message: Option<String>,
    pub output_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictEventPayload { pub short_id: String, pub suggested_path: PathBuf, pub conflicting_path: PathBuf }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletedEventPayload { pub short_id: String, pub output_path: PathBuf, pub title: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedEventPayload { pub short_id: String, pub reason: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardEventPayload { pub url: String, pub extractor: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationClickedPayload { pub short_id: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueUpdatedPayload { pub items: Vec<DownloadItem> }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchUpdatedPayload { pub channel_id: String, pub new_count: u32 }
