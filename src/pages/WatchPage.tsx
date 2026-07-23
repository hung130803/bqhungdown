import { Fragment, useEffect, useState } from "react";
import * as cmd from "@/ipc/commands";
import { useSettingsStore } from "@/stores/useSettingsStore";
import { useQueueStore } from "@/stores/useQueueStore";
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
  // Quản lý nhiều kênh đa quốc gia: tìm nhanh (nhóm gập/mở thay cho bộ lọc).
  const [search, setSearch] = useState("");

  // ── Dialog "➕ Thêm kênh": tên kênh đích + nhóm + key nguồn đầu tiên ──
  const [addOpen, setAddOpen] = useState(false);
  const [addName, setAddName] = useState("");
  const [addGrp, setAddGrp] = useState("");
  const [addUrl, setAddUrl] = useState("");
  // Mặc định VIDEO DÀI — vét lẫn Shorts vào kênh cắt là hỏng format.
  const [addTab, setAddTab] = useState("videos");
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

  // Kênh đang chạy riêng (▶/🔄) — hiện spinner trên đúng thẻ đó.
  const [busyKenh, setBusyKenh] = useState<Record<string, boolean>>({});

  /** ▶ Chạy RIÊNG kênh này: quét mọi key của nó ngay. */
  const runOne = async (k: Kenh) => {
    if (busyKenh[k.key]) return;
    setBusyKenh((m) => ({ ...m, [k.key]: true }));
    setError(null);
    try {
      for (const c of k.keys.filter((c) => c.enabled)) {
        await cmd.checkWatchedOne(c.id);
      }
      await reload();
    } catch (e) {
      setError(formatErr(e));
    } finally {
      setBusyKenh((m) => ({ ...m, [k.key]: false }));
    }
  };

  /** ➕ Tải THÊM 1 video hôm nay: hoàn 1 suất + vét lấy video kế tiếp theo
   *  view (video cũ đã ở done_ids nên không lặp), GIỮ video đã tải. */
  const redoOne = async (k: Kenh) => {
    if (busyKenh[k.key]) return;
    // Hoàn suất trên đúng key đã tải hôm nay (thường là 1).
    const doneKey = k.keys.find(
      (c) => c.dripDate === todayStr && (c.dripCount ?? 0) > 0,
    );
    if (!doneKey) return;
    setBusyKenh((m) => ({ ...m, [k.key]: true }));
    setError(null);
    try {
      await cmd.redoWatchedToday(doneKey.id);
      await reload();
    } catch (e) {
      setError(formatErr(e));
    } finally {
      setBusyKenh((m) => ({ ...m, [k.key]: false }));
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
  // Lọc theo tên + sắp xếp trong kho (view cao nhất / mới nhất).
  const [pickerSearch, setPickerSearch] = useState("");
  const [pickerSort, setPickerSort] = useState<"views" | "newest">("views");
  // Tab loại video trong kho: video dài / Shorts (kho luôn lấy CẢ 2 để lọc).
  const [pickerType, setPickerType] = useState<"videos" | "shorts">("videos");
  // Tuổi kho đã lưu (giây); null = vừa lấy thật từ mạng.
  const [pickerCacheAge, setPickerCacheAge] = useState<number | null>(null);

  const openPicker = async (c: WatchedChannel, forceRefresh = false) => {
    setPickerFor(c);
    if (!forceRefresh) {
      setPickerSel(c.picked ?? []);
      setPickerSearch("");
      // Mở kho: tab video theo chế độ kênh (kênh Shorts → mở tab Shorts sẵn).
      setPickerType(c.tab === "shorts" ? "shorts" : "videos");
    }
    setPickerVideos([]);
    setPickerErr(null);
    setPickerCacheAge(null);
    setPickerLoading(true);
    try {
      // limit 0 = lấy CẢ kênh → được lưu KHO trên đĩa: lần sau mở tức thì,
      // không load lại 1000 video. 🔄 Làm mới (forceRefresh) mới lấy thật.
      // Luôn lấy "all" để kho có CẢ Video dài + Shorts, rồi lọc theo tab.
      const res = await cmd.fetchChannelVideos(c.url, 0, false, "all", forceRefresh);
      // Giữ nguyên thứ tự kênh (mới→cũ) — sắp xếp lúc hiển thị theo pickerSort.
      setPickerVideos(res.videos);
      setPickerCacheAge(res.cachedAgeSecs ?? null);
    } catch (e) {
      setPickerErr(formatErr(e));
    } finally {
      setPickerLoading(false);
    }
  };

  // Đếm số Video dài / Shorts trong kho (cho nhãn 2 tab).
  const pickerCountVideos = pickerVideos.filter((v) => !v.isShort).length;
  const pickerCountShorts = pickerVideos.filter((v) => v.isShort).length;

  /** Danh sách kho sau LỌC LOẠI (dài/Shorts) + lọc tên + sắp xếp
   *  (dùng chung cho list + nút tích hết). */
  const pickerShown = (() => {
    const term2 = pickerSearch.trim().toLowerCase();
    const base = pickerVideos.filter(
      (v) =>
        (pickerType === "shorts" ? v.isShort : !v.isShort) &&
        (!term2 || (v.title ?? "").toLowerCase().includes(term2)),
    );
    if (pickerSort === "views") {
      return [...base].sort((a, b) => (b.viewCount ?? -1) - (a.viewCount ?? -1));
    }
    return base; // "newest": thứ tự kênh gốc = mới nhất trước
  })();

  /** Tích TOÀN BỘ video đang hiển thị (bỏ video đã làm), theo đúng thứ tự. */
  const pickAllShown = () => {
    if (!pickerFor) return;
    setPickerSel((sel) => {
      const have = new Set(sel.map((p) => p.id));
      const add = pickerShown
        .filter((v) => {
          const id = videoIdOf(v.url);
          return !have.has(id) && !(pickerFor.doneIds?.includes(id) ?? false);
        })
        .map((v) => ({
          id: videoIdOf(v.url),
          url: v.url,
          title: v.title,
          viewCount: v.viewCount ?? null,
          thumbnail: v.thumbnail ?? null,
        }));
      return [...sel, ...add];
    });
  };

  /** Bỏ tích toàn bộ video đang hiển thị. */
  const unpickAllShown = () => {
    const shownIds = new Set(pickerShown.map((v) => videoIdOf(v.url)));
    setPickerSel((sel) => sel.filter((p) => !shownIds.has(p.id)));
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
  // Lượt tải đang chạy (store toàn cục đã nghe event tiến trình) — soi theo
  // thư mục lưu để gắn thanh tiến trình vào đúng thẻ kênh.
  const queueItems = useQueueStore((s) => s.items);
  const activeFor = (dir?: string | null) => {
    if (!dir) return [];
    const norm = (p: string) => p.replace(/[/\\]+$/, "").toLowerCase();
    return queueItems.filter(
      (it) =>
        (it.state === "downloading" || it.state === "queued" || it.state === "paused") &&
        norm(String(it.request.saveFolder)) === norm(dir),
    );
  };
  const nOn = channels.filter((c) => c.enabled).length;
  // Ngày local YYYY-MM-DD — khớp cách backend ghi drip_date.
  const now = new Date();
  const todayStr = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
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

      {/* Thanh công cụ: tìm + quản lý nhóm + ▶ Chạy tất cả */}
      <div className="flex items-center gap-2 flex-wrap text-sm">
        <input
          type="search"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="🔍 Tìm kênh / nhóm…"
          className="flex-1 min-w-[160px] px-3 py-1.5 rounded-md bg-surface border border-border text-fg placeholder:text-muted"
        />
        <button
          onClick={() => setShowGroups(true)}
          className="px-2.5 py-1.5 rounded-md bg-surface-2 border border-border text-fg"
          title="Quản lý nhóm: thêm / đổi tên / xóa"
        >
          🏷 Nhóm
        </button>
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
          const gOpen = term ? true : (openGroups[g] ?? defaultOpen);
          const kOpen = openKenh[k.key] ?? !k.name; // thẻ chưa đặt tên tự xổ
          const isFirstInGroup = i === 0 || (kenhVisible[i - 1].group || "") !== g;
          const inGroup = kenhVisible.filter((x) => (x.group || "") === g);
          const gDry = inGroup.filter((x) => x.keys.some((c) => c.sourceEmpty)).length;
          const dirOk = !!k.rep.destDir;
          // Số thứ tự trong nhóm (1, 2, 3…) để user định vị nhanh 50-300 kênh.
          const stt = kenhVisible.slice(0, i + 1).filter((x) => (x.group || "") === g).length;
          // ✅ hôm nay kênh này ĐÃ tự tải video chưa (đếm theo ngày local —
          // khớp drip_date backend).
          const downloadedToday = k.keys.some(
            (c) => c.dripDate === todayStr && (c.dripCount ?? 0) > 0,
          );
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
                {stt}. {k.name || "(chưa đặt tên)"}
              </span>
              {!anyOn && <span className="text-xs text-muted shrink-0">⏸ tạm dừng</span>}
              <span className="text-xs text-muted shrink-0">{k.keys.length} key</span>
              {/* Trạng thái NGÀY HÔM NAY: ✅ đã tải / ⏳ chưa / 🔴 hết nguồn */}
              {downloadedToday ? (
                <span className="text-xs text-success font-semibold shrink-0" title="Hôm nay kênh này ĐÃ tự tải video — bên cắt cứ thế xử lý">
                  ✅ đã tải hôm nay
                </span>
              ) : dry ? (
                <span className="text-xs text-danger font-semibold shrink-0">🔴 HẾT video — đổi key</span>
              ) : anyOn ? (
                <span className="text-xs text-muted shrink-0" title="Chưa có video nào tải hôm nay — bấm ▶ hoặc chờ lượt quét">
                  ⏳ chưa tải hôm nay
                </span>
              ) : null}
              {/* ▶ chạy RIÊNG kênh này · 🔄 không ưng video hôm nay thì đổi cái khác */}
              {anyOn && !downloadedToday && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    void runOne(k);
                  }}
                  disabled={!!busyKenh[k.key]}
                  className="px-2 py-0.5 rounded-md bg-accent text-accent-fg text-xs font-medium shrink-0 disabled:opacity-50"
                  title="Chạy RIÊNG kênh này ngay: có video mới thì tải, không có thì tự vét theo chế độ"
                >
                  {busyKenh[k.key] ? "…" : "▶"}
                </button>
              )}
              {downloadedToday && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    void redoOne(k);
                  }}
                  disabled={!!busyKenh[k.key]}
                  className="px-2 py-0.5 rounded-md bg-accent/15 border border-accent text-fg text-xs shrink-0 disabled:opacity-50"
                  title={"TẢI THÊM 1 video nữa cho hôm nay (GIỮ video đã tải, lấy thêm cái kế tiếp theo view).\nDùng khi muốn nhiều hơn 1 video/ngày để làm."}
                >
                  {busyKenh[k.key] ? "…" : "➕ Tải thêm"}
                </button>
              )}
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
            {/* Kênh đang TẢI → thanh tiến trình + tốc độ ngay dưới (kể cả khi gập) */}
            {activeFor(k.rep.destDir).map((it) => {
              const pct = it.bytesTotal
                ? Math.min(100, (it.bytesDownloaded / it.bytesTotal) * 100)
                : null;
              return (
                <div key={it.shortId} className="px-3 pb-1.5 -mt-0.5">
                  <div className="flex items-center gap-2 text-[11px] text-muted">
                    <span className="shrink-0">
                      {it.state === "downloading" ? "⬇" : it.state === "paused" ? "⏸" : "🕐"}
                    </span>
                    <span className="truncate flex-1 text-fg" title={it.title}>{it.title}</span>
                    {it.state === "queued" && <span className="shrink-0">chờ tải…</span>}
                    {it.state === "downloading" && it.speedBps != null && (
                      <span className="shrink-0 font-medium text-fg">{fmtSpeed(it.speedBps)}</span>
                    )}
                    {pct != null && <span className="shrink-0">{pct.toFixed(0)}%</span>}
                    {it.state === "downloading" && it.etaSec != null && (
                      <span className="shrink-0">còn {fmtEta(it.etaSec)}</span>
                    )}
                  </div>
                  <div className="h-1 mt-0.5 rounded bg-surface-2 overflow-hidden">
                    <div
                      className="h-full bg-accent transition-all"
                      style={{ width: `${pct ?? (it.state === "downloading" ? 30 : 2)}%` }}
                    />
                  </div>
                </div>
              );
            })}
            {kOpen && (
            <>
            {/* Cài đặt kênh: TÊN · nhóm · chế độ · hạn mức · 📁 · ✕ */}
            <div className="flex items-center gap-2 px-3 pt-1 flex-wrap border-t border-border/60">
              <span className="text-xs text-muted shrink-0">✏ Tên:</span>
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
                className={`flex-1 min-w-[140px] px-2 py-1 rounded-md border text-sm font-semibold bg-surface-2 text-fg ${
                  k.name ? "border-border" : "border-warning"
                }`}
                title="Sửa tên KÊNH của anh (kênh TikTok đích) rồi Enter hoặc bấm ra ngoài để lưu. Áp cho mọi key. Video vẫn nằm ở thư mục 📁 cũ (đổi thư mục ở nút 📁)."
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
              {/* Loại video vét/theo dõi — vét luôn ưu tiên VIDEO DÀI trừ khi chọn Shorts */}
              <select
                value={k.rep.tab || "all"}
                onChange={(e) => {
                  const v = e.target.value as "all" | "videos" | "shorts";
                  void forKenh(k, (id) => cmd.setWatchedTab(id, v));
                }}
                className="px-1.5 py-1 rounded-md bg-surface-2 border border-border text-fg text-xs shrink-0"
                title={"Loại video lấy từ nguồn:\n• Video dài — chỉ video thường (khuyên dùng để cắt)\n• Shorts — chỉ Shorts\n• Tất cả — theo dõi cả 2, nhưng TỰ VÉT vẫn chỉ lấy video dài"}
              >
                <option value="videos">🎬 Video dài</option>
                <option value="shorts">📱 Shorts</option>
                <option value="all">Tất cả</option>
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
              {/* Chất lượng TỐI ĐA — thiếu thì tự lấy mức THẤP hơn gần nhất,
                  không bao giờ tải vượt (1080 không bao giờ ra 1440/4K) */}
              <select
                value={k.rep.maxHeight ?? 1080}
                onChange={(e) => {
                  const n = parseInt(e.target.value, 10);
                  void forKenh(k, (id) => cmd.setWatchedMaxHeight(id, n));
                }}
                className="px-1.5 py-1 rounded-md bg-surface-2 border border-border text-fg text-xs shrink-0"
                title={"Chất lượng TỐI ĐA của kênh (mặc định 1080p).\nNguồn không có mức này → tự lấy mức THẤP hơn gần nhất (vd 720p).\nKHÔNG BAO GIỜ tải vượt mức đã chọn."}
              >
                <option value={480}>480p</option>
                <option value={720}>720p</option>
                <option value={1080}>1080p</option>
                <option value={1440}>1440p</option>
                <option value={2160}>4K</option>
              </select>
              {/* 📁 = MỞ thư mục xem video đã tải (đổi thư mục ở dòng dưới) */}
              {k.rep.destDir && (
                <button
                  onClick={() => void cmd.openInFolder(k.rep.destDir!)}
                  className="px-2 py-1 rounded-md text-xs shrink-0 border bg-surface-2 text-fg border-border"
                  title={`MỞ thư mục xem video đã tải của kênh: ${k.rep.destDir}`}
                >
                  📁 Mở
                </button>
              )}
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
                <span className="truncate max-w-[60%]" title={`Thư mục lưu video của kênh: ${k.rep.destDir}`}>
                  📁 {k.rep.destDir}
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
                    className="ml-1.5 text-accent hover:underline"
                    title="Đổi sang thư mục khác"
                  >
                    đổi…
                  </button>
                </span>
              ) : (
                <span className="text-danger">
                  ⚠ CHƯA chọn thư mục — video sẽ rơi thư mục mặc định!
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
                    className="ml-1.5 text-accent hover:underline"
                  >
                    📁 Chọn thư mục…
                  </button>
                </span>
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
              {/* BƯỚC 1 — THƯ MỤC TRƯỚC: chưa chọn thì không cho tạo */}
              <div className={`p-2 rounded-md border ${addDir ? "border-border bg-surface-2/50" : "border-danger bg-danger/5"}`}>
                <div className="text-xs font-medium text-fg mb-1">1️⃣ Thư mục lưu video của kênh (bắt buộc)</div>
                {addDir ? (
                  <div className="text-xs text-fg truncate">
                    📁 {addDir}
                    <button onClick={() => setAddDir("")} className="ml-1.5 text-danger hover:underline">✕ bỏ</button>
                  </div>
                ) : (
                  <div className="text-xs text-danger">
                    ⚠ Chưa chọn — video của kênh này sẽ lưu vào đây, tool cắt đọc từ đây.
                  </div>
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
                  className="mt-1.5 px-2 py-1 rounded-md bg-accent text-accent-fg text-xs font-medium"
                >
                  📁 Chọn thư mục…
                </button>
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
                <div className="text-xs text-muted mb-1">3️⃣ Nhóm (bắt buộc — Mỹ, Hàn, Nhật…)</div>
                <div className="flex gap-2">
                  <select
                    value={addGrp}
                    onChange={(e) => setAddGrp(e.target.value)}
                    className={`flex-1 px-2 py-1.5 rounded-md border text-fg ${addGrp ? "bg-surface-2 border-border" : "bg-danger/5 border-danger"}`}
                  >
                    <option value="">— chọn nhóm —</option>
                    {groups.map((g) => (
                      <option key={g} value={g}>{g}</option>
                    ))}
                  </select>
                  <button
                    onClick={() => setShowGroups(true)}
                    className="px-2 py-1.5 rounded-md bg-surface-2 border border-border text-fg text-xs shrink-0"
                    title="Chưa có nhóm nào ưng? Thêm nhóm mới tại đây"
                  >
                    🏷 Thêm nhóm…
                  </button>
                </div>
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
                disabled={!addName.trim() || !addUrl.trim() || !addGrp || !addDir.trim() || adding}
                title={
                  !addDir.trim()
                    ? "Chọn thư mục lưu trước (bước 1️⃣)"
                    : !addGrp
                      ? "Chọn nhóm (bước 3️⃣)"
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
            {/* 2 tab loại: Video dài / Shorts — tự chọn phần muốn tích */}
            <div className="px-3 pt-2 flex items-center gap-2">
              <button
                onClick={() => setPickerType("videos")}
                className={`px-3 py-1 rounded-md text-xs font-medium border ${
                  pickerType === "videos"
                    ? "bg-accent text-accent-fg border-accent"
                    : "bg-surface-2 text-fg border-border"
                }`}
              >
                🎬 Video dài ({pickerCountVideos})
              </button>
              <button
                onClick={() => setPickerType("shorts")}
                className={`px-3 py-1 rounded-md text-xs font-medium border ${
                  pickerType === "shorts"
                    ? "bg-accent text-accent-fg border-accent"
                    : "bg-surface-2 text-fg border-border"
                }`}
              >
                📱 Shorts ({pickerCountShorts})
              </button>
            </div>
            {/* Thanh công cụ kho: lọc tên + sắp xếp + tích hết / bỏ tích */}
            <div className="px-3 py-2 border-b border-border flex items-center gap-2 flex-wrap">
              <input
                type="search"
                value={pickerSearch}
                onChange={(e) => setPickerSearch(e.target.value)}
                placeholder="🔍 Lọc theo tên video…"
                className="flex-1 min-w-[140px] px-2.5 py-1 rounded-md bg-surface-2 border border-border text-fg text-xs placeholder:text-muted"
              />
              <select
                value={pickerSort}
                onChange={(e) => setPickerSort(e.target.value as "views" | "newest")}
                className="px-1.5 py-1 rounded-md bg-surface-2 border border-border text-fg text-xs shrink-0"
                title="Thứ tự hiển thị kho"
              >
                <option value="views">👁 View cao nhất</option>
                <option value="newest">🕐 Mới nhất</option>
              </select>
              <button
                onClick={pickAllShown}
                disabled={pickerShown.length === 0}
                className="px-2 py-1 rounded-md bg-surface-2 border border-border text-fg text-xs shrink-0 disabled:opacity-40"
                title="Tích toàn bộ video đang hiển thị (bỏ qua video đã làm) — theo đúng thứ tự đang xem"
              >
                ☑ Tích hết ({pickerShown.filter((v) => !(pickerFor.doneIds?.includes(videoIdOf(v.url)) ?? false)).length})
              </button>
              <button
                onClick={unpickAllShown}
                disabled={pickerSel.length === 0}
                className="px-2 py-1 rounded-md bg-surface-2 border border-border text-fg text-xs shrink-0 disabled:opacity-40"
                title="Bỏ tích toàn bộ video đang hiển thị"
              >
                ☐ Bỏ tích
              </button>
              {/* Kho đã lưu → mở tức thì; 🔄 mới đi lấy lại từ mạng */}
              {pickerCacheAge != null && (
                <span className="text-[11px] text-muted shrink-0" title="Danh sách lấy từ kho đã lưu trên máy — mở tức thì, không load lại">
                  ⚡ kho lưu {fmtAgeSecs(pickerCacheAge)}
                </span>
              )}
              <button
                onClick={() => {
                  if (pickerFor) void openPicker(pickerFor, true);
                }}
                disabled={pickerLoading}
                className="px-2 py-1 rounded-md bg-surface-2 border border-border text-fg text-xs shrink-0 disabled:opacity-40"
                title="Lấy lại danh sách MỚI từ kênh nguồn (cập nhật video mới đăng + số view) — ghi đè kho đã lưu"
              >
                🔄 Làm mới
              </button>
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
              {!pickerLoading && pickerVideos.length > 0 && pickerShown.length === 0 && (
                <div className="p-6 text-center text-sm text-muted">Không video nào khớp ô lọc.</div>
              )}
              {pickerShown.map((v) => {
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
                    {/* Ảnh video — nhìn phát biết cái nào đáng làm */}
                    {v.thumbnail ? (
                      <img
                        src={v.thumbnail}
                        loading="lazy"
                        alt=""
                        className="w-24 h-[54px] object-cover rounded shrink-0 bg-surface-2"
                      />
                    ) : (
                      <div className="w-24 h-[54px] rounded bg-surface-2 shrink-0 flex items-center justify-center text-lg">
                        🎬
                      </div>
                    )}
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

/** Tuổi cache người đọc được: `vừa xong`, `25 phút trước`, `3 giờ trước`. */
function fmtAgeSecs(sec: number): string {
  if (sec < 60) return "vừa xong";
  const m = Math.floor(sec / 60);
  if (m < 60) return `${m} phút trước`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h} giờ trước`;
  return `${Math.floor(h / 24)} ngày trước`;
}

/** Tốc độ tải người đọc được: `3.2 MB/s`, `850 KB/s`. */
function fmtSpeed(bps: number): string {
  if (bps >= 1_048_576) return `${(bps / 1_048_576).toFixed(1)} MB/s`;
  if (bps >= 1024) return `${Math.round(bps / 1024)} KB/s`;
  return `${Math.round(bps)} B/s`;
}

/** Thời gian còn lại: `45s`, `2p30`, `1g05`. */
function fmtEta(sec: number): string {
  if (sec < 60) return `${Math.round(sec)}s`;
  const m = Math.floor(sec / 60);
  if (m < 60) return `${m}p${String(Math.round(sec % 60)).padStart(2, "0")}`;
  return `${Math.floor(m / 60)}g${String(m % 60).padStart(2, "0")}`;
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
