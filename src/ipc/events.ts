/**
 * Typed `listen` helpers cho Tauri events emit từ backend.
 *
 * Mỗi helper trả `Promise<UnlistenFn>` để caller có thể cleanup khi
 * unmount/teardown. Tên event là `string const` lưu trong `EVENTS` để cả TS
 * lẫn Rust (qua `src-tauri/src/events.rs`) cùng tham chiếu.
 *
 * Tham chiếu: design.md — Tauri Events, ipc/events.ts.
 */

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  DownloadItem,
  DownloadState,
  ProgressSnapshot,
} from "@/types/models";

// ──────────────────────────────────────────────────────────────────────────────
// Event name constants (phải khớp với `src-tauri/src/events.rs`)
// ──────────────────────────────────────────────────────────────────────────────

export const EVENTS = {
  DownloadProgress: "download://progress",
  DownloadState: "download://state",
  DownloadConflict: "download://conflict",
  DownloadCompleted: "download://completed",
  DownloadFailed: "download://failed",
  ClipboardDetected: "clipboard://detected",
  NotificationClicked: "notification://clicked",
  SettingsChanged: "settings://changed",
  QueueUpdated: "queue://updated",
} as const;

// ──────────────────────────────────────────────────────────────────────────────
// Payload shapes
// ──────────────────────────────────────────────────────────────────────────────

export interface ProgressEventPayload {
  shortId: string;
  progress: ProgressSnapshot;
}

export interface StateEventPayload {
  shortId: string;
  state: DownloadState;
  errorMessage: string | null;
  outputPath: string | null;
}

export interface ConflictEventPayload {
  shortId: string;
  suggestedPath: string;
  conflictingPath: string;
}

export interface CompletedEventPayload {
  shortId: string;
  outputPath: string;
  title: string;
}

export interface FailedEventPayload {
  shortId: string;
  reason: string;
}

export interface ClipboardEventPayload {
  url: string;
  extractor: string;
}

export interface NotificationClickedPayload {
  shortId: string;
}

export interface QueueUpdatedPayload {
  items: DownloadItem[];
}

// ──────────────────────────────────────────────────────────────────────────────
// Listen helpers
// ──────────────────────────────────────────────────────────────────────────────

export function onDownloadProgress(
  handler: (p: ProgressEventPayload) => void,
): Promise<UnlistenFn> {
  return listen<ProgressEventPayload>(EVENTS.DownloadProgress, (e) =>
    handler(e.payload),
  );
}

export function onDownloadState(
  handler: (p: StateEventPayload) => void,
): Promise<UnlistenFn> {
  return listen<StateEventPayload>(EVENTS.DownloadState, (e) =>
    handler(e.payload),
  );
}

export function onDownloadConflict(
  handler: (p: ConflictEventPayload) => void,
): Promise<UnlistenFn> {
  return listen<ConflictEventPayload>(EVENTS.DownloadConflict, (e) =>
    handler(e.payload),
  );
}

export function onDownloadCompleted(
  handler: (p: CompletedEventPayload) => void,
): Promise<UnlistenFn> {
  return listen<CompletedEventPayload>(EVENTS.DownloadCompleted, (e) =>
    handler(e.payload),
  );
}

export function onDownloadFailed(
  handler: (p: FailedEventPayload) => void,
): Promise<UnlistenFn> {
  return listen<FailedEventPayload>(EVENTS.DownloadFailed, (e) =>
    handler(e.payload),
  );
}

export function onClipboardDetected(
  handler: (p: ClipboardEventPayload) => void,
): Promise<UnlistenFn> {
  return listen<ClipboardEventPayload>(EVENTS.ClipboardDetected, (e) =>
    handler(e.payload),
  );
}

export function onNotificationClicked(
  handler: (p: NotificationClickedPayload) => void,
): Promise<UnlistenFn> {
  return listen<NotificationClickedPayload>(EVENTS.NotificationClicked, (e) =>
    handler(e.payload),
  );
}

export function onQueueUpdated(
  handler: (p: QueueUpdatedPayload) => void,
): Promise<UnlistenFn> {
  return listen<QueueUpdatedPayload>(EVENTS.QueueUpdated, (e) =>
    handler(e.payload),
  );
}
