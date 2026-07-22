import { Fragment, useEffect, useState } from "react";
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
  // Quản lý nhiều kênh đa quốc gia: tìm nhanh + lọc theo nhóm.
  const [search, setSearch] = useState("");
  const [groupFilter, setGroupFilter] = useState("");

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
      // Vừa tích hàng chờ mà kênh chưa ở chế độ 🎯 → chuyển luôn để hàng
      // chờ thực sự chạy (khỏi quên đổi chế độ).
      if (pickerSel.length > 0 && pickerFor.sourceMode !== "picked") {
        await cmd.setWatchedSourceMode(pickerFor.id, "picked");
      }
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

  const setMode = async (id: string, mode: "new" | "picked" | "auto") => {
    try {
      await cmd.setWatchedSourceMode(id, mode);
      await reload();
    } catch (e) {
      setError(formatErr(e));
    }
  };

  const setGroup = async (id: string, group: string) => {
    try {
      await cmd.setWatchedGroup(id, group.trim() || null);
      await reload();
    } catch (e) {
      setError(formatErr(e));
    }
  };

  const setTarget = async (id: string, target: string) => {
    try {
      await cmd.setWatchedTarget(id, target.trim() || null);
      await reload();
    } catch (e) {
      setError(formatErr(e));
    }
  };

  const pickRoot = async () => {
    try {
      const dir = await cmd.pickFolder();
      if (!dir) return;
      await updateSettings({ watchRoot: dir });
    } catch (e) {
      setError(formatErr(e));
    }
  };

  // ── Quản lý NHÓM (thêm/sửa/xóa) — danh sách lưu trong Settings ──
  const [showGroups, setShowGroups] = useState(false);
  const [newGroup, setNewGroup] = useState("");

  const savedGroups = settings?.watchGroups ?? [];

  const addGroup = async () => {
    const g = newGroup.trim();
    if (!g) return;
    if (savedGroups.includes(g)) {
      setNewGroup("");
      return;
    }
    await updateSettings({ watchGroups: [...savedGroups, g] });
    setNewGroup("");
  };

  const renameGroup = async (oldName: string, newName: string) => {
    const n = newName.trim();
    if (!n || n === oldName) return;
    try {
      await updateSettings({
        watchGroups: savedGroups.map((g) => (g === oldName ? n : g)),
      });
      // Đổi tên lan sang mọi kênh đang mang nhóm cũ.
      for (const c of channels.filter((c) => c.group === oldName)) {
        await cmd.setWatchedGroup(c.id, n);
      }
      if (groupFilter === oldName) setGroupFilter(n);
      await reload();
    } catch (e) {
      setError(formatErr(e));
    }
  };

  const deleteGroup = async (name: string) => {
    try {
      await updateSettings({ watchGroups: savedGroups.filter((g) => g !== name) });
      // Kênh thuộc nhóm bị xóa → về "chưa phân nhóm" (không đụng gì khác).
      for (const c of channels.filter((c) => c.group === name)) {
        await cmd.setWatchedGroup(c.id, null);
      }
      if (groupFilter === name) setGroupFilter("");
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

  // Nhóm = danh sách user đặt (Settings) ∪ nhóm cũ còn dính trên kênh.
  const groups = [
    ...new Set([
      ...savedGroups,
      ...(channels.map((c) => c.group?.trim()).filter(Boolean) as string[]),
    ]),
  ];
  const term = search.trim().toLowerCase();
  const visible = channels
    .filter((c) => !groupFilter || (groupFilter === "__none" ? !c.group : c.group === groupFilter))
    .filter(
      (c) =>
        !term ||
        (c.title ?? "").toLowerCase().includes(term) ||
        c.url.toLowerCase().includes(term) ||
        (c.group ?? "").toLowerCase().includes(term),
    )
    .sort((a, b) =>
      (a.group ?? "￿").localeCompare(b.group ?? "￿", "vi") ||
      (a.title ?? a.url).localeCompare(b.title ?? b.url, "vi"),
    );
  const nOn = channels.filter((c) => c.enabled).length;
  const nEmpty = channels.filter(
    (c) => c.sourceMode === "picked" && (c.picked?.length ?? 0) === 0,
  ).length;

  return (
    <div className="max-w-3xl mx-auto space-y-4">
      <div className="flex items-center gap-3 flex-wrap">
        <h2 className="text-xl font-medium text-fg">Theo dõi kênh</h2>
        <span className="text-xs px-2 py-0.5 rounded-full bg-surface-2 border border-border text-muted">
          {channels.length} kênh · {nOn} đang bật
        </span>
        {nEmpty > 0 && (
          <span className="text-xs px-2 py-0.5 rounded-full bg-warning/15 border border-warning text-warning">
            ⚠ {nEmpty} kênh hết hàng chờ
          </span>
        )}
      </div>
      <p className="text-sm text-muted -mt-2">
        Video MỚI đăng luôn tự phát hiện (RSS ~1-2 phút). Kênh không đăng gì vẫn có bài nhờ chế độ
        nguồn: 🎯 hàng chờ tay hoặc 🤖 tự vét kho theo view — tối đa 1-3 video/ngày mỗi kênh.
      </p>

      {/* Thư mục trung chuyển gốc — gõ tên KÊNH ĐÍCH là video tự về <gốc>\<tên> */}
      <div className="flex items-center gap-2 text-sm flex-wrap px-3 py-2 rounded-lg border border-border bg-surface">
        <span className="shrink-0">📂 Trung chuyển gốc:</span>
        {settings?.watchRoot ? (
          <span className="text-fg font-medium truncate flex-1 min-w-[120px]" title={settings.watchRoot}>
            {settings.watchRoot}
          </span>
        ) : (
          <span className="text-warning flex-1 min-w-[120px]">
            chưa chọn — cần cho ô "Kênh đích" tự tạo thư mục
          </span>
        )}
        <button
          onClick={() => void pickRoot()}
          className="px-3 py-1 rounded-md bg-surface-2 border border-border text-fg text-xs shrink-0"
          title="Thư mục chứa toàn bộ thư mục kênh của dây chuyền — gõ tên Kênh đích là video tự về <gốc>\<tên kênh>"
        >
          Chọn…
        </button>
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

      {/* Thanh công cụ: tìm + lọc nhóm + chu kỳ + kiểm tra ngay */}
      <div className="flex items-center gap-2 flex-wrap text-sm">
        <input
          type="search"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="🔍 Tìm kênh / nhóm…"
          className="flex-1 min-w-[160px] px-3 py-1.5 rounded-md bg-surface border border-border text-fg placeholder:text-muted"
        />
        <select
          value={groupFilter}
          onChange={(e) => setGroupFilter(e.target.value)}
          className="px-2 py-1.5 rounded-md bg-surface border border-border text-fg"
          title="Lọc theo nhóm/quốc gia"
        >
          <option value="">Mọi nhóm</option>
          {groups.map((g) => (
            <option key={g} value={g}>{g}</option>
          ))}
          <option value="__none">Chưa phân nhóm</option>
        </select>
        <button
          onClick={() => setShowGroups(true)}
          className="px-2.5 py-1.5 rounded-md bg-surface-2 border border-border text-fg"
          title="Quản lý nhóm: thêm / đổi tên / xóa"
        >
          🏷 Nhóm
        </button>
        <label className="flex items-center gap-1.5 text-muted">
          <span>mỗi</span>
          <input
            type="number"
            min={1}
            max={1440}
            value={interval}
            onChange={(e) => {
              const n = parseInt(e.target.value, 10);
              if (Number.isFinite(n) && n >= 1) void updateSettings({ watchIntervalMin: n });
            }}
            className="w-16 px-2 py-1.5 rounded-md bg-surface border border-border text-fg"
          />
          <span>phút</span>
        </label>
        <button
          onClick={() => void checkNow()}
          disabled={checking || channels.length === 0}
          className="px-3 py-1.5 rounded-md bg-accent text-accent-fg font-medium disabled:opacity-50"
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
        {channels.length > 0 && visible.length === 0 && (
          <div className="text-sm text-muted text-center py-6">
            Không kênh nào khớp tìm kiếm/bộ lọc.
          </div>
        )}
        {visible.map((c, i) => (
          <Fragment key={c.id}>
          {(i === 0 || (visible[i - 1].group ?? "") !== (c.group ?? "")) && (
            <div className="flex items-center gap-2 pt-2 px-1">
              <span className="text-xs font-semibold text-fg uppercase tracking-wide">
                🏷 {c.group || "Chưa phân nhóm"}
              </span>
              <span className="text-xs text-muted">
                {visible.filter((x) => (x.group ?? "") === (c.group ?? "")).length} kênh
              </span>
              <div className="flex-1 border-t border-border" />
            </div>
          )}
          <div className="rounded-lg border border-border bg-surface">
            {/* Tầng 1: bật/tắt · tên · nhóm · các nút điều khiển */}
            <div className="flex items-center gap-2 px-3 pt-2.5 flex-wrap">
              <input
                type="checkbox"
                checked={c.enabled}
                onChange={(e) => void toggle(c.id, e.target.checked)}
                className="h-4 w-4 shrink-0"
                title={c.enabled ? "Đang theo dõi" : "Đã tạm dừng"}
              />
              <div
                className={`text-sm font-medium truncate flex-1 min-w-[140px] ${c.enabled ? "text-fg" : "text-muted line-through"}`}
                title={c.url}
              >
                {c.title || c.url}
              </div>
              {/* KÊNH ĐÍCH (kênh TikTok của user) — gõ tên là video tự về <gốc>\<tên> */}
              <input
                key={`${c.id}:t:${c.targetName ?? ""}`}
                type="text"
                defaultValue={c.targetName ?? ""}
                placeholder="kênh đích…"
                onBlur={(e) => {
                  if (e.target.value.trim() !== (c.targetName ?? "")) void setTarget(c.id, e.target.value);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                }}
                className={`w-28 px-2 py-1 rounded-md border text-xs shrink-0 placeholder:text-muted ${
                  c.targetName ? "bg-accent/15 text-fg border-accent" : "bg-surface-2 text-fg border-border"
                }`}
                title={"Tên KÊNH ĐÍCH (kênh TikTok của anh) mà nguồn này nuôi.\nGõ tên → video tự về <Trung chuyển gốc>\\<tên kênh> — tool cắt nhận đúng kênh.\n(📁 chọn tay vẫn được và ưu tiên hơn)"}
              />
              {/* Nhóm/quốc gia — chọn từ danh sách đã đặt (🏷 Nhóm để thêm/sửa/xóa) */}
              <select
                value={c.group ?? ""}
                onChange={(e) => {
                  if (e.target.value === "__manage") {
                    setShowGroups(true);
                    return;
                  }
                  void setGroup(c.id, e.target.value);
                }}
                className={`px-1.5 py-1 rounded-md border text-xs shrink-0 max-w-[110px] ${
                  c.group
                    ? "bg-accent/15 text-fg border-accent"
                    : "bg-surface-2 text-muted border-border"
                }`}
                title="Nhóm/quốc gia của kênh — quản lý danh sách bằng nút 🏷 Nhóm"
              >
                <option value="">— nhóm —</option>
                {groups.map((g) => (
                  <option key={g} value={g}>{g}</option>
                ))}
                <option value="__manage">➕ Quản lý nhóm…</option>
              </select>
              {/* Chế độ nguồn khi kênh không đăng video mới */}
              <select
                value={c.sourceMode ?? "new"}
                onChange={(e) => void setMode(c.id, e.target.value as "new" | "picked" | "auto")}
                className="px-1.5 py-1 rounded-md bg-surface-2 border border-border text-fg text-xs shrink-0"
                title={"Nguồn video của kênh:\n• Video mới — chỉ tải video mới đăng\n• 🎯 Hàng chờ — tự tải dần video ANH ĐÃ TÍCH trong kho\n• 🤖 Tự vét — app TỰ CHỌN video view cao nhất chưa làm trong kho\n(Video mới đăng luôn được ưu tiên chiếm suất ngày trước)"}
              >
                <option value="new">Video mới</option>
                <option value="picked">🎯 Hàng chờ</option>
                <option value="auto">🤖 Tự vét</option>
              </select>
              <select
                value={c.dailyLimit ?? 1}
                onChange={(e) => void setDaily(c.id, parseInt(e.target.value, 10))}
                className="px-1.5 py-1 rounded-md bg-surface-2 border border-border text-fg text-xs shrink-0"
                title="Số video TỰ TẢI tối đa mỗi ngày (video mới + hàng chờ/tự vét)"
              >
                <option value={1}>1/ngày</option>
                <option value={2}>2/ngày</option>
                <option value={3}>3/ngày</option>
              </select>
              <button
                onClick={() => void openPicker(c)}
                className={`px-2 py-1 rounded-md text-xs shrink-0 border ${
                  (c.picked?.length ?? 0) > 0
                    ? "bg-accent text-accent-fg border-accent"
                    : "bg-surface-2 text-fg border-border"
                }`}
                title="Mở KHO video kênh nguồn — tích chọn video nên làm (hàng chờ 🎯)"
              >
                🎯
              </button>
              <button
                onClick={() => void setDest(c.id)}
                className={`px-2 py-1 rounded-md text-xs shrink-0 border ${
                  c.destDir
                    ? "bg-accent text-accent-fg border-accent"
                    : "bg-surface-2 text-fg border-border"
                }`}
                title={
                  c.destDir
                    ? `Video lưu vào: ${c.destDir} — bấm để đổi`
                    : "Chọn THƯ MỤC LƯU RIÊNG cho kênh (nối với tool cắt ghép)"
                }
              >
                📁
              </button>
              <button
                onClick={() => void toggleAuto(c.id, !c.autoDownload)}
                className={`px-2 py-1 rounded-md text-xs font-medium shrink-0 border ${
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
                className="px-2 py-1 rounded-md border border-border text-fg text-xs shrink-0 hover:bg-surface-2"
                title="Bỏ theo dõi"
              >
                ✕
              </button>
            </div>
            {/* Tầng 2: thông tin phụ — loại video, kiểm tra gần nhất, hàng chờ, thư mục, lỗi */}
            <div className="px-3 pb-2.5 pt-1 text-xs text-muted flex items-center gap-x-2 gap-y-0.5 flex-wrap">
              <span>{tabLabel(c.tab)}</span>
              <span>·</span>
              <span>{c.lastChecked ? `kiểm tra ${timeAgo(c.lastChecked)}` : "chưa kiểm tra"}</span>
              {typeof c.lastNewCount === "number" && c.lastNewCount > 0 && (
                <span className="text-success">+{c.lastNewCount} mới</span>
              )}
              {(c.sourceMode ?? "new") === "picked" &&
                ((c.picked?.length ?? 0) > 0 ? (
                  <span>· 🎯 còn {c.picked!.length} chờ làm</span>
                ) : (
                  <span className="text-warning">· ⚠ HẾT hàng chờ — bấm 🎯 tích thêm</span>
                ))}
              {(c.sourceMode ?? "new") === "auto" && <span>· 🤖 tự vét kho theo view</span>}
              {c.destDir ? (
                <span className="truncate max-w-[55%]" title={`Thư mục (chọn tay 📁, ưu tiên hơn tên kênh): ${c.destDir}`}>
                  · <span className="text-accent font-medium">→ làm kênh: {baseName(c.destDir)}</span> 📁
                  <button
                    onClick={() => void clearDest(c.id)}
                    className="ml-1 text-danger hover:underline"
                    title="Bỏ thư mục chọn tay — quay về dùng ô Kênh đích (nếu có)"
                  >
                    ✕
                  </button>
                </span>
              ) : c.targetName ? (
                settings?.watchRoot ? (
                  <span
                    className="truncate max-w-[55%]"
                    title={`Video tự về: ${settings.watchRoot}\\${c.targetName}`}
                  >
                    · <span className="text-accent font-medium">→ làm kênh: {c.targetName}</span>
                  </span>
                ) : (
                  <span className="text-danger">
                    · ⚠ đã đặt kênh đích nhưng CHƯA chọn 📂 Trung chuyển gốc — video sẽ rơi thư mục mặc định!
                  </span>
                )
              ) : (
                <span className="text-warning">· ⚠ chưa gán kênh đích — gõ ô "kênh đích…" hoặc bấm 📁</span>
              )}
              {c.lastError && (
                <span className="text-danger truncate max-w-full" title={c.lastError}>
                  ⚠ {c.lastError}
                </span>
              )}
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
          </Fragment>
        ))}
      </div>

      {/* Dialog QUẢN LÝ NHÓM — thêm / đổi tên / xóa */}
      {showGroups && (
        <div className="fixed inset-0 z-50 bg-black/50 flex items-center justify-center p-4">
          <div className="bg-surface border border-border rounded-lg w-full max-w-md max-h-[80vh] flex flex-col">
            <div className="p-3 border-b border-border">
              <div className="text-sm font-medium text-fg">🏷 Quản lý nhóm kênh</div>
              <div className="text-xs text-muted mt-0.5">
                Đặt nhóm theo quốc gia/loại (Mỹ, Hàn, TikTok beta…) rồi gán cho kênh.
                Đổi tên nhóm sẽ đổi trên MỌI kênh; xóa nhóm thì kênh về "chưa phân nhóm".
              </div>
            </div>
            <div className="p-3 flex gap-2">
              <input
                type="text"
                value={newGroup}
                onChange={(e) => setNewGroup(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter") void addGroup(); }}
                placeholder="Tên nhóm mới…"
                className="flex-1 px-3 py-1.5 rounded-md bg-surface-2 border border-border text-fg text-sm placeholder:text-muted"
              />
              <button
                onClick={() => void addGroup()}
                disabled={!newGroup.trim()}
                className="px-3 py-1.5 rounded-md bg-accent text-accent-fg text-sm font-medium disabled:opacity-50"
              >
                + Thêm
              </button>
            </div>
            <div className="flex-1 overflow-y-auto">
              {groups.length === 0 && (
                <div className="p-4 text-center text-sm text-muted">Chưa có nhóm nào.</div>
              )}
              {groups.map((g) => {
                const n = channels.filter((c) => c.group === g).length;
                return (
                  <div key={g} className="flex items-center gap-2 px-3 py-2 border-t border-border">
                    <input
                      key={`g:${g}`}
                      type="text"
                      defaultValue={g}
                      onBlur={(e) => void renameGroup(g, e.target.value)}
                      onKeyDown={(e) => { if (e.key === "Enter") (e.target as HTMLInputElement).blur(); }}
                      className="flex-1 px-2 py-1 rounded-md bg-surface-2 border border-border text-fg text-sm"
                      title="Sửa tên rồi Enter/rời ô — đổi trên mọi kênh đang mang nhóm này"
                    />
                    <span className="text-xs text-muted shrink-0">{n} kênh</span>
                    <button
                      onClick={() => void deleteGroup(g)}
                      className="px-2 py-1 rounded-md border border-border text-danger text-xs shrink-0 hover:bg-surface-2"
                      title={`Xóa nhóm "${g}" — ${n} kênh về "chưa phân nhóm"`}
                    >
                      Xóa
                    </button>
                  </div>
                );
              })}
            </div>
            <div className="p-3 border-t border-border flex justify-end">
              <button
                onClick={() => setShowGroups(false)}
                className="px-4 py-1.5 rounded-md bg-accent text-accent-fg text-sm font-medium"
              >
                Xong
              </button>
            </div>
          </div>
        </div>
      )}

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

/** Khúc CUỐI đường dẫn = tên kênh đích trong tool cắt (INTEGRATION.md:
 *  thư mục trung chuyển đặt đúng tên kênh). `D:\TC\Kênh A` -> `Kênh A`. */
function baseName(p: string): string {
  const parts = p.replace(/[/\\]+$/, "").split(/[/\\]/);
  return parts[parts.length - 1] || p;
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
