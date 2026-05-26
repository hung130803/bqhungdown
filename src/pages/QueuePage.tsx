import { useEffect } from "react";
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

  // Sort newest first using createdAt (descending).
  const sortedItems = [...items].sort((a, b) => {
    const ta = new Date(a.createdAt).getTime();
    const tb = new Date(b.createdAt).getTime();
    return tb - ta;
  });

  if (items.length === 0) {
    return <p className="text-muted text-center py-12">{t("queue.empty")}</p>;
  }

  return (
    <>
      <ConflictDialog />
      <div className="space-y-2 max-w-3xl mx-auto">
        {terminalCount > 0 && (
          <div className="flex justify-end">
            <button
              onClick={clearTerminal}
              className="px-3 py-1.5 text-xs rounded-md border border-border hover:bg-surface-2 text-muted"
            >
              Xoá {terminalCount} mục đã xong khỏi danh sách
            </button>
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
