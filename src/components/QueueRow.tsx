import { useTranslation } from "react-i18next";
import type { DownloadItem } from "@/types/models";
import { useQueueStore } from "@/stores/useQueueStore";
import * as cmd from "@/ipc/commands";
import { ProgressBar } from "./ProgressBar";
import { Thumbnail } from "./Thumbnail";
import { TitleWithCopy } from "./TitleWithCopy";
import { formatRelative } from "@/lib/time";
import { platformInfo } from "@/lib/platforms";

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

  const stateLabel = t(`queue.states.${item.state}`);
  const isActive = item.state === "downloading";
  const isPaused = item.state === "paused";
  const isQueued = item.state === "queued";
  const isTerminal = ["completed", "failed", "cancelled", "skipped"].includes(item.state);

  const resolvePath = async (): Promise<string | null> => {
    if (item.outputPath) return item.outputPath;
    if (!item.request.saveFolder) return null;
    try {
      return await cmd.findOutputFile(item.request.saveFolder, item.title);
    } catch {
      return null;
    }
  };

  const openOutput = async () => {
    const p = await resolvePath();
    if (p) {
      await cmd.openInFolder(p);
      return;
    }
    if (item.request.saveFolder) await cmd.openInFolder(item.request.saveFolder);
  };

  const openVideo = async () => {
    const p = await resolvePath();
    if (p) {
      try {
        await cmd.openFile(p);
        return;
      } catch {
        // fall through
      }
    }
    if (item.request.saveFolder) await cmd.openInFolder(item.request.saveFolder);
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
    <div className="p-3 rounded-xl bg-surface border border-border hover:border-accent/40 transition-colors space-y-3">
      <div className="flex gap-3">
        {/* Thumbnail */}
        <div className="aspect-video w-40 sm:w-44 shrink-0">
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
            <div className="text-xs text-danger break-words">{item.errorMessage}</div>
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
