import { useTranslation } from "react-i18next";
import { useUrlStore } from "@/stores/useUrlStore";
import { selectBest } from "@/lib/best-format";
import { formatBytes } from "@/lib/format";
import type { QualityFormat } from "@/types/models";

export function QualityPicker() {
  const { t } = useTranslation();
  const md = useUrlStore(s => s.metadata);
  const mode = useUrlStore(s => s.mode);
  const formatId = useUrlStore(s => s.formatId);
  const setFormatId = useUrlStore(s => s.setFormatId);
  if (!md || mode !== "video") return null;

  const videoFormats = md.formats.filter(f => !f.isAudioOnly);
  if (videoFormats.length === 0) {
    return <p className="text-sm text-muted">Không có định dạng video khả dụng.</p>;
  }
  const best = selectBest(videoFormats);

  const fmtLabel = (f: QualityFormat) => {
    const parts = [
      f.height ? `${f.height}p` : f.resolution,
      f.fps ? `${Math.round(f.fps)}fps` : null,
      f.vcodec && f.vcodec !== "none" ? f.vcodec.split(".")[0] : null,
      f.filesize ? formatBytes(f.filesize) : null,
      f.ext,
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
        {videoFormats.map(f => (
          <label key={f.formatId} className={`flex items-center gap-2 px-3 py-2 rounded-md cursor-pointer ${formatId === f.formatId ? "bg-accent/10 border border-accent" : "bg-surface border border-border"}`}>
            <input type="radio" checked={formatId === f.formatId} onChange={() => setFormatId(f.formatId)} />
            <span className="text-sm">{fmtLabel(f)}</span>
          </label>
        ))}
      </div>
    </div>
  );
}
