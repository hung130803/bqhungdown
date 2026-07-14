import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { DownloadItem } from "@/types/models";
import { useQueueStore } from "@/stores/useQueueStore";
import * as cmd from "@/ipc/commands";
import { ProgressBar } from "./ProgressBar";
import { Thumbnail } from "./Thumbnail";
import { TitleWithCopy } from "./TitleWithCopy";
import { formatRelative } from "@/lib/time";
import { platformInfo } from "@/lib/platforms";
import { startFileDrag } from "@/lib/drag-out";

const STATE_DOT: Record<string, string> = {
  queued: "bg-muted",
  downloading: "bg-accent animate-pulse",
  paused: "bg-warning",
  completed: "bg-success",
  failed: "bg-danger",
  cancelled: "bg-danger",
  skipped: "bg-muted",
};

/** Friendly title — fallback to URL host+path when title still equals raw URL. */
function displayTitle(item: DownloadItem): string {
  const t = (item.title ?? "").trim();
  if (t && t !== item.request.url) return t;
  try {
    const u = new URL(item.request.url);
    return u.pathname.length > 1 ? `${u.host}${u.pathname}` : u.href;
  } catch {
    return item.request.url;
  }
}

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

export function QueueRow({ item }: { item: DownloadItem }) {
  const { t } = useTranslation();
  const pause = useQueueStore((s) => s.pause);
  const resume = useQueueStore((s) => s.resume);
  const cancel = useQueueStore((s) => s.cancel);
  const retry = useQueueStore((s) => s.retry);
  const forceDownload = useQueueStore((s) => s.forceDownload);
  const removeLocal = useQueueStore((s) => s.remove);

  const stateLabel = t(`queue.states.${item.state}`);
  const isActive = item.state === "downloading";
  const isPaused = item.state === "paused";
  const isQueued = item.state === "queued";
  const isTerminal = ["completed", "failed", "cancelled", "skipped"].includes(item.state);

  /** Resolve to a path that ACTUALLY exists on disk, or null if user deleted it. */
  const resolvePath = async (): Promise<string | null> => {
    if (item.outputPath) {
      // outputPath was recorded by the runner → trust it as the canonical
      // file for this row. If it's gone, the user deleted it and we should
      // NOT silently fall back to a same-title file (which would be a
      // different, older download).
      try {
        if (await cmd.pathExists(item.outputPath)) return item.outputPath;
      } catch {
        // ignore
      }
      return null;
    }
    // No outputPath recorded (legacy items): best-effort scan by title.
    if (!item.request.saveFolder) return null;
    try {
      const found = await cmd.findOutputFile(item.request.saveFolder, item.title);
      if (found && (await cmd.pathExists(found))) return found;
    } catch {
      // ignore
    }
    return null;
  };

  /** Drop this row from the queue both server- and client-side. */
  const dismissRow = async () => {
    // Update UI immediately so the row disappears without waiting for the
    // backend round-trip or the queue://updated event.
    removeLocal(item.shortId);
    try {
      await cmd.removeQueueItem(item.shortId);
    } catch {
      // ignore — backend may not have the command yet (older build) or the
      // item was already gone. Either way, the local row is already removed.
    }
  };

  const openOutput = async () => {
    const p = await resolvePath();
    if (p) {
      await cmd.openInFolder(p);
      return;
    }
    // File no longer exists — auto-dismiss the row.
    void dismissRow();
  };

  const openVideo = async () => {
    const p = await resolvePath();
    if (p) {
      try {
        await cmd.openFile(p);
        return;
      } catch {
        // fall through to dismiss
      }
    }
    // File missing — drop the row so the user doesn't see the broken entry.
    void dismissRow();
  };

  /** Native OS-level drag for completed downloads — drop the file into
   *  CapCut / Premiere / Explorer just like dragging from File Explorer.
   *  This is unused (drag is handled globally by HistoryPage / QueuePage
   *  via the data-file-path attribute below) but kept for reference. */
  const _handleDragStart = async (e: React.MouseEvent) => {
    if (item.state !== "completed") return;
    if (e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey) return;
    const p = await resolvePath();
    if (p) {
      e.preventDefault();
      e.stopPropagation();
      void p; // path resolved, but actual drag now started by global handler
    }
  };
  void _handleDragStart;

  // Pre-resolve file path so the global mousedown handler can read it
  // synchronously via the `data-file-path` DOM attribute below.
  const [filePath, setFilePath] = useState<string | null>(item.outputPath ?? null);
  useEffect(() => {
    let cancelled = false;
    if (item.outputPath) {
      setFilePath(item.outputPath);
      return;
    }
    if (item.state !== "completed" || !item.request.saveFolder) return;
    (async () => {
      try {
        const found = await cmd.findOutputFile(item.request.saveFolder, item.title);
        if (!cancelled && found) setFilePath(found);
      } catch {
        // ignore
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [item.shortId, item.outputPath, item.request.saveFolder, item.title, item.state]);

  // Drag-out gesture: track mousedown anywhere on the row, fire startDrag
  // once user moves > 4px. Buttons / inputs inside the row stop propagation
  // automatically because of how React event bubbling works with
  // form-control elements.
  const dragRef = useState({ pressed: false, x: 0, y: 0 })[0];
  const dragCanStart = item.state === "completed" && !!filePath;
  const onRowDown = (e: React.MouseEvent) => {
    if (!dragCanStart) return;
    if (e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey) return;
    const tg = e.target as HTMLElement;
    if (tg.closest("button, a, input, label, select, textarea")) return;
    // Suppress native text-selection drag — see HistoryRow comment.
    e.preventDefault();
    dragRef.pressed = true;
    dragRef.x = e.clientX;
    dragRef.y = e.clientY;
  };
  const onRowMove = (e: React.MouseEvent) => {
    if (!dragRef.pressed || !filePath) return;
    if (Math.abs(e.clientX - dragRef.x) > 4 || Math.abs(e.clientY - dragRef.y) > 4) {
      dragRef.pressed = false;
      void startFileDrag(filePath);
    }
  };
  const onRowUp = () => {
    dragRef.pressed = false;
  };

  const openSource = (e: React.MouseEvent) => {
    e.preventDefault();
    void cmd.openUrl(item.request.url);
  };

  const timeStamp = item.finishedAt ?? item.createdAt;
  const timeLabel = formatRelative(timeStamp);
  const platform = platformInfo(item.extractor).label;
  const title = displayTitle(item);

  return (
    <div
      className={`p-3 rounded-xl bg-surface border border-border hover:border-accent/40 transition-colors space-y-3 ${
        dragCanStart ? "cursor-grab active:cursor-grabbing" : ""
      }`}
      onMouseDown={onRowDown}
      onMouseMove={onRowMove}
      onMouseUp={onRowUp}
      onMouseLeave={onRowUp}
      title={dragCanStart ? "Kéo để thả vào CapCut / Premiere / thư mục khác" : undefined}
    >
      <div className="flex gap-3">
        {/* Thumbnail */}
        <div className="aspect-video w-40 sm:w-44 shrink-0 rounded-lg overflow-hidden">
          <Thumbnail src={item.thumbnail} extractor={item.extractor} alt={title} />
        </div>

        <div className="flex-1 min-w-0 flex flex-col gap-1.5">
          {/* Top row: channel · platform · time · state */}
          <div className="flex items-center gap-1.5 text-xs text-muted flex-wrap">
            {item.channel && (
              <>
                <span className="font-medium text-fg/80 truncate max-w-[180px]" title={item.channel}>
                  {item.channel}
                </span>
                <span>·</span>
              </>
            )}
            <span>{platform}</span>
            <span>·</span>
            <span>{timeLabel}</span>
            <span>·</span>
            <span className="inline-flex items-center gap-1">
              <span className={`h-1.5 w-1.5 rounded-full ${STATE_DOT[item.state] ?? "bg-muted"}`} />
              {stateLabel}
            </span>
            <span className="text-[10px] text-muted bg-surface-2 px-1.5 py-0.5 rounded font-mono ml-auto">
              #{item.shortId}
            </span>
            {isTerminal && (
              <button
                onClick={() => void dismissRow()}
                title="Xoá khỏi danh sách"
                className="text-muted hover:text-danger text-sm leading-none px-1"
              >
                ✕
              </button>
            )}
          </div>

          {/* Title — plain selectable text + small copy button */}
          <TitleWithCopy title={title} />

          {/* URL link */}
          <a
            href={item.request.url}
            onClick={openSource}
            className="text-xs text-muted hover:text-accent truncate underline-offset-2 hover:underline"
            title={item.request.url}
          >
            {shortUrl(item.request.url)}
          </a>

          {item.errorMessage && (
            // whitespace-pre-line: thông báo lỗi giờ gồm dòng hướng dẫn + dòng
            // "(Chi tiết kỹ thuật: ...)" — cần xuống dòng đúng chỗ cho dễ đọc.
            <div className="text-xs text-danger break-words whitespace-pre-line">{item.errorMessage}</div>
          )}

          {/* Actions */}
          <div className="flex flex-wrap gap-1.5 mt-auto pt-1">
            {isActive && (
              <button onClick={() => void pause(item.shortId)} className="px-2.5 py-1 text-xs rounded-md border border-border hover:bg-surface-2">
                {t("queue.pause")}
              </button>
            )}
            {isPaused && (
              <button onClick={() => void resume(item.shortId)} className="px-2.5 py-1 text-xs rounded-md border border-border hover:bg-surface-2">
                {t("queue.resume")}
              </button>
            )}
            {!isTerminal && (
              <button onClick={() => void cancel(item.shortId)} className="px-2.5 py-1 text-xs rounded-md border border-danger text-danger hover:bg-danger/10">
                {t("queue.cancel")}
              </button>
            )}
            {(item.state === "failed" || item.state === "cancelled") && (
              <button onClick={() => void retry(item.shortId)} className="px-2.5 py-1 text-xs rounded-md border border-border hover:bg-surface-2">
                {t("queue.retry")}
              </button>
            )}
            {item.state === "skipped" && (
              // Video bị bỏ qua vì đã có trong danh sách đã-tải — nút này tải
              // bất chấp (file cũ còn trên máy thì bản mới tự thêm " (1)").
              <button
                onClick={() => void forceDownload(item.shortId)}
                className="px-2.5 py-1 text-xs rounded-md bg-accent text-accent-fg hover:opacity-90"
                title="Video này bị bỏ qua vì từng tải rồi — bấm để tải lại bất chấp"
              >
                ⬇ Vẫn tải video này
              </button>
            )}
            {item.state === "completed" && (
              <>
                <button onClick={() => void openVideo()} className="px-2.5 py-1 text-xs rounded-md bg-accent text-accent-fg hover:opacity-90">
                  ▶ Mở video
                </button>
                <button onClick={() => void openOutput()} className="px-2.5 py-1 text-xs rounded-md border border-border hover:bg-surface-2">
                  {t("queue.open")}
                </button>
              </>
            )}
          </div>
        </div>
      </div>

      {(isActive || isPaused || isQueued) && (
        <ProgressBar
          downloaded={item.bytesDownloaded}
          total={item.bytesTotal}
          speedBps={item.speedBps}
          etaSec={item.etaSec}
        />
      )}
    </div>
  );
}
