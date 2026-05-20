import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { HistoryEntry } from "@/types/models";
import { useHistoryStore } from "@/stores/useHistoryStore";
import * as cmd from "@/ipc/commands";
import { Thumbnail } from "./Thumbnail";
import { TitleWithCopy } from "./TitleWithCopy";
import { formatRelative } from "@/lib/time";
import { platformInfo } from "@/lib/platforms";
import { startFileDrag, startMultiFileDrag } from "@/lib/drag-out";

export type HistoryViewMode = "list" | "compact" | "grid";

function shortUrl(raw: string): string {
  try {
    const u = new URL(raw);
    const path = u.pathname.length > 1 ? u.pathname : "";
    const search = u.search ?? "";
    const tail = `${path}${search}`;
    const max = 60;
    return u.host + (tail.length > max ? tail.slice(0, max) + "…" : tail);
  } catch {
    return raw;
  }
}

interface RowProps {
  entry: HistoryEntry;
  selectable?: boolean;
  selected?: boolean;
  view?: HistoryViewMode;
  onCheckboxMouseDown?: (shortId: string, e: React.MouseEvent) => void;
  /** Plain click on a selected row → untick that row. */
  onPlainClickSelected?: (shortId: string) => void;
}

/** Wire native OS drag-out — works ANYWHERE on the row. We do NOT use HTML5
 *  draggable={true} because Tauri plugin's drag wouldn't survive that.
 *  Instead we listen for mousedown then a small movement, and call `startDrag`
 *  synchronously inside the same gesture.
 *
 *  Returns props you spread on the row container. Buttons/links/checkboxes
 *  inside the row still work because they handle mousedown themselves and
 *  stop propagation. */
function useRowDragOut(
  filePath: string | null,
  shortId: string,
  canDrag: boolean,
  selected: boolean,
  onPlainClick?: (id: string) => void,
) {
  const startedRef = useState({ pressed: false, dragged: false, x: 0, y: 0 })[0];
  const onMouseDown = (e: React.MouseEvent) => {
    if (!canDrag || !filePath) return;
    if (e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey) return;
    const target = e.target as HTMLElement;
    if (target.closest("button, a, input, label, select, textarea")) return;
    if (!selected) return; // unselected rows are handled by the page-level rubber-band

    e.preventDefault();
    startedRef.pressed = true;
    startedRef.dragged = false;
    startedRef.x = e.clientX;
    startedRef.y = e.clientY;
  };
  const onMouseMove = (e: React.MouseEvent) => {
    if (!startedRef.pressed || !filePath) return;
    const dx = Math.abs(e.clientX - startedRef.x);
    const dy = Math.abs(e.clientY - startedRef.y);
    if (dx > 4 || dy > 4) {
      startedRef.pressed = false;
      startedRef.dragged = true;
      const selectedRows = Array.from(
        document.querySelectorAll<HTMLElement>("[data-history-id][data-selected='true']"),
      );
      const ids = new Set(selectedRows.map((r) => r.getAttribute("data-history-id") || ""));
      if (ids.has(shortId) && selectedRows.length > 1) {
        const paths: string[] = [];
        for (const row of selectedRows) {
          const p = row.getAttribute("data-file-path");
          if (p) paths.push(p);
        }
        void startMultiFileDrag(paths.length > 0 ? paths : [filePath]);
      } else {
        void startFileDrag(filePath);
      }
    }
  };
  const onMouseUp = () => {
    // Plain click on an already-selected row (no drag) → untick this row.
    if (startedRef.pressed && !startedRef.dragged && onPlainClick) {
      onPlainClick(shortId);
    }
    startedRef.pressed = false;
    startedRef.dragged = false;
  };
  return { onMouseDown, onMouseMove, onMouseUp, onMouseLeave: onMouseUp };
}

function useHistoryRowHandlers(entry: HistoryEntry) {
  const del = useHistoryStore((s) => s.delete);

  const handleDelete = async () => {
    await del(entry.shortId, false);
  };

  const resolvePath = async (): Promise<string | null> => {
    if (entry.outputPath) return entry.outputPath;
    if (!entry.saveFolder) return null;
    try {
      const found = await cmd.findOutputFile(entry.saveFolder, entry.title);
      if (found) {
        try {
          await cmd.updateHistoryOutputPath(entry.shortId, found);
        } catch {
          /* best-effort */
        }
        return found;
      }
      return null;
    } catch {
      return null;
    }
  };

  const handleOpenFile = async () => {
    const path = await resolvePath();
    if (path) {
      try {
        await cmd.openFile(path);
        return;
      } catch {
        // fall through
      }
    }
    if (entry.saveFolder) await cmd.openInFolder(entry.saveFolder);
  };

  const handleOpenFolder = async () => {
    const path = await resolvePath();
    if (path) {
      await cmd.openInFolder(path);
      return;
    }
    if (entry.saveFolder) await cmd.openInFolder(entry.saveFolder);
  };

  const handleOpenSource = (e: React.MouseEvent) => {
    e.preventDefault();
    void cmd.openUrl(entry.url);
  };

  return {
    handleDelete,
    handleOpenFile,
    handleOpenFolder,
    handleOpenSource,
  };
}

/** Best-effort sync access to the output file path. Pre-resolves once on mount
 *  so the synchronous mousedown handler in HistoryPage can read it via the
 *  `data-file-path` DOM attribute. */
function useResolvedFilePath(entry: HistoryEntry): string | null {
  const [path, setPath] = useState<string | null>(entry.outputPath || null);
  useEffect(() => {
    let cancelled = false;
    if (entry.outputPath) {
      setPath(entry.outputPath);
      return;
    }
    if (entry.status !== "completed" || !entry.saveFolder) return;
    (async () => {
      try {
        const found = await cmd.findOutputFile(entry.saveFolder, entry.title);
        if (!cancelled && found) {
          setPath(found);
          try {
            await cmd.updateHistoryOutputPath(entry.shortId, found);
          } catch {
            // ignore
          }
        }
      } catch {
        // ignore
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [entry.shortId, entry.outputPath, entry.saveFolder, entry.title, entry.status]);
  return path;
}

function SelectCell({
  shortId,
  selected,
  onMouseDown,
  align = "start",
}: {
  shortId: string;
  selected: boolean;
  onMouseDown?: (id: string, e: React.MouseEvent) => void;
  align?: "start" | "center";
}) {
  return (
    <div
      className={`flex ${align === "center" ? "items-center" : "items-start pt-1"} cursor-pointer select-none`}
      onMouseDown={(e) => onMouseDown?.(shortId, e)}
    >
      <input
        type="checkbox"
        checked={selected}
        onChange={() => {}}
        className="h-4 w-4 cursor-pointer pointer-events-none"
        tabIndex={-1}
      />
    </div>
  );
}

export function HistoryRow(props: RowProps) {
  const { view = "list" } = props;
  if (view === "compact") return <HistoryRowCompact {...props} />;
  if (view === "grid") return <HistoryCard {...props} />;
  return <HistoryRowDetailed {...props} />;
}

// ── List (detailed) ────────────────────────────────────────────────────────
function HistoryRowDetailed({
  entry,
  selectable,
  selected,
  onCheckboxMouseDown,
  onPlainClickSelected,
}: RowProps) {
  const { t } = useTranslation();
  const { handleDelete, handleOpenFile, handleOpenFolder, handleOpenSource } =
    useHistoryRowHandlers(entry);
  const filePath = useResolvedFilePath(entry);

  const time = formatRelative(entry.finishedAt);
  const canOpen = entry.status === "completed";
  const platform = platformInfo(entry.extractor).label;
  const dragProps = useRowDragOut(filePath, entry.shortId, !!canOpen, !!selected, onPlainClickSelected);

  return (
    <div
      className={`p-3 rounded-xl border transition-colors flex gap-3 group ${
        selected && canOpen && filePath ? "cursor-grab active:cursor-grabbing" : ""
      } ${
        selected
          ? "bg-accent/10 border-accent"
          : "bg-surface border-border hover:border-accent/40"
      }`}
      data-history-id={entry.shortId}
      data-file-path={canOpen && filePath ? filePath : undefined}
      data-selected={selected ? "true" : undefined}
      {...dragProps}
    >
      {selectable && (
        <SelectCell
          shortId={entry.shortId}
          selected={!!selected}
          onMouseDown={onCheckboxMouseDown}
        />
      )}
      <div
        className="aspect-video w-40 sm:w-44 shrink-0 rounded-lg overflow-hidden"
        title={canOpen && filePath ? "Kéo để thả vào CapCut / Premiere / thư mục khác" : undefined}
      >
        <Thumbnail src={entry.thumbnail} extractor={entry.extractor} alt={entry.title} />
      </div>

      <div className="flex-1 min-w-0 flex flex-col gap-1.5">
        <div className="flex items-center gap-1.5 text-xs text-muted flex-wrap">
          {entry.channel && (
            <>
              <span className="font-medium text-fg/80 truncate max-w-[180px]" title={entry.channel}>
                {entry.channel}
              </span>
              <span>·</span>
            </>
          )}
          <span>{platform}</span>
          <span>·</span>
          <span>{time}</span>
          {entry.edited && (
            <span className="inline-flex items-center gap-1 text-[10px] bg-success/15 text-success px-1.5 py-0.5 rounded-full font-medium">
              ✓ Đã edit
            </span>
          )}
          <span className="text-[10px] text-muted bg-surface-2 px-1.5 py-0.5 rounded font-mono ml-auto">
            #{entry.shortId}
          </span>
        </div>

        <TitleWithCopy title={entry.title || entry.url} />

        <div className="flex items-center gap-2 text-xs min-w-0">
          <a
            href={entry.url}
            onClick={handleOpenSource}
            className="text-muted hover:text-accent truncate underline-offset-2 hover:underline"
            title={entry.url}
          >
            {shortUrl(entry.url)}
          </a>
        </div>
        {entry.error && (
          <div className="text-xs text-danger truncate" title={entry.error}>
            {entry.error}
          </div>
        )}

        <div className="flex flex-wrap gap-1.5 mt-auto pt-1">
          {canOpen && (
            <>
              <button
                onClick={() => void handleOpenFile()}
                className="px-2.5 py-1 text-xs rounded-md bg-accent text-accent-fg hover:opacity-90"
              >
                ▶ Mở video
              </button>
              <button
                onClick={() => void handleOpenFolder()}
                className="px-2.5 py-1 text-xs rounded-md border border-border hover:bg-surface-2"
              >
                {t("history.openFolder")}
              </button>
            </>
          )}
          <button
            onClick={() => void handleDelete()}
            className="px-2.5 py-1 text-xs rounded-md border border-danger text-danger hover:bg-danger/10 ml-auto"
          >
            {t("history.delete")}
          </button>
        </div>
      </div>
    </div>
  );
}

// ── Compact (single line) ──────────────────────────────────────────────────
function HistoryRowCompact({
  entry,
  selectable,
  selected,
  onCheckboxMouseDown,
  onPlainClickSelected,
}: RowProps) {
  const { t } = useTranslation();
  const { handleDelete, handleOpenFile, handleOpenFolder } =
    useHistoryRowHandlers(entry);
  const filePath = useResolvedFilePath(entry);
  const canOpen = entry.status === "completed";
  const platform = platformInfo(entry.extractor).label;
  const time = formatRelative(entry.finishedAt);
  const dragProps = useRowDragOut(filePath, entry.shortId, !!canOpen, !!selected, onPlainClickSelected);

  return (
    <div
      className={`px-3 py-2 rounded-lg border flex items-center gap-3 ${
        selected && canOpen && filePath ? "cursor-grab active:cursor-grabbing" : ""
      } ${
        selected
          ? "bg-accent/10 border-accent"
          : "bg-surface border-border hover:border-accent/40"
      }`}
      data-history-id={entry.shortId}
      data-file-path={canOpen && filePath ? filePath : undefined}
      data-selected={selected ? "true" : undefined}
      {...dragProps}
    >
      {selectable && (
        <SelectCell
          shortId={entry.shortId}
          selected={!!selected}
          onMouseDown={onCheckboxMouseDown}
          align="center"
        />
      )}
      <div
        className="aspect-video w-16 shrink-0 rounded overflow-hidden"
        title={canOpen && filePath ? "Kéo để thả vào CapCut / Premiere / thư mục khác" : undefined}
      >
        <Thumbnail src={entry.thumbnail} extractor={entry.extractor} alt={entry.title} />
      </div>
      <div className="flex-1 min-w-0">
        <div className="text-sm text-fg truncate" title={entry.title}>
          {entry.title || entry.url}
        </div>
        <div className="text-xs text-muted truncate flex items-center gap-1.5">
          {entry.channel && <span className="truncate max-w-[160px]">{entry.channel}</span>}
          {entry.channel && <span>·</span>}
          <span>{platform}</span>
          <span>·</span>
          <span>{time}</span>
          {entry.edited && (
            <span className="text-[10px] bg-success/15 text-success px-1.5 py-0.5 rounded-full font-medium">
              ✓ Đã edit
            </span>
          )}
        </div>
      </div>
      <div className="flex items-center gap-1 shrink-0">
        {canOpen && (
          <>
            <button
              onClick={() => void handleOpenFile()}
              title="Mở video"
              className="px-2 py-1 text-xs rounded-md bg-accent text-accent-fg hover:opacity-90"
            >
              ▶
            </button>
            <button
              onClick={() => void handleOpenFolder()}
              title={t("history.openFolder")}
              className="px-2 py-1 text-xs rounded-md border border-border hover:bg-surface-2"
            >
              📁
            </button>
          </>
        )}
        <button
          onClick={() => void handleDelete()}
          title={t("history.delete")}
          className="px-2 py-1 text-xs rounded-md border border-danger text-danger hover:bg-danger/10"
        >
          ✕
        </button>
      </div>
    </div>
  );
}

// ── Grid card ──────────────────────────────────────────────────────────────
function HistoryCard({
  entry,
  selectable,
  selected,
  onCheckboxMouseDown,
  onPlainClickSelected,
}: RowProps) {
  const { t } = useTranslation();
  const { handleDelete, handleOpenFile, handleOpenFolder } =
    useHistoryRowHandlers(entry);
  const filePath = useResolvedFilePath(entry);
  const canOpen = entry.status === "completed";
  const platform = platformInfo(entry.extractor).label;
  const time = formatRelative(entry.finishedAt);
  const dragProps = useRowDragOut(filePath, entry.shortId, !!canOpen, !!selected, onPlainClickSelected);

  return (
    <div
      className={`rounded-xl border overflow-hidden flex flex-col group ${
        selected && canOpen && filePath ? "cursor-grab active:cursor-grabbing" : ""
      } ${
        selected
          ? "bg-accent/10 border-accent"
          : "bg-surface border-border hover:border-accent/40"
      }`}
      data-history-id={entry.shortId}
      data-file-path={canOpen && filePath ? filePath : undefined}
      data-selected={selected ? "true" : undefined}
      {...dragProps}
    >
      <div
        className="relative aspect-video"
        title={canOpen && filePath ? "Kéo để thả vào CapCut / Premiere / thư mục khác" : undefined}
      >
        <Thumbnail src={entry.thumbnail} extractor={entry.extractor} alt={entry.title} />
        {selectable && (
          <div className="absolute top-2 left-2 z-10 bg-surface/90 rounded p-1 backdrop-blur">
            <SelectCell
              shortId={entry.shortId}
              selected={!!selected}
              onMouseDown={onCheckboxMouseDown}
              align="center"
            />
          </div>
        )}
        {entry.edited && (
          <span className="absolute top-2 right-2 inline-flex items-center gap-1 text-[10px] bg-success/90 text-white px-1.5 py-0.5 rounded-full font-medium backdrop-blur">
            ✓ Đã edit
          </span>
        )}
      </div>
      <div className="p-2.5 flex-1 flex flex-col gap-1.5">
        <div className="text-xs text-muted truncate flex items-center gap-1.5">
          {entry.channel && (
            <>
              <span className="font-medium text-fg/80 truncate max-w-[120px]">{entry.channel}</span>
              <span>·</span>
            </>
          )}
          <span>{platform}</span>
          <span>·</span>
          <span>{time}</span>
        </div>
        <div className="text-sm text-fg line-clamp-2 leading-snug" title={entry.title}>
          {entry.title || entry.url}
        </div>
        <div className="flex items-center gap-1 mt-auto pt-1">
          {canOpen && (
            <>
              <button
                onClick={() => void handleOpenFile()}
                className="flex-1 px-2 py-1 text-xs rounded-md bg-accent text-accent-fg hover:opacity-90"
              >
                ▶ Mở
              </button>
              <button
                onClick={() => void handleOpenFolder()}
                title={t("history.openFolder")}
                className="px-2 py-1 text-xs rounded-md border border-border hover:bg-surface-2"
              >
                📁
              </button>
            </>
          )}
          <button
            onClick={() => void handleDelete()}
            title={t("history.delete")}
            className="px-2 py-1 text-xs rounded-md border border-danger text-danger hover:bg-danger/10"
          >
            ✕
          </button>
        </div>
      </div>
    </div>
  );
}
