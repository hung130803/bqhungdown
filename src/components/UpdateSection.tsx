import { useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";

/**
 * Mục "Cập nhật ứng dụng" trong Cài đặt — LUÔN hiện (khác với UpdateBanner chỉ
 * hiện khi tình cờ có bản mới lúc mở app). Cho user:
 *   - Xem phiên bản đang dùng.
 *   - Bấm "Kiểm tra cập nhật" bất cứ lúc nào.
 *   - Nếu có bản mới → tải + cài + khởi động lại (cài đặt/lịch sử giữ nguyên vì
 *     nằm ở APPDATA, không nằm trong installer).
 */
export function UpdateSection() {
  const [version, setVersion] = useState<string>("");
  const [state, setState] = useState<"idle" | "checking" | "available" | "uptodate" | "error">("idle");
  const [update, setUpdate] = useState<Update | null>(null);
  const [downloading, setDownloading] = useState(false);
  const [pct, setPct] = useState<number | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  useEffect(() => {
    getVersion().then(setVersion).catch(() => setVersion(""));
  }, []);

  const doCheck = async () => {
    setState("checking");
    setMsg(null);
    try {
      const result = await check();
      if (result) {
        setUpdate(result);
        setState("available");
      } else {
        setState("uptodate");
      }
    } catch (e) {
      setState("error");
      setMsg(String(e));
    }
  };

  const doInstall = async () => {
    if (!update) return;
    setDownloading(true);
    setMsg(null);
    try {
      let total = 0;
      let done = 0;
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          done += event.data.chunkLength;
          setPct(total ? Math.floor((done / total) * 100) : null);
        }
      });
      await relaunch();
    } catch (e) {
      setMsg(String(e));
      setDownloading(false);
    }
  };

  return (
    <div className="space-y-2 rounded-md border border-border bg-surface p-3">
      <div className="flex items-center justify-between gap-3">
        <div>
          <div className="text-sm font-medium text-fg">Cập nhật ứng dụng</div>
          <div className="text-xs text-muted">
            Phiên bản đang dùng: <b>v{version || "?"}</b>
          </div>
        </div>
        {state !== "available" && !downloading && (
          <button
            onClick={() => void doCheck()}
            disabled={state === "checking"}
            className="px-3 py-2 rounded-md bg-surface-2 border border-border text-fg text-sm disabled:opacity-50"
          >
            {state === "checking" ? "Đang kiểm tra…" : "Kiểm tra cập nhật"}
          </button>
        )}
      </div>

      {state === "uptodate" && (
        <p className="text-xs text-success">✓ Bạn đang dùng bản mới nhất.</p>
      )}
      {state === "available" && update && (
        <div className="flex items-center gap-3">
          <span className="text-sm text-fg">
            Có bản mới <b>v{update.version}</b>!
          </span>
          <button
            onClick={() => void doInstall()}
            disabled={downloading}
            className="px-3 py-1.5 text-xs rounded-md bg-accent text-accent-fg hover:opacity-90 disabled:opacity-50"
          >
            {downloading ? `Đang tải ${pct != null ? pct + "%" : "…"}` : "Cập nhật ngay"}
          </button>
        </div>
      )}
      {state === "error" && (
        <p className="text-xs text-muted">
          Chưa kiểm tra được (có thể do mạng, hoặc bản đang chạy không phải bản cài đặt).
          {msg && <span className="block text-danger/80 mt-0.5 break-all">{msg}</span>}
        </p>
      )}
    </div>
  );
}
