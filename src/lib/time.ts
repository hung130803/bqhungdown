/**
 * Format an ISO timestamp as a human-friendly relative string in Vietnamese.
 *
 * Examples:
 *   "vài giây trước"
 *   "5 phút trước"
 *   "2 giờ trước"
 *   "Hôm nay 14:32"
 *   "Hôm qua 09:15"
 *   "3 ngày trước"
 *   "12/05/2026"
 */
export function formatRelative(isoOrDate: string | Date | null | undefined): string {
  if (!isoOrDate) return "—";
  const d = typeof isoOrDate === "string" ? new Date(isoOrDate) : isoOrDate;
  if (isNaN(d.getTime())) return "—";

  const now = new Date();
  const diffMs = now.getTime() - d.getTime();
  const diffSec = Math.floor(diffMs / 1000);
  const diffMin = Math.floor(diffSec / 60);
  const diffHour = Math.floor(diffMin / 60);

  if (diffSec < 0) return d.toLocaleString("vi-VN");
  if (diffSec < 30) return "vài giây trước";
  if (diffMin < 1) return `${diffSec} giây trước`;
  if (diffMin < 60) return `${diffMin} phút trước`;

  // Compute calendar-day difference (not just 24h windows).
  const startOf = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const dayDiff = Math.floor((startOf(now) - startOf(d)) / (24 * 3600 * 1000));

  const hhmm = d.toLocaleTimeString("vi-VN", { hour: "2-digit", minute: "2-digit" });

  if (dayDiff === 0) {
    if (diffHour < 6) return `${diffHour} giờ trước`;
    return `Hôm nay ${hhmm}`;
  }
  if (dayDiff === 1) return `Hôm qua ${hhmm}`;
  if (dayDiff < 7) return `${dayDiff} ngày trước`;
  if (dayDiff < 30) return `${Math.floor(dayDiff / 7)} tuần trước`;
  if (dayDiff < 365) return `${Math.floor(dayDiff / 30)} tháng trước`;
  return `${Math.floor(dayDiff / 365)} năm trước`;
}

/** Returns true if `iso` looks like a date today. */
export function isToday(iso: string | null | undefined): boolean {
  if (!iso) return false;
  const d = new Date(iso);
  if (isNaN(d.getTime())) return false;
  const n = new Date();
  return d.getFullYear() === n.getFullYear() && d.getMonth() === n.getMonth() && d.getDate() === n.getDate();
}
