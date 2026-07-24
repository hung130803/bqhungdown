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
    /// True khi video đã có trong sổ tải (download_archive.txt) = ĐÃ TẢI. UI
    /// hiện "tích vàng · đã tải" thay vì ẩn đi; bấm Khôi phục để tải lại.
    #[serde(default)]
    pub downloaded: bool,
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
    // MỌI trường (trừ id/url định danh) đều CÓ default: bản mới thêm trường mới
    // thì watchlist.json CŨ (thiếu trường đó) VẪN đọc được. Thiếu default =
    // parse hỏng = load() trả rỗng = MẤT SẠCH kênh khi cập nhật (bug đã gặp:
    // trường "tab" thêm sau làm file cũ hỏng).
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Which tab to watch: "all" | "videos" | "shorts".
    #[serde(default = "default_tab")]
    pub tab: String,
    #[serde(default = "now_utc")]
    pub added_at: DateTime<Utc>,
    #[serde(default)]
    pub last_checked: Option<DateTime<Utc>>,
    /// How many new videos the last check enqueued.
    #[serde(default)]
    pub last_new_count: Option<u32>,
    /// Last error message (e.g. bot block), shown in UI; None when OK.
    #[serde(default)]
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
    /// TÊN KÊNH ĐÍCH (kênh TikTok của user) mà nguồn này nuôi. Khi đặt +
    /// có `Settings.watch_root`, video tự về `<watch_root>\<target_name>`
    /// (xem `resolve_watch_folder`). `dest_dir` đặt tay vẫn ưu tiên hơn.
    #[serde(default)]
    pub target_name: Option<String>,
    /// Nhóm/quốc gia do user gán ("Mỹ", "Hàn"...) — trang Theo dõi lọc và
    /// gom theo nhãn này khi quản lý nhiều kênh. Rỗng = chưa phân nhóm.
    #[serde(default)]
    pub group: Option<String>,
    /// Nguồn video khi kênh không đăng gì mới (video MỚI luôn ưu tiên):
    /// "new" = chỉ video mới | "picked" = rót từ hàng chờ user tích 🎯 |
    /// "auto" = tự vét kho: app tự chọn video view cao nhất CHƯA làm.
    #[serde(default = "default_source_mode")]
    pub source_mode: String,
    /// Ngày (local) đã quét kho cho chế độ "auto" — mỗi ngày chỉ quét 1 lần
    /// để không giã yt-dlp/API mỗi vòng kiểm tra.
    #[serde(default)]
    pub auto_fetch_date: Option<String>,
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
    /// Id video user CHỦ ĐỘNG BỎ QUA (⛔ trong Kho video) — MỌI đường lấy
    /// video (video mới / hàng chờ / vét / ➕ Tải thêm) không bao giờ chọn.
    #[serde(default)]
    pub skipped_ids: Vec<String>,
    /// Id video ĐANG TẢI qua dây chuyền (auto/drip/picked), CHƯA chốt xong.
    /// Chỉ chuyển sang `done_ids` khi tải THÀNH CÔNG (có trong history
    /// Completed). Nếu hủy/lỗi → gỡ khỏi đây + trả lại suất + gỡ seen_ids
    /// để lấy lại (xem reconcile trong watcher). Chống "chưa tải xong đã
    /// coi là đã làm".
    #[serde(default)]
    pub dl_pending: Vec<String>,
    /// Chất lượng TỐI ĐA của kênh (1080, 720…). None = mặc định chung
    /// (Settings.max_height, mặc định 1080). Không bao giờ tải VƯỢT mức
    /// này; nguồn thiếu thì lấy mức thấp hơn gần nhất.
    #[serde(default)]
    pub max_height: Option<u32>,
    /// true = lần quét kho gần nhất (chế độ 🤖 tự vét) KHÔNG còn video nào
    /// chưa làm — kho nguồn ĐÃ CẠN, user cần đổi key. UI hiện badge đỏ.
    /// Tự về false ngay khi có video mới đăng hoặc tải được video.
    #[serde(default)]
    pub source_empty: bool,
    /// GHI CHÚ MINH BẠCH lần TỰ VÉT gần nhất: video nào được máy chọn +
    /// vì sao (số view thật + ngày đăng). UI hiện dưới thẻ kênh để user
    /// biết rõ "đã lấy video X · 12.500 view · đăng 20/06" — không mơ hồ.
    #[serde(default)]
    pub last_pick: Option<String>,
}

fn default_daily_limit() -> u32 {
    1
}

fn default_source_mode() -> String {
    "new".into()
}

/// Tab theo dõi mặc định khi watchlist.json cũ thiếu trường `tab`.
fn default_tab() -> String {
    "all".into()
}

/// Mốc thời gian mặc định khi file cũ thiếu `added_at` (chỉ để không hỏng
/// parse; giá trị thật sẽ có khi kênh được thêm/sửa lại).
fn now_utc() -> DateTime<Utc> {
    Utc::now()
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
    /// Chất lượng TỐI ĐA riêng cho lượt tải này (kênh theo dõi đặt riêng).
    /// None = dùng `Settings.max_height` chung. Không bao giờ vượt mức này —
    /// thiếu thì yt-dlp tự lấy mức THẤP hơn gần nhất (xem args_builder).
    #[serde(default)]
    pub max_height: Option<u32>,
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
    /// Danh sách NHÓM kênh theo dõi do user tự đặt (Mỹ, Hàn…) — trang Theo
    /// dõi quản lý thêm/sửa/xóa; kênh gán nhóm bằng chọn từ danh sách này.
    #[serde(default)]
    pub watch_groups: Vec<String>,
    /// Cho phép nền TỰ quét/tải kênh theo dõi theo chu kỳ. Mặc định TẮT —
    /// user chỉ muốn tải khi CHÍNH TAY bấm ▶ Chạy tất cả / ▶ từng kênh.
    #[serde(default)]
    pub watch_auto_enabled: bool,
    /// THƯ MỤC TRUNG CHUYỂN GỐC của dây chuyền (INTEGRATION.md). Khi kênh
    /// theo dõi có `target_name`, video tự về `<watch_root>\<target_name>`
    /// — user chỉ gõ tên kênh đích, không phải chọn thư mục từng kênh.
    #[serde(default)]
    pub watch_root: Option<String>,
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
            watch_groups: Vec::new(),
            watch_auto_enabled: false,
            watch_root: None,
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
    pub watch_groups: Option<Vec<String>>,
    pub watch_auto_enabled: Option<bool>,
    /// `Some(None)` = bỏ gốc trung chuyển; `Some(Some(path))` = đặt.
    #[serde(default, deserialize_with = "deserialize_optional_optional_string")]
    pub watch_root: Option<Option<String>>,
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
    /// Chất lượng tối đa riêng (kênh theo dõi) — None = mặc định chung.
    #[serde(default)]
    pub max_height: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapPayload {
    pub settings: Settings,
    pub queue: Vec<DownloadItem>,
    pub ffmpeg_available: bool,
    pub aria2c_available: bool,
}
