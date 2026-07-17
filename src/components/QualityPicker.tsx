import { useTranslation } from "react-i18next";
import { useUrlStore } from "@/stores/useUrlStore";
import { useSettingsStore } from "@/stores/useSettingsStore";
import { selectBest } from "@/lib/best-format";
import { formatBytes } from "@/lib/format";
import type { QualityFormat } from "@/types/models";

/** Điểm ưu tiên codec: h264 phát được mọi máy (khớp merge mp4) > vp9 > av1. */
function codecScore(f: QualityFormat): number {
  const c = (f.vcodec ?? "").toLowerCase();
  if (c.startsWith("avc") || c.startsWith("h264")) return 2;
  if (c.startsWith("vp9") || c.startsWith("vp09")) return 1;
  return 0;
}

/** Tên quen thuộc cho từng mức phân giải. */
function resNickname(h: number): string | null {
  if (h >= 2160) return "4K";
  if (h >= 1440) return "2K";
  if (h >= 1080) return "Full HD";
  if (h >= 720) return "HD";
  return null;
}

/**
 * Gộp danh sách format thô của yt-dlp (3–6 biến thể MỖI mức phân giải:
 * h264/vp9/av1 × mp4/webm × bitrate khác nhau) thành 1 dòng cho mỗi mức
 * chất lượng, sắp từ cao xuống thấp. Mỗi mức tách thường/60fps riêng vì
 * đó là lựa chọn có ý nghĩa với người dùng.
 * Trong các biến thể cùng mức: ưu tiên h264 (tương thích nhất), rồi bitrate.
 */
export function dedupeByQuality(formats: QualityFormat[]): QualityFormat[] {
  const byLevel = new Map<string, QualityFormat>();
  for (const f of formats) {
    const h = f.height ?? 0;
    const hiFps = (f.fps ?? 0) >= 45;
    const key = `${h}${hiFps ? "-hi" : ""}`;
    const cur = byLevel.get(key);
    if (
      !cur ||
      codecScore(f) > codecScore(cur) ||
      (codecScore(f) === codecScore(cur) && (f.tbr ?? 0) > (cur.tbr ?? 0))
    ) {
      byLevel.set(key, f);
    }
  }
  return [...byLevel.values()].sort(
    (a, b) => (b.height ?? 0) - (a.height ?? 0) || (b.fps ?? 0) - (a.fps ?? 0),
  );
}

export function QualityPicker() {
  const { t } = useTranslation();
  const md = useUrlStore(s => s.metadata);
  const mode = useUrlStore(s => s.mode);
  const formatId = useUrlStore(s => s.formatId);
  const setFormatId = useUrlStore(s => s.setFormatId);
  const maxHeight = useSettingsStore(s => s.settings?.maxHeight ?? 1080);
  if (!md || mode !== "video") return null;

  // Loại các format không phải video thật:
  //   - mhtml: storyboard (chuỗi ảnh preview, không phải video)
  //   - vcodec="none": audio-only (đã lọc qua isAudioOnly nhưng giữ phòng hờ)
  //   - các format không có resolution / height (metadata lỗi)
  const videoFormats = md.formats.filter((f) => {
    if (f.isAudioOnly) return false;
    if (f.ext === "mhtml") return false;
    if (!f.height && !f.resolution) return false;
    if (f.vcodec === "none") return false;
    return true;
  });
  if (videoFormats.length === 0) {
    return <p className="text-sm text-muted">Không có định dạng video khả dụng.</p>;
  }
  const levels = dedupeByQuality(videoFormats);
  const best = selectBest(videoFormats, maxHeight);

  const fmtLabel = (f: QualityFormat) => {
    const h = f.height ?? 0;
    const fps = f.fps ?? 0;
    const parts = [
      h ? `${h}p${fps >= 45 ? Math.round(fps) : ""}` : f.resolution,
      h ? resNickname(h) : null,
      f.filesize ? `~${formatBytes(f.filesize)}` : null,
    ].filter(Boolean);
    return parts.join(" · ");
  };

  return (
    <div className="space-y-2">
      <label className="text-sm font-medium text-fg">Chất lượng</label>
      <div className="space-y-1 max-h-64 overflow-y-auto">
        <label className={`flex items-center gap-2 px-3 py-2 rounded-md cursor-pointer ${formatId === null ? "bg-accent/10 border border-accent" : "bg-surface border border-border"}`}>
          <input type="radio" checked={formatId === null} onChange={() => setFormatId(null)} />
          <span>{t("home.qualityBest")} {best && `(${fmtLabel(best)})`}</span>
        </label>
        {levels.map(f => (
          <label key={f.formatId} className={`flex items-center gap-2 px-3 py-2 rounded-md cursor-pointer ${formatId === f.formatId ? "bg-accent/10 border border-accent" : "bg-surface border border-border"}`}>
            <input type="radio" checked={formatId === f.formatId} onChange={() => setFormatId(f.formatId)} />
            <span className="text-sm">{fmtLabel(f)}</span>
          </label>
        ))}
      </div>
    </div>
  );
}
