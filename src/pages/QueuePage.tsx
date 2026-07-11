import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";
import { useQueueStore } from "@/stores/useQueueStore";
import { QueueRow } from "@/components/QueueRow";
import { ConflictDialog } from "@/components/ConflictDialog";
import * as cmd from "@/ipc/commands";

const TERMINAL = ["completed", "failed", "cancelled", "skipped"];

export function QueuePage() {
  const { t } = useTranslation();
  const items = useQueueStore((s) => s.items ?? []);
  const refresh = useQueueStore((s) => s.refresh);
  const clearTerminal = useQueueStore((s) => s.clearTerminal);
  const removeLocal = useQueueStore((s) => s.remove);
  const [params] = useSearchParams();
  const focus = params.get("focus");

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Auto-prune on mount: drop completed rows whose recorded file is gone.
  // Runs once per visit so the user doesn't see stale entries pointing at
  // files they already deleted from disk / history.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const completed = items.filter((i) => i.state === "completed" && i.outputPath);
      for (const it of completed) {
        if (cancelled) return;
        try {
          const exists = await cmd.pathExists(it.outputPath as string);
          if (!exists) {
            removeLocal(it.shortId);
            try {
              await cmd.removeQueueItem(it.shortId);
            } catch {
              // ignore — local removal is enough
            }
          }
        } catch {
          // ignore individual failures
        }
      }
    })();
    return () => {
      cancelled = true;
    };
    // Intentionally only run when item count changes; we don't want to
    // re-check on every progress tick.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [items.length]);

  useEffect(() => {
    if (!focus) return;
    const el = document.getElementById(`queue-${focus}`);
    if (el) el.scrollIntoView({ behavior: "smooth", block: "center" });
  }, [focus, items.length]);

  const terminalCount = items.filter((i) => TERMINAL.includes(i.state)).length;
  const failedCount = items.filter((i) => i.state === "failed").length;

  // Nút "Thử lại tất cả video lỗi" — kịch bản: tải cả kênh dính lỗi hàng loạt
  // (thiếu cookie / bị chặn tạm), user sửa nguyên nhân xong chỉ cần 1 nút
  // thay vì bấm Thử lại từng video.
  const [retryingAll, setRetryingAll] = useState(false);
  const retryAll = async () => {
    if (retryingAll) return;
    setRetryingAll(true);
    try {
      await cmd.retryAllFailed();
      await refresh();
    } catch {
      /* ignore — refresh dưới finally vẫn cập nhật UI */
    } finally {
      setRetryingAll(false);
    }
  };

  // Group by save folder so a whole channel can be dropped at once.
  const groups = useMemo(() => {
    const m = new Map<string, { label: string; count: number }>();
    for (const it of items) {
      const folder = it.request?.saveFolder ?? "";
      if (!folder) continue;
      const label = folder.split(/[\\/]/).filter(Boolean).pop() || folder;
      const g = m.get(folder) ?? { label, count: 0 };
      g.count++;
      m.set(folder, g);
    }
    return [...m.entries()].map(([folder, g]) => ({ folder, ...g }));
  }, [items]);

  const [undoLabel, setUndoLabel] = useState<string | null>(null);

  const removeGroup = async (folder: string, label: string, count: number) => {
    if (!window.confirm(`Xóa cả kênh "${label}" (${count} mục) khỏi hàng đợi?\nVideo đã tải xong vẫn còn trên máy, chỉ xóa các mục trong danh sách + đang chờ.`)) {
      return;
    }
    try {
      await cmd.removeQueueGroup(folder);
      setUndoLabel(label);
      await refresh();
    } catch {
      /* ignore */
    }
  };

  const undoRemove = async () => {
    try {
      await cmd.undoRemoveGroup();
      setUndoLabel(null);
      await refresh();
    } catch {
      /* ignore */
    }
  };

  // Sort newest first using createdAt (descending).
  const sortedItems = [...items].sort((a, b) => {
    const ta = new Date(a.createdAt).getTime();
    const tb = new Date(b.createdAt).getTime();
    return tb - ta;
  });

  const undoBanner = undoLabel ? (
    <div className="flex items-center justify-between gap-2 px-3 py-2 mb-2 rounded-md bg-warning/10 border border-warning text-sm">
      <span className="text-fg">Đã xóa kênh "{undoLabel}" khỏi hàng đợi.</span>
      <button
        onClick={() => void undoRemove()}
        className="px-3 py-1 rounded-md bg-warning text-accent-fg text-xs font-medium shrink-0"
      >
        ↩ Hoàn tác
      </button>
    </div>
  ) : null;

  if (items.length === 0) {
    return (
      <div className="max-w-3xl mx-auto">
        {undoBanner}
        <p className="text-muted text-center py-12">{t("queue.empty")}</p>
      </div>
    );
  }

  return (
    <>
      <ConflictDialog />
      <div className="space-y-2 max-w-3xl mx-auto">
        {undoBanner}
        {groups.length >= 1 && items.length > 1 && (
          <div className="flex flex-wrap gap-1.5 pb-1">
            <span className="text-xs text-muted self-center mr-1">Xóa cả kênh:</span>
            {groups.map((g) => (
              <button
                key={g.folder}
                onClick={() => void removeGroup(g.folder, g.label, g.count)}
                className="inline-flex items-center gap-1 px-2 py-1 rounded-md border border-border bg-surface text-xs text-fg hover:border-danger hover:text-danger"
                title={`Xóa cả kênh "${g.label}" khỏi hàng đợi`}
              >
                {g.label} <span className="text-muted">({g.count})</span> ✕
              </button>
            ))}
          </div>
        )}
        {(terminalCount > 0 || failedCount > 0) && (
          <div className="flex justify-end gap-2 flex-wrap">
            {failedCount > 0 && (
              <button
                onClick={() => void retryAll()}
                disabled={retryingAll}
                className="px-3 py-1.5 text-xs rounded-md bg-accent text-accent-fg font-medium disabled:opacity-60"
                title="Đưa toàn bộ video lỗi vào tải lại — dùng sau khi đã sửa nguyên nhân (thêm cookie, bấm Sửa lỗi tải ngay...)"
              >
                {retryingAll ? "⏳ Đang đưa vào hàng đợi…" : `🔄 Thử lại ${failedCount} video lỗi`}
              </button>
            )}
            {terminalCount > 0 && (
              <button
                onClick={clearTerminal}
                className="px-3 py-1.5 text-xs rounded-md border border-border hover:bg-surface-2 text-muted"
              >
                Xoá {terminalCount} mục đã xong khỏi danh sách
              </button>
            )}
          </div>
        )}
        {sortedItems.map((item) => (
          <div
            key={item.shortId}
            id={`queue-${item.shortId}`}
            className={focus === item.shortId ? "ring-2 ring-accent rounded-md" : ""}
          >
            <QueueRow item={item} />
          </div>
        ))}
      </div>
    </>
  );
}
