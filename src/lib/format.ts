export function formatBytes(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = n; let i = 0;
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
  return `${v.toFixed(v >= 100 ? 0 : v >= 10 ? 1 : 2)} ${units[i]}`;
}

export function formatSpeed(bps: number | null | undefined): string {
  if (bps == null || !Number.isFinite(bps)) return "—";
  return `${formatBytes(bps)}/s`;
}

export function formatEta(sec: number | null | undefined): string {
  if (sec == null || !Number.isFinite(sec)) return "—";
  const s = Math.max(0, Math.floor(sec));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = s % 60;
  if (h > 0) return `${h}:${String(m).padStart(2, "0")}:${String(ss).padStart(2, "0")}`;
  return `${m}:${String(ss).padStart(2, "0")}`;
}

export function formatDuration(sec: number | null | undefined): string {
  return formatEta(sec);
}

export function formatPercent(p: number | null | undefined): string {
  if (p == null || !Number.isFinite(p)) return "—";
  return `${Math.max(0, Math.min(100, p)).toFixed(p >= 10 ? 0 : 1)}%`;
}
