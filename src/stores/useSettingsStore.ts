/**
 * Zustand store cho Settings.
 *
 * - `hydrate(settings)` được gọi sau khi `app_bootstrap` trả về snapshot ban đầu
 *   (xem design.md — Stores).
 * - `refresh()` đọc lại Settings qua command `get_settings`.
 * - `update(patch)` push thay đổi qua `update_settings`; backend tự debounce
 *   persist và emit `settings://changed` để các store khác đồng bộ.
 *
 * Tham chiếu: design.md — Stores (Zustand), settings_store.rs.
 */

import { create } from "zustand";
import type { Settings, SettingsPatch } from "@/types/models";
import * as cmd from "@/ipc/commands";
import { formatError } from "@/lib/error";

interface SettingsState {
  settings: Settings | null;
  loading: boolean;
  error: string | null;
  hydrate: (settings: Settings) => void;
  refresh: () => Promise<void>;
  update: (patch: SettingsPatch) => Promise<Settings>;
}

export const useSettingsStore = create<SettingsState>()((set) => ({
  settings: null,
  loading: false,
  error: null,
  hydrate: (settings) => set({ settings, loading: false, error: null }),
  refresh: async () => {
    set({ loading: true, error: null });
    try {
      const s = await cmd.getSettings();
      set({ settings: s, loading: false });
    } catch (e: unknown) {
      set({ loading: false, error: formatError(e) });
    }
  },
  update: async (patch) => {
    const next = await cmd.updateSettings(patch);
    set({ settings: next });
    return next;
  },
}));
