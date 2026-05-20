/**
 * Zustand store cho URL nhập trên HomePage và lựa chọn tải.
 *
 * Giữ trạng thái phiên cho:
 * - URL đang nhập + kết quả `validate_url`/`fetch_metadata`.
 * - Selection của user: mode (video/audio), formatId, save folder, sub langs,
 *   conflict policy, playlist all.
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

  mode: "video",
  formatId: null,
  saveFolder: "",
  subLangs: [],
  autoTranslateTo: null,
  onConflict: "ask",
  playlistAll: false,

  setUrl: (s) =>
    set({
      url: s,
      valid: null,
      extractor: null,
      metadata: null,
      error: null,
    }),
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
      set({ valid: v.valid, extractor: v.extractor, validating: false });
    } catch (e: unknown) {
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
    set({ fetching: true, error: null, metadata: null });
    try {
      const md = await cmd.fetchMetadata(url);
      set({ metadata: md, fetching: false, extractor: md.extractor });
    } catch (e: unknown) {
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
    }),
}));
