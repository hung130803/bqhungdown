/**
 * Zustand store cho hàng đợi tải (Queue).
 *
 * - `applyProgress(shortId, snapshot)` được gọi từ listener `download://progress`
 *   để patch thông tin tốc độ/ETA mà không reorder list.
 * - `applyState(shortId, state, errorMessage, outputPath)` xử lý
 *   `download://state`; tự gắn `finishedAt` cho các terminal state.
 * - Các method `pause/resume/cancel/retry` ủy quyền sang backend; UI sẽ nhận
 *   state cập nhật qua event listener thay vì optimistic update.
 *
 * Tham chiếu: design.md — Stores (Zustand), Queue_Manager.
 */

import { create } from "zustand";
import type {
  DownloadItem,
  DownloadState,
  ProgressSnapshot,
} from "@/types/models";
import * as cmd from "@/ipc/commands";

const TERMINAL_STATES: DownloadState[] = [
  "completed",
  "failed",
  "cancelled",
  "skipped",
];

interface QueueState {
  items: DownloadItem[];
  byId: (id: string) => DownloadItem | undefined;
  setAll: (items: DownloadItem[]) => void;
  upsert: (item: DownloadItem) => void;
  applyProgress: (shortId: string, progress: ProgressSnapshot) => void;
  applyState: (
    shortId: string,
    state: DownloadState,
    errorMessage: string | null,
    outputPath: string | null,
  ) => void;
  remove: (shortId: string) => void;
  clearTerminal: () => void;
  refresh: () => Promise<void>;
  pause: (id: string) => Promise<void>;
  resume: (id: string) => Promise<void>;
  cancel: (id: string) => Promise<void>;
  retry: (id: string) => Promise<void>;
}

export const useQueueStore = create<QueueState>()((set, get) => ({
  items: [],
  byId: (id) => get().items.find((i) => i.shortId === id),
  setAll: (items) => set({ items }),
  upsert: (item) =>
    set((state) => {
      const idx = state.items.findIndex((i) => i.shortId === item.shortId);
      if (idx === -1) return { items: [item, ...state.items] };
      const next = state.items.slice();
      next[idx] = item;
      return { items: next };
    }),
  applyProgress: (shortId, progress) =>
    set((state) => ({
      items: state.items.map((i) =>
        i.shortId === shortId
          ? {
              ...i,
              bytesDownloaded: progress.bytesDownloaded,
              bytesTotal: progress.bytesTotal,
              speedBps: progress.speedBps,
              etaSec: progress.etaSec,
            }
          : i,
      ),
    })),
  applyState: (shortId, state, errorMessage, outputPath) =>
    set((s) => ({
      items: s.items.map((i) =>
        i.shortId === shortId
          ? {
              ...i,
              state,
              errorMessage,
              outputPath,
              finishedAt: TERMINAL_STATES.includes(state)
                ? new Date().toISOString()
                : i.finishedAt,
            }
          : i,
      ),
    })),
  remove: (shortId) =>
    set((s) => ({ items: s.items.filter((i) => i.shortId !== shortId) })),
  clearTerminal: () =>
    set((s) => ({ items: s.items.filter((i) => !TERMINAL_STATES.includes(i.state)) })),
  refresh: async () => {
    const items = await cmd.listQueue();
    set({ items });
  },
  pause: async (id) => {
    await cmd.pauseDownload(id);
  },
  resume: async (id) => {
    await cmd.resumeDownload(id);
  },
  cancel: async (id) => {
    await cmd.cancelDownload(id);
  },
  retry: async (id) => {
    await cmd.retryDownload(id);
  },
}));
