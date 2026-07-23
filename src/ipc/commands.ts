/**
 * Typed wrappers cho backend.
 *
 * - Desktop (Tauri): gọi Rust backend qua `invoke`
 * - Web (VITE_WEB_MODE=1): gọi API server qua fetch
 *
 * Module này chọn implementation phù hợp lúc load.
 */

import type {
  Settings,
  SettingsPatch,
  DownloadItem,
  DownloadOptions,
  HistoryEntry,
  VideoMetadata,
  UrlValidation,
  ConflictChoice,
  BootstrapPayload,
  SubtitleTrack,
  ExtractorInfo,
  ChannelFetchResult,
  WatchedChannel,
  PickedVideo,
  Bookmark,
} from "@/types/models";

export interface DouyinPost {
  id: string;
  url: string;
  title: string;
  thumbnail: string;
  isPhoto: boolean;
}

// ── Backend abstraction ──────────────────────────────────────────────────────

const IS_WEB = import.meta.env.VITE_WEB_MODE === "1";
const API_BASE = import.meta.env.VITE_API_URL || "";

async function api<T>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    method: body ? "POST" : "GET",
    headers: body ? { "Content-Type": "application/json" } : {},
    body: body ? JSON.stringify(body) : undefined,
  });
  const data = await res.json();
  if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`);
  return data;
}

// ── Bootstrap ────────────────────────────────────────────────────────────────

export async function appBootstrap(): Promise<BootstrapPayload> {
  if (IS_WEB) {
    return {
      settings: { defaultFolder: "", maxConcurrency: 3, theme: "system", language: "vi", clipboardWatcher: false, notifications: true, aria2cEnabled: false } as Settings,
      queue: [],
      ffmpegAvailable: true,
      aria2cAvailable: false,
    };
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<BootstrapPayload>("app_bootstrap");
}

// ── URL / metadata ──────────────────────────────────────────────────────────

export async function validateUrl(url: string): Promise<UrlValidation> {
  if (IS_WEB) {
    return { valid: true, extractor: detectExtractor(url) };
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<UrlValidation>("validate_url", { url });
}

export async function fetchMetadata(url: string): Promise<VideoMetadata> {
  if (IS_WEB) {
    const raw = await api<any>("/api/info", { url });
    // Transform API response to match VideoMetadata shape
    return {
      url,
      extractor: raw.extractor || detectExtractor(url),
      title: raw.title || "Không có tiêu đề",
      channel: raw.channel || null,
      thumbnail: raw.thumbnail || null,
      durationSec: raw.duration || null,
      formats: (raw.formats || []).map((f: any) => ({
        format_id: f.format_id,
        ext: f.ext,
        resolution: f.resolution || (f.height ? `${f.height}p` : "audio"),
        filesize: f.filesize || f.filesize_approx || null,
        vcodec: f.vcodec || "none",
        acodec: f.acodec || "none",
      })),
      subtitles: [],
      playlistEntries: null,
      playlistTotal: null,
    } as VideoMetadata;
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<VideoMetadata>("fetch_metadata", { url });
}

export async function cancelChannelFetch(): Promise<void> {
  if (IS_WEB) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<void>("cancel_channel_fetch");
}

export async function fetchChannelVideos(
  url: string,
  limit: number = 0,
  detailed: boolean = false,
  tab: "all" | "videos" | "shorts" | "streams" = "videos",
  forceRefresh: boolean = false,
): Promise<ChannelFetchResult> {
  if (IS_WEB) {
    throw new Error("Tính năng kênh chưa hỗ trợ trên web. Dùng app desktop.");
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<ChannelFetchResult>("fetch_channel_videos", { url, limit, detailed, tab, forceRefresh });
}

// ── Enqueue ──────────────────────────────────────────────────────────────────

export async function enqueueDownload(input: {
  url: string;
  options: DownloadOptions;
  title?: string;
  thumbnail?: string | null;
  extractor?: string;
  channel?: string | null;
}): Promise<DownloadItem> {
  if (IS_WEB) {
    // Trigger direct download in browser
    const resp = await fetch(`${API_BASE}/api/download`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        url: input.url,
        formatId: input.options?.formatId || null,
        isAudioOnly: input.options?.mode === "audio",
      }),
    });
    if (!resp.ok) {
      const err = await resp.json().catch(() => ({ error: "Lỗi tải video" }));
      throw new Error(err.error || "Lỗi tải video");
    }
    const disposition = resp.headers.get("Content-Disposition") || "";
    const match = disposition.match(/filename="?([^";\n]+)"?/);
    const filename = match?.[1] || `${input.title || "video"}.mp4`;
    const blob = await resp.blob();

    // Auto-download via browser
    const blobUrl = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = blobUrl;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(blobUrl);

    return {
      shortId: Math.random().toString(36).slice(2, 8),
      request: {
        url: input.url,
        mode: input.options?.mode || "video",
        formatId: input.options?.formatId || null,
        saveFolder: "",
        subLangs: [],
        autoTranslateTo: null,
        onConflict: "ask",
        useAria2c: false,
        playlistAll: false,
      },
      title: input.title || "Video",
      thumbnail: input.thumbnail ?? null,
      channel: input.channel ?? null,
      extractor: input.extractor || detectExtractor(input.url),
      state: "completed",
      bytesDownloaded: 0,
      bytesTotal: null,
      speedBps: null,
      etaSec: null,
      attempt: 0,
      errorMessage: null,
      outputPath: filename,
      createdAt: new Date().toISOString(),
      finishedAt: new Date().toISOString(),
    } as unknown as DownloadItem;
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<DownloadItem>("enqueue_download", input);
}

export async function enqueueBatch(input: {
  urls: string[];
  options: DownloadOptions;
}): Promise<DownloadItem[]> {
  if (IS_WEB) {
    return Promise.all(input.urls.map((url) =>
      enqueueDownload({ url, options: input.options, extractor: detectExtractor(url) })
    ));
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<DownloadItem[]>("enqueue_batch", input);
}

export async function enqueuePlaylist(input: {
  playlistUrl: string;
  selected: string[];
  options: DownloadOptions;
  allWithYesPlaylist?: boolean;
}): Promise<DownloadItem[]> {
  if (IS_WEB) throw new Error("Playlist chưa hỗ trợ trên web.");
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<DownloadItem[]>("enqueue_playlist", input);
}

// ── Queue control ────────────────────────────────────────────────────────────

export async function pauseDownload(shortId: string): Promise<void> {
  if (IS_WEB) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<void>("pause_download", { shortId });
}

export async function resumeDownload(shortId: string): Promise<void> {
  if (IS_WEB) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<void>("resume_download", { shortId });
}

export async function cancelDownload(shortId: string): Promise<void> {
  if (IS_WEB) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<void>("cancel_download", { shortId });
}

// Thử lại TẤT CẢ mục lỗi một phát (sau khi thêm cookie / hết bị chặn).
// Trả về số video được đưa lại vào hàng đợi.
export async function retryAllFailed(): Promise<number> {
  if (IS_WEB) return 0;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<number>("retry_all_failed");
}

// Tạm dừng / tiếp tục TẤT CẢ. Trả số mục bị ảnh hưởng.
export async function pauseAllDownloads(): Promise<number> {
  if (IS_WEB) return 0;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<number>("pause_all_downloads");
}
export async function resumeAllDownloads(): Promise<number> {
  if (IS_WEB) return 0;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<number>("resume_all_downloads");
}

// Nút "Kiểm tra proxy" — trả chuỗi kết quả (Ok) hoặc ném lỗi (Err) tiếng Việt.
export async function testProxy(proxy: string): Promise<string> {
  if (IS_WEB) return Promise.reject("unavailable");
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<string>("test_proxy", { proxy });
}

// Nút "Vẫn tải video này" trên mục Bỏ qua — tải bất chấp danh sách đã-tải.
export async function forceDownload(shortId: string): Promise<DownloadItem> {
  if (IS_WEB) return Promise.reject("unavailable");
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<DownloadItem>("force_download", { shortId });
}

export async function retryDownload(shortId: string): Promise<DownloadItem> {
  if (IS_WEB) throw new Error("Retry chưa hỗ trợ trên web.");
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<DownloadItem>("retry_download", { shortId });
}

export async function resolveConflict(shortId: string, choice: ConflictChoice): Promise<void> {
  if (IS_WEB) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<void>("resolve_conflict", { shortId, choice });
}

export async function listQueue(): Promise<DownloadItem[]> {
  if (IS_WEB) return [];
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<DownloadItem[]>("list_queue");
}

export async function removeQueueItem(shortId: string): Promise<void> {
  if (IS_WEB) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<void>("remove_queue_item", { shortId });
}

export async function removeQueueGroup(folder: string): Promise<number> {
  if (IS_WEB) return 0;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<number>("remove_queue_group", { folder });
}

export async function undoRemoveGroup(): Promise<number> {
  if (IS_WEB) return 0;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<number>("undo_remove_group");
}

export async function pathExists(path: string): Promise<boolean> {
  if (IS_WEB) return false;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<boolean>("path_exists", { path });
}

// ── Settings ─────────────────────────────────────────────────────────────────

export async function getSettings(): Promise<Settings> {
  if (IS_WEB) return { defaultFolder: "", maxConcurrency: 3, theme: "system", language: "vi", clipboardWatcher: false, notifications: true, aria2cEnabled: false } as Settings;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<Settings>("get_settings");
}

export async function updateSettings(patch: SettingsPatch): Promise<Settings> {
  if (IS_WEB) return { ...(await getSettings()), ...patch };
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<Settings>("update_settings", { patch });
}

/**
 * Kiểm tra YouTube Data API key. Trả về `{ ok: true }` nếu key chạy được
 * (đèn xanh), hoặc `{ ok: false, error }` kèm lý do (đèn đỏ).
 */
export async function validateYoutubeApiKey(
  key: string,
): Promise<{ ok: boolean; error?: string }> {
  if (IS_WEB) return { ok: false, error: "Không khả dụng trên web" };
  const { invoke } = await import("@tauri-apps/api/core");
  try {
    await invoke("validate_youtube_api_key", { key });
    return { ok: true };
  } catch (e) {
    return { ok: false, error: String(e) };
  }
}

// ── Filesystem helpers ───────────────────────────────────────────────────────

export async function pickFolder(): Promise<string | null> {
  if (IS_WEB) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<string | null>("pick_folder");
}

export async function pickFile(): Promise<string | null> {
  if (IS_WEB) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<string | null>("pick_file");
}

export async function checkFolderWritable(path: string): Promise<boolean> {
  if (IS_WEB) return true;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<boolean>("check_folder_writable", { path });
}

export async function cleanJunkFiles(folder: string): Promise<number> {
  if (IS_WEB) return 0;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<number>("clean_junk_files", { folder });
}

// ── History ─────────────────────────────────────────────────────────────────

export async function listHistory(input: {
  query?: string | null;
  limit?: number;
  offset?: number;
}): Promise<HistoryEntry[]> {
  if (IS_WEB) return [];
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<HistoryEntry[]>("list_history", {
    query: input.query ?? null,
    limit: input.limit ?? 200,
    offset: input.offset ?? 0,
  });
}

export async function deleteHistoryEntry(shortId: string, deleteFile = false): Promise<void> {
  if (IS_WEB) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<void>("delete_history_entry", { shortId, deleteFile });
}

export async function deleteHistoryEntries(shortIds: string[], deleteFiles = false): Promise<number> {
  if (IS_WEB) return shortIds.length;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<number>("delete_history_entries", { shortIds, deleteFiles });
}

export async function setHistoryEdited(shortIds: string[], edited: boolean): Promise<number> {
  if (IS_WEB) return shortIds.length;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<number>("set_history_edited", { shortIds, edited });
}

export async function clearHistory(deleteFiles = false): Promise<number> {
  if (IS_WEB) return 0;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<number>("clear_history", { deleteFiles });
}

export async function redownloadFromHistory(shortId: string): Promise<DownloadItem> {
  if (IS_WEB) throw new Error("Chưa hỗ trợ trên web.");
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<DownloadItem>("redownload_from_history", { shortId });
}

// ── Clipboard watcher ────────────────────────────────────────────────────────

export async function setClipboardWatcher(enabled: boolean): Promise<void> {
  if (IS_WEB) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<void>("set_clipboard_watcher", { enabled });
}

// ── Shell helpers ───────────────────────────────────────────────────────────

export async function openInFolder(path: string): Promise<void> {
  if (IS_WEB) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<void>("open_in_folder", { path });
}

export async function openFile(path: string): Promise<void> {
  if (IS_WEB) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<void>("open_file", { path });
}

export async function findOutputFile(saveFolder: string, title: string): Promise<string | null> {
  if (IS_WEB) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<string | null>("find_output_file", { saveFolder, title });
}

export async function updateHistoryOutputPath(shortId: string, outputPath: string): Promise<void> {
  if (IS_WEB) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<void>("update_history_output_path", { shortId, outputPath });
}

export async function openUrl(url: string): Promise<void> {
  if (IS_WEB) { window.open(url, "_blank"); return; }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<void>("open_url", { url });
}

// ── Misc ────────────────────────────────────────────────────────────────────

export async function getSubtitleLangs(url: string): Promise<SubtitleTrack[]> {
  if (IS_WEB) return [];
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<SubtitleTrack[]>("get_subtitle_langs", { url });
}

export async function listExtractors(): Promise<ExtractorInfo[]> {
  if (IS_WEB) return [];
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<ExtractorInfo[]>("list_extractors");
}

// ── JS runtime (Deno) ─────────────────────────────────────────────────────────

export async function denoStatus(): Promise<string> {
  if (IS_WEB) return "unknown";
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<string>("deno_status");
}

export async function retryDeno(): Promise<void> {
  if (IS_WEB) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<void>("retry_deno");
}

// ── Nút "Sửa lỗi tải ngay" ────────────────────────────────────────────────────
// Chạy ngay quy trình tự phục hồi: update yt-dlp nightly + Deno + PO token.
// Trả về thông báo kết quả (nhiều dòng) để hiện cho user.
export async function fixDownloadEngine(): Promise<string> {
  if (IS_WEB) return "unavailable";
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<string>("fix_download_engine");
}

// ── Saved bookmarks ───────────────────────────────────────────────────────────

export async function listBookmarks(): Promise<Bookmark[]> {
  if (IS_WEB) return [];
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<Bookmark[]>("list_bookmarks");
}

export async function addBookmark(url: string, note?: string): Promise<Bookmark> {
  if (IS_WEB) throw new Error("Chưa hỗ trợ trên web.");
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<Bookmark>("add_bookmark", { url, note: note ?? "" });
}

export async function removeBookmark(id: string): Promise<void> {
  if (IS_WEB) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<void>("remove_bookmark", { id });
}

export async function updateBookmarkNote(id: string, note: string): Promise<void> {
  if (IS_WEB) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<void>("update_bookmark_note", { id, note });
}

// ── Auto-watch channels ───────────────────────────────────────────────────────

export async function listWatchedChannels(): Promise<WatchedChannel[]> {
  if (IS_WEB) return [];
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WatchedChannel[]>("list_watched_channels");
}

export async function addWatchedChannel(url: string, tab: string = "all"): Promise<WatchedChannel> {
  if (IS_WEB) throw new Error("Theo dõi kênh chưa hỗ trợ trên web. Dùng app desktop.");
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WatchedChannel>("add_watched_channel", { url, tab });
}

export async function removeWatchedChannel(id: string): Promise<void> {
  if (IS_WEB) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<void>("remove_watched_channel", { id });
}

export async function setWatchedDestDir(
  id: string,
  destDir: string | null,
): Promise<WatchedChannel | null> {
  if (IS_WEB) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WatchedChannel | null>("set_watched_dest_dir", { id, destDir });
}

export async function setWatchedEnabled(id: string, enabled: boolean): Promise<WatchedChannel | null> {
  if (IS_WEB) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WatchedChannel | null>("set_watched_enabled", { id, enabled });
}

/** Lưu HÀNG CHỜ LÀM (video đã tích chọn từ kho) của kênh theo dõi. */
export async function setWatchedPicked(
  id: string,
  picked: PickedVideo[],
): Promise<WatchedChannel | null> {
  if (IS_WEB) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WatchedChannel | null>("set_watched_picked", { id, picked });
}

/** Đặt số video tự tải tối đa mỗi ngày (1-3) cho kênh theo dõi. */
export async function setWatchedDailyLimit(
  id: string,
  limit: number,
): Promise<WatchedChannel | null> {
  if (IS_WEB) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WatchedChannel | null>("set_watched_daily_limit", { id, limit });
}

/** Đặt chế độ nguồn: "new" | "picked" (hàng chờ 🎯) | "auto" (tự vét kho). */
export async function setWatchedSourceMode(
  id: string,
  mode: "new" | "picked" | "auto",
): Promise<WatchedChannel | null> {
  if (IS_WEB) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WatchedChannel | null>("set_watched_source_mode", { id, mode });
}

/** Đổi loại video theo dõi/vét: "videos" (dài) | "shorts" | "all". */
export async function setWatchedTab(
  id: string,
  tab: "all" | "videos" | "shorts",
): Promise<WatchedChannel | null> {
  if (IS_WEB) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WatchedChannel | null>("set_watched_tab", { id, tab });
}

/** Đặt TÊN KÊNH ĐÍCH (kênh TikTok của user) — video tự về <gốc>\<tên>. */
export async function setWatchedTarget(
  id: string,
  target: string | null,
): Promise<WatchedChannel | null> {
  if (IS_WEB) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WatchedChannel | null>("set_watched_target", { id, target });
}

/** Gán nhóm/quốc gia cho kênh theo dõi (null/rỗng = bỏ nhóm). */
export async function setWatchedGroup(
  id: string,
  group: string | null,
): Promise<WatchedChannel | null> {
  if (IS_WEB) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WatchedChannel | null>("set_watched_group", { id, group });
}

export async function checkWatchedNow(): Promise<WatchedChannel[]> {
  if (IS_WEB) return [];
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WatchedChannel[]>("check_watched_now");
}

export async function setWatchedAutoDownload(id: string, auto: boolean): Promise<WatchedChannel | null> {
  if (IS_WEB) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WatchedChannel | null>("set_watched_auto_download", { id, auto });
}

export async function downloadPending(id: string, videoUrl: string): Promise<WatchedChannel | null> {
  if (IS_WEB) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WatchedChannel | null>("download_pending", { id, videoUrl });
}

export async function dismissPending(id: string, videoUrl: string): Promise<WatchedChannel | null> {
  if (IS_WEB) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<WatchedChannel | null>("dismiss_pending", { id, videoUrl });
}

// ── Douyin scraper ──────────────────────────────────────────────────────────

export async function scrapeDouyinChannel(url: string): Promise<DouyinPost[]> {
  if (IS_WEB) throw new Error("Kênh Douyin chưa hỗ trợ trên web. Dùng app desktop.");
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<DouyinPost[]>("scrape_douyin_channel", { url });
}

// ── Helpers ─────────────────────────────────────────────────────────────────

function detectExtractor(url: string): string {
  const u = url.toLowerCase();
  if (u.includes("youtube.com") || u.includes("youtu.be")) return "youtube";
  if (u.includes("tiktok.com")) return "tiktok";
  if (u.includes("douyin.com")) return "douyin";
  if (u.includes("instagram.com")) return "instagram";
  if (u.includes("facebook.com") || u.includes("fb.watch")) return "facebook";
  if (u.includes("twitter.com") || u.includes("x.com")) return "twitter";
  if (u.includes("reddit.com")) return "reddit";
  if (u.includes("pinterest.com")) return "pinterest";
  if (u.includes("threads.net")) return "threads";
  return "unknown";
}
