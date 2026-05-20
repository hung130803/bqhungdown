/**
 * Typed wrappers quanh `@tauri-apps/api/core` `invoke`.
 *
 * Mọi command đều trả `Promise<T>`. Nếu backend trả `Err(AppError)`, Tauri sẽ
 * throw — caller `try/catch` và parse `AppErrorPayload` (xem `@/types/models`).
 *
 * Tên command tuân theo Tauri convention (snake_case tiếng Anh) và phải khớp
 * với `#[tauri::command]` bên Rust (`src-tauri/src/commands/*`).
 *
 * Tham chiếu: design.md — IPC Commands (Tauri).
 */

import { invoke } from "@tauri-apps/api/core";
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
} from "@/types/models";

// ──────────────────────────────────────────────────────────────────────────────
// Bootstrap
// ──────────────────────────────────────────────────────────────────────────────

export async function appBootstrap(): Promise<BootstrapPayload> {
  return invoke<BootstrapPayload>("app_bootstrap");
}

// ──────────────────────────────────────────────────────────────────────────────
// URL / metadata
// ──────────────────────────────────────────────────────────────────────────────

export async function validateUrl(url: string): Promise<UrlValidation> {
  return invoke<UrlValidation>("validate_url", { url });
}

export async function fetchMetadata(url: string): Promise<VideoMetadata> {
  return invoke<VideoMetadata>("fetch_metadata", { url });
}

// ──────────────────────────────────────────────────────────────────────────────
// Enqueue
// ──────────────────────────────────────────────────────────────────────────────

export async function enqueueDownload(input: {
  url: string;
  options: DownloadOptions;
  title?: string;
  thumbnail?: string | null;
  extractor?: string;
  channel?: string | null;
}): Promise<DownloadItem> {
  return invoke<DownloadItem>("enqueue_download", input);
}

export async function enqueueBatch(input: {
  urls: string[];
  options: DownloadOptions;
}): Promise<DownloadItem[]> {
  return invoke<DownloadItem[]>("enqueue_batch", input);
}

export async function enqueuePlaylist(input: {
  playlistUrl: string;
  selected: string[];
  options: DownloadOptions;
  allWithYesPlaylist?: boolean;
}): Promise<DownloadItem[]> {
  return invoke<DownloadItem[]>("enqueue_playlist", input);
}

// ──────────────────────────────────────────────────────────────────────────────
// Queue control
// ──────────────────────────────────────────────────────────────────────────────

export async function pauseDownload(shortId: string): Promise<void> {
  await invoke<void>("pause_download", { shortId });
}

export async function resumeDownload(shortId: string): Promise<void> {
  await invoke<void>("resume_download", { shortId });
}

export async function cancelDownload(shortId: string): Promise<void> {
  await invoke<void>("cancel_download", { shortId });
}

export async function retryDownload(shortId: string): Promise<DownloadItem> {
  return invoke<DownloadItem>("retry_download", { shortId });
}

export async function resolveConflict(
  shortId: string,
  choice: ConflictChoice,
): Promise<void> {
  await invoke<void>("resolve_conflict", { shortId, choice });
}

export async function listQueue(): Promise<DownloadItem[]> {
  return invoke<DownloadItem[]>("list_queue");
}

// ──────────────────────────────────────────────────────────────────────────────
// Settings
// ──────────────────────────────────────────────────────────────────────────────

export async function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export async function updateSettings(patch: SettingsPatch): Promise<Settings> {
  return invoke<Settings>("update_settings", { patch });
}

// ──────────────────────────────────────────────────────────────────────────────
// Filesystem helpers
// ──────────────────────────────────────────────────────────────────────────────

export async function pickFolder(): Promise<string | null> {
  return invoke<string | null>("pick_folder");
}

export async function pickFile(): Promise<string | null> {
  return invoke<string | null>("pick_file");
}

export async function checkFolderWritable(path: string): Promise<boolean> {
  return invoke<boolean>("check_folder_writable", { path });
}

// ──────────────────────────────────────────────────────────────────────────────
// History
// ──────────────────────────────────────────────────────────────────────────────

export async function listHistory(input: {
  query?: string | null;
  limit?: number;
  offset?: number;
}): Promise<HistoryEntry[]> {
  return invoke<HistoryEntry[]>("list_history", {
    query: input.query ?? null,
    limit: input.limit ?? 200,
    offset: input.offset ?? 0,
  });
}

export async function deleteHistoryEntry(
  shortId: string,
  deleteFile: boolean = false,
): Promise<void> {
  await invoke<void>("delete_history_entry", { shortId, deleteFile });
}

export async function clearHistory(deleteFiles: boolean = false): Promise<number> {
  return invoke<number>("clear_history", { deleteFiles });
}

export async function redownloadFromHistory(
  shortId: string,
): Promise<DownloadItem> {
  return invoke<DownloadItem>("redownload_from_history", { shortId });
}

// ──────────────────────────────────────────────────────────────────────────────
// Clipboard watcher
// ──────────────────────────────────────────────────────────────────────────────

export async function setClipboardWatcher(enabled: boolean): Promise<void> {
  await invoke<void>("set_clipboard_watcher", { enabled });
}

// ──────────────────────────────────────────────────────────────────────────────
// Shell-out helpers
// ──────────────────────────────────────────────────────────────────────────────

export async function openInFolder(path: string): Promise<void> {
  await invoke<void>("open_in_folder", { path });
}

export async function openFile(path: string): Promise<void> {
  await invoke<void>("open_file", { path });
}

export async function findOutputFile(saveFolder: string, title: string): Promise<string | null> {
  return invoke<string | null>("find_output_file", { saveFolder, title });
}

export async function updateHistoryOutputPath(shortId: string, outputPath: string): Promise<void> {
  await invoke<void>("update_history_output_path", { shortId, outputPath });
}

export async function openUrl(url: string): Promise<void> {
  await invoke<void>("open_url", { url });
}

// ──────────────────────────────────────────────────────────────────────────────
// Misc
// ──────────────────────────────────────────────────────────────────────────────

export async function getSubtitleLangs(url: string): Promise<SubtitleTrack[]> {
  return invoke<SubtitleTrack[]>("get_subtitle_langs", { url });
}

export async function listExtractors(): Promise<ExtractorInfo[]> {
  return invoke<ExtractorInfo[]>("list_extractors");
}
