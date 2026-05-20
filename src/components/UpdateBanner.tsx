import { useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

/**
 * Background updater banner.
 *
 * On app start, ping the GitHub Releases endpoint configured in
 * `tauri.conf.json` (plugins.updater.endpoints). If a newer version exists,
 * show a non-blocking banner with a "Cập nhật" button. Click → download +
 * install + relaunch. Cài đặt + lịch sử đều giữ nguyên (data ở APPDATA/Roaming
 * không nằm trong installer).
 *
 * Errors are silent — user with no internet should not be nagged.
 */
export function UpdateBanner() {
  const [update, setUpdate] = useState<Update | null>(null);
  const [downloading, setDownloading] = useState(false);
  const [progress, setProgress] = useState<{ downloaded: number; total?: number }>({ downloaded: 0 });
  const [error, setError] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const result = await check();
        if (!cancelled && result) {
          setUpdate(result);
        }
      } catch {
        // network down / not bundled installer / dev mode — silently ignore
      }
    })();
    return () => { cancelled = true; };
  }, []);

  if (!update || dismissed) return null;

  const onInstall = async () => {
    setDownloading(true);
    setError(null);
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          setProgress({ downloaded: 0, total: event.data.contentLength });
        } else if (event.event === "Progress") {
          setProgress((p) => ({ ...p, downloaded: p.downloaded + event.data.chunkLength }));
        }
      });
      await relaunch();
    } catch (e) {
      setError(String(e));
      setDownloading(false);
    }
  };

  const pct = progress.total ? Math.floor((progress.downloaded / progress.total) * 100) : null;

  return (
    <div className="bg-accent/15 border-b border-accent/30 px-4 py-2.5 flex items-center gap-3 text-sm">
      <span className="text-accent text-base">⬆</span>
      <div className="flex-1 min-w-0">
        <span className="text-fg font-medium">Có bản v{update.version} mới.</span>
        {update.body && (
          <span className="text-muted ml-2 truncate">{update.body.split("\n")[0].slice(0, 80)}</span>
        )}
        {downloading && (
          <span className="text-muted ml-2">
            Đang tải {pct != null ? `${pct}%` : "…"}
          </span>
        )}
        {error && <span className="text-danger ml-2">Lỗi: {error}</span>}
      </div>
      {!downloading && (
        <>
          <button
            onClick={() => void onInstall()}
            className="px-3 py-1 text-xs rounded-md bg-accent text-accent-fg hover:opacity-90"
          >
            Cập nhật
          </button>
          <button
            onClick={() => setDismissed(true)}
            className="px-2 py-1 text-xs text-muted hover:text-fg"
            title="Để sau"
          >
            ✕
          </button>
        </>
      )}
    </div>
  );
}
