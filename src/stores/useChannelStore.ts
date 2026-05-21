/**
 * Persistent state for the "Tải kênh" tab so the user can navigate to
 * other tabs and come back without losing the fetched video list / filters /
 * selection.
 */

import { create } from "zustand";
import type { ChannelInfo, ChannelVideo } from "@/types/models";

interface ChannelState {
  url: string;
  setUrl: (u: string) => void;

  loading: boolean;
  setLoading: (b: boolean) => void;

  /** Khi nào fetch hiện tại được start (epoch ms). Dùng để hiển thị
   *  "Đang lấy 23s…" và giữ nguyên giá trị khi user chuyển tab rồi quay lại. */
  fetchStartedAt: number | null;
  setFetchStartedAt: (t: number | null) => void;

  errorMsg: string | null;
  setError: (m: string | null) => void;

  /** Khi bật, fetch chậm hơn nhiều nhưng có view_count cho YouTube channel.
   *  Mặc định tắt vì kênh lớn (>50 video) sẽ rất lâu. */
  detailed: boolean;
  setDetailed: (b: boolean) => void;

  /** Loại video YouTube cần lấy. "all" = video dài + shorts gộp lại. */
  channelTab: "videos" | "shorts" | "streams" | "all";
  setChannelTab: (t: "videos" | "shorts" | "streams" | "all") => void;

  /** Sub-tab đang active trong khu vực Hàng loạt / Kênh. Lưu lại để khi
   *  user chuyển sang trang khác rồi quay lại không bị reset về Hàng loạt. */
  subTab: "batch" | "channel";
  setSubTab: (t: "batch" | "channel") => void;

  info: ChannelInfo | null;
  videos: ChannelVideo[];
  setResult: (info: ChannelInfo, videos: ChannelVideo[]) => void;
  resetResult: () => void;

  /** Selection: stores URLs of UNTICKED videos. Empty set = everything ticked.
   *  Accepts either a new Set or an updater function (React-style). */
  excluded: Set<string>;
  setExcluded: (next: Set<string> | ((prev: Set<string>) => Set<string>)) => void;
  toggleExcluded: (url: string) => void;
}

export const useChannelStore = create<ChannelState>()((set, get) => ({
  url: "",
  setUrl: (u) => set({ url: u }),

  loading: false,
  setLoading: (b) => set({ loading: b }),

  fetchStartedAt: null,
  setFetchStartedAt: (t) => set({ fetchStartedAt: t }),

  errorMsg: null,
  setError: (m) => set({ errorMsg: m }),

  detailed: false,
  setDetailed: (b) => set({ detailed: b }),

  channelTab: "all",
  setChannelTab: (t) => set({ channelTab: t }),

  subTab: "batch",
  setSubTab: (t) => set({ subTab: t }),

  info: null,
  videos: [],
  setResult: (info, videos) => set({ info, videos, excluded: new Set() }),
  resetResult: () => set({ info: null, videos: [], excluded: new Set() }),

  excluded: new Set(),
  setExcluded: (next) => {
    if (typeof next === "function") {
      set({ excluded: next(get().excluded) });
    } else {
      set({ excluded: next });
    }
  },
  toggleExcluded: (url) => {
    const cur = get().excluded;
    const next = new Set(cur);
    if (next.has(url)) next.delete(url);
    else next.add(url);
    set({ excluded: next });
  },
}));
