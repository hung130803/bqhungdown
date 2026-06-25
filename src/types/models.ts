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
}

export interface ChannelVideo {
  url: string;
  title: string;
  durationSec?: number | null;
  viewCount?: number | null;
  /** `YYYYMMDD` when extractor exposes it. */
  uploadDate?: string | null;
  thumbnail?: string | null;
  /** True if this entry came from the channel's Shorts tab (or is < 60s).
   *  UI uses this to render a 2-column "Video dài | Shorts" split. */
  isShort?: boolean;
  /** True khi entry là TikTok photo post (slideshow ảnh). UI hiện badge. */
  isPhoto?: boolean;
}

export interface ChannelFetchResult {
  info: ChannelInfo;
  videos: ChannelVideo[];
}

// ──────────────────────────────────────────────────────────────────────────────
// Auto-watch channels
// ──────────────────────────────────────────────────────────────────────────────

export interface WatchedChannel {
  id: string;
  url: string;
  title?: string | null;
  enabled: boolean;
  /** "all" | "videos" | "shorts" */
  tab: string;
  addedAt: string;
  lastChecked?: string | null;
  /** New videos enqueued on the last check. */
  lastNewCount?: number | null;
  lastError?: string | null;
  seenIds?: string[];
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
