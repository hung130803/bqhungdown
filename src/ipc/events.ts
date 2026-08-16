/**
 * Typed `listen` helpers cho Tauri events emit từ backend.
 * Trên web (không có Tauri), các listener trả về no-op.
 */

import { type UnlistenFn } from "@tauri-apps/api/event";
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
  DouyinScraperProgress: "bqd-douyin-scraper-progress",
  DouyinScraperStarted: "bqd-douyin-scraper-started",
  /** Cảnh báo lượt quét Douyin bị thiếu/bị chặn (vẫn có kết quả một phần). */
  DouyinScraperNote: "bqd-douyin-scraper-note",
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

export interface DouyinScraperProgressPayload {
  count: number;
}

export interface DouyinScraperStartedPayload {
  label: string;
  secUid: string;
}

export interface DouyinScraperNotePayload {
  message: string;
}

// ──────────────────────────────────────────────────────────────────────────────
// Listen helpers — web-safe: falls back to no-op on non-Tauri browsers
// ──────────────────────────────────────────────────────────────────────────────

const noopUnlisten = () => {};

async function tauriListen<T>(
  _event: string,
  _handler: (payload: T) => void,
): Promise<UnlistenFn> {
  const { listen } = await import("@tauri-apps/api/event");
  try {
    return await listen<T>(_event, (e) => _handler(e.payload));
  } catch {
    return noopUnlisten;
  }
}

export function onDownloadProgress(
  handler: (p: ProgressEventPayload) => void,
): Promise<UnlistenFn> {
  return tauriListen(EVENTS.DownloadProgress, handler);
}

export function onDownloadState(
  handler: (p: StateEventPayload) => void,
): Promise<UnlistenFn> {
  return tauriListen(EVENTS.DownloadState, handler);
}

export function onDownloadConflict(
  handler: (p: ConflictEventPayload) => void,
): Promise<UnlistenFn> {
  return tauriListen(EVENTS.DownloadConflict, handler);
}

export function onDownloadCompleted(
  handler: (p: CompletedEventPayload) => void,
): Promise<UnlistenFn> {
  return tauriListen(EVENTS.DownloadCompleted, handler);
}

export function onDownloadFailed(
  handler: (p: FailedEventPayload) => void,
): Promise<UnlistenFn> {
  return tauriListen(EVENTS.DownloadFailed, handler);
}

export function onClipboardDetected(
  handler: (p: ClipboardEventPayload) => void,
): Promise<UnlistenFn> {
  return tauriListen(EVENTS.ClipboardDetected, handler);
}

export function onNotificationClicked(
  handler: (p: NotificationClickedPayload) => void,
): Promise<UnlistenFn> {
  return tauriListen(EVENTS.NotificationClicked, handler);
}

export function onQueueUpdated(
  handler: (p: QueueUpdatedPayload) => void,
): Promise<UnlistenFn> {
  return tauriListen(EVENTS.QueueUpdated, handler);
}

export function onDouyinScraperProgress(
  handler: (p: DouyinScraperProgressPayload) => void,
): Promise<UnlistenFn> {
  return tauriListen(EVENTS.DouyinScraperProgress, handler);
}

export function onDouyinScraperStarted(
  handler: (p: DouyinScraperStartedPayload) => void,
): Promise<UnlistenFn> {
  return tauriListen(EVENTS.DouyinScraperStarted, handler);
}

export function onDouyinScraperNote(
  handler: (p: DouyinScraperNotePayload) => void,
): Promise<UnlistenFn> {
  return tauriListen(EVENTS.DouyinScraperNote, handler);
}

// ──────────────────────────────────────────────────────────────────────────────
// React hooks — auto-manage subscription/unsubscription lifecycle
// ──────────────────────────────────────────────────────────────────────────────

import { useEffect } from "react";

export function useEventListener<T>(
  setup: (handler: (payload: T) => void) => Promise<UnlistenFn>,
  handler: (payload: T) => void,
): void {
  useEffect(() => {
    let cancelled = false;
    setup((payload) => {
      if (!cancelled) handler(payload);
    }).then((unlisten) => {
      if (!cancelled) unlisten();
    });
    return () => {
      cancelled = true;
    };
  }, [setup, handler]);
}

export function useDownloadProgress(handler: (p: ProgressEventPayload) => void): void {
  useEventListener(onDownloadProgress, handler);
}

export function useDownloadState(handler: (p: StateEventPayload) => void): void {
  useEventListener(onDownloadState, handler);
}

export function useDownloadConflict(handler: (p: ConflictEventPayload) => void): void {
  useEventListener(onDownloadConflict, handler);
}

export function useDownloadCompleted(handler: (p: CompletedEventPayload) => void): void {
  useEventListener(onDownloadCompleted, handler);
}

export function useDownloadFailed(handler: (p: FailedEventPayload) => void): void {
  useEventListener(onDownloadFailed, handler);
}

export function useClipboardDetected(handler: (p: ClipboardEventPayload) => void): void {
  useEventListener(onClipboardDetected, handler);
}

export function useNotificationClicked(handler: (p: NotificationClickedPayload) => void): void {
  useEventListener(onNotificationClicked, handler);
}

export function useQueueUpdated(handler: (p: QueueUpdatedPayload) => void): void {
  useEventListener(onQueueUpdated, handler);
}
