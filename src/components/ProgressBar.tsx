import { formatBytes, formatPercent, formatSpeed, formatEta } from "@/lib/format";

interface Props {
  downloaded: number;
  total: number | null;
  speedBps: number | null;
  etaSec: number | null;
}

export function ProgressBar({ downloaded, total, speedBps, etaSec }: Props) {
  const pct = total && total > 0 ? (downloaded / total) * 100 : null;
  return (
    <div className="space-y-1">
      <div className="h-2 rounded-full bg-surface-2 overflow-hidden">
        <div className="h-full bg-accent transition-[width]" style={{ width: pct != null ? `${Math.min(100, pct)}%` : "0%" }} />
      </div>
      <div className="flex justify-between text-xs text-muted">
        <span>{formatBytes(downloaded)} / {formatBytes(total)} · {formatPercent(pct)}</span>
        <span>{formatSpeed(speedBps)} · ETA {formatEta(etaSec)}</span>
      </div>
    </div>
  );
}
