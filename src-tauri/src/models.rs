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
    /// Số video đã bị ẩn khỏi danh sách vì đã tải trước đó (download-archive).
    /// `None` khi tính năng "Bỏ qua video đã tải" tắt. UI hiện "đã ẩn N".
    #[serde(default)]
    pub hidden_downloaded: Option<u32>,
    /// YouTube channel id (`UC...`) — dùng cho RSS feed kiểm tra nhanh.
    #[serde(default)]
    pub channel_id: Option<String>,
    /// Ghi chú khi lấy qua YouTube Data API có nhảy key (key hết quota →
    /// chuyển key khác). UI hiện banner để người dùng biết. `None` = không có.
    #[serde(default)]
    pub api_note: Option<String>,
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
  /// True khi entry là post ảnh/slideshow (TikTok photo posts), không phải video.
    /// UI hiện badge "📷 Ảnh" trên row. Backend dùng duration_sec=None +
    /// extractor=tiktok làm proxy, hoặc field _type/url-based heuristic.
    #[serde(default)]
    pub is_photo: bool,
    /// True khi video lấy từ tab Shorts của YouTube (hoặc duration < 60s).
    /// Frontend dùng để render 2 nhóm "Video dài" / "Shorts" riêng biệt.
    #[serde(default)]
    pub is_short: bool,
    /// Hashtag lấy từ tiêu đề + mô tả (chỉ điền khi dùng YouTube Data API).
    #[serde(default)]
    pub hashtags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {

    pub url: String,
    pub title: String,
    pub duration_sec: Option<u64>,
}

/// A channel the user is auto-watching. The monitor periodically re-fetches it
/// and enqueues any video whose id isn't in `seen_ids` yet. `seen_ids` is
/// seeded with the channel's current videos when first added (baseline), so we
/// only ever grab uploads that appear AFTER watching starts — never the backlog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchedChannel {
    pub id: String,
    pub url: String,
    pub title: Option<String>,
    pub enabled: bool,
    /// Which tab to watch: "all" | "videos" | "shorts".
    pub tab: String,
    pub added_at: DateTime<Utc>,
    pub last_checked: Option<DateTime<Utc>>,
    /// How many new videos the last check enqueued.
    pub last_new_count: Option<u32>,
    /// Last error message (e.g. bot block), shown in UI; None when OK.
    pub last_error: Option<String>,
    /// YouTube channel id (`UC...`), resolved on first add. Enables the fast
    /// RSS-feed check (~1-2 min latency) instead of a heavy yt-dlp scrape.
    #[serde(default)]
    pub channel_id: Option<String>,
    /// Khi true: video mới phát hiện được TỰ TẢI. Khi false ("Chỉ báo"): chỉ
    /// thông báo + đưa vào `pending` để user tự bấm tải. Mặc định true.
    #[serde(default = "default_true")]
    pub auto_download: bool,
    /// Video mới phát hiện ở chế độ "Chỉ báo" (chưa tải), chờ user xử lý.
    #[serde(default)]
    pub pending: Vec<DetectedVideo>,
    /// Video ids already handled (baseline + everything enqueued since).
    #[serde(default)]
    pub seen_ids: Vec<String>,
    /// Thư mục lưu RIÊNG cho video mới của kênh này (dây chuyền 2 tool: mỗi
    /// kênh 1 thư mục trung chuyển — xem INTEGRATION.md). None/rỗng = dùng
    /// thư mục tải mặc định chung trong Settings.
    #[serde(default)]
    pub dest_dir: Option<String>,
    /// Hàng chờ làm: video user tích chọn từ kho, tự tải dần mỗi ngày.
    #[serde(default)]
    pub picked: Vec<PickedVideo>,
    /// Số video TỰ TẢI tối đa mỗi ngày (video mới + hàng chờ). Clamp 1..=3.
    #[serde(default = "default_daily_limit")]
    pub daily_limit: u32,
    /// Ngày local `YYYY-MM-DD` của lần tự tải gần nhất + số đã tải hôm đó —
    /// để hạn mức ngày không reset khi app khởi động lại.
    #[serde(default)]
    pub drip_date: Option<String>,
    #[serde(default)]
    pub drip_count: u32,
    /// Id video đã tự tải qua theo dõi/hàng chờ — dialog kho đánh dấu
    /// "đã làm" để user không tích lại video cũ.
    #[serde(default)]
    pub done_ids: Vec<String>,
}

fn default_daily_limit() -> u32 {
    1
}

/// Video user đã TÍCH CHỌN từ kho kênh nguồn — "hàng chờ làm". Watcher mỗi
/// ngày tự lấy tối đa `daily_limit` video từ đầu hàng tải về `dest_dir`
/// (video MỚI đăng chiếm hạn mức trước) — dây chuyền luôn có bài kể cả khi
/// kênh nguồn không đăng gì. Xem INTEGRATION.md.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedVideo {
    pub id: String,
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub view_count: Option<u64>,
    #[serde(default)]
    pub thumbnail: Option<String>,
}

/// A saved channel/video the user wants to come back to (download or watch
/// later). A simple bookmark list with an optional note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub id: String,
    pub url: String,
    pub note: String,
    pub added_at: DateTime<Utc>,
}

/// A new video detected by the watcher in "notify only" mode — shown in the UI
/// with how long ago it was published, awaiting a manual download/dismiss.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedVideo {
    pub id: String,
    pub url: String,
    pub title: String,
    pub thumbnail: Option<String>,
    /// ISO-8601 publish time from the RSS `<published>` tag (or `YYYYMMDD` from
    /// yt-dlp). UI computes "đăng X phút trước" from this.
    pub published: Option<String>,
    pub detected_at: DateTime<Utc>,
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
    /// Nút "Vẫn tải video này" trên mục Bỏ qua: chạy download KHÔNG kèm
    /// `--download-archive` để yt-dlp không né video đã có trong danh sách
    /// đã-tải. File cũ còn trên máy thì tự thêm ` (1)` chứ không ghi đè.
    #[serde(default)]
    pub force_redownload: bool,
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
    /// Số lần đã tự thử lại do bị giới hạn tốc độ / chặn bot (đếm riêng với
    /// `attempt` để lỗi rate-limit được thử lại nhiều lần với cooldown dài).
    #[serde(default)]
    pub bot_retries: u8,
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
    /// aria2c TẮT mặc định — khôi phục ĐÚNG cấu hình v0.1.77 (user xác nhận
    /// "nhanh nhất, cực kỳ nhanh"). v0.1.77 = aria2c TẮT + KHÔNG `proto` trong
    /// sort → native `-N 32` xé song song video HLS nhiều mảnh = nhanh. Thủ phạm
    /// làm chậm là `proto` (thêm ở v0.1.80) ép video thành DASH 1 file → `-N`
    /// vô dụng — đã gỡ. aria2c chỉ là tuỳ chọn cho ai muốn, không bật mặc định.
    pub aria2c_enabled: bool,
    /// (CŨ) các cờ migration aria2c đời trước — giữ để đọc file cũ, không dùng.
    #[serde(default)]
    pub aria2c_migrated: bool,
    #[serde(default)]
    pub aria2c_reverted: bool,
    #[serde(default)]
    pub aria2c_speed_restored: bool,
    #[serde(default)]
    pub aria2c_default_native: bool,
    #[serde(default)]
    pub aria2c_speed_measured: bool,
    /// (CŨ) migration TẮT aria2c đời trước — giữ để đọc file cũ, không dùng.
    #[serde(default)]
    pub aria2c_user_native: bool,
    /// Migration CHỐT (2026-07-17): ÉP TẮT aria2c 1 lần cho mọi file settings cũ
    /// — khôi phục đúng v0.1.77 (native `-N 32`, user xác nhận "nhanh nhất").
    /// Cần thiết vì ai lỡ cài v0.1.86 (aria2c bật) sẽ còn aria2c=true trên đĩa.
    /// Sau lần này, user tự bật lại trong Cài đặt thì được tôn trọng.
    #[serde(default)]
    pub aria2c_speed_final: bool,
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
    /// Khi true, yt-dlp chạy với `--download-archive <file>` → tự bỏ qua video
    /// đã tải xong trước đó (so theo extractor+id). Cực hữu ích cho reup: tải
    /// lại 1 kênh sẽ không tải trùng video cũ. File archive nằm ở app_data_dir.
    /// Mặc định bật. Archive chỉ ghi nhận video tải từ lúc bật trở đi.
    #[serde(default = "default_true")]
    pub skip_downloaded: bool,
    /// Phút giữa mỗi lần auto-watch kiểm tra kênh mới. Clamp `5..=1440`.
    #[serde(default = "default_watch_interval")]
    pub watch_interval_min: u32,
    /// Danh sách proxy (mỗi dòng 1 cái, ví dụ `http://user:pass@host:port` hoặc
    /// `socks5://host:port`). Rỗng = không dùng proxy. App tự xoay vòng + đổi
    /// proxy khi bị chặn bot — cách duy nhất tải số lượng lớn không bị YouTube
    /// chặn IP. Nên dùng proxy DÂN CƯ (residential), proxy datacenter hay bị chặn.
    #[serde(default)]
    pub proxies: Vec<String>,
    /// Bật PO Token provider (bgutil) — YouTube giờ bắt buộc PO token cho hầu
    /// hết client; thiếu nó là nguồn 403 Forbidden số 1. App tự tải + chạy
    /// server token ngầm (best-effort). Mặc định BẬT; ai đã chủ động tắt
    /// trong settings cũ thì vẫn giữ tắt.
    #[serde(default = "default_true")]
    pub po_token_enabled: bool,
    /// Migration một lần: settings tạo từ thời PO token mặc định TẮT sẽ được
    /// tự bật lại đúng 1 lần (fix 403 hàng loạt giữa 2026). Sau đó user tắt
    /// thủ công thì được tôn trọng vĩnh viễn. File mới luôn `true`.
    #[serde(default)]
    pub po_token_migrated: bool,
    /// Khi bị YouTube giới hạn tốc độ / chặn bot, đợi bao nhiêu phút rồi tự tải
    /// lại (thay vì bỏ cuộc). Clamp `1..=120`. Mặc định 10.
    #[serde(default = "default_cooldown")]
    pub rate_limit_cooldown_min: u32,
    /// Tải kênh → tự tạo thư mục con theo tên kênh (`<thư mục>/<tên kênh>`).
    /// Mặc định bật, để tải nhiều kênh không lẫn lộn.
    #[serde(default = "default_true")]
    pub channel_subfolder: bool,
    /// Bấm X → thu nhỏ xuống khay + chạy ngầm (để tiếp tục tải) thay vì thoát.
    /// Mặc định bật. Tắt → bấm X là thoát hẳn.
    #[serde(default = "default_true")]
    pub minimize_to_tray: bool,
    /// (CŨ) 1 YouTube Data API key — giữ lại để chuyển dữ liệu sang
    /// `youtube_api_keys`. Không dùng trực tiếp nữa.
    #[serde(default)]
    pub youtube_api_key: Option<String>,
    /// Danh sách YouTube Data API key. Khi có, lấy view/thời lượng/ngày/hashtag
    /// cả kênh trong vài giây. Nhiều key → key này hết quota tự nhảy sang key
    /// kế tiếp. Rỗng = dùng yt-dlp như cũ.
    #[serde(default)]
    pub youtube_api_keys: Vec<String>,
    /// Độ phân giải TỐI ĐA khi chọn "Tốt nhất" (format_id = None). 0 = không
    /// giới hạn (vớ luôn 4K/8K nếu có). Mặc định 1080 vì 4K to gấp ~5.5 lần
    /// 1080p → tải hàng loạt lâu hơn nhiều dù mạng nhanh. User muốn 4K thì đặt
    /// 2160 (hoặc 0) hoặc chọn tay chất lượng cao trong ô Chất lượng.
    #[serde(default = "default_max_height")]
    pub max_height: u32,
}

fn default_cooldown() -> u32 {
    10
}

fn default_true() -> bool {
    true
}

fn default_watch_interval() -> u32 {
    60
}

fn default_max_height() -> u32 {
    1080
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
            aria2c_migrated: true,
            aria2c_reverted: true,
            aria2c_speed_restored: true,
            aria2c_default_native: true,
            aria2c_speed_measured: true,
            aria2c_user_native: true,
            aria2c_speed_final: true,
            cookies_browser: None,
            cookies_file: None,
            skip_downloaded: true,
            watch_interval_min: 60,
            proxies: Vec::new(),
            po_token_enabled: true,
            po_token_migrated: true,
            rate_limit_cooldown_min: 10,
            channel_subfolder: true,
            minimize_to_tray: true,
            youtube_api_key: None,
            youtube_api_keys: Vec::new(),
            max_height: 1080,
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
    pub skip_downloaded: Option<bool>,
    pub watch_interval_min: Option<u32>,
    pub proxies: Option<Vec<String>>,
    pub po_token_enabled: Option<bool>,
    pub rate_limit_cooldown_min: Option<u32>,
    pub channel_subfolder: Option<bool>,
    pub minimize_to_tray: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional_optional_string")]
    pub youtube_api_key: Option<Option<String>>,
    pub youtube_api_keys: Option<Vec<String>>,
    pub max_height: Option<u32>,
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
