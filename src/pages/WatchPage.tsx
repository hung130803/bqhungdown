import { useEffect, useState } from "react";
import * as cmd from "@/ipc/commands";
import { useSettingsStore } from "@/stores/useSettingsStore";
import type { WatchedChannel } from "@/types/models";

/**
 * "Theo dõi kênh" — auto-watch list. The backend monitor periodically checks
 * each enabled channel and auto-enqueues new uploads (baseline-seeded so it
 * never grabs the backlog). This page manages the list + interval + manual check.
 */
export function WatchPage() {
  const settings = useSettingsStore((s) => s.settings);
  const updateSettings = useSettingsStore((s) => s.update);

  const [channels, setChannels] = useState<WatchedChannel[]>([]);
  const [url, setUrl] = useState("");
  const [tab, setTab] = useState("all");
  const [adding, setAdding] = useState(false);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reload = async () => {
    try {
      setChannels(await cmd.listWatchedChannels());
    } catch (e) {
      setError(formatErr(e));
    }
  };

  useEffect(() => {
    void reload();
  }, []);

  const add = async () => {
    const u = url.trim();
    if (!u || adding) return;
    setAdding(true);
    setError(null);
    try {
      await cmd.addWatchedChannel(u, tab);
      setUrl("");
      await reload();
    } catch (e) {
      setError(formatErr(e));
    } finally {
      setAdding(false);
    }
  };

  const remove = async (id: string) => {
    try {
      await cmd.removeWatchedChannel(id);
      await reload();
    } catch (e) {
      setError(formatErr(e));
    }
  };

  const toggle = async (id: string, enabled: boolean) => {
    try {
      await cmd.setWatchedEnabled(id, enabled);
      await reload();
    } catch (e) {
      setError(formatErr(e));
    }
  };

  const checkNow = async () => {
    if (checking) return;
    setChecking(true);
    setError(null);
    try {
      setChannels(await cmd.checkWatchedNow());
    } catch (e) {
      setError(formatErr(e));
    } finally {
      setChecking(false);
    }
  };

  const interval = settings?.watchIntervalMin ?? 60;

  return (
    <div className="max-w-2xl mx-auto space-y-5">
      <div>
        <h2 className="text-xl font-medium text-fg">Theo dõi kênh</h2>
        <p className="text-sm text-muted mt-1">
          Thêm kênh vào đây — app sẽ tự kiểm tra định kỳ và tải video mới đăng (không tải lại video cũ).
          Khi mới thêm, app chỉ ghi nhận video hiện có làm mốc, KHÔNG tải hết kho cũ.
        </p>
      </div>

      {/* Add channel */}
      <div className="space-y-2 p-3 rounded-lg border border-border bg-surface">
        <div className="flex gap-2 flex-wrap">
          <input
            type="url"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") void add(); }}
            placeholder="https://www.youtube.com/@TenKenh hoặc https://www.tiktok.com/@user"
            className="flex-1 min-w-[240px] px-3 py-2 rounded-md bg-surface-2 border border-border text-fg placeholder:text-muted text-sm"
          />
          <select
            value={tab}
            onChange={(e) => setTab(e.target.value)}
            className="px-2 py-2 rounded-md bg-surface-2 border border-border text-fg text-sm"
            title="Loại video cần theo dõi"
          >
            <option value="all">Tất cả</option>
            <option value="videos">Video dài</option>
            <option value="shorts">Shorts</option>
          </select>
          <button
            onClick={() => void add()}
            disabled={!url.trim() || adding}
            className="px-4 py-2 rounded-md bg-accent text-accent-fg font-medium text-sm disabled:opacity-50"
          >
            {adding ? "Đang thêm…" : "Thêm kênh"}
          </button>
        </div>
      </div>

      {/* Interval + check now */}
      <div className="flex items-center gap-3 flex-wrap text-sm">
        <span className="text-fg">Kiểm tra mỗi</span>
        <input
          type="number"
          min={5}
          max={1440}
          value={interval}
          onChange={(e) => {
            const n = parseInt(e.target.value, 10);
            if (Number.isFinite(n) && n >= 5) void updateSettings({ watchIntervalMin: n });
          }}
          className="w-20 px-2 py-1.5 rounded-md bg-surface border border-border text-fg"
        />
        <span className="text-fg">phút</span>
        <button
          onClick={() => void checkNow()}
          disabled={checking || channels.length === 0}
          className="ml-auto px-3 py-1.5 rounded-md bg-surface-2 border border-border text-fg disabled:opacity-50"
        >
          {checking ? "Đang kiểm tra…" : "Kiểm tra ngay"}
        </button>
      </div>

      {error && (
        <div className="px-3 py-2 rounded-md bg-danger/10 border border-danger text-danger text-sm">
          {error}
        </div>
      )}

      {/* List */}
      <div className="space-y-2">
        {channels.length === 0 && (
          <p className="text-muted text-center py-8 text-sm">Chưa theo dõi kênh nào.</p>
        )}
        {channels.map((c) => (
          <div key={c.id} className="flex items-center gap-3 p-3 rounded-lg border border-border bg-surface">
            <input
              type="checkbox"
              checked={c.enabled}
              onChange={(e) => void toggle(c.id, e.target.checked)}
              className="h-4 w-4 shrink-0"
              title={c.enabled ? "Đang theo dõi" : "Đã tạm dừng"}
            />
            <div className="flex-1 min-w-0">
              <div className="text-sm font-medium text-fg truncate" title={c.url}>
                {c.title || c.url}
              </div>
              <div className="text-xs text-muted truncate flex items-center gap-2">
                <span>{tabLabel(c.tab)}</span>
                <span>·</span>
                <span>{c.lastChecked ? `Kiểm tra: ${formatTime(c.lastChecked)}` : "Chưa kiểm tra"}</span>
                {typeof c.lastNewCount === "number" && c.lastNewCount > 0 && (
                  <>
                    <span>·</span>
                    <span className="text-success">+{c.lastNewCount} video mới</span>
                  </>
                )}
              </div>
              {c.lastError && (
                <div className="text-xs text-danger truncate mt-0.5" title={c.lastError}>
                  ⚠ {c.lastError}
                </div>
              )}
            </div>
            <button
              onClick={() => void remove(c.id)}
              className="px-2 py-1 rounded-md border border-border text-fg shrink-0 hover:bg-surface-2"
              title="Bỏ theo dõi"
            >
              ✕
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}

function tabLabel(tab: string): string {
  if (tab === "videos") return "Video dài";
  if (tab === "shorts") return "Shorts";
  return "Tất cả";
}

function formatTime(iso: string): string {
  try {
    return new Date(iso).toLocaleString("vi-VN");
  } catch {
    return iso;
  }
}

function formatErr(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object") {
    const obj = e as Record<string, unknown>;
    if (typeof obj.message === "string") return obj.message;
    if (typeof obj.data === "string") return obj.data;
    if (typeof obj.kind === "string") return obj.kind;
  }
  return String(e);
}
