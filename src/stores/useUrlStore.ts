/**
 * Zustand store cho URL nhập trên HomePage và lựa chọn tải.
 *
 * Giữ trạng thái phiên cho:
 * - URL đang nhập + kết quả `validate_url`/`fetch_metadata`.
 * - Selection của user: mode (video/audio), formatId, save folder, sub langs,
 *   conflict policy, playlist all.
 *
 * Anti-race: mỗi `setUrl` tăng `fetchGen`. `fetchMetadata` check gen trước
 * khi ghi kết quả — nếu gen đã thay đổi thì bỏ qua kết quả cũ.
 *
 * Tham chiếu: design.md — Stores (Zustand), HomePage.
 */

import { create } from "zustand";
import type {
  VideoMetadata,
  DownloadMode,
  ConflictPolicy,
} from "@/types/models";
import * as cmd from "@/ipc/commands";
import { formatError } from "@/lib/error";

interface UrlState {
  url: string;
  validating: boolean;
  valid: boolean | null;
  extractor: string | null;
  metadata: VideoMetadata | null;
  fetching: boolean;
  error: string | null;

  // Anti-race: tăng mỗi khi setUrl được gọi.
  // fetchMetadata so sánh gen trước khi ghi kết quả.
  fetchGen: number;

  // Selection
  mode: DownloadMode;
  formatId: string | null;
  saveFolder: string;
  subLangs: string[];
  autoTranslateTo: string | null;
  onConflict: ConflictPolicy;
  playlistAll: boolean;

  setUrl: (s: string) => void;
  setMode: (m: DownloadMode) => void;
  setFormatId: (id: string | null) => void;
  setSaveFolder: (f: string) => void;
  setSubLangs: (langs: string[]) => void;
  setAutoTranslateTo: (lang: string | null) => void;
  setOnConflict: (p: ConflictPolicy) => void;
  setPlaylistAll: (b: boolean) => void;

  validate: () => Promise<void>;
  fetchMetadata: () => Promise<void>;
  reset: () => void;
}

export const useUrlStore = create<UrlState>()((set, get) => ({
  url: "",
  validating: false,
  valid: null,
  extractor: null,
  metadata: null,
  fetching: false,
  error: null,
  fetchGen: 0,

  mode: "video",
  formatId: null,
  saveFolder: "",
  subLangs: [],
  autoTranslateTo: null,
  onConflict: "ask",
  playlistAll: false,

  setUrl: (s) =>
    set((state) => ({
      url: s,
      valid: null,
      extractor: null,
      metadata: null,
      error: null,
      // Tăng gen để invalidate mọi fetch đang chạy cho URL cũ.
      fetchGen: state.fetchGen + 1,
    })),
  setMode: (m) => set({ mode: m }),
  setFormatId: (id) => set({ formatId: id }),
  setSaveFolder: (f) => set({ saveFolder: f }),
  setSubLangs: (l) => set({ subLangs: l }),
  setAutoTranslateTo: (l) => set({ autoTranslateTo: l }),
  setOnConflict: (p) => set({ onConflict: p }),
  setPlaylistAll: (b) => set({ playlistAll: b }),

  validate: async () => {
    const url = get().url.trim();
    if (!url) {
      set({ valid: null, extractor: null });
      return;
    }
    set({ validating: true });
    try {
      const v = await cmd.validateUrl(url);
      // Bỏ qua kết quả nếu URL đã thay đổi trong lúc validate.
      if (get().url.trim() !== url) return;
      set({ valid: v.valid, extractor: v.extractor, validating: false });
    } catch (e: unknown) {
      if (get().url.trim() !== url) return;
      set({
        valid: false,
        extractor: null,
        validating: false,
        error: formatError(e),
      });
    }
  },

  fetchMetadata: async () => {
    const url = get().url.trim();
    if (!url) return;
    const gen = get().fetchGen;
    set({ fetching: true, error: null, metadata: null });
    try {
      const md = await cmd.fetchMetadata(url);
      // Bỏ qua kết quả nếu setUrl đã được gọi lại (URL khác rồi).
      if (get().fetchGen !== gen) return;
      set({ metadata: md, fetching: false, extractor: md.extractor });
    } catch (e: unknown) {
      if (get().fetchGen !== gen) return;
      set({ fetching: false, error: formatError(e) });
    }
  },

  reset: () =>
    set({
      url: "",
      valid: null,
      extractor: null,
      metadata: null,
      error: null,
      formatId: null,
      subLangs: [],
      autoTranslateTo: null,
      playlistAll: false,
      fetchGen: 0,
    }),
}));
