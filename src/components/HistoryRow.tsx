import { useTranslation } from "react-i18next";
import type { HistoryEntry } from "@/types/models";
import { useHistoryStore } from "@/stores/useHistoryStore";
import * as cmd from "@/ipc/commands";
import { Thumbnail } from "./Thumbnail";
import { TitleWithCopy } from "./TitleWithCopy";
import { formatRelative } from "@/lib/time";
import { platformInfo } from "@/lib/platforms";

const STATUS_DOT: Record<string, string> = {
  completed: "bg-success",
  failed: "bg-danger",
  cancelled: "bg-muted",
};

/** Friendly host string for the "go to source" link, e.g. `youtu.be/dQw4w9...`. */
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

export function HistoryRow({ entry }: { entry: HistoryEntry }) {
  const { t } = useTranslation();
  const del = useHistoryStore((s) => s.delete);

  const handleDelete = async () => {
    // Direct delete — file on disk is untouched.
    await del(entry.shortId, false);
  };

  const resolvePath = async (): Promise<string | null> => {
    if (entry.outputPath) return entry.outputPath;
    if (!entry.saveFolder) return null;
    try {
      const found = await cmd.findOutputFile(entry.saveFolder, entry.title);
      if (found) {
        // Backfill into DB so next click is instant and "Mở thư mục" highlights file correctly.
        try { await cmd.updateHistoryOutputPath(entry.shortId, found); } catch { /* best-effort */ }
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
        // file moved/missing → fall through to folder
      }
    }
    if (entry.saveFolder) await cmd.openInFolder(entry.saveFolder);
  };

  const handleOpenFolder = async () => {
    const path = await resolvePath();
    if (path) {
      // explorer /select, will highlight the file
      await cmd.openInFolder(path);
      return;
    }
    if (entry.saveFolder) await cmd.openInFolder(entry.saveFolder);
  };

  const handleOpenSource = (e: React.MouseEvent) => {
    e.preventDefault();
    void cmd.openUrl(entry.url);
  };

  const time = formatRelative(entry.finishedAt);
  const canOpen = entry.status === "completed";
  const platform = platformInfo(entry.extractor).label;
  const statusLabel = t(`history.status.${entry.status}`);

  return (
    <div className="p-3 rounded-xl bg-surface border border-border hover:border-accent/40 transition-colors flex gap-3 group">
      {/* Thumbnail — 16:9 ratio, fixed width on md+ */}
      <div className="aspect-video w-40 sm:w-44 shrink-0">
        <Thumbnail src={entry.thumbnail} extractor={entry.extractor} alt={entry.title} />
      </div>

      <div className="flex-1 min-w-0 flex flex-col gap-1.5">
        {/* Top row: channel · platform · time · status */}
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
          <span>·</span>
          <span className="inline-flex items-center gap-1">
            <span className={`h-1.5 w-1.5 rounded-full ${STATUS_DOT[entry.status] ?? "bg-muted"}`} />
            {statusLabel}
          </span>
          <span className="text-[10px] text-muted bg-surface-2 px-1.5 py-0.5 rounded font-mono ml-auto">
            #{entry.shortId}
          </span>
        </div>

        {/* Title — plain selectable text + small copy button */}
        <TitleWithCopy title={entry.title || entry.url} />

        {/* URL line + error (if any) */}
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

        {/* Action row */}
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
