/**
 * Zustand store cho hàng đợi tải (Queue).
 *
 * - `applyProgress(shortId, snapshot)` được gọi từ listener `download://progress`
 *   để patch thông tin tốc độ/ETA mà không reorder list.
 * - `applyState(shortId, state, errorMessage, outputPath)` xử lý
 *   `download://state`; tự gắn `finishedAt` cho các terminal state.
 * - Các method `pause/resume/cancel/retry` dùng optimistic update: UI cập
 *   nhật ngay, rollback nếu backend lỗi.
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
  forceDownload: (id: string) => Promise<void>;
}

// GOM TIẾN ĐỘ: backend gửi mỗi mục 4 lần/giây; 5 mục tải song song = 20 lần
// dựng lại MỌI trang đang nghe hàng đợi (Theo dõi kênh 155 thẻ) mỗi giây —
// anh Hùng 16/11/2026: chạy dây chuyền là máy đơ. Nay gom lại, mỗi
// PROGRESS_FLUSH_MS mới vẽ 1 lần (chỉ con số trên thanh tiến trình chậm hơn,
// tải không đổi). Trạng thái (xong/lỗi/huỷ) vẫn vẽ NGAY.
const PROGRESS_FLUSH_MS = 1000;
const pendingProgress = new Map<string, ProgressSnapshot>();
let progressTimer: ReturnType<typeof setTimeout> | undefined;

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
  applyProgress: (shortId, progress) => {
    pendingProgress.set(shortId, progress);
    // Vẽ ngay rồi khoá PROGRESS_FLUSH_MS; trong lúc khoá chỉ gom. Hết khoá mà
    // còn gom được thì vẽ tiếp + khoá tiếp, không còn thì nhả khoá.
    const flush = () => {
      if (pendingProgress.size === 0) {
        progressTimer = undefined;
        return;
      }
      progressTimer = setTimeout(flush, PROGRESS_FLUSH_MS);
      const batch = new Map(pendingProgress);
      pendingProgress.clear();
      set((state) => ({
        items: state.items.map((i) => {
          const p = batch.get(i.shortId);
          return p
            ? {
                ...i,
                bytesDownloaded: p.bytesDownloaded,
                bytesTotal: p.bytesTotal,
                speedBps: p.speedBps,
                etaSec: p.etaSec,
              }
            : i;
        }),
      }));
    };
    if (progressTimer === undefined) flush();
  },
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
  // CHỈ xoá mục ĐÃ TẢI XONG (completed). KHÔNG đụng mục lỗi/huỷ — để user còn
  // "Thử lại" (trước đây gộp cả lỗi vào "đã xong" → bấm nhầm mất video lỗi).
  clearTerminal: () =>
    set((s) => ({ items: s.items.filter((i) => i.state !== "completed") })),
  refresh: async () => {
    const items = await cmd.listQueue();
    set({ items });
  },

  /** Optimistic: update UI immediately, rollback on backend failure. */
  pause: async (id) => {
    const prev = get().items.find((i) => i.shortId === id);
    if (!prev || prev.state !== "downloading") return;
    set((s) => ({
      items: s.items.map((i) =>
        i.shortId === id ? { ...i, state: "paused" } : i,
      ),
    }));
    try {
      await cmd.pauseDownload(id);
    } catch {
      set((s) => ({
        items: s.items.map((i) => (i.shortId === id ? { ...i, state: prev.state } : i)),
      }));
    }
  },

  /** Optimistic: update UI immediately, rollback on backend failure. */
  resume: async (id) => {
    const prev = get().items.find((i) => i.shortId === id);
    if (!prev || prev.state !== "paused") return;
    set((s) => ({
      items: s.items.map((i) =>
        i.shortId === id ? { ...i, state: "downloading" } : i,
      ),
    }));
    try {
      await cmd.resumeDownload(id);
    } catch {
      set((s) => ({
        items: s.items.map((i) => (i.shortId === id ? { ...i, state: prev.state } : i)),
      }));
    }
  },

  /** Optimistic: update UI immediately, rollback on backend failure. */
  cancel: async (id) => {
    const prev = get().items.find((i) => i.shortId === id);
    if (!prev || TERMINAL_STATES.includes(prev.state)) return;
    set((s) => ({
      items: s.items.map((i) =>
        i.shortId === id ? { ...i, state: "cancelled" } : i,
      ),
    }));
    try {
      await cmd.cancelDownload(id);
    } catch {
      set((s) => ({
        items: s.items.map((i) => (i.shortId === id ? { ...i, state: prev.state } : i)),
      }));
    }
  },

  /** Optimistic: update UI immediately, rollback on backend failure. */
  retry: async (id) => {
    const prev = get().items.find((i) => i.shortId === id);
    if (!prev || !TERMINAL_STATES.includes(prev.state)) return;
    set((s) => ({
      items: s.items.map((i) =>
        i.shortId === id ? { ...i, state: "queued", errorMessage: null } : i,
      ),
    }));
    try {
      await cmd.retryDownload(id);
    } catch {
      set((s) => ({
        items: s.items.map((i) => (i.shortId === id ? { ...i, state: prev.state } : i)),
      }));
    }
  },

  /** Nút "Vẫn tải video này" trên mục Bỏ qua — tải bất chấp danh sách đã-tải. */
  forceDownload: async (id) => {
    const prev = get().items.find((i) => i.shortId === id);
    if (!prev) return;
    set((s) => ({
      items: s.items.map((i) =>
        i.shortId === id ? { ...i, state: "queued", errorMessage: null } : i,
      ),
    }));
    try {
      await cmd.forceDownload(id);
    } catch {
      set((s) => ({
        items: s.items.map((i) => (i.shortId === id ? { ...i, state: prev.state } : i)),
      }));
    }
  },
}));
