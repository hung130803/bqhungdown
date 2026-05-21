import { useMemo, useState } from "react";
import * as cmd from "@/ipc/commands";
import type { ChannelInfo, ChannelVideo } from "@/types/models";
import { Thumbnail } from "./Thumbnail";

type SortKey = "newest" | "oldest" | "popular" | "longest" | "shortest";
type LengthFilter = "all" | "short" | "medium" | "long";
type DateFilter = "all" | "7d" | "30d" | "90d" | "1y" | "custom";

/** Cap on how many videos we ask yt-dlp to enumerate. Higher = more time
 *  spent at fetch. 500 covers virtually every "channel scrape" use case
 *  while still finishing in 30-60s for big channels. */
const HARD_FETCH_LIMIT = 500;

function parseDate(yyyymmdd: string | null | undefined): Date | null {
  if (!yyyymmdd || yyyymmdd.length !== 8) return null;
  const y = +yyyymmdd.slice(0, 4);
  const m = +yyyymmdd.slice(4, 6);
  const d = +yyyymmdd.slice(6, 8);
  if (!y || !m || !d) return null;
  return new Date(y, m - 1, d);
}

function formatDuration(sec: number | null | undefined): string {
  if (sec == null) return "";
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  if (m >= 60) {
    const h = Math.floor(m / 60);
    const mm = m % 60;
    return `${h}:${String(mm).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  }
  return `${m}:${String(s).padStart(2, "0")}`;
}

function formatViews(n: number | null | undefined): string {
  if (n == null) return "";
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

interface Props {
  /** Submit selected video URLs to be queued via enqueue_batch. */
  onSubmit: (urls: string[]) => Promise<void> | void;
}

/**
 * Channel-mode input — flow:
 *   1. User pastes channel/user URL → press "Lấy danh sách".
 *   2. We fetch all videos (up to HARD_FETCH_LIMIT) so they can filter
 *      freely without re-fetching.
 *   3. User filters/sorts (date range, view count, length, sort), unticks
 *      what they don't want, then queues the rest.
 */
export function ChannelInput({ onSubmit }: Props) {
  const [url, setUrl] = useState("");
  const [info, setInfo] = useState<ChannelInfo | null>(null);
  const [videos, setVideos] = useState<ChannelVideo[]>([]);
  const [loading, setLoading] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const [sortKey, setSortKey] = useState<SortKey>("newest");
  const [lengthFilter, setLengthFilter] = useState<LengthFilter>("all");
  const [dateFilter, setDateFilter] = useState<DateFilter>("all");
  const [customFromDate, setCustomFromDate] = useState<string>("");
  const [customToDate, setCustomToDate] = useState<string>("");
  const [minViews, setMinViews] = useState<string>("");
  const [excluded, setExcluded] = useState<Set<string>>(new Set());
  const [submitting, setSubmitting] = useState(false);

  const handleFetch = async () => {
    if (!url.trim()) return;
    setLoading(true);
    setErrorMsg(null);
    setInfo(null);
    setVideos([]);
    setExcluded(new Set());
    try {
      const r = await cmd.fetchChannelVideos(url.trim(), HARD_FETCH_LIMIT);
      setInfo(r.info);
      setVideos(r.videos);
    } catch (e) {
      const msg = (e as { message?: string })?.message ?? String(e);
      setErrorMsg(msg || "Không lấy được danh sách kênh. Kiểm tra URL hoặc cookies.");
    } finally {
      setLoading(false);
    }
  };

  /** Apply filters + sort. */
  const filtered = useMemo(() => {
    let list = videos.slice();

    // Date filter
    if (dateFilter !== "all") {
      const now = Date.now();
      let fromMs: number | null = null;
      let toMs: number | null = null;
      if (dateFilter === "7d") fromMs = now - 7 * 86400_000;
      else if (dateFilter === "30d") fromMs = now - 30 * 86400_000;
      else if (dateFilter === "90d") fromMs = now - 90 * 86400_000;
      else if (dateFilter === "1y") fromMs = now - 365 * 86400_000;
      else if (dateFilter === "custom") {
        fromMs = customFromDate ? new Date(customFromDate).getTime() : null;
        toMs = customToDate ? new Date(customToDate).getTime() : null;
      }
      list = list.filter((v) => {
        const d = parseDate(v.uploadDate);
        if (!d) return false;
        const t = d.getTime();
        if (fromMs != null && t < fromMs) return false;
        if (toMs != null && t > toMs + 86400_000) return false;
        return true;
      });
    }

    // Length filter
    if (lengthFilter !== "all") {
      list = list.filter((v) => {
        const d = v.durationSec ?? 0;
        if (lengthFilter === "short") return d > 0 && d < 60;
        if (lengthFilter === "medium") return d >= 60 && d <= 600;
        if (lengthFilter === "long") return d > 600;
        return true;
      });
    }

    // Min views
    const minV = +minViews;
    if (Number.isFinite(minV) && minV > 0) {
      list = list.filter((v) => (v.viewCount ?? 0) >= minV);
    }

    // Sort
    if (sortKey === "popular") {
      list.sort((a, b) => (b.viewCount ?? 0) - (a.viewCount ?? 0));
    } else if (sortKey === "longest") {
      list.sort((a, b) => (b.durationSec ?? 0) - (a.durationSec ?? 0));
    } else if (sortKey === "shortest") {
      list.sort((a, b) => (a.durationSec ?? 0) - (b.durationSec ?? 0));
    } else if (sortKey === "oldest") {
      // Backend returns newest-first; reverse for oldest.
      list.reverse();
    }
    // newest = backend default order, no-op.
    return list;
  }, [videos, sortKey, lengthFilter, dateFilter, customFromDate, customToDate, minViews]);

  const selectedCount = filtered.filter((v) => !excluded.has(v.url)).length;

  const toggleAll = () => {
    const allInFilteredExcluded = filtered.every((v) => excluded.has(v.url));
    if (allInFilteredExcluded) {
      // Unexclude every video in current filter view.
      setExcluded((prev) => {
        const next = new Set(prev);
        for (const v of filtered) next.delete(v.url);
        return next;
      });
    } else {
      // Exclude every video in current filter view.
      setExcluded((prev) => {
        const next = new Set(prev);
        for (const v of filtered) next.add(v.url);
        return next;
      });
    }
  };
  const toggleOne = (u: string) => {
    setExcluded((prev) => {
      const next = new Set(prev);
      if (next.has(u)) next.delete(u);
      else next.add(u);
      return next;
    });
  };

  const handleSubmit = async () => {
    const urls = filtered.filter((v) => !excluded.has(v.url)).map((v) => v.url);
    if (urls.length === 0) return;
    setSubmitting(true);
    try {
      await Promise.resolve(onSubmit(urls));
      setUrl("");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="space-y-3">
      <label className="text-sm font-medium text-fg">Tải kênh</label>
      <div className="flex gap-2 flex-wrap">
        <input
          type="url"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="https://www.youtube.com/@MrBeast hoặc https://www.tiktok.com/@khaby.lame"
          disabled={loading}
          className="flex-1 min-w-[260px] px-3 py-2 rounded-md bg-surface border border-border text-fg placeholder:text-muted text-sm disabled:opacity-50"
        />
        <button
          onClick={() => void handleFetch()}
          disabled={!url.trim() || loading}
          className="px-3 py-2 rounded-md bg-surface-2 border border-border text-fg text-sm disabled:opacity-50"
        >
          {loading ? "Đang lấy…" : "Lấy danh sách"}
        </button>
      </div>
      <p className="text-xs text-muted">
        App sẽ tải tới {HARD_FETCH_LIMIT} video gần nhất. Sau đó bạn có thể lọc theo ngày, view, độ dài.
      </p>

      {errorMsg && (
        <div className="px-3 py-2 rounded-md bg-danger/10 border border-danger text-danger text-sm">
          {errorMsg}
        </div>
      )}

      {info && (
        <div className="space-y-3 mt-2">
          {/* Channel header */}
          <div className="flex items-center gap-3 p-3 rounded-xl bg-surface border border-border">
            {info.thumbnail && (
              <img
                src={info.thumbnail}
                alt={info.title}
                className="w-12 h-12 rounded-full object-cover"
              />
            )}
            <div className="flex-1 min-w-0">
              <div className="text-sm font-medium text-fg truncate">{info.title || info.url}</div>
              <div className="text-xs text-muted">
                {info.videoCount ? `${info.videoCount.toLocaleString()} video trên kênh · ` : ""}
                Đã lấy {videos.length} video
              </div>
            </div>
          </div>

          {/* Filter / sort row */}
          <div className="flex items-center gap-2 flex-wrap text-sm">
            <select
              value={sortKey}
              onChange={(e) => setSortKey(e.target.value as SortKey)}
              className="px-2 py-1.5 rounded-md bg-surface border border-border"
            >
              <option value="newest">Mới nhất</option>
              <option value="oldest">Cũ nhất</option>
              <option value="popular">Nhiều view nhất</option>
              <option value="longest">Dài nhất</option>
              <option value="shortest">Ngắn nhất</option>
            </select>
            <select
              value={dateFilter}
              onChange={(e) => setDateFilter(e.target.value as DateFilter)}
              className="px-2 py-1.5 rounded-md bg-surface border border-border"
            >
              <option value="all">Mọi thời gian</option>
              <option value="7d">7 ngày qua</option>
              <option value="30d">30 ngày qua</option>
              <option value="90d">3 tháng qua</option>
              <option value="1y">1 năm qua</option>
              <option value="custom">Tuỳ chọn…</option>
            </select>
            {dateFilter === "custom" && (
              <>
                <input
                  type="date"
                  value={customFromDate}
                  onChange={(e) => setCustomFromDate(e.target.value)}
                  className="px-2 py-1.5 rounded-md bg-surface border border-border"
                  title="Từ ngày"
                />
                <input
                  type="date"
                  value={customToDate}
                  onChange={(e) => setCustomToDate(e.target.value)}
                  className="px-2 py-1.5 rounded-md bg-surface border border-border"
                  title="Đến ngày"
                />
              </>
            )}
            <select
              value={lengthFilter}
              onChange={(e) => setLengthFilter(e.target.value as LengthFilter)}
              className="px-2 py-1.5 rounded-md bg-surface border border-border"
            >
              <option value="all">Mọi độ dài</option>
              <option value="short">Ngắn (&lt; 1 phút)</option>
              <option value="medium">Vừa (1–10 phút)</option>
              <option value="long">Dài (&gt; 10 phút)</option>
            </select>
            <input
              type="number"
              min={0}
              value={minViews}
              onChange={(e) => setMinViews(e.target.value)}
              placeholder="View tối thiểu"
              className="px-2 py-1.5 rounded-md bg-surface border border-border w-32"
            />
            <button
              onClick={toggleAll}
              className="px-3 py-1.5 rounded-md border border-border hover:bg-surface-2"
            >
              {filtered.every((v) => excluded.has(v.url))
                ? "Chọn tất cả"
                : "Bỏ chọn tất cả"}
            </button>
            <span className="text-muted text-xs ml-auto">
              Sẽ thêm {selectedCount} / {filtered.length} video vào hàng đợi
            </span>
          </div>

          {/* Video list */}
          <div className="space-y-1.5 max-h-[480px] overflow-y-auto rounded-lg border border-border">
            {filtered.length === 0 && (
              <p className="text-muted text-center py-8 text-sm">Không có video nào khớp bộ lọc.</p>
            )}
            {filtered.map((v) => {
              const checked = !excluded.has(v.url);
              const date = parseDate(v.uploadDate);
              return (
                <label
                  key={v.url}
                  className={`flex items-center gap-3 px-3 py-2 cursor-pointer ${
                    checked ? "bg-accent/5" : "bg-surface"
                  }`}
                >
                  <input
                    type="checkbox"
                    checked={checked}
                    onChange={() => toggleOne(v.url)}
                    className="h-4 w-4"
                  />
                  <div className="aspect-video w-24 shrink-0 rounded overflow-hidden">
                    <Thumbnail
                      src={v.thumbnail ?? null}
                      extractor={info.extractor}
                      alt={v.title}
                    />
                  </div>
                  <div className="flex-1 min-w-0">
                    <div className="text-sm text-fg truncate" title={v.title}>
                      {v.title || v.url}
                    </div>
                    <div className="text-xs text-muted truncate flex items-center gap-2">
                      {v.durationSec != null && <span>{formatDuration(v.durationSec)}</span>}
                      {v.viewCount != null && <span>{formatViews(v.viewCount)} views</span>}
                      {date && <span>{date.toLocaleDateString("vi-VN")}</span>}
                    </div>
                  </div>
                </label>
              );
            })}
          </div>

          <button
            onClick={() => void handleSubmit()}
            disabled={selectedCount === 0 || submitting}
            className="w-full py-2.5 rounded-md bg-accent text-accent-fg font-medium disabled:opacity-50"
          >
            {submitting ? "Đang thêm…" : `Tải ${selectedCount} video vào hàng đợi`}
          </button>
        </div>
      )}
    </div>
  );
}
