import { Fragment, useEffect, useState } from "react";
import * as cmd from "@/ipc/commands";
import { useSettingsStore } from "@/stores/useSettingsStore";
import type { ChannelVideo, PickedVideo, WatchedChannel } from "@/types/models";
import { EmptyState } from "@/components/EmptyState";

/** 1 KÊNH đích của user (kênh TikTok) = nhiều key nguồn chung tên kênh.
 *  `rep` = key đầu tiên, đại diện cấu hình mức kênh (nhóm/chế độ/thư mục). */
type Kenh = {
  key: string;
  name: string;
  group: string;
  keys: WatchedChannel[];
  rep: WatchedChannel;
};

/**
 * "Theo dõi kênh" — auto-watch list. The backend monitor periodically checks
 * each enabled channel and auto-enqueues new uploads (baseline-seeded so it
 * never grabs the backlog). This page manages the list + interval + manual check.
 */
export function WatchPage() {
  const settings = useSettingsStore((s) => s.settings);
  const updateSettings = useSettingsStore((s) => s.update);

  const [channels, setChannels] = useState<WatchedChannel[]>([]);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Quản lý nhiều kênh đa quốc gia: tìm nhanh + lọc theo nhóm.
  const [search, setSearch] = useState("");
  const [groupFilter, setGroupFilter] = useState("");

  // ── Dialog "➕ Thêm kênh": tên kênh đích + nhóm + key nguồn đầu tiên ──
  const [addOpen, setAddOpen] = useState(false);
  const [addName, setAddName] = useState("");
  const [addGrp, setAddGrp] = useState("");
  const [addUrl, setAddUrl] = useState("");
  const [addTab, setAddTab] = useState("all");
  const [addDir, setAddDir] = useState("");
  const [adding, setAdding] = useState(false);
  // Ô "dán key mới" trong từng thẻ kênh (map theo key của thẻ).
  const [keyInputs, setKeyInputs] = useState<Record<string, string>>({});
  // 50-300 kênh: nhóm GẬP/MỞ được + thẻ kênh THU GỌN 1 dòng, bấm mới xổ.
  const [openGroups, setOpenGroups] = useState<Record<string, boolean>>({});
  const [openKenh, setOpenKenh] = useState<Record<string, boolean>>({});

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

  /** Tạo KÊNH mới: thêm key nguồn đầu tiên rồi gắn tên/nhóm/chế độ 🤖. */
  const addKenh = async () => {
    const name = addName.trim();
    const u = addUrl.trim();
    if (!name || !u || adding) return;
    setAdding(true);
    setError(null);
    try {
      const c = await cmd.addWatchedChannel(u, addTab);
      await cmd.setWatchedTarget(c.id, name);
      if (addGrp) await cmd.setWatchedGroup(c.id, addGrp);
      // Mặc định 🤖 tự vét: có video mới thì lấy mới, không thì vét view
      // cao nhất chưa làm — đúng quy trình vận hành kênh.
      await cmd.setWatchedSourceMode(c.id, "auto");
      if (addDir.trim()) await cmd.setWatchedDestDir(c.id, addDir.trim());
      setAddOpen(false);
      setAddName("");
      setAddUrl("");
      setAddDir("");
      await reload();
    } catch (e) {
      setError(formatErr(e));
    } finally {
      setAdding(false);
    }
  };

  /** Thêm 1 key nguồn nữa vào kênh có sẵn — copy nguyên cấu hình kênh. */
  const addKeyToKenh = async (k: Kenh) => {
    const u = (keyInputs[k.key] ?? "").trim();
    if (!u || adding) return;
    setAdding(true);
    setError(null);
    try {
      const rep = k.rep;
      const c = await cmd.addWatchedChannel(u, rep.tab || "all");
      if (rep.targetName) await cmd.setWatchedTarget(c.id, rep.targetName);
      await cmd.setWatchedGroup(c.id, rep.group ?? null);
      await cmd.setWatchedSourceMode(
        c.id, (rep.sourceMode as "new" | "picked" | "auto") ?? "auto",
      );
      await cmd.setWatchedDailyLimit(c.id, rep.dailyLimit ?? 1);
      if (rep.destDir) await cmd.setWatchedDestDir(c.id, rep.destDir);
      setKeyInputs((m) => ({ ...m, [k.key]: "" }));
      await reload();
    } catch (e) {
      setError(formatErr(e));
    } finally {
      setAdding(false);
    }
  };

  /** Áp 1 thao tác cho MỌI key của kênh (cài đặt mức kênh). */
  const forKenh = async (k: Kenh, fn: (id: string) => Promise<unknown>) => {
    try {
      for (const c of k.keys) await fn(c.id);
      await reload();
    } catch (e) {
      setError(formatErr(e));
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
  // Gom key nguồn thành KÊNH ĐÍCH: mọi key cùng tên kênh (targetName, hoặc
  // tên thư mục 📁) nằm chung 1 thẻ; key chưa gắn gì đứng thẻ riêng.
  const kenhMap = new Map<string, Kenh>();
  for (const c of channels) {
    const name = c.targetName?.trim() || (c.destDir ? baseName(c.destDir) : "");
    const key = name || `⁇${c.id}`;
    const existing = kenhMap.get(key);
    if (existing) {
      existing.keys.push(c);
    } else {
      kenhMap.set(key, {
        key,
        name,
        group: c.group?.trim() ?? "",
        keys: [c],
        rep: c,
      });
    }
  }
  const kenhAll = [...kenhMap.values()];
  const kenhVisible = kenhAll
    .filter((k) => !groupFilter || (groupFilter === "__none" ? !k.group : k.group === groupFilter))
    .filter(
      (k) =>
        !term ||
        k.name.toLowerCase().includes(term) ||
        k.group.toLowerCase().includes(term) ||
        k.keys.some(
          (c) =>
            (c.title ?? "").toLowerCase().includes(term) ||
            c.url.toLowerCase().includes(term),
        ),
    )
    .sort(
      (a, b) =>
        (a.group || "￿").localeCompare(b.group || "￿", "vi") ||
        a.name.localeCompare(b.name, "vi"),
    );
  const nOn = channels.filter((c) => c.enabled).length;
  // Kênh cần chú ý: kho cạn (🤖 hết video chưa làm) / hết hàng chờ (🎯).
  const nDry = kenhAll.filter((k) => k.keys.some((c) => c.sourceEmpty)).length;
  const nEmpty = kenhAll.filter(
    (k) =>
      (k.rep.sourceMode ?? "new") === "picked" &&
      k.keys.every((c) => (c.picked?.length ?? 0) === 0),
  ).length;

  return (
    <div className="max-w-3xl mx-auto space-y-4">
      <div className="flex items-center gap-3 flex-wrap">
        <h2 className="text-xl font-medium text-fg">Kênh của tôi</h2>
        <span className="text-xs px-2 py-0.5 rounded-full bg-surface-2 border border-border text-muted">
          {kenhAll.length} kênh · {channels.length} key · {nOn} key đang chạy
        </span>
        {nDry > 0 && (
          <span className="text-xs px-2 py-0.5 rounded-full bg-danger/15 border border-danger text-danger">
            🔴 {nDry} kênh HẾT video — đổi key
          </span>
        )}
        {nEmpty > 0 && (
          <span className="text-xs px-2 py-0.5 rounded-full bg-warning/15 border border-warning text-warning">
            ⚠ {nEmpty} kênh hết hàng chờ
          </span>
        )}
        <button
          onClick={() => setAddOpen(true)}
          className="ml-auto px-3 py-1.5 rounded-md bg-accent text-accent-fg text-sm font-medium"
        >
          ➕ Thêm kênh
        </button>
      </div>
      <p className="text-sm text-muted -mt-2">
        Mỗi KÊNH = kênh TikTok của anh: đặt tên, gán nhóm, dán key YouTube nguồn. App tự tải video
        MỚI để đăng; hôm nào nguồn không đăng thì tự vét video NHIỀU VIEW nhất chưa làm, lấy dần —
        vét cạn kho sẽ báo 🔴 để anh đổi key.
      </p>

      {/* Trung chuyển gốc — KHÔNG bắt buộc: chỉ là lối tắt khỏi phải 📁 chọn
          tay từng kênh (đã chọn tay thì bỏ qua dòng này). */}
      <div className="flex items-center gap-2 text-xs text-muted flex-wrap px-1 -mt-1">
        <span className="shrink-0">📂 Trung chuyển gốc (không bắt buộc):</span>
        {settings?.watchRoot ? (
          <span className="text-fg truncate flex-1 min-w-[120px]" title={settings.watchRoot}>
            {settings.watchRoot}
          </span>
        ) : (
          <span className="flex-1 min-w-[120px]">
            chưa dùng — anh đang 📁 chọn thư mục tay từng kênh thì kệ nó
          </span>
        )}
        <button
          onClick={() => void pickRoot()}
          className="px-2 py-0.5 rounded-md bg-surface-2 border border-border text-fg shrink-0"
          title="Lối tắt: chọn 1 thư mục gốc, từ đó kênh mới chỉ cần gõ tên là tự có thư mục <gốc>\<tên> — khỏi bấm chọn 50 lần"
        >
          Chọn…
        </button>
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
          title={"Quét MỌI kênh ngay 1 phát: kênh có video mới → tải; không có mà để 🤖 → tự lấy video view cao nhất chưa làm.\n(App mở là tự chạy mỗi chu kỳ bên cạnh — nút này chỉ để khỏi chờ.)"}
        >
          {checking ? "Đang chạy…" : "▶ Chạy tất cả"}
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
        {channels.length > 0 && kenhVisible.length === 0 && (
          <div className="text-sm text-muted text-center py-6">
            Không kênh nào khớp tìm kiếm/bộ lọc.
          </div>
        )}
        {kenhVisible.map((k, i) => {
          const anyOn = k.keys.some((c) => c.enabled);
          const dry = k.keys.some((c) => c.sourceEmpty);
          const mode = (k.rep.sourceMode ?? "new") as string;
          const pickedTotal = k.keys.reduce((s, c) => s + (c.picked?.length ?? 0), 0);
          const g = k.group || "";
          // Ít kênh thì mở sẵn hết cho dễ nhìn; nhiều kênh (50-300) thì gập
          // theo nhóm, đang tìm/lọc thì luôn mở để thấy kết quả.
          const defaultOpen = kenhAll.length <= 8;
          const gOpen = term || groupFilter ? true : (openGroups[g] ?? defaultOpen);
          const kOpen = openKenh[k.key] ?? !k.name; // thẻ chưa đặt tên tự xổ
          const isFirstInGroup = i === 0 || (kenhVisible[i - 1].group || "") !== g;
          const inGroup = kenhVisible.filter((x) => (x.group || "") === g);
          const gDry = inGroup.filter((x) => x.keys.some((c) => c.sourceEmpty)).length;
          const dirOk = !!k.rep.destDir || (!!k.name && !!settings?.watchRoot);
          return (
          <Fragment key={k.key}>
          {isFirstInGroup && (
            <button
              onClick={() => setOpenGroups((m) => ({ ...m, [g]: !gOpen }))}
              className="w-full flex items-center gap-2 pt-2 px-1 text-left"
              title={gOpen ? "Gập nhóm" : "Mở nhóm"}
            >
              <span className="text-xs text-muted w-3">{gOpen ? "▾" : "▸"}</span>
              <span className="text-xs font-semibold text-fg uppercase tracking-wide">
                🏷 {g || "Chưa phân nhóm"}
              </span>
              <span className="text-xs text-muted">{inGroup.length} kênh</span>
              {gDry > 0 && (
                <span className="text-xs text-danger font-medium">🔴 {gDry} hết video</span>
              )}
              <div className="flex-1 border-t border-border" />
            </button>
          )}
          {gOpen && (
          <div className={`rounded-lg border bg-surface ${dry ? "border-danger" : "border-border"}`}>
            {/* Dòng THU GỌN của kênh — bấm để xổ chi tiết */}
            <div
              className="flex items-center gap-2 px-3 py-2 cursor-pointer hover:bg-surface-2/50"
              onClick={() => setOpenKenh((m) => ({ ...m, [k.key]: !kOpen }))}
            >
              <span className="text-xs text-muted w-3 shrink-0">{kOpen ? "▾" : "▸"}</span>
              <input
                type="checkbox"
                checked={anyOn}
                onClick={(e) => e.stopPropagation()}
                onChange={(e) =>
                  void forKenh(k, (id) => cmd.setWatchedEnabled(id, e.target.checked))
                }
                className="h-4 w-4 shrink-0"
                title={anyOn ? "Kênh đang chạy — bỏ tích để tạm dừng mọi key" : "Kênh đang tạm dừng"}
              />
              <span
                className={`text-sm font-semibold truncate ${anyOn ? "text-fg" : "text-muted"}`}
              >
                {k.name || "(chưa đặt tên)"}
              </span>
              {!anyOn && <span className="text-xs text-muted shrink-0">⏸ tạm dừng</span>}
              <span className="text-xs text-muted shrink-0">{k.keys.length} key</span>
              {dry && <span className="text-xs text-danger font-semibold shrink-0">🔴 HẾT video — đổi key</span>}
              {!dry && mode === "picked" && pickedTotal === 0 && (
                <span className="text-xs text-warning shrink-0">⚠ hết hàng chờ</span>
              )}
              {!dirOk && <span className="text-xs text-warning shrink-0">⚠ thiếu thư mục</span>}
              {k.keys.some((c) => c.lastError) && (
                <span className="text-xs text-danger shrink-0">⚠ lỗi key</span>
              )}
              <span className="flex-1" />
              <span className="text-[11px] text-muted shrink-0">
                {mode === "auto" ? "🤖" : mode === "picked" ? "🎯" : "🆕"} · {k.rep.dailyLimit ?? 1}/ngày
              </span>
            </div>
            {kOpen && (
            <>
            {/* Cài đặt kênh: TÊN · nhóm · chế độ · hạn mức · 📁 · ✕ */}
            <div className="flex items-center gap-2 px-3 pt-1 flex-wrap border-t border-border/60">
              <input
                key={`${k.key}:name`}
                type="text"
                defaultValue={k.name}
                placeholder="đặt tên kênh…"
                onBlur={(e) => {
                  const v = e.target.value.trim();
                  if (v && v !== k.name) void forKenh(k, (id) => cmd.setWatchedTarget(id, v));
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                }}
                className={`flex-1 min-w-[140px] px-2 py-1 rounded-md border text-sm font-semibold ${
                  k.name
                    ? `bg-transparent border-transparent hover:border-border ${anyOn ? "text-fg" : "text-muted"}`
                    : "bg-surface-2 border-warning text-fg"
                }`}
                title="Tên KÊNH của anh (kênh TikTok đích) — video tự về <Trung chuyển gốc>\tên này. Sửa tên là đổi cho mọi key."
              />
              <select
                value={k.group}
                onChange={(e) => {
                  if (e.target.value === "__manage") {
                    setShowGroups(true);
                    return;
                  }
                  const v = e.target.value;
                  void forKenh(k, (id) => cmd.setWatchedGroup(id, v || null));
                }}
                className={`px-1.5 py-1 rounded-md border text-xs shrink-0 max-w-[110px] ${
                  k.group ? "bg-accent/15 text-fg border-accent" : "bg-surface-2 text-muted border-border"
                }`}
                title="Nhóm/quốc gia — quản lý danh sách bằng nút 🏷 Nhóm"
              >
                <option value="">— nhóm —</option>
                {groups.map((g) => (
                  <option key={g} value={g}>{g}</option>
                ))}
                <option value="__manage">➕ Quản lý nhóm…</option>
              </select>
              <select
                value={mode}
                onChange={(e) => {
                  const v = e.target.value as "new" | "picked" | "auto";
                  void forKenh(k, (id) => cmd.setWatchedSourceMode(id, v));
                }}
                className="px-1.5 py-1 rounded-md bg-surface-2 border border-border text-fg text-xs shrink-0"
                title={"Nguồn video của kênh:\n• 🤖 Tự vét — video MỚI trước, không có thì tự lấy video VIEW CAO NHẤT chưa làm (khuyên dùng)\n• 🎯 Hàng chờ — tự tải dần video anh đã tích trong kho\n• Video mới — chỉ tải video mới đăng"}
              >
                <option value="auto">🤖 Tự vét</option>
                <option value="picked">🎯 Hàng chờ</option>
                <option value="new">Video mới</option>
              </select>
              <select
                value={k.rep.dailyLimit ?? 1}
                onChange={(e) => {
                  const n = parseInt(e.target.value, 10);
                  void forKenh(k, (id) => cmd.setWatchedDailyLimit(id, n));
                }}
                className="px-1.5 py-1 rounded-md bg-surface-2 border border-border text-fg text-xs shrink-0"
                title="Số video TỰ TẢI tối đa mỗi ngày cho kênh này"
              >
                <option value={1}>1/ngày</option>
                <option value={2}>2/ngày</option>
                <option value={3}>3/ngày</option>
              </select>
              <button
                onClick={() => {
                  void (async () => {
                    try {
                      const dir = await cmd.pickFolder();
                      if (!dir) return;
                      await forKenh(k, (id) => cmd.setWatchedDestDir(id, dir));
                    } catch (e) {
                      setError(formatErr(e));
                    }
                  })();
                }}
                className={`px-2 py-1 rounded-md text-xs shrink-0 border ${
                  k.rep.destDir ? "bg-accent text-accent-fg border-accent" : "bg-surface-2 text-fg border-border"
                }`}
                title={
                  k.rep.destDir
                    ? `Thư mục chọn tay: ${k.rep.destDir} — bấm để đổi`
                    : "Chọn thư mục TAY cho kênh (bình thường không cần — tên kênh tự tạo thư mục)"
                }
              >
                📁
              </button>
              <button
                onClick={() => void forKenh(k, (id) => cmd.removeWatchedChannel(id))}
                className="px-2 py-1 rounded-md border border-border text-fg text-xs shrink-0 hover:bg-surface-2"
                title={`XÓA kênh "${k.name || "?"}" cùng toàn bộ ${k.keys.length} key nguồn`}
              >
                ✕
              </button>
            </div>
            {/* Dòng trạng thái kênh: thư mục + kho cạn + hàng chờ */}
            <div className="px-3 pb-2 pt-1 text-xs text-muted flex items-center gap-x-2 gap-y-0.5 flex-wrap">
              {k.rep.destDir ? (
                <span className="truncate max-w-[60%]" title={`Thư mục chọn tay (ưu tiên hơn tên kênh): ${k.rep.destDir}`}>
                  📁 {k.rep.destDir}
                  <button
                    onClick={() => void forKenh(k, (id) => cmd.setWatchedDestDir(id, null))}
                    className="ml-1 text-danger hover:underline"
                    title="Bỏ thư mục tay — quay về tự tạo theo tên kênh"
                  >
                    ✕
                  </button>
                </span>
              ) : k.name && settings?.watchRoot ? (
                <span className="truncate max-w-[60%]" title="Thư mục tự tạo theo tên kênh — tool cắt nhận đúng kênh này">
                  📂 {settings.watchRoot}\{k.name}
                </span>
              ) : k.name ? (
                <span className="text-danger">
                  ⚠ CHƯA chọn 📂 Trung chuyển gốc — video sẽ rơi thư mục mặc định!
                </span>
              ) : (
                <span className="text-warning">⚠ đặt tên kênh để video tự về đúng thư mục</span>
              )}
              {dry && (
                <span className="text-danger font-semibold">· 🔴 HẾT video kho — đổi key!</span>
              )}
              {mode === "picked" &&
                (pickedTotal > 0 ? (
                  <span>· 🎯 còn {pickedTotal} chờ làm</span>
                ) : (
                  <span className="text-warning">· ⚠ hết hàng chờ — bấm 🎯 trên key để tích thêm</span>
                ))}
            </div>

            {/* Các KEY nguồn của kênh */}
            <div className="border-t border-border">
              {k.keys.map((c) => (
                <Fragment key={c.id}>
                <div className="flex items-center gap-2 px-3 py-1.5 border-t first:border-t-0 border-border/60">
                  <span className="text-xs shrink-0" title={c.enabled ? "key đang chạy" : "key tạm dừng"}>
                    {c.enabled ? "🔗" : "⏸"}
                  </span>
                  <div className="flex-1 min-w-0">
                    <div className="text-xs text-fg truncate" title={c.url}>
                      {c.title || c.url}
                    </div>
                    <div className="text-[11px] text-muted truncate">
                      {tabLabel(c.tab)} · {c.lastChecked ? `kiểm tra ${timeAgo(c.lastChecked)}` : "chưa kiểm tra"}
                      {typeof c.lastNewCount === "number" && c.lastNewCount > 0 && (
                        <span className="text-success"> · +{c.lastNewCount} mới</span>
                      )}
                      {c.sourceEmpty && <span className="text-danger"> · kho cạn</span>}
                      {c.lastError && (
                        <span className="text-danger" title={c.lastError}> · ⚠ {c.lastError}</span>
                      )}
                    </div>
                  </div>
                  <button
                    onClick={() => void openPicker(c)}
                    className={`px-1.5 py-0.5 rounded text-xs shrink-0 border ${
                      (c.picked?.length ?? 0) > 0
                        ? "bg-accent text-accent-fg border-accent"
                        : "bg-surface-2 text-fg border-border"
                    }`}
                    title="Mở KHO video của key này — xem/tích video nên làm"
                  >
                    🎯
                  </button>
                  <button
                    onClick={() => void toggle(c.id, !c.enabled)}
                    className="px-1.5 py-0.5 rounded border border-border text-fg text-xs shrink-0 hover:bg-surface-2"
                    title={c.enabled ? "Tạm dừng key này" : "Chạy lại key này"}
                  >
                    {c.enabled ? "⏸" : "▶"}
                  </button>
                  <button
                    onClick={() => void remove(c.id)}
                    className="px-1.5 py-0.5 rounded border border-border text-danger text-xs shrink-0 hover:bg-surface-2"
                    title="Xóa key này khỏi kênh"
                  >
                    ✕
                  </button>
                </div>
                {/* Video chờ tải tay (key ở chế độ Chỉ báo) */}
                {c.pending && c.pending.length > 0 && c.pending.map((p) => (
                  <div key={p.id} className="flex items-center gap-2 pl-9 pr-3 py-1 border-t border-border/40">
                    <div className="text-xs text-fg flex-1 min-w-0 truncate" title={p.title}>
                      {p.title || p.url}
                      <span className="text-muted"> · đăng {timeAgo(p.published ?? p.detectedAt)}</span>
                    </div>
                    <button
                      onClick={() => void downloadOne(c.id, p.url)}
                      className="px-2 py-0.5 rounded bg-accent text-accent-fg text-xs shrink-0"
                    >
                      Tải
                    </button>
                    <button
                      onClick={() => void dismissOne(c.id, p.url)}
                      className="px-1.5 py-0.5 rounded border border-border text-fg text-xs shrink-0"
                    >
                      Bỏ qua
                    </button>
                  </div>
                ))}
                </Fragment>
              ))}
              {/* Dán key nguồn mới vào kênh */}
              <div className="flex gap-2 px-3 py-2 border-t border-border/60">
                <input
                  type="url"
                  value={keyInputs[k.key] ?? ""}
                  onChange={(e) => setKeyInputs((m) => ({ ...m, [k.key]: e.target.value }))}
                  onKeyDown={(e) => { if (e.key === "Enter") void addKeyToKenh(k); }}
                  placeholder="＋ dán link key YouTube nguồn mới cho kênh này…"
                  className="flex-1 min-w-[180px] px-2 py-1 rounded-md bg-surface-2 border border-border text-fg text-xs placeholder:text-muted"
                />
                <button
                  onClick={() => void addKeyToKenh(k)}
                  disabled={!(keyInputs[k.key] ?? "").trim() || adding}
                  className="px-2.5 py-1 rounded-md bg-surface-2 border border-border text-fg text-xs disabled:opacity-40"
                >
                  Thêm key
                </button>
              </div>
            </div>
            </>
            )}
          </div>
          )}
          </Fragment>
          );
        })}
      </div>

      {/* Dialog ➕ THÊM KÊNH: tên + nhóm + key nguồn đầu tiên */}
      {addOpen && (
        <div className="fixed inset-0 z-50 bg-black/50 flex items-center justify-center p-4">
          <div className="bg-surface border border-border rounded-lg w-full max-w-md flex flex-col">
            <div className="p-3 border-b border-border">
              <div className="text-sm font-medium text-fg">➕ Thêm kênh</div>
              <div className="text-xs text-muted mt-0.5">
                Kênh = kênh TikTok của anh. Đặt tên, chọn nhóm, dán key YouTube nguồn — app tự lo phần còn lại
                (chế độ 🤖: video mới trước, không có thì vét view cao nhất).
              </div>
            </div>
            <div className="p-3 space-y-2.5 text-sm">
              {/* BƯỚC 1 — THƯ MỤC TRƯỚC: không có chỗ lưu chuẩn thì không cho tạo */}
              <div className={`p-2 rounded-md border ${settings?.watchRoot || addDir ? "border-border bg-surface-2/50" : "border-danger bg-danger/5"}`}>
                <div className="text-xs font-medium text-fg mb-1">1️⃣ Thư mục lưu (bắt buộc chọn trước)</div>
                {addDir ? (
                  <div className="text-xs text-fg truncate">
                    📁 {addDir}
                    <button onClick={() => setAddDir("")} className="ml-1.5 text-danger hover:underline">✕ bỏ</button>
                  </div>
                ) : settings?.watchRoot ? (
                  <div className="text-xs text-fg truncate">
                    📂 {settings.watchRoot}\{addName.trim() || "<tên kênh>"}{" "}
                    <span className="text-muted">(tự tạo theo tên kênh)</span>
                  </div>
                ) : (
                  <div className="text-xs text-danger">
                    ⚠ Chưa có chỗ lưu — chọn 1 trong 2 nút dưới rồi mới tạo được kênh.
                  </div>
                )}
                <div className="flex gap-2 mt-1.5">
                  {!settings?.watchRoot && (
                    <button
                      onClick={() => void pickRoot()}
                      className="px-2 py-1 rounded-md bg-accent text-accent-fg text-xs font-medium"
                      title="Chọn 1 LẦN cho tất cả kênh — mỗi kênh tự có thư mục con theo tên"
                    >
                      📂 Chọn Trung chuyển gốc (khuyên dùng)
                    </button>
                  )}
                  <button
                    onClick={() => {
                      void (async () => {
                        try {
                          const dir = await cmd.pickFolder();
                          if (dir) setAddDir(dir);
                        } catch (e) {
                          setError(formatErr(e));
                        }
                      })();
                    }}
                    className="px-2 py-1 rounded-md bg-surface-2 border border-border text-fg text-xs"
                  >
                    📁 Chọn tay riêng kênh này…
                  </button>
                </div>
              </div>
              <div>
                <div className="text-xs text-muted mb-1">2️⃣ Tên kênh (tên/ID TikTok của anh)</div>
                <input
                  type="text"
                  value={addName}
                  onChange={(e) => setAddName(e.target.value)}
                  placeholder="vd: Kênh Mỹ 1"
                  className="w-full px-3 py-1.5 rounded-md bg-surface-2 border border-border text-fg placeholder:text-muted"
                />
              </div>
              <div>
                <div className="text-xs text-muted mb-1">3️⃣ Nhóm</div>
                <select
                  value={addGrp}
                  onChange={(e) => setAddGrp(e.target.value)}
                  className="w-full px-2 py-1.5 rounded-md bg-surface-2 border border-border text-fg"
                >
                  <option value="">— chưa phân nhóm —</option>
                  {groups.map((g) => (
                    <option key={g} value={g}>{g}</option>
                  ))}
                </select>
              </div>
              <div>
                <div className="text-xs text-muted mb-1">4️⃣ Key nguồn (link kênh YouTube)</div>
                <div className="flex gap-2">
                  <input
                    type="url"
                    value={addUrl}
                    onChange={(e) => setAddUrl(e.target.value)}
                    placeholder="https://www.youtube.com/@TenKenh"
                    className="flex-1 px-3 py-1.5 rounded-md bg-surface-2 border border-border text-fg placeholder:text-muted"
                  />
                  <select
                    value={addTab}
                    onChange={(e) => setAddTab(e.target.value)}
                    className="px-2 py-1.5 rounded-md bg-surface-2 border border-border text-fg"
                    title="Loại video theo dõi"
                  >
                    <option value="all">Tất cả</option>
                    <option value="videos">Video dài</option>
                    <option value="shorts">Shorts</option>
                  </select>
                </div>
              </div>
            </div>
            <div className="p-3 border-t border-border flex gap-2 justify-end">
              <button
                onClick={() => setAddOpen(false)}
                className="px-3 py-1.5 rounded-md border border-border text-fg text-sm hover:bg-surface-2"
              >
                Hủy
              </button>
              <button
                onClick={() => void addKenh()}
                disabled={
                  !addName.trim() || !addUrl.trim() || adding ||
                  (!settings?.watchRoot && !addDir.trim())
                }
                title={
                  !settings?.watchRoot && !addDir.trim()
                    ? "Chọn thư mục lưu trước (bước 1️⃣) rồi mới tạo được"
                    : undefined
                }
                className="px-4 py-1.5 rounded-md bg-accent text-accent-fg text-sm font-medium disabled:opacity-50"
              >
                {adding ? "Đang tạo…" : "Tạo kênh"}
              </button>
            </div>
          </div>
        </div>
      )}

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
