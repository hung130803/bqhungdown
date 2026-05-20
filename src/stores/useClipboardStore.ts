/**
 * Zustand store cho banner clipboard.
 *
 * Lưu URL được phát hiện gần nhất từ event `clipboard://detected` và set các
 * URL đã bị user "dismiss" trong session để tránh hiện lại liên tục.
 *
 * Tham chiếu: design.md — Stores (Zustand), ClipboardBanner.
 */

import { create } from "zustand";

interface ClipboardState {
  detectedUrl: string | null;
  detectedExtractor: string | null;
  dismissed: Set<string>;
  setDetected: (url: string | null, extractor: string | null) => void;
  dismiss: (url: string) => void;
  isDismissed: (url: string) => boolean;
  clear: () => void;
}

export const useClipboardStore = create<ClipboardState>()((set, get) => ({
  detectedUrl: null,
  detectedExtractor: null,
  dismissed: new Set<string>(),
  setDetected: (url, extractor) =>
    set({ detectedUrl: url, detectedExtractor: extractor }),
  dismiss: (url) =>
    set((s) => {
      const next = new Set(s.dismissed);
      next.add(url);
      return {
        dismissed: next,
        detectedUrl: s.detectedUrl === url ? null : s.detectedUrl,
      };
    }),
  isDismissed: (url) => get().dismissed.has(url),
  clear: () => set({ detectedUrl: null, detectedExtractor: null }),
}));
