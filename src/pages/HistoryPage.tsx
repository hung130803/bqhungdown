import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { HistoryEntry, HistoryStatus } from "@/types/models";
import { useHistoryStore } from "@/stores/useHistoryStore";
import { HistoryRow, type HistoryViewMode } from "@/components/HistoryRow";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ContextMenu, type ContextMenuItem } from "@/components/ContextMenu";
import { platformInfo } from "@/lib/platforms";
import * as cmd from "@/ipc/commands";

type StatusFilter = "all" | HistoryStatus;
type SortKey = "time" | "channel" | "title";
type SortDir = "asc" | "desc";
type EditFilter = "all" | "edited" | "not_edited";

const VIEW_KEY = "bqhungdown.history.view";

const DAY_MS = 24 * 60 * 60 * 1000;
const RUBBER_BAND_THRESHOLD = 5; // px to drag before showing selection box

function bucketLabel(ts: number, now: number): string {
  const today = new Date(now);
  today.setHours(0, 0, 0, 0);
  const todayMs = today.getTime();
  if (ts >= todayMs) return "Hôm nay";
  if (ts >= todayMs - DAY_MS) return "Hôm qua";
  if (ts >= todayMs - 7 * DAY_MS) return "Tuần này";
  if (ts >= todayMs - 30 * DAY_MS) return "Tháng này";
  return "Cũ hơn";
}
const DATE_ORDER = ["Hôm nay", "Hôm qua", "Tuần này", "Tháng này", "Cũ hơn"];

const STATUS_LABELS: Record<HistoryStatus, string> = {
  completed: "Hoàn tất",
  failed: "Thất bại",
  cancelled: "Đã huỷ",
};

interface RubberBandState {
  active: boolean;
  /** Anchor pinned in page (content) coordinates — survives scrolling. */
  anchorPageX: number;
  anchorPageY: number;
  /** Latest cursor position in viewport coordinates. */
  curClientX: number;
  curClientY: number;
  /** True once user has moved past the threshold — only then we draw the box. */
  visible: boolean;
  /** True when the mousedown happened on a row's empty area (not background). */
  startedOnRow: boolean;
  /** When the click started on a row, this is its short id so a plain click
   *  (no drag) can toggle that exact row. */
  startedOnRowId: string | null;
}

const INITIAL_BAND: RubberBandState = {
  active: false,
  anchorPageX: 0,
  anchorPageY: 0,
  curClientX: 0,
  curClientY: 0,
  visible: false,
  startedOnRow: false,
  startedOnRowId: null,
};

export function HistoryPage() {
  const { t } = useTranslation();
  const entries = useHistoryStore((s) => s.entries);
  const query = useHistoryStore((s) => s.query);
  const setQuery = useHistoryStore((s) => s.setQuery);
  const refresh = useHistoryStore((s) => s.refresh);
  const clearAll = useHistoryStore((s) => s.clearAll);

  const [confirmOpen, setConfirmOpen] = useState(false);
  const [confirmMode, setConfirmMode] = useState<"all" | "selected">("all");
  const [alsoDeleteFiles, setAlsoDeleteFiles] = useState(false);

  // Filters / sort state.
  const [platform, setPlatform] = useState<string>("all");
  const [status, setStatus] = useState<StatusFilter>("all");
  const [editFilter, setEditFilter] = useState<EditFilter>("all");
  const [sortKey, setSortKey] = useState<SortKey>("time");
  const [sortDir, setSortDir] = useState<SortDir>("desc");
  const [groupByDate, setGroupByDate] = useState(true);
  const [view, setView] = useState<HistoryViewMode>(() => {
    const saved = (typeof localStorage !== "undefined" && localStorage.getItem(VIEW_KEY)) || "list";
    return (saved === "compact" || saved === "grid" ? saved : "list") as HistoryViewMode;
  });
  useEffect(() => {
    try {
      localStorage.setItem(VIEW_KEY, view);
    } catch {
      // ignore
    }
  }, [view]);

  // Multi-select state.
  const [selected, setSelected] = useState<Set<string>>(new Set());
  // Ref mirror so global event handlers can read selection synchronously
  // without re-registering on every state change.
  const selectedRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    selectedRef.current = selected;
  }, [selected]);

  // Right-click context menu state.
  const [ctxMenu, setCtxMenu] = useState<{
    x: number;
    y: number;
    targetIds: string[];
  } | null>(null);

  // Rubber-band selection state. We store ALL state in a ref to avoid
  // re-renders on every mousemove; only the box overlay reads from a
  // mirrored state slice when the band becomes visible.
  const bandRef = useRef<RubberBandState>(INITIAL_BAND);
  const [bandTick, setBandTick] = useState(0); // bumped to redraw the box
  const containerRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  /** Distinct platforms present in the data. */
  const platforms = useMemo(() => {
    const map = new Map<string, string>();
    for (const e of entries) {
      map.set(e.extractor, platformInfo(e.extractor).label);
    }
    return Array.from(map.entries()).map(([id, label]) => ({ id, label }));
  }, [entries]);

  const availableStatuses = useMemo(() => {
    const set = new Set<HistoryStatus>();
    for (const e of entries) set.add(e.status);
    return set;
  }, [entries]);

  const filtered = useMemo(() => {
    let out = entries;
    if (platform !== "all") out = out.filter((e) => e.extractor === platform);
    if (status !== "all") out = out.filter((e) => e.status === status);
    if (editFilter === "edited") out = out.filter((e) => e.edited);
    else if (editFilter === "not_edited") out = out.filter((e) => !e.edited);
    const factor = sortDir === "asc" ? 1 : -1;
    out = [...out].sort((a, b) => {
      switch (sortKey) {
        case "channel":
          return (
            factor *
            (a.channel ?? "").localeCompare(b.channel ?? "", "vi", { sensitivity: "base" })
          );
        case "title":
          return (
            factor *
            (a.title ?? "").localeCompare(b.title ?? "", "vi", { sensitivity: "base" })
          );
        case "time":
        default: {
          const ta = new Date(a.finishedAt).getTime();
          const tb = new Date(b.finishedAt).getTime();
          return factor * (ta - tb);
        }
      }
    });
    return out;
  }, [entries, platform, status, editFilter, sortKey, sortDir]);

  const groups = useMemo(() => {
    if (sortKey === "time") {
      if (!groupByDate) return null;
      const now = Date.now();
      const map = new Map<string, HistoryEntry[]>();
      for (const e of filtered) {
        const k = bucketLabel(new Date(e.finishedAt).getTime(), now);
        const arr = map.get(k) ?? [];
        arr.push(e);
        map.set(k, arr);
      }
      return DATE_ORDER.filter((b) => map.has(b)).map((b) => ({
        bucket: b,
        items: map.get(b) as HistoryEntry[],
      }));
    }
    if (sortKey === "channel") {
      const map = new Map<string, HistoryEntry[]>();
      for (const e of filtered) {
        const k = e.channel?.trim() || "Không rõ kênh";
        const arr = map.get(k) ?? [];
        arr.push(e);
        map.set(k, arr);
      }
      return Array.from(map.entries()).map(([bucket, items]) => ({ bucket, items }));
    }
    return null;
  }, [filtered, groupByDate, sortKey]);

  // Rubber-band helpers ──────────────────────────────────────────────────────

  /** Run a hit-test of the current band against the items inside container.
   *  Computed entirely in viewport coordinates (clientX/Y) for accuracy under
   *  scroll. Anchor is stored in page coordinates so it survives scrolling
   *  while the drag is active. */
  const recomputeBandSelection = () => {
    const band = bandRef.current;
    // Convert anchor (page) → client by subtracting current scroll.
    const anchorClientX = band.anchorPageX - window.scrollX;
    const anchorClientY = band.anchorPageY - window.scrollY;
    const x1 = Math.min(anchorClientX, band.curClientX);
    const y1 = Math.min(anchorClientY, band.curClientY);
    const x2 = Math.max(anchorClientX, band.curClientX);
    const y2 = Math.max(anchorClientY, band.curClientY);

    const next = new Set<string>();
    // Hit-test EVERY row on the page (across groups, not just one container)
    // so kéo từ trên xuống dưới hoặc qua biên section đều bắt được.
    document.querySelectorAll<HTMLElement>("[data-history-id]").forEach((el) => {
      const id = el.getAttribute("data-history-id");
      if (!id) return;
      const r = el.getBoundingClientRect();
      // Any pixel overlap between band rectangle and row rectangle.
      const overlaps = !(x2 < r.left || x1 > r.right || y2 < r.top || y1 > r.bottom);
      if (overlaps) next.add(id);
    });
    setSelected(next);
  };

  // Global mousedown so user can start a rubber-band even when clicking
  // outside the inner container (e.g. the empty margins on either side of
  // the page). We still ignore clicks on interactive elements anywhere on
  // the page so toolbar buttons / dropdowns keep working normally.
  //
  // Ctrl+click on a row toggles that single row without affecting the rest —
  // matches the Windows Explorer mental model. We intercept at mousedown so
  // the click event on the row's body doesn't end up triggering "open file".
  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (e.button !== 0) return;
      const target = e.target as HTMLElement;
      // Ignore mousedown that originates inside the page chrome (header/nav)
      // or interactive widgets — let those handle their own clicks.
      // We also exempt thumbnails so they can start a native OS-level drag
      // out to external apps (CapCut, Premiere, Explorer).
      if (target.closest("button, a, input, label, select, textarea, video, header, nav")) {
        return;
      }
      // Ctrl/Cmd+click on a row → toggle just that row, no drag, no rubber-band.
      // Handle this BEFORE the "skip selected row" check so users can ctrl-click
      // a selected row to deselect it.
      if (e.ctrlKey || e.metaKey) {
        const rowEl = target.closest("[data-history-id]") as HTMLElement | null;
        if (rowEl) {
          const id = rowEl.getAttribute("data-history-id");
          if (id) {
            e.preventDefault();
            e.stopPropagation();
            setSelected((prev) => {
              const next = new Set(prev);
              if (next.has(id)) next.delete(id);
              else next.add(id);
              return next;
            });
            return;
          }
        }
      }

      // Mousedown trên row đã tích → row tự xử lý drag-out (xem
      // HistoryRow.useRowDragOut). Không bắt đầu rubber-band ở đây.
      // Mousedown trên row CHƯA tích / không có file → rubber-band như cũ
      // ⇒ user khoanh chọn được trực tiếp trên thân của các row chưa chọn.
      const onSelectedRow = !!target.closest("[data-history-id][data-selected='true']");
      if (onSelectedRow) return;

      // Allow dragging to start anywhere outside interactive widgets.
      // Block image clicks ONLY when not in modifier mode so users can still
      // open thumbnails normally; here we just prevent default to suppress
      // image-drag and text selection.
      e.preventDefault();
      const rowEl = target.closest("[data-history-id]") as HTMLElement | null;
      const startedOnRow = !!rowEl;
      const startedOnRowId = rowEl?.getAttribute("data-history-id") ?? null;
      bandRef.current = {
        active: true,
        anchorPageX: e.pageX,
        anchorPageY: e.pageY,
        curClientX: e.clientX,
        curClientY: e.clientY,
        visible: false,
        startedOnRow,
        startedOnRowId,
      };
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, []);

  // Global mousemove + mouseup handlers (so user can drag past container edges).
  useEffect(() => {
    let scrollRAF: number | null = null;
    const tickAutoScroll = () => {
      const band = bandRef.current;
      if (!band.active) return;
      const EDGE = 50; // px from viewport edge to trigger auto-scroll
      const SPEED = 16; // px per frame
      const vh = window.innerHeight;
      let dy = 0;
      if (band.curClientY < EDGE) dy = -SPEED * (1 - band.curClientY / EDGE);
      else if (band.curClientY > vh - EDGE)
        dy = SPEED * (1 - (vh - band.curClientY) / EDGE);
      if (dy !== 0) {
        window.scrollBy(0, dy);
        recomputeBandSelection();
        setBandTick((n) => n + 1);
      }
      scrollRAF = requestAnimationFrame(tickAutoScroll);
    };

    const onMove = (e: MouseEvent) => {
      const band = bandRef.current;
      if (!band.active) return;
      // Convert client → updated page so anchor remains pinned even after scroll.
      band.curClientX = e.clientX;
      band.curClientY = e.clientY;
      const dxPage = Math.abs(e.pageX - band.anchorPageX);
      const dyPage = Math.abs(e.pageY - band.anchorPageY);
      if (!band.visible && (dxPage > RUBBER_BAND_THRESHOLD || dyPage > RUBBER_BAND_THRESHOLD)) {
        band.visible = true;
        // Once we cross the threshold, also start the auto-scroll RAF loop.
        if (scrollRAF === null) scrollRAF = requestAnimationFrame(tickAutoScroll);
      }
      if (band.visible) {
        recomputeBandSelection();
        setBandTick((n) => n + 1);
      }
    };
    const onUp = () => {
      const band = bandRef.current;
      if (!band.active) return;
      // Plain click without drag — handle based on where the click happened:
      //   • on a row  → toggle just that row (Windows Explorer style click)
      //   • elsewhere → clear the whole selection
      if (!band.visible) {
        if (band.startedOnRowId) {
          const id = band.startedOnRowId;
          setSelected((prev) => {
            const next = new Set(prev);
            if (next.has(id)) next.delete(id);
            else next.add(id);
            return next;
          });
        } else if (!band.startedOnRow) {
          setSelected(new Set());
        }
      }
      bandRef.current = INITIAL_BAND;
      if (scrollRAF !== null) {
        cancelAnimationFrame(scrollRAF);
        scrollRAF = null;
      }
      setBandTick((n) => n + 1);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      if (scrollRAF !== null) cancelAnimationFrame(scrollRAF);
    };
    // recomputeBandSelection reads from refs so we don't need it in deps.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Checkbox click on individual rows still works (toggle that row only).
  const onCheckboxMouseDown = (id: string, e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const allSelected = filtered.length > 0 && selected.size === filtered.length;
  const someSelected = selected.size > 0 && !allSelected;
  const toggleAll = () => {
    setSelected((prev) =>
      prev.size === filtered.length ? new Set() : new Set(filtered.map((e) => e.shortId)),
    );
  };

  // Confirm / delete actions ─────────────────────────────────────────────────
  const onClearAllClick = () => {
    if (entries.length === 0) return;
    setConfirmMode("all");
    setAlsoDeleteFiles(false);
    setConfirmOpen(true);
  };
  const onDeleteSelectedClick = () => {
    if (selected.size === 0) return;
    setConfirmMode("selected");
    setAlsoDeleteFiles(false);
    setConfirmOpen(true);
  };
  const onConfirmClear = async () => {
    if (confirmMode === "all") {
      await clearAll(alsoDeleteFiles);
    } else {
      const ids = Array.from(selected);
      try {
        await cmd.deleteHistoryEntries(ids, alsoDeleteFiles);
      } catch (e) {
        console.error("[history] batch delete failed", e);
      }
      setSelected(new Set());
      await refresh();
    }
    setConfirmOpen(false);
  };

  // Right-click anywhere on a history row → open the context menu. If the
  // row is part of the current multi-selection, operate on the whole batch;
  // otherwise just on that single row.
  useEffect(() => {
    const onCtx = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      const row = target.closest("[data-history-id]") as HTMLElement | null;
      if (!row) return;
      const id = row.getAttribute("data-history-id");
      if (!id) return;
      e.preventDefault();
      const targetIds =
        selectedRef.current.has(id) && selectedRef.current.size > 0
          ? Array.from(selectedRef.current)
          : [id];
      setCtxMenu({ x: e.clientX, y: e.clientY, targetIds });
    };
    window.addEventListener("contextmenu", onCtx);
    return () => window.removeEventListener("contextmenu", onCtx);
  }, []);

  // Ctrl+A while History page is mounted → select every filtered row instead
  // of letting the browser select all text. Re-runs when filtered list shape
  // changes so it always reflects the current view.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      if (e.key !== "a" && e.key !== "A") return;
      // Don't hijack if user is typing in an input / textarea.
      const tag = (document.activeElement?.tagName || "").toLowerCase();
      if (tag === "input" || tag === "textarea") return;
      e.preventDefault();
      setSelected(new Set(filtered.map((it) => it.shortId)));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [filtered]);

  /** Toggle edit flag on a batch and refresh the list. */
  const handleSetEdited = async (ids: string[], edited: boolean) => {
    if (ids.length === 0) return;
    try {
      await cmd.setHistoryEdited(ids, edited);
    } catch (err) {
      console.error("[history] set edited failed", err);
    }
    await refresh();
  };

  // Render row directly — HistoryRow now puts data-history-id on its root
  // along with data-file-path/data-selected, so the multi-file drag selector
  // `[data-history-id][data-selected='true']` matches the same element.
  // Plain-click toggling is handled in the page's mouseup (onUp).
  const renderRow = (e: HistoryEntry) => (
    <HistoryRow
      key={e.shortId}
      entry={e}
      view={view}
      selectable
      selected={selected.has(e.shortId)}
      onCheckboxMouseDown={onCheckboxMouseDown}
      onPlainClickSelected={(id) => {
        // Untick that single row.
        setSelected((prev) => {
          const next = new Set(prev);
          next.delete(id);
          return next;
        });
      }}
    />
  );

  const renderItems = (items: HistoryEntry[]) =>
    view === "grid" ? (
      <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 gap-3">
        {items.map(renderRow)}
      </div>
    ) : view === "compact" ? (
      <div className="space-y-1">{items.map(renderRow)}</div>
    ) : (
      <div className="space-y-2">{items.map(renderRow)}</div>
    );

  // Compute the band overlay box (viewport coords) — silenced by `bandTick`.
  void bandTick;
  const band = bandRef.current;
  const showBand = band.active && band.visible;

  // While the band is visible, lock text-selection on the entire document so
  // images / titles don't get highlighted as the cursor passes through them.
  useEffect(() => {
    if (showBand) {
      const prev = document.body.style.userSelect;
      document.body.style.userSelect = "none";
      return () => {
        document.body.style.userSelect = prev;
      };
    }
  }, [showBand]);

  const bandStyle: React.CSSProperties | undefined = showBand
    ? (() => {
        const anchorClientX = band.anchorPageX - window.scrollX;
        const anchorClientY = band.anchorPageY - window.scrollY;
        const x = Math.min(anchorClientX, band.curClientX);
        const y = Math.min(anchorClientY, band.curClientY);
        const w = Math.abs(band.curClientX - anchorClientX);
        const h = Math.abs(band.curClientY - anchorClientY);
        return { left: x, top: y, width: w, height: h, position: "fixed" };
      })()
    : undefined;

  return (
    <div
      ref={containerRef}
      className={`space-y-4 max-w-[1400px] mx-auto relative ${showBand ? "select-none" : ""}`}
    >
      {/* Search + clear-all toolbar */}
      <div className="flex items-center gap-3 flex-wrap">
        <input
          type="search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("history.searchPlaceholder")}
          className="flex-1 min-w-[200px] px-3 py-2 rounded-md bg-surface border border-border text-fg placeholder:text-muted"
        />
        {entries.length > 0 && (
          <button
            onClick={onClearAllClick}
            className="px-3 py-2 rounded-md border border-danger text-danger text-sm hover:bg-danger/10 shrink-0"
          >
            Xoá tất cả ({entries.length})
          </button>
        )}
      </div>

      {/* Filters / sort row */}
      {entries.length > 0 && (
        <div className="flex items-center gap-2 flex-wrap text-sm">
          {platforms.length > 1 && (
            <select
              value={platform}
              onChange={(e) => setPlatform(e.target.value)}
              className="px-2 py-1.5 rounded-md bg-surface border border-border"
            >
              <option value="all">Tất cả nền tảng</option>
              {platforms.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))}
            </select>
          )}
          {availableStatuses.size > 1 && (
            <select
              value={status}
              onChange={(e) => setStatus(e.target.value as StatusFilter)}
              className="px-2 py-1.5 rounded-md bg-surface border border-border"
            >
              <option value="all">Tất cả trạng thái</option>
              {(["completed", "failed", "cancelled"] as HistoryStatus[])
                .filter((s) => availableStatuses.has(s))
                .map((s) => (
                  <option key={s} value={s}>
                    {STATUS_LABELS[s]}
                  </option>
                ))}
            </select>
          )}
          <select
            value={editFilter}
            onChange={(e) => setEditFilter(e.target.value as EditFilter)}
            className="px-2 py-1.5 rounded-md bg-surface border border-border"
            title="Lọc theo trạng thái edit"
          >
            <option value="all">Edit: Tất cả</option>
            <option value="not_edited">Edit: Chưa edit</option>
            <option value="edited">Edit: Đã edit</option>
          </select>
          <select
            value={sortKey}
            onChange={(e) => setSortKey(e.target.value as SortKey)}
            className="px-2 py-1.5 rounded-md bg-surface border border-border"
          >
            <option value="time">Sắp xếp: Thời gian</option>
            <option value="channel">Sắp xếp: Kênh (gom nhóm)</option>
            <option value="title">Sắp xếp: Tiêu đề (A→Z)</option>
          </select>
          <button
            onClick={() => setSortDir((d) => (d === "asc" ? "desc" : "asc"))}
            className="px-2 py-1.5 rounded-md border border-border hover:bg-surface-2"
            title={sortDir === "asc" ? "Đảo chiều thành Giảm dần" : "Đảo chiều thành Tăng dần"}
          >
            {sortDir === "asc" ? "↑ Tăng" : "↓ Giảm"}
          </button>
          {sortKey === "time" && (
            <label className="inline-flex items-center gap-1.5 px-2 py-1.5 rounded-md border border-border cursor-pointer">
              <input
                type="checkbox"
                checked={groupByDate}
                onChange={(e) => setGroupByDate(e.target.checked)}
                className="h-3.5 w-3.5"
              />
              <span>Nhóm theo ngày</span>
            </label>
          )}

          <div className="ml-auto inline-flex rounded-md border border-border overflow-hidden">
            {(
              [
                { id: "list" as const, icon: "≡", label: "Danh sách" },
                { id: "compact" as const, icon: "—", label: "Gọn" },
                { id: "grid" as const, icon: "▦", label: "Lưới" },
              ]
            ).map((v) => (
              <button
                key={v.id}
                onClick={() => setView(v.id)}
                className={`px-2.5 py-1.5 text-xs ${
                  view === v.id ? "bg-accent text-accent-fg" : "hover:bg-surface-2"
                }`}
                title={v.label}
              >
                {v.icon}
              </button>
            ))}
          </div>
        </div>
      )}

      {/* Selection toolbar */}
      {entries.length > 0 && (
        <div className="flex items-center gap-2 text-sm flex-wrap">
          <label className="inline-flex items-center gap-2 px-2 py-1.5 rounded-md border border-border cursor-pointer">
            <input
              type="checkbox"
              checked={allSelected}
              ref={(el) => {
                if (el) el.indeterminate = someSelected;
              }}
              onChange={toggleAll}
              className="h-3.5 w-3.5"
            />
            <span>
              {selected.size > 0
                ? `Đã chọn ${selected.size} / ${filtered.length}`
                : `Chọn tất cả (${filtered.length})`}
            </span>
          </label>
          {selected.size > 0 && (
            <>
              <button
                onClick={onDeleteSelectedClick}
                className="px-3 py-1.5 rounded-md border border-danger text-danger hover:bg-danger/10"
              >
                Xoá {selected.size} mục đã chọn
              </button>
              <button
                onClick={() => setSelected(new Set())}
                className="px-3 py-1.5 rounded-md border border-border hover:bg-surface-2"
              >
                Bỏ chọn
              </button>
            </>
          )}
          <span className="text-muted text-xs ml-auto">
            Mẹo: Bấm + kéo để khoanh chọn (cả khi đè lên video). Tích xong → kéo từ video đã tích để thả vào CapCut.
          </span>
        </div>
      )}

      {/* List */}
      {filtered.length === 0 ? (
        <p className="text-muted text-center py-12">{t("history.empty")}</p>
      ) : groups ? (
        <div className="space-y-5">
          {groups.map((g) => (
            <section key={g.bucket} className="space-y-2">
              <h3 className="text-xs uppercase tracking-wide text-muted px-1">
                {g.bucket} <span className="text-fg/60">({g.items.length})</span>
              </h3>
              {renderItems(g.items)}
            </section>
          ))}
        </div>
      ) : (
        renderItems(filtered)
      )}

      {/* Rubber-band overlay — fixed to viewport so it follows scrolling. */}
      {showBand && bandStyle && (
        <div
          aria-hidden
          className="pointer-events-none border-2 border-dashed border-accent bg-accent/10 z-50"
          style={bandStyle}
        />
      )}

      {ctxMenu && (() => {
        const targetEntries = entries.filter((e) => ctxMenu.targetIds.includes(e.shortId));
        const allEdited = targetEntries.length > 0 && targetEntries.every((e) => e.edited);
        const single = targetEntries.length === 1 ? targetEntries[0] : null;
        const items: ContextMenuItem[] = [
          {
            id: "mark-edited",
            icon: allEdited ? "↺" : "✓",
            label: allEdited
              ? `Bỏ đánh dấu đã edit${targetEntries.length > 1 ? ` (${targetEntries.length})` : ""}`
              : `Đánh dấu đã edit${targetEntries.length > 1 ? ` (${targetEntries.length})` : ""}`,
            onClick: () => void handleSetEdited(ctxMenu.targetIds, !allEdited),
          },
        ];
        if (single && single.status === "completed") {
          items.push({
            id: "open-file",
            icon: "▶",
            label: "Mở video",
            onClick: () => {
              if (single.outputPath) void cmd.openFile(single.outputPath);
              else if (single.saveFolder) void cmd.openInFolder(single.saveFolder);
            },
          });
          items.push({
            id: "open-folder",
            icon: "📁",
            label: "Mở thư mục chứa file",
            onClick: () => {
              if (single.outputPath) void cmd.openInFolder(single.outputPath);
              else if (single.saveFolder) void cmd.openInFolder(single.saveFolder);
            },
          });
          items.push({
            id: "copy-title",
            icon: "📋",
            label: "Sao chép tiêu đề",
            onClick: () => void navigator.clipboard.writeText(single.title || single.url),
          });
        }
        items.push({
          id: "delete",
          icon: "🗑",
          label: `Xoá khỏi lịch sử${targetEntries.length > 1 ? ` (${targetEntries.length})` : ""}`,
          danger: true,
          separator: true,
          onClick: () => {
            void cmd.deleteHistoryEntries(ctxMenu.targetIds, false).then(() => {
              setSelected(new Set());
              void refresh();
            });
          },
        });
        return <ContextMenu x={ctxMenu.x} y={ctxMenu.y} items={items} onClose={() => setCtxMenu(null)} />;
      })()}

      <ConfirmDialog
        open={confirmOpen}
        title={
          confirmMode === "all"
            ? `Xoá tất cả ${entries.length} mục lịch sử?`
            : `Xoá ${selected.size} mục đã chọn?`
        }
        message={
          <span>
            Hành động này sẽ xoá các mục lịch sử trong app.
            <br />
            Tích vào ô bên dưới nếu bạn muốn xoá luôn các <strong>file video trên đĩa</strong>.
          </span>
        }
        confirmText="Xoá"
        cancelText="Huỷ"
        variant="danger"
        extraToggle={{
          label: "Xoá luôn các file video trên đĩa",
          value: alsoDeleteFiles,
          onChange: setAlsoDeleteFiles,
        }}
        onConfirm={() => void onConfirmClear()}
        onCancel={() => setConfirmOpen(false)}
      />
    </div>
  );
}
