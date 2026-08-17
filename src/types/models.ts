/**
 * Domain models cho frontend.
 *
 * Mirror các struct/enum bên Rust (`src-tauri/src/models.rs`). Backend serialize
 * struct với `#[serde(rename_all = "camelCase")]`, nên mọi field bên TS dùng
 * camelCase. Enum-type variants dùng lowercase string (Rust `rename_all = "lowercase"`).
 *
 * Tham chiếu: design.md — Data Models — TypeScript.
 */

// ──────────────────────────────────────────────────────────────────────────────
// Enums
// ──────────────────────────────────────────────────────────────────────────────

export type DownloadState =
  | "queued"
  | "downloading"
  | "paused"
  | "completed"
  | "failed"
  | "cancelled"
  | "skipped";

export type DownloadMode = "video" | "audio";

export type ConflictPolicy = "ask" | "overwrite" | "skip" | "rename";

/**
 * User choice khi `ConflictPolicy === "ask"` và file đã tồn tại.
 *
 * Rust enum `ConflictChoice` dùng `#[serde(rename_all = "lowercase")]`, nên
 * variant `AutoRename` được serialize thành `"autorename"` (không phải
 * `"autoRename"` hay `"auto_rename"`).
 */
export type ConflictChoice = "overwrite" | "skip" | "autorename";

export type Theme = "light" | "dark" | "system";
export type Language = "vi" | "en";
export type HistoryStatus = "completed" | "failed" | "cancelled";

// ──────────────────────────────────────────────────────────────────────────────
// Metadata
// ──────────────────────────────────────────────────────────────────────────────

export interface QualityFormat {
  formatId: string;
  ext: string;
  resolution: string | null;
  fps: number | null;
  vcodec: string | null;
  acodec: string | null;
  abr: number | null;
  vbr: number | null;
  filesize: number | null;
  isAudioOnly: boolean;
  isVideoOnly: boolean;
  formatNote: string | null;
  tbr: number | null;
  height: number | null;
  width: number | null;
}

export interface SubtitleTrack {
  langCode: string;
  langName: string;
  isAuto: boolean;
}

export interface PlaylistEntry {
  url: string;
  title: string;
  durationSec: number | null;
}

export interface VideoMetadata {
  url: string;
  extractor: string;
  title: string;
  channel: string | null;
  thumbnail: string | null;
  durationSec: number | null;
  formats: QualityFormat[];
  subtitles: SubtitleTrack[];
  playlistEntries: PlaylistEntry[] | null;
  playlistTotal: number | null;
}

// ──────────────────────────────────────────────────────────────────────────────
// Download request / queue items
// ──────────────────────────────────────────────────────────────────────────────

export interface DownloadRequest {
  url: string;
  mode: DownloadMode;
  formatId: string | null;
  saveFolder: string;
  subLangs: string[];
  autoTranslateTo: string | null;
  onConflict: ConflictPolicy;
  useAria2c: boolean;
  playlistAll: boolean;
}

export interface DownloadItem {
  shortId: string;
  request: DownloadRequest;
  title: string;
  thumbnail: string | null;
  /** Tên kênh / uploader (YouTube channel, TikTok username...). */
  channel?: string | null;
  extractor: string;
  state: DownloadState;
  bytesDownloaded: number;
  bytesTotal: number | null;
  speedBps: number | null;
  etaSec: number | null;
  attempt: number;
  errorMessage: string | null;
  outputPath: string | null;
  /** ISO-8601 timestamp. */
  createdAt: string;
  /** ISO-8601 timestamp; null khi item chưa terminal. */
  finishedAt: string | null;
}

// ──────────────────────────────────────────────────────────────────────────────
// History
// ──────────────────────────────────────────────────────────────────────────────

export interface HistoryEntry {
  shortId: string;
  url: string;
  title: string;
  extractor: string;
  formatId: string | null;
  mode: DownloadMode;
  saveFolder: string;
  outputPath: string | null;
  status: HistoryStatus;
  error: string | null;
  /** ISO-8601 timestamp. */
  finishedAt: string;
  /** Tên kênh / uploader. Có thể null với extractor không trả field này. */
  channel?: string | null;
  /** URL thumbnail (https://...). UI dùng `<img src>` trực tiếp. */
  thumbnail?: string | null;
  /** User flag: đã edit xong file này chưa. */
  edited?: boolean;
  /** ISO-8601 timestamp khi user đánh dấu edit. */
  editedAt?: string | null;
}

// ──────────────────────────────────────────────────────────────────────────────
// Channel listing
// ──────────────────────────────────────────────────────────────────────────────

export interface ChannelInfo {
  url: string;
  title: string;
  thumbnail?: string | null;
  /** Total videos on the channel — not always available. */
  videoCount?: number | null;
  extractor: string;
  /** Số video bị ẩn vì đã tải trước đó (khi bật "Bỏ qua video đã tải"). */
  hiddenDownloaded?: number | null;
  /** Ghi chú khi lấy qua YouTube API có nhảy key (key hết quota → đổi key). */
  apiNote?: string | null;
}

export interface ChannelVideo {
  url: string;
  title: string;
  durationSec?: number | null;
  viewCount?: number | null;
  /** Lượt THÍCH (tim). Douyin/TikTok trả số này thật, còn `viewCount` của
   *  DOUYIN thì KHÔNG — web API luôn trả `play_count = 0` kể cả khi có cookie
   *  đăng nhập (đo 16/08/2026: 0/21 bài có view, 21/21 bài có tim).
   *  TikTok thì có ĐỦ CẢ HAI (đo 17/08/2026 bằng chính yt-dlp của app). */
  likeCount?: number | null;
  /** Lượt BÌNH LUẬN. Douyin `statistics.comment_count`; TikTok `comment_count`;
   *  YouTube `statistics.commentCount`. Cả ba đều nằm sẵn trong gói app ĐANG
   *  tải — bóc thêm không tốn lượt gọi mạng / quota nào. */
  commentCount?: number | null;
  /** Lượt CHIA SẺ. Douyin `statistics.share_count`; TikTok `repost_count`.
   *  YouTube KHÔNG công bố ở API công khai nên luôn null ở đó.
   *
   *  Với Douyin (không có lượt xem), đây là thước "hot" tốt nhất lấy được
   *  MIỄN PHÍ: bài nổi thường có chia sẻ CAO HƠN bình luận (đo kênh anh Hùng:
   *  12.272 vs 908 · 1.593 vs 325). */
  shareCount?: number | null;
  /** `YYYYMMDD` when extractor exposes it. */
  uploadDate?: string | null;
  thumbnail?: string | null;
  /** True if this entry came from the channel's Shorts tab (or is < 60s).
   *  UI uses this to render a 2-column "Video dài | Shorts" split. */
  isShort?: boolean;
  /** True khi entry là TikTok photo post (slideshow ảnh). UI hiện badge. */
  isPhoto?: boolean;
  /** Hashtag (#abc) lấy từ tiêu đề + mô tả — chỉ có khi dùng YouTube Data API. */
  hashtags?: string[];
  /** True khi video đã có trong sổ tải = ĐÃ TẢI. UI hiện "tích vàng · đã tải"
   *  (không ẩn nữa); bấm Khôi phục để coi như chưa tải + tải lại được. */
  downloaded?: boolean;
}

export interface ChannelFetchResult {
  info: ChannelInfo;
  videos: ChannelVideo[];
  /** Tuổi cache (giây) khi kết quả lấy từ KHO ĐÃ LƯU; null = vừa lấy thật. */
  cachedAgeSecs?: number | null;
}

// ──────────────────────────────────────────────────────────────────────────────
// Auto-watch channels
// ──────────────────────────────────────────────────────────────────────────────

export interface Bookmark {
  id: string;
  url: string;
  note: string;
  addedAt: string;
}

export interface DetectedVideo {
  id: string;
  url: string;
  title: string;
  thumbnail?: string | null;
  /** ISO-8601 publish time (or YYYYMMDD). UI shows "đăng X phút trước". */
  published?: string | null;
  detectedAt: string;
}

export interface WatchedChannel {
  id: string;
  url: string;
  title?: string | null;
  enabled: boolean;
  /** "all" | "videos" | "shorts" */
  tab: string;
  addedAt: string;
  lastChecked?: string | null;
  /** New videos detected on the last check. */
  lastNewCount?: number | null;
  lastError?: string | null;
  channelId?: string | null;
  /** True = auto-download new videos; false = chỉ báo (notify only). */
  autoDownload: boolean;
  /** Videos detected in notify-only mode, awaiting manual download. */
  pending?: DetectedVideo[];
  seenIds?: string[];
  /** Thư mục lưu RIÊNG cho video mới của kênh (dây chuyền — INTEGRATION.md).
   *  null/undefined = dùng thư mục tải mặc định chung. */
  destDir?: string | null;
  /** TÊN KÊNH ĐÍCH (kênh TikTok của user) — video tự về
   *  `<watchRoot>\<tên>`; destDir đặt tay vẫn ưu tiên hơn. */
  targetName?: string | null;
  /** Nhóm/quốc gia user gán ("Mỹ", "Hàn"...). Rỗng = chưa phân nhóm. */
  group?: string | null;
  /** Nguồn khi kênh không đăng mới: "new" | "picked" (hàng chờ 🎯) |
   *  "auto" (tự vét kho theo view). Video MỚI luôn ưu tiên. */
  sourceMode?: string;
  /** Ngày local đã quét kho cho chế độ auto (1 lần/ngày). */
  autoFetchDate?: string | null;
  /** Hàng chờ làm — video đã tích chọn từ kho, tự tải dần mỗi ngày. */
  picked?: PickedVideo[];
  /** Số video tự tải tối đa/ngày (video mới + hàng chờ), 1-3. */
  dailyLimit?: number;
  /** Ngày local YYYY-MM-DD + số video đã tự tải hôm đó. */
  dripDate?: string | null;
  dripCount?: number;
  /** Id video đã tự tải — dialog kho đánh dấu "đã làm". */
  doneIds?: string[];
  /** Id video user CHỦ ĐỘNG ⛔ BỎ QUA — mọi đường lấy video né vĩnh viễn. */
  skippedIds?: string[];
  /** Id video đang tải qua dây chuyền (chưa chốt) — kho coi như "đang làm". */
  dlPending?: string[];
  /** true = kho nguồn ĐÃ CẠN (quét 🤖 không còn video chưa làm) — đổi key. */
  sourceEmpty?: boolean;
  /** Chất lượng TỐI ĐA riêng của kênh (1080, 720…). null = mặc định chung
   *  (1080). Không bao giờ tải vượt; thiếu thì lấy mức thấp hơn gần nhất. */
  maxHeight?: number | null;
  /** Ghi chú minh bạch lần TỰ VÉT gần nhất: "🔥 tự lấy: <tên> · N view". */
  lastPick?: string | null;
}

/** Một video trong hàng chờ làm của kênh theo dõi. */
export interface PickedVideo {
  id: string;
  url: string;
  title: string;
  viewCount?: number | null;
  thumbnail?: string | null;
}

// ──────────────────────────────────────────────────────────────────────────────
// Settings
// ──────────────────────────────────────────────────────────────────────────────

export interface Settings {
  defaultFolder: string;
  maxConcurrency: number;
  theme: Theme;
  language: Language;
  clipboardWatcher: boolean;
  notifications: boolean;
  aria2cEnabled: boolean;
  /**
   * Browser export cookies cho yt-dlp. Bắt buộc cho Douyin / Bilibili / IG
   * private / YouTube age-gated. `null` = không gửi cookies.
   * Giá trị: "chrome" | "firefox" | "edge" | "brave" | "chromium" | "vivaldi" | "opera" | "safari".
   */
  cookiesBrowser?: string | null;
  /**
   * Đường dẫn tới file cookies.txt (Netscape format). Ưu tiên hơn `cookiesBrowser`
   * vì Edge/Chrome trên Windows mới mã hoá AppBound khiến browser-based fail.
   */
  cookiesFile?: string | null;
  /**
   * Bỏ qua video đã tải xong trước đó (yt-dlp `--download-archive`). Hữu ích
   * khi tải lại 1 kênh: không tải trùng. Mặc định bật.
   */
  skipDownloaded: boolean;
  /** Phút giữa mỗi lần auto-watch kiểm tra kênh (5–1440). */
  watchIntervalMin: number;
  /** Danh sách proxy (mỗi dòng 1 cái). App tự xoay vòng + đổi khi bị chặn bot. */
  proxies: string[];
  /** Bật PO Token (bgutil) — giảm chặn bot không cần cookie (app tự tải + chạy ngầm). */
  poTokenEnabled: boolean;
  /** Bị giới hạn tốc độ → đợi bao nhiêu phút rồi tự tải lại (1–120). */
  rateLimitCooldownMin: number;
  /** Tải kênh → tạo thư mục con theo tên kênh. */
  channelSubfolder: boolean;
  /** Bấm X → chạy ngầm dưới khay thay vì thoát (để tiếp tục tải). */
  minimizeToTray: boolean;
  /** (CŨ) 1 key — giữ để tương thích; dữ liệu được chuyển sang youtubeApiKeys. */
  youtubeApiKey?: string | null;
  /**
   * Danh sách YouTube Data API v3 key. Khi có → "Lấy danh sách kênh" dùng API
   * lấy view/thời lượng/ngày/hashtag chính xác cho cả kênh trong vài giây.
   * Nhiều key → key hết quota tự nhảy sang key kế. Rỗng = dùng yt-dlp như cũ.
   */
  youtubeApiKeys?: string[];
  /** Danh sách NHÓM kênh theo dõi user tự đặt (Mỹ, Hàn…) — thêm/sửa/xóa
   *  trong trang Theo dõi; kênh gán nhóm bằng chọn từ danh sách này. */
  watchGroups?: string[];
  /** Thư mục TRUNG CHUYỂN GỐC của dây chuyền — kênh có targetName thì
   *  video tự về `<watchRoot>\<targetName>`. */
  watchRoot?: string | null;
  /**
   * Độ phân giải TỐI ĐA khi chọn "Tốt nhất". 0 = không giới hạn (vớ 4K/8K).
   * Mặc định 1080 vì 4K to gấp ~5.5 lần 1080p → tải hàng loạt lâu hơn nhiều.
   */
  maxHeight?: number;
}

export type SettingsPatch = Partial<Settings>;

// ──────────────────────────────────────────────────────────────────────────────
// Progress / events
// ──────────────────────────────────────────────────────────────────────────────

export interface ProgressSnapshot {
  bytesDownloaded: number;
  bytesTotal: number | null;
  speedBps: number | null;
  etaSec: number | null;
  /** Phần trăm 0..100; null khi không xác định. */
  percent: number | null;
}

// ──────────────────────────────────────────────────────────────────────────────
// Misc / commands payloads
// ──────────────────────────────────────────────────────────────────────────────

export interface UrlValidation {
  valid: boolean;
  extractor: string | null;
}

export interface ExtractorInfo {
  name: string;
  hostPattern: string;
  featured: boolean;
}

export interface DownloadOptions {
  mode: DownloadMode;
  formatId: string | null;
  saveFolder: string;
  subLangs: string[];
  autoTranslateTo: string | null;
  onConflict: ConflictPolicy;
  playlistAll: boolean | null;
  /** Polite mode — adds 2-5s sleep between yt-dlp requests so big channel
   *  batches don't trigger YouTube/TikTok rate limits. */
  polite?: boolean;
}

export interface BootstrapPayload {
  settings: Settings;
  queue: DownloadItem[];
  ffmpegAvailable: boolean;
  aria2cAvailable: boolean;
}

/**
 * Tagged union mirror của `AppError` Rust. Backend có thể gắn thêm chi tiết
 * tại field `data` (ví dụ stderr cho `YtDlpFailed`).
 */
export interface AppErrorPayload {
  kind:
    | "InvalidUrl"
    | "UnsupportedSite"
    | "YtDlpFailed"
    | "FfmpegMissing"
    | "SaveFolderUnavailable"
    | "Timeout"
    | "IllegalTransition"
    | "Io"
    | "ConfigCorrupt"
    | "InvalidSetting";
  data?: unknown;
}

// ──────────────────────────────────────────────────────────────────────────────
// Featured platforms
// ──────────────────────────────────────────────────────────────────────────────

export const FEATURED_PLATFORMS = [
  "youtube",
  "tiktok",
  "facebook",
  "instagram",
  "twitter",
  "twitch",
] as const;

export type FeaturedPlatform = (typeof FEATURED_PLATFORMS)[number];
