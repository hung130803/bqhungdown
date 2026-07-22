import { useEffect, useState } from "react";
import * as cmd from "@/ipc/commands";
import { useSettingsStore } from "@/stores/useSettingsStore";
import type { ChannelVideo, PickedVideo, WatchedChannel } from "@/types/models";
import { EmptyState } from "@/components/EmptyState";

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

  // Live refresh when the background monitor detects new videos.
  useEffect(() => {
    let un: (() => void) | undefined;
    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      un = await listen("watch://updated", () => { void reload(); });
    })();
    return () => un?.();
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

  const toggleAuto = async (id: string, auto: boolean) => {
    try {
      await cmd.setWatchedAutoDownload(id, auto);
      await reload();
    } catch (e) {
      setError(formatErr(e));
    }
  };

  const setDest = async (id: string) => {
    try {
      const dir = await cmd.pickFolder();
      if (!dir) return; // hủy hộp thoại -> giữ nguyên
      await cmd.setWatchedDestDir(id, dir);
      await reload();
    } catch (e) {
      setError(formatErr(e));
    }
  };

  const clearDest = async (id: string) => {
    try {
      await cmd.setWatchedDestDir(id, null);
      await reload();
    } catch (e) {
      setError(formatErr(e));
    }
  };

  const downloadOne = async (id: string, videoUrl: string) => {
    try {
      await cmd.downloadPending(id, videoUrl);
      await reload();
    } catch (e) {
      setError(formatErr(e));
    }
  };

  const dismissOne = async (id: string, videoUrl: string) => {
    try {
      await cmd.dismissPending(id, videoUrl);
      await reload();
    } catch (e) {
      setError(formatErr(e));
    }
  };

  // ── Hàng chờ làm: dialog kho video kênh nguồn, tích chọn video sẽ tự tải dần ──
  const [pickerFor, setPickerFor] = useState<WatchedChannel | null>(null);
  const [pickerVideos, setPickerVideos] = useState<ChannelVideo[]>([]);
  const [pickerLoading, setPickerLoading] = useState(false);
  const [pickerErr, setPickerErr] = useState<string | null>(null);
  // Thứ tự tích = thứ tự tự tải (video tích trước được làm trước).
  const [pickerSel, setPickerSel] = useState<PickedVideo[]>([]);
  const [pickerSaving, setPickerSaving] = useState(false);

  const openPicker = async (c: WatchedChannel) => {
    setPickerFor(c);
    setPickerVideos([]);
    setPickerErr(null);
    setPickerSel(c.picked ?? []);
    setPickerLoading(true);
    try {
      const res = await cmd.fetchChannelVideos(c.url, 300, false, (c.tab as "all" | "videos" | "shorts") || "all", false);
      // Nhiều view nhất lên đầu (không có số view thì giữ nguyên thứ tự mới→cũ).
      const vids = [...res.videos].sort((a, b) => (b.viewCount ?? -1) - (a.viewCount ?? -1));
      setPickerVideos(vids);
    } catch (e) {
      setPickerErr(formatErr(e));
    } finally {
      setPickerLoading(false);
    }
  };

  const togglePick = (v: ChannelVideo) => {
    const id = videoIdOf(v.url);
    setPickerSel((sel) =>
      sel.some((p) => p.id === id)
        ? sel.filter((p) => p.id !== id)
        : [...sel, { id, url: v.url, title: v.title, viewCount: v.viewCount ?? null, thumbnail: v.thumbnail ?? null }],
    );
  };

  const savePicker = async () => {
    if (!pickerFor || pickerSaving) return;
    setPickerSaving(true);
    try {
      await cmd.setWatchedPicked(pickerFor.id, pickerSel);
      setPickerFor(null);
      await reload();
    } catch (e) {
      setPickerErr(formatErr(e));
    } finally {
      setPickerSaving(false);
    }
  };

  const setDaily = async (id: string, limit: number) => {
    try {
      await cmd.setWatchedDailyLimit(id, limit);
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
          Thêm kênh vào đây — app tự kiểm tra định kỳ và tải video mới đăng (không tải lại video cũ).
          Với YouTube dùng kiểm tra nhanh qua RSS nên đặt 1-2 phút là phát hiện video mới gần như tức thì.
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
          min={1}
          max={1440}
          value={interval}
          onChange={(e) => {
            const n = parseInt(e.target.value, 10);
            if (Number.isFinite(n) && n >= 1) void updateSettings({ watchIntervalMin: n });
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
          <EmptyState
            icon="🔔"
            title="Chưa theo dõi kênh nào"
            hint="Thêm kênh để app tự kiểm tra video mới định kỳ và tải về giúp bạn — không cần ngồi canh."
          />
        )}
        {channels.map((c) => (
          <div key={c.id} className="rounded-lg border border-border bg-surface">
            <div className="flex items-center gap-3 p-3">
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
                {c.destDir && (
                  <div className="text-xs text-muted truncate mt-0.5" title={c.destDir}>
                    📁 {c.destDir}
                    <button
                      onClick={() => void clearDest(c.id)}
                      className="ml-1.5 text-danger hover:underline"
                      title="Bỏ thư mục riêng — video mới về thư mục tải mặc định"
                    >
                      ✕
                    </button>
                  </div>
                )}
                {/* Hàng chờ làm: còn bao nhiêu video đã tích, cạn thì cảnh báo */}
                {(c.picked?.length ?? 0) > 0 ? (
                  <div className="text-xs text-muted mt-0.5">
                    🎯 còn {c.picked!.length} video chờ làm · tự tải {c.dailyLimit ?? 1}/ngày
                  </div>
                ) : (
                  c.destDir && (
                    <div className="text-xs text-warning mt-0.5">
                      ⚠ Hết hàng chờ — chỉ còn chờ video MỚI. Bấm 🎯 tích thêm video kho.
                    </div>
                  )
                )}
              </div>
              {/* Số video tự tải tối đa mỗi ngày (mới + hàng chờ) */}
              <select
                value={c.dailyLimit ?? 1}
                onChange={(e) => void setDaily(c.id, parseInt(e.target.value, 10))}
                className="px-1.5 py-1 rounded-md bg-surface-2 border border-border text-fg text-xs shrink-0"
                title="Số video TỰ TẢI tối đa mỗi ngày (video mới đăng + hàng chờ đã tích)"
              >
                <option value={1}>1/ngày</option>
                <option value={2}>2/ngày</option>
                <option value={3}>3/ngày</option>
              </select>
              {/* Kho video kênh nguồn — tích chọn video "nên làm" */}
              <button
                onClick={() => void openPicker(c)}
                className={`px-2 py-1 rounded-md text-xs shrink-0 border ${
                  (c.picked?.length ?? 0) > 0
                    ? "bg-accent text-accent-fg border-accent"
                    : "bg-surface-2 text-fg border-border"
                }`}
                title="Mở KHO video kênh nguồn — tích chọn video nên làm, app tự tải dần mỗi ngày"
              >
                🎯
              </button>
              {/* Thư mục lưu RIÊNG của kênh (dây chuyền cắt ghép — INTEGRATION.md) */}
              <button
                onClick={() => void setDest(c.id)}
                className={`px-2 py-1 rounded-md text-xs shrink-0 border ${
                  c.destDir
                    ? "bg-accent text-accent-fg border-accent"
                    : "bg-surface-2 text-fg border-border"
                }`}
                title={
                  c.destDir
                    ? `Video mới lưu vào: ${c.destDir} — bấm để đổi`
                    : "Chọn THƯ MỤC LƯU RIÊNG cho video mới của kênh này (nối với tool cắt ghép)"
                }
              >
                📁
              </button>
              {/* Tự tải vs Chỉ báo */}
              <button
                onClick={() => void toggleAuto(c.id, !c.autoDownload)}
                className={`px-2.5 py-1 rounded-md text-xs font-medium shrink-0 border ${
                  c.autoDownload
                    ? "bg-accent text-accent-fg border-accent"
                    : "bg-surface-2 text-fg border-border"
                }`}
                title={c.autoDownload ? "Đang TỰ TẢI video mới — bấm để chỉ báo" : "Chỉ BÁO video mới — bấm để tự tải"}
              >
                {c.autoDownload ? "Tự tải" : "Chỉ báo"}
              </button>
              <button
                onClick={() => void remove(c.id)}
                className="px-2 py-1 rounded-md border border-border text-fg shrink-0 hover:bg-surface-2"
                title="Bỏ theo dõi"
              >
                ✕
              </button>
            </div>

            {/* Video mới phát hiện (chế độ "Chỉ báo") — chờ bấm tải */}
            {c.pending && c.pending.length > 0 && (
              <div className="border-t border-border">
                <div className="px-3 py-1.5 text-xs text-muted bg-surface-2">
                  {c.pending.length} video mới — bấm "Tải" để lấy về
                </div>
                {c.pending.map((p) => (
                  <div key={p.id} className="flex items-center gap-3 px-3 py-2 border-t border-border">
                    <div className="text-sm text-fg flex-1 min-w-0">
                      <div className="truncate" title={p.title}>{p.title || p.url}</div>
                      <div className="text-xs text-muted">đăng {timeAgo(p.published ?? p.detectedAt)}</div>
                    </div>
                    <button
                      onClick={() => void downloadOne(c.id, p.url)}
                      className="px-3 py-1 rounded-md bg-accent text-accent-fg text-xs font-medium shrink-0"
                    >
                      Tải
                    </button>
                    <button
                      onClick={() => void dismissOne(c.id, p.url)}
                      className="px-2 py-1 rounded-md border border-border text-fg text-xs shrink-0 hover:bg-surface-2"
                      title="Bỏ qua, không tải"
                    >
                      Bỏ qua
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        ))}
      </div>

      {/* Dialog KHO VIDEO kênh nguồn — tích chọn "hàng chờ làm" */}
      {pickerFor && (
        <div className="fixed inset-0 z-50 bg-black/50 flex items-center justify-center p-4">
          <div className="bg-surface border border-border rounded-lg w-full max-w-2xl max-h-[85vh] flex flex-col">
            <div className="p-3 border-b border-border">
              <div className="text-sm font-medium text-fg truncate">
                🎯 Kho video: {pickerFor.title || pickerFor.url}
              </div>
              <div className="text-xs text-muted mt-0.5">
                Tích video "nên làm" — app tự tải dần mỗi ngày ({pickerFor.dailyLimit ?? 1}/ngày,
                video MỚI đăng chiếm suất trước). Thứ tự tích = thứ tự làm. Đã tích {pickerSel.length} video.
              </div>
            </div>
            <div className="flex-1 overflow-y-auto">
              {pickerLoading && (
                <div className="p-6 text-center text-sm text-muted">Đang lấy kho video…</div>
              )}
              {pickerErr && (
                <div className="m-3 px-3 py-2 rounded-md bg-danger/10 border border-danger text-danger text-sm">
                  {pickerErr}
                </div>
              )}
              {!pickerLoading && !pickerErr && pickerVideos.length === 0 && (
                <div className="p-6 text-center text-sm text-muted">Không lấy được video nào.</div>
              )}
              {pickerVideos.map((v) => {
                const id = videoIdOf(v.url);
                const done = pickerFor.doneIds?.includes(id) ?? false;
                const sel = pickerSel.some((p) => p.id === id);
                const order = sel ? pickerSel.findIndex((p) => p.id === id) + 1 : 0;
                return (
                  <label
                    key={id}
                    className={`flex items-center gap-3 px-3 py-2 border-t border-border text-sm ${
                      done ? "opacity-50" : "cursor-pointer hover:bg-surface-2"
                    }`}
                  >
                    <input
                      type="checkbox"
                      checked={sel}
                      disabled={done}
                      onChange={() => togglePick(v)}
                      className="h-4 w-4 shrink-0"
                    />
                    <div className="flex-1 min-w-0">
                      <div className="truncate text-fg" title={v.title}>{v.title || v.url}</div>
                      <div className="text-xs text-muted">
                        {v.viewCount != null && <>👁 {formatViews(v.viewCount)} · </>}
                        {v.uploadDate && <>{timeAgo(v.uploadDate)} · </>}
                        {done ? "✅ đã làm" : sel ? `#${order} trong hàng chờ` : ""}
                      </div>
                    </div>
                  </label>
                );
              })}
            </div>
            <div className="p-3 border-t border-border flex gap-2 justify-end">
              <button
                onClick={() => setPickerFor(null)}
                className="px-3 py-1.5 rounded-md border border-border text-fg text-sm hover:bg-surface-2"
              >
                Hủy
              </button>
              <button
                onClick={() => void savePicker()}
                disabled={pickerSaving}
                className="px-4 py-1.5 rounded-md bg-accent text-accent-fg text-sm font-medium disabled:opacity-50"
              >
                {pickerSaving ? "Đang lưu…" : `Lưu hàng chờ (${pickerSel.length})`}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

/** Rút id video từ URL — PHẢI khớp từng bước với backend
 *  `channel_fetcher::extract_video_id` (lệch là vỡ chống trùng giữa hàng chờ
 *  và seen_ids/done_ids): ?v=/&v= trước, rồi /shorts/ /embed/ /video/ /v/,
 *  không khớp thì dùng nguyên URL làm id (giống watcher::video_id_of). */
function videoIdOf(url: string): string {
  const vIdx = url.indexOf("?v=") >= 0 ? url.indexOf("?v=") : url.indexOf("&v=");
  if (vIdx >= 0) {
    const id = url.slice(vIdx + 3).split("&")[0];
    if (id) return id;
  }
  for (const marker of ["/shorts/", "/embed/", "/video/", "/v/"]) {
    const i = url.indexOf(marker);
    if (i >= 0) {
      const id = url.slice(i + marker.length).split(/[/?#]/)[0];
      if (id) return id;
    }
  }
  return url;
}

function formatViews(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1).replace(/\.0$/, "")}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1).replace(/\.0$/, "")}K`;
  return String(n);
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

/** "vừa xong" / "X phút trước" / "X giờ trước" / "X ngày trước". Accepts ISO or
 *  YYYYMMDD (yt-dlp date-only). */
function timeAgo(value?: string | null): string {
  if (!value) return "";
  let d: Date;
  if (/^\d{8}$/.test(value)) {
    d = new Date(+value.slice(0, 4), +value.slice(4, 6) - 1, +value.slice(6, 8));
  } else {
    d = new Date(value);
  }
  const ms = Date.now() - d.getTime();
  if (!Number.isFinite(ms) || ms < 0) return "vừa xong";
  const min = Math.floor(ms / 60000);
  if (min < 1) return "vừa xong";
  if (min < 60) return `${min} phút trước`;
  const h = Math.floor(min / 60);
  if (h < 24) return `${h} giờ trước`;
  return `${Math.floor(h / 24)} ngày trước`;
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
