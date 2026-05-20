/**
 * Zustand store cho History (lịch sử tải).
 *
 * - `setQuery(q)` cập nhật chuỗi tìm kiếm và auto-refresh; backend search
 *   case-insensitive trên `title`/`url` (xem history_store.rs).
 * - `delete(shortId)` xoá khỏi DB và optimistic remove khỏi state.
 * - `redownload(shortId)` enqueue lại cùng URL/format với Short_ID mới; UI sẽ
 *   nhận DownloadItem mới qua sự kiện `queue://updated`.
 *
 * Tham chiếu: design.md — Stores (Zustand), Tauri Commands (history).
 */

import { create } from "zustand";
import type { HistoryEntry } from "@/types/models";
import * as cmd from "@/ipc/commands";

interface HistoryState {
  entries: HistoryEntry[];
  query: string;
  loading: boolean;
  setQuery: (q: string) => void;
  refresh: () => Promise<void>;
  redownload: (shortId: string) => Promise<void>;
  delete: (shortId: string, deleteFile?: boolean) => Promise<void>;
  clearAll: (deleteFiles?: boolean) => Promise<void>;
}

export const useHistoryStore = create<HistoryState>()((set, get) => ({
  entries: [],
  query: "",
  loading: false,
  setQuery: (q) => {
    set({ query: q });
    void get().refresh();
  },
  refresh: async () => {
    set({ loading: true });
    try {
      const entries = await cmd.listHistory({ query: get().query || null });
      set({ entries, loading: false });
    } catch {
      set({ loading: false });
    }
  },
  redownload: async (shortId) => {
    await cmd.redownloadFromHistory(shortId);
  },
  delete: async (shortId, deleteFile = false) => {
    await cmd.deleteHistoryEntry(shortId, deleteFile);
    set((s) => ({ entries: s.entries.filter((e) => e.shortId !== shortId) }));
  },
  clearAll: async (deleteFiles = false) => {
    await cmd.clearHistory(deleteFiles);
    set({ entries: [] });
  },
}));
