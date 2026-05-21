use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadState {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed,
    Cancelled,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadMode {
    Video,
    Audio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConflictPolicy {
    Ask,
    Overwrite,
    Skip,
    Rename,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConflictChoice {
    Overwrite,
    Skip,
    AutoRename,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Vi,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryStatus {
    Completed,
    Failed,
    Cancelled,
}

/// Browser được dùng để export cookies cho yt-dlp.
/// Map thẳng sang giá trị `--cookies-from-browser <name>`.
/// (Trong Settings ta dùng `Option<String>` để JSON đơn giản; enum này chỉ
///  để validate input từ UI nếu cần.)
pub fn cookies_browser_is_valid(name: &str) -> bool {
    matches!(
        name,
        "chrome" | "firefox" | "edge" | "brave" | "chromium" | "vivaldi" | "opera" | "safari"
    )
}

/// Events for the queue/download finite-state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueueEvent {
    Start,
    Complete,
    Fail,
    Cancel,
    Pause,
    Resume,
    Retry,
    Skip,
}

// ---------------------------------------------------------------------------
// Metadata structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityFormat {
    pub format_id: String,
    pub ext: String,
    pub resolution: Option<String>,
    pub fps: Option<f32>,
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    pub abr: Option<f32>,
    pub vbr: Option<f32>,
    pub filesize: Option<u64>,
    pub is_audio_only: bool,
    pub is_video_only: bool,
    pub format_note: Option<String>,
    pub tbr: Option<f32>,
    pub height: Option<u32>,
    pub width: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleTrack {
    pub lang_code: String,
    pub lang_name: String,
    pub is_auto: bool,
}

/// Channel/user listing — returned by `fetch_channel_videos` to show a
/// preview before the user enqueues a batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelInfo {
    pub url: String,
    pub title: String,
    pub thumbnail: Option<String>,
    /// `playlist_count` from yt-dlp; not always provided by the extractor.
    pub video_count: Option<u32>,
    pub extractor: String,
}

/// Single entry inside a channel listing — what the user picks via checkbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelVideo {
    pub url: String,
    pub title: String,
    pub duration_sec: Option<u64>,
    pub view_count: Option<u64>,
    /// Upload date in `YYYYMMDD` form when available — left as opaque string
    /// because not every extractor sets it.
    pub upload_date: Option<String>,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {

    pub url: String,
    pub title: String,
    pub duration_sec: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoMetadata {
    pub url: String,
    pub extractor: String,
    pub title: String,
    pub channel: Option<String>,
    pub thumbnail: Option<String>,
    pub duration_sec: Option<u64>,
    pub formats: Vec<QualityFormat>,
    pub subtitles: Vec<SubtitleTrack>,
    pub playlist_entries: Option<Vec<PlaylistEntry>>,
    pub playlist_total: Option<u32>,
}

// ---------------------------------------------------------------------------
// Download request / item
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRequest {
    pub url: String,
    pub mode: DownloadMode,
    pub format_id: Option<String>,
    pub save_folder: PathBuf,
    pub sub_langs: Vec<String>,
    pub auto_translate_to: Option<String>,
    pub on_conflict: ConflictPolicy,
    /// Resolved from `Settings::aria2c_enabled` at enqueue time.
    pub use_aria2c: bool,
    /// When true, instructs yt-dlp to expand the playlist (`--yes-playlist`).
    pub playlist_all: bool,
    /// "Polite mode" — adds random sleep between requests (yt-dlp
    /// `--sleep-interval` + `--max-sleep-interval`) so big channel batches
    /// don't trip rate limiting. Default false; UI flips on for channel
    /// downloads.
    #[serde(default)]
    pub polite: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItem {
    pub short_id: String,
    pub request: DownloadRequest,
    pub title: String,
    pub thumbnail: Option<String>,
    /// Tên kênh / uploader, đính kèm khi enqueue từ trang Paste URL.
    #[serde(default)]
    pub channel: Option<String>,
    pub extractor: String,
    pub state: DownloadState,
    pub bytes_downloaded: u64,
    pub bytes_total: Option<u64>,
    pub speed_bps: Option<f64>,
    pub eta_sec: Option<u64>,
    pub attempt: u8,
    pub error_message: Option<String>,
    pub output_path: Option<PathBuf>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub short_id: String,
    pub url: String,
    pub title: String,
    pub extractor: String,
    pub format_id: Option<String>,
    pub mode: DownloadMode,
    pub save_folder: PathBuf,
    pub output_path: Option<PathBuf>,
    pub status: HistoryStatus,
    pub error: Option<String>,
    pub finished_at: DateTime<Utc>,
    /// Tên kênh / uploader (YouTube channel, TikTok username...). Có thể null
    /// với extractor không expose field này.
    #[serde(default)]
    pub channel: Option<String>,
    /// URL thumbnail (https://...). UI render qua `<img src>`; CSP cho phép
    /// `img-src https: http: data:`.
    #[serde(default)]
    pub thumbnail: Option<String>,
    /// User flag: đã edit xong file này chưa? Mặc định `false`. Cập nhật qua
    /// command `mark_history_edited`. UI hiện badge ✓ trên row + filter.
    #[serde(default)]
    pub edited: bool,
    /// Khi nào user đánh dấu edit (epoch ms). Hữu ích cho sort / hiển thị.
    #[serde(default)]
    pub edited_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub default_folder: PathBuf,
    /// Allowed range: `1..=10`.
    pub max_concurrency: u8,
    pub theme: Theme,
    pub language: Language,
    pub clipboard_watcher: bool,
    pub notifications: bool,
    pub aria2c_enabled: bool,
    /// Khi != None, yt-dlp được chạy với `--cookies-from-browser <name>`.
    /// Cần cho các site chặn bot mạnh: Douyin, Bilibili, các site châu Á, hay
    /// video YouTube giới hạn tuổi. None = không gửi cookies (mặc định).
    /// Giá trị: "chrome" | "firefox" | "edge" | "brave" | "chromium" | "vivaldi" | "opera" | "safari".
    /// Dùng String thay enum để JSON patch chỉ cần `null` để clear, `"edge"` để set.
    #[serde(default)]
    pub cookies_browser: Option<String>,
    /// Đường dẫn tới file cookies.txt (định dạng Netscape) — alternative cho
    /// `cookies_browser` khi browser cookies bị mã hoá AppBound (Edge/Chrome
    /// trên Windows mới). Khi cả 2 cùng set, file ưu tiên hơn browser.
    #[serde(default)]
    pub cookies_file: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        let base = dirs::download_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")));
        let default_folder = base.join("BQHungDown");
        // Best-effort create — non-fatal if it fails, user can pick another folder.
        let _ = std::fs::create_dir_all(&default_folder);
        Self {
            default_folder,
            max_concurrency: 3,
            theme: Theme::System,
            language: Language::Vi,
            clipboard_watcher: true,
            notifications: true,
            aria2c_enabled: false,
            cookies_browser: None,
            cookies_file: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub default_folder: Option<PathBuf>,
    pub max_concurrency: Option<u8>,
    pub theme: Option<Theme>,
    pub language: Option<Language>,
    pub clipboard_watcher: Option<bool>,
    pub notifications: Option<bool>,
    pub aria2c_enabled: Option<bool>,
    /// `Some(Some(name))` → set; `Some(None)` → clear; `None` → leave alone.
    /// Serde with `default` lets the frontend send literal JSON `null` to clear.
    #[serde(default, deserialize_with = "deserialize_optional_optional_string")]
    pub cookies_browser: Option<Option<String>>,
    /// Same Option<Option<String>> trick for the cookies.txt file path.
    #[serde(default, deserialize_with = "deserialize_optional_optional_string")]
    pub cookies_file: Option<Option<String>>,
}

/// Custom serde deserializer that distinguishes:
/// - missing field → `None`         (leave value alone)
/// - JSON `null`   → `Some(None)`   (explicit clear)
/// - JSON string   → `Some(Some(s))`(set)
fn deserialize_optional_optional_string<'de, D>(d: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    Option::<String>::deserialize(d).map(Some)
}

// ---------------------------------------------------------------------------
// Progress / validation / extractors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressSnapshot {
    pub bytes_downloaded: u64,
    pub bytes_total: Option<u64>,
    pub speed_bps: Option<f64>,
    pub eta_sec: Option<u64>,
    pub percent: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UrlValidation {
    pub valid: bool,
    pub extractor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractorInfo {
    pub name: String,
    pub host_pattern: String,
    pub featured: bool,
}

// ---------------------------------------------------------------------------
// Command payloads
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadOptions {
    pub mode: DownloadMode,
    pub format_id: Option<String>,
    pub save_folder: PathBuf,
    pub sub_langs: Vec<String>,
    pub auto_translate_to: Option<String>,
    pub on_conflict: ConflictPolicy,
    pub playlist_all: Option<bool>,
    /// "Polite mode" — slow random delays between requests to avoid IP bans
    /// when downloading many videos from the same channel.
    #[serde(default)]
    pub polite: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapPayload {
    pub settings: Settings,
    pub queue: Vec<DownloadItem>,
    pub ffmpeg_available: bool,
    pub aria2c_available: bool,
}
