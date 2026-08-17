import { useEffect, useMemo, useRef, useState } from "react";
import * as cmd from "@/ipc/commands";
import { onDouyinScraperProgress, onDouyinScraperNote } from "@/ipc/events";
// `ChannelInfo` KHÔNG còn được import: giao diện không dựng thông tin kênh nữa,
// Rust gửi lên nguyên khối. Thấy nó quay lại đây là dấu hiệu ai đó lại ghép tay.
import type { ChannelVideo } from "@/types/models";
import { formatDuration } from "@/lib/format";
import { Thumbnail } from "./Thumbnail";
import { useChannelStore } from "@/stores/useChannelStore";
import { useSettingsStore } from "@/stores/useSettingsStore";

/** Detect if a URL is a Douyin user channel link. */
function isDouyinChannelUrl(url: string): boolean {
  const u = url.toLowerCase();
  return u.includes("douyin.com") && (u.includes("/user/") || u.includes("v.douyin.com"));
}

type SortKey = "newest" | "oldest" | "popular" | "longest" | "shortest";
type LengthFilter = "all" | "short" | "medium" | "long";
type DateFilter = "all" | "7d" | "30d" | "90d" | "1y" | "custom" | "year" | "month";

/** THƯỚC ĐO "bài nào hot". Mỗi nền tảng cho một bộ số khác nhau, nên thay vì
 *  đoán, app hỏi thẳng dữ liệu xem có gì rồi chỉ cho chọn thứ CÓ THẬT.
 *   · YouTube  → xem · tim · bình luận            (API không cho chia sẻ)
 *   · TikTok   → xem · tim · bình luận · chia sẻ  (đủ cả 4)
 *   · Douyin   → tim · bình luận · chia sẻ        (KHÔNG bao giờ có lượt xem)
 */
type Metric = "views" | "likes" | "comments" | "shares";

/** Thứ tự ưu tiên khi tự chọn thước đo mặc định: cái nào "sát người xem" nhất
 *  mà dữ liệu CÓ THẬT thì lấy. Douyin không có view nên rơi xuống tim — giữ
 *  đúng hành vi 0.3.1 mà anh Hùng đã quen. */
const THỨ_TỰ_ƯU_TIÊN: Metric[] = ["views", "likes", "shares", "comments"];

const NHÃN_THƯỚC_ĐO: Record<Metric, string> = {
  views: "lượt xem",
  likes: "lượt tim",
  comments: "lượt bình luận",
  shares: "lượt chia sẻ",
};

/** Ký hiệu đứng trước con số trên mỗi dòng — để không thể lẫn ba số với nhau. */
const KÝ_HIỆU: Record<Metric, string> = {
  views: "",
  likes: "❤",
  comments: "💬",
  shares: "↗",
};

function sốCủa(v: ChannelVideo, m: Metric): number | null {
  if (m === "views") return v.viewCount ?? null;
  if (m === "likes") return v.likeCount ?? null;
  if (m === "comments") return v.commentCount ?? null;
  return v.shareCount ?? null;
}

const RUBBER_THRESHOLD = 5;
const SHORTS_THRESHOLD_SEC = 60;

function parseDate(yyyymmdd: string | null | undefined): Date | null {
  if (!yyyymmdd || yyyymmdd.length !== 8) return null;
  const y = +yyyymmdd.slice(0, 4);
  const m = +yyyymmdd.slice(4, 6);
  const d = +yyyymmdd.slice(6, 8);
  if (!y || !m || !d) return null;
  return new Date(y, m - 1, d);
}

function formatComma(n: number | null | undefined): string {
  if (n == null) return "";
  return n.toLocaleString("en-US");
}

function parseCommaNum(s: string): number | null {
  const cleaned = s.replace(/[,\s]/g, "");
  if (!cleaned) return null;
  const n = parseInt(cleaned, 10);
  return Number.isFinite(n) && n >= 0 ? n : null;
}

function isShortVideo(v: ChannelVideo): boolean {
  return Boolean(v.isShort) || (v.durationSec != null && v.durationSec < SHORTS_THRESHOLD_SEC);
}

/** Backend trả AppError dạng `{ kind, data }`. Convert thành chuỗi VN
 *  dễ đọc thay vì để frontend show "[object Object]". */
function formatErr(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object") {
    const obj = e as Record<string, unknown>;
    if (typeof obj.message === "string") return obj.message;
    const kind = typeof obj.kind === "string" ? obj.kind : "";
    const data = obj.data;
    if (kind === "YtDlpFailed" && typeof data === "string") return data;
    if (kind === "Timeout") return "Hết thời gian chờ. Thử URL ngắn hơn hoặc kiểm tra mạng.";
    if (kind === "InvalidUrl") return "URL không hợp lệ.";
    if (kind === "UnsupportedSite") return "Không hỗ trợ kênh này.";
    if (kind === "Other" && typeof data === "string") return data;
    if (typeof data === "string" && data.length > 0) return `${kind}: ${data}`;
    return kind || JSON.stringify(e);
  }
  return String(e);
}

interface Props {
  onSubmit: (urls: string[], channelName?: string) => Promise<void> | void;
}

export function ChannelInput({ onSubmit }: Props) {
  const url = useChannelStore((s) => s.url);
  const setUrl = useChannelStore((s) => s.setUrl);
  const loading = useChannelStore((s) => s.loading);
  const setLoading = useChannelStore((s) => s.setLoading);
  const errorMsg = useChannelStore((s) => s.errorMsg);
  const setError = useChannelStore((s) => s.setError);
  const info = useChannelStore((s) => s.info);
  const videos = useChannelStore((s) => s.videos);
  const setResult = useChannelStore((s) => s.setResult);
  const resetResult = useChannelStore((s) => s.resetResult);
  const excluded = useChannelStore((s) => s.excluded);
  const setExcluded = useChannelStore((s) => s.setExcluded);
  const toggleExcluded = useChannelStore((s) => s.toggleExcluded);
  const fetchStartedAt = useChannelStore((s) => s.fetchStartedAt);
  const setFetchStartedAt = useChannelStore((s) => s.setFetchStartedAt);
  const detailed = useChannelStore((s) => s.detailed);
  const setDetailed = useChannelStore((s) => s.setDetailed);

  const [sortKey, setSortKey] = useState<SortKey>("newest");
  const [lengthFilter, setLengthFilter] = useState<LengthFilter>("all");
  const [dateFilter, setDateFilter] = useState<DateFilter>("all");
  const [customFromDate, setCustomFromDate] = useState<string>("");
  const [customToDate, setCustomToDate] = useState<string>("");
  // Lọc theo năm / tháng cụ thể (so khớp tiền tố YYYYMMDD — chuẩn, không lệch múi giờ).
  const [filterYear, setFilterYear] = useState<string>("");
  const [filterMonth, setFilterMonth] = useState<string>("");
  const [minViewsRaw, setMinViewsRaw] = useState<string>("");
  const [maxViewsRaw, setMaxViewsRaw] = useState<string>("");
  const [submitting, setSubmitting] = useState(false);
  /** Tab kết quả đang xem: "long" hoặc "short". */
  const [resultTab, setResultTab] = useState<"long" | "short">("long");
  /** Số video đã scrape được (Douyin WebView scraper). */
  const [scrapeProgress, setScrapeProgress] = useState(0);

  /** URL các video vừa bấm "Khôi phục" trong phiên này -> bỏ dấu "đã tải"
   *  ngay trên UI (backend đã gỡ khỏi sổ tải). */
  const [restoredUrls, setRestoredUrls] = useState<Set<string>>(new Set());
  const isDownloaded = (v: ChannelVideo) => !!v.downloaded && !restoredUrls.has(v.url);
  const restoreOne = async (u: string) => {
    try {
      await cmd.restoreDownloaded([u]);
      setRestoredUrls((prev) => new Set(prev).add(u));
    } catch {
      /* im lặng — không chặn thao tác khác */
    }
  };

  /** Tên thư mục lưu cho kênh này (mặc định = tên kênh, sửa được). */
  const channelSubfolder = useSettingsStore((s) => s.settings?.channelSubfolder ?? true);
  const [folderName, setFolderName] = useState("");
  const [addedMsg, setAddedMsg] = useState<string | null>(null);
  /** Cảnh báo từ lượt quét Douyin (thiếu video / bị chặn giữa chừng). */
  const [douyinNote, setDouyinNote] = useState<string | null>(null);
  useEffect(() => {
    if (info?.title) setFolderName(info.title);
  }, [info?.title]);

  /** Đồng hồ ms hiện tại — re-render mỗi giây để tính elapsedSec. */
  const [, setNowTick] = useState(0);
  useEffect(() => {
    if (!loading || fetchStartedAt == null) return;
    const t = setInterval(() => setNowTick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, [loading, fetchStartedAt]);

  /** Listen for Douyin scraper progress events (video count updates). */
  useEffect(() => {
    let ignore = false;
    const setup = async () => {
      const unlisten = await onDouyinScraperProgress((p) => {
        if (!ignore) setScrapeProgress(p.count);
      });
      return unlisten;
    };
    let unlisten: (() => void) | undefined;
    setup().then((u) => { unlisten = u; });
    return () => {
      ignore = true;
      unlisten?.();
    };
  }, []);

  /** Cảnh báo khi lượt quét Douyin RA KẾT QUẢ nhưng thiếu (chưa có cookie đăng
   *  nhập → Douyin cắt ở ~20 video) hoặc bị chặn giữa chừng. Không chặn thao
   *  tác, chỉ nói cho anh Hùng biết vì sao danh sách ngắn. */
  useEffect(() => {
    let ignore = false;
    let unlisten: (() => void) | undefined;
    onDouyinScraperNote((p) => {
      if (!ignore) setDouyinNote(p.message);
    }).then((u) => { unlisten = u; });
    return () => {
      ignore = true;
      unlisten?.();
    };
  }, []);
  const elapsedSec =
    loading && fetchStartedAt != null
      ? Math.max(0, Math.floor((Date.now() - fetchStartedAt) / 1000))
      : 0;

  // ── Rubber-band selection state ────────────────────────────────────────
  const bandRef = useRef({
    active: false,
    visible: false,
    anchorPageX: 0,
    anchorPageY: 0,
    curClientX: 0,
    curClientY: 0,
    initial: new Set<string>(),
    /** URLs đã từng nằm trong rubber-band trong session này — sticky:
     *  một khi bị "chạm" thì giữ trạng thái đó dù chuột kéo đi xa. */
    dragTouched: new Set<string>(),
    mode: "add" as "add" | "remove",
    suppressClick: false,
    /** True khi mousedown ngoài row → mouseup mà không drag = clear all. */
    clearOnUp: false,
  });
  const [, setBandTick] = useState(0);
  /** RAF state để auto-scroll mượt khi kéo gần mép list. */
  const scrollRafRef = useRef<{ raf: number | null; speed: number; el: HTMLElement | null }>({
    raf: null,
    speed: 0,
    el: null,
  });

  const handleFetch = async (forceRefresh = false) => {
    if (!url.trim()) return;
    setLoading(true);
    setFetchStartedAt(Date.now());
    setError(null);
    setAddedMsg(null);
    setDouyinNote(null);
    setScrapeProgress(0);
    resetResult();

    const trimmed = url.trim();

    try {
      // ── Douyin: use WebView-based scraper ───────────────────────────────
      if (isDouyinChannelUrl(trimmed)) {
        // DÙNG THẲNG mảng Rust trả về. Ở 0.3.0 chỗ này có một vòng `.map()`
        // tự dựng lại ChannelVideo và chỉ chép 4 trường — `uploadDate` và
        // `likeCount` Rust gửi lên bị vứt đi ngay tại đây. Đó là lý do anh
        // Hùng cài đúng 0.3.0 mà vẫn không thấy ngày đăng lẫn lượt tim, dù
        // Rust đúng và cả 2 bộ test đều xanh. ĐỪNG dựng lại đối tượng ở đây.
        const r = await cmd.scrapeDouyinChannel(trimmed);

        if (r.videos.length === 0) {
          setError("Không lấy được video nào từ kênh này.");
          return;
        }

        // DÙNG THẲNG `r.info` — 0.3.1 ghép tay ở ĐÚNG CHỖ NÀY:
        //   title: `Kênh Douyin — ${videos.length} video`   → tên kênh BỊA,
        //          tên thật ("大雄探片") không bao giờ hiện;
        //   thumbnail: videos[0]?.thumbnail                 → ẢNH BÌA BÀI MỚI
        //          NHẤT bị đem làm ảnh đại diện kênh.
        // Đó là lý do anh Hùng báo "tên kênh ảnh avatar kênh không chuẩn".
        // Rust nay gửi lên tên + ảnh THẬT; ĐỪNG dựng lại `info` ở đây nữa.
        setResult(r.info, r.videos);
        return;
      }

      // ── YouTube / TikTok / other: use yt-dlp via Rust backend ───────────
      const r = await cmd.fetchChannelVideos(trimmed, 0, detailed, "all", forceRefresh);
      setResult(r.info, r.videos);
    } catch (e) {
      const msg = formatErr(e);
      if (!msg.includes("Đã huỷ") && !msg.toLowerCase().includes("cancel")) {
        setError(msg || "Không lấy được danh sách kênh. Kiểm tra URL hoặc cookies.");
      }
    } finally {
      setLoading(false);
      setFetchStartedAt(null);
    }
  };

  const handleCancel = async () => {
    try {
      await cmd.cancelChannelFetch();
    } catch {
      /* noop */
    }
    setLoading(false);
    setFetchStartedAt(null);
    setError(null);
  };

  // ── THƯỚC ĐO "BÀI NÀO HOT" ────────────────────────────────────────────
  // YouTube trả lượt XEM. DOUYIN thì KHÔNG (play_count luôn 0, kể cả có cookie
  // đăng nhập) nhưng trả lượt TIM / BÌNH LUẬN / CHIA SẺ thật. TikTok có đủ cả
  // bốn. Nên KHÔNG đoán theo tên nền tảng: hỏi thẳng dữ liệu xem có số nào,
  // rồi sort/lọc/hiện đều dùng chung đúng số anh Hùng đang chọn.
  const thướcĐoCóThật = useMemo<Metric[]>(
    () => THỨ_TỰ_ƯU_TIÊN.filter((m) => videos.some((v) => sốCủa(v, m) != null)),
    [videos],
  );
  // Thước đo anh Hùng tự chọn. `null` = chưa chọn → dùng cái đầu tiên có thật.
  const [thướcĐoChọn, setThướcĐoChọn] = useState<Metric | null>(null);
  const thướcĐo: Metric =
    thướcĐoChọn && thướcĐoCóThật.includes(thướcĐoChọn)
      ? thướcĐoChọn
      : (thướcĐoCóThật[0] ?? "views");
  const nhãnThướcĐo = NHÃN_THƯỚC_ĐO[thướcĐo];
  const hasViews = thướcĐoCóThật.includes("views");
  const metricOf = (v: ChannelVideo): number | null => sốCủa(v, thướcĐo);

  /** Chung 1 hàm lọc: nhận list nguồn → áp toàn bộ filter / sort. Dùng riêng
   *  cho long & shorts để bộ lọc apply trên cả 2 cột. */
  const applyFilter = (src: ChannelVideo[]): ChannelVideo[] => {
    let list = src.slice();
    if (dateFilter === "year") {
      // Lọc theo năm: khớp 4 ký tự đầu của uploadDate (YYYYMMDD) — chuẩn, không lệch múi giờ.
      if (filterYear) list = list.filter((v) => (v.uploadDate ?? "").startsWith(filterYear));
    } else if (dateFilter === "month") {
      // Lọc theo tháng cụ thể trong 1 năm: khớp tiền tố YYYYMM.
      if (filterYear && filterMonth) {
        const key = filterYear + filterMonth.padStart(2, "0");
        list = list.filter((v) => (v.uploadDate ?? "").startsWith(key));
      }
    } else if (dateFilter !== "all") {
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
    if (lengthFilter !== "all") {
      list = list.filter((v) => {
        const d = v.durationSec ?? 0;
        if (lengthFilter === "short") return d > 0 && d < 60;
        if (lengthFilter === "medium") return d >= 60 && d <= 600;
        if (lengthFilter === "long") return d > 600;
        return true;
      });
    }
    const minV = parseCommaNum(minViewsRaw);
    const maxV = parseCommaNum(maxViewsRaw);
    if (minV != null) list = list.filter((v) => (metricOf(v) ?? 0) >= minV);
    if (maxV != null) list = list.filter((v) => (metricOf(v) ?? 0) <= maxV);
    if (sortKey === "popular") {
      list.sort((a, b) => (metricOf(b) ?? 0) - (metricOf(a) ?? 0));
    } else if (sortKey === "longest") {
      list.sort((a, b) => (b.durationSec ?? 0) - (a.durationSec ?? 0));
    } else if (sortKey === "shortest") {
      list.sort((a, b) => (a.durationSec ?? 0) - (b.durationSec ?? 0));
    } else if (sortKey === "newest" || sortKey === "oldest") {
      // Khi CÓ ngày thật thì sắp theo ngày, đừng tin thứ tự nguồn.
      // Douyin trả bài GHIM lên đầu nên thứ tự nguồn KHÔNG theo thời gian —
      // đo 16/08/2026: 3 bài đầu là 2026-01-18, 2025-12-10, 2025-10-26 rồi
      // mới tới 2026-07-28. "Mới nhất" kiểu cũ (giữ nguyên thứ tự nguồn) là
      // sai hẳn; "Cũ nhất" (reverse) cũng sai theo.
      const coNgay = list.length > 0 && list.every((v) => (v.uploadDate ?? "").length >= 8);
      if (coNgay) {
        // uploadDate là YYYYMMDD nên so chuỗi = so thời gian.
        list.sort((a, b) =>
          sortKey === "newest"
            ? (b.uploadDate ?? "").localeCompare(a.uploadDate ?? "")
            : (a.uploadDate ?? "").localeCompare(b.uploadDate ?? ""),
        );
      } else if (sortKey === "oldest") {
        list.reverse();
      }
    }
    return list;
  };

  // Tách video → 2 nhóm rồi lọc/sort riêng để mỗi cột độc lập.
  // Chỉ áp dụng cho YouTube — nền tảng khác (TikTok/Douyin) hiển thị 1 list.
  const isYoutube = (info?.extractor ?? "").toLowerCase().includes("youtube");
  /** Đang làm việc với kênh Douyin — theo link đang gõ HOẶC kết quả đã lấy. */
  const douyinMode =
    isDouyinChannelUrl(url) || (info?.extractor ?? "").toLowerCase().includes("douyin");
  const longList = useMemo(
    () => (isYoutube ? applyFilter(videos.filter((v) => !isShortVideo(v))) : []),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [videos, sortKey, thướcĐo, lengthFilter, dateFilter, customFromDate, customToDate, filterYear, filterMonth, minViewsRaw, maxViewsRaw, isYoutube],
  );
  const shortList = useMemo(
    () => (isYoutube ? applyFilter(videos.filter(isShortVideo)) : []),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [videos, sortKey, thướcĐo, lengthFilter, dateFilter, customFromDate, customToDate, filterYear, filterMonth, minViewsRaw, maxViewsRaw, isYoutube],
  );
  /** Cho non-YouTube: mọi entry chung 1 list. */
  const allList = useMemo(
    () => (isYoutube ? [] : applyFilter(videos)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [videos, sortKey, thướcĐo, lengthFilter, dateFilter, customFromDate, customToDate, filterYear, filterMonth, minViewsRaw, maxViewsRaw, isYoutube],
  );
  /** Items thuộc tab đang hiện. Cho non-YouTube → luôn dùng allList. */
  const visible = isYoutube ? (resultTab === "long" ? longList : shortList) : allList;
  const selectedCount = visible.filter((v) => !excluded.has(v.url)).length;

  /** NGUỒN DUY NHẤT cho "sẽ tải cái gì". Nhãn nút VÀ handleSubmit đều đọc đây
   *  — trước kia mỗi chỗ tự tính một kiểu nên lệch nhau (nút đếm cứng
   *  longList+shortList = 0 với Douyin, còn submit lại đúng). */
  const selectedForSubmit = useMemo(
    () => (isYoutube ? [...longList, ...shortList] : allList).filter((v) => !excluded.has(v.url)),
    [isYoutube, longList, shortList, allList, excluded],
  );

  /** Các năm thực sự có video (cho dropdown "Theo năm/tháng"), mới → cũ. */
  const availableYears = useMemo(() => {
    const set = new Set<string>();
    for (const v of videos) {
      const y = (v.uploadDate ?? "").slice(0, 4);
      if (y.length === 4) set.add(y);
    }
    return Array.from(set).sort((a, b) => b.localeCompare(a));
  }, [videos]);

  const toggleAll = () => {
    const allInExcluded = visible.every((v) => excluded.has(v.url));
    const next = new Set(excluded);
    if (allInExcluded) {
      for (const v of visible) next.delete(v.url);
    } else {
      for (const v of visible) next.add(v.url);
    }
    setExcluded(next);
  };

  // ── Rubber-band ────────────────────────────────────────────────────────
  const onListMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    const target = e.target as HTMLElement;
    if (target.closest("input, button, a, select, textarea")) return;
    const row = target.closest("[data-vid-url]") as HTMLElement | null;
    let mode: "add" | "remove" = "add";
    let clearOnUp = false;
    if (row) {
      const id = row.getAttribute("data-vid-url") ?? "";
      mode = excluded.has(id) ? "add" : "remove";
    } else {
      // Click vào background ngoài row → bỏ chọn tất cả khi mouseup
      // mà không drag.
      clearOnUp = true;
    }
    bandRef.current = {
      active: true,
      visible: false,
      anchorPageX: e.pageX,
      anchorPageY: e.pageY,
      curClientX: e.clientX,
      curClientY: e.clientY,
      initial: new Set(excluded),
      dragTouched: new Set<string>(),
      mode,
      suppressClick: false,
      clearOnUp,
    };
  };

  /** Sticky select: video nào đã từng nằm trong rect → giữ luôn (k bỏ
   *  khi chuột kéo ngược ra). Ta chỉ THÊM vào dragTouched, k bao giờ xoá. */
  const recompute = () => {
    const band = bandRef.current;
    const ax = band.anchorPageX - window.scrollX;
    const ay = band.anchorPageY - window.scrollY;
    const x1 = Math.min(ax, band.curClientX);
    const y1 = Math.min(ay, band.curClientY);
    const x2 = Math.max(ax, band.curClientX);
    const y2 = Math.max(ay, band.curClientY);
    document.querySelectorAll<HTMLElement>("[data-vid-url]").forEach((el) => {
      const id = el.getAttribute("data-vid-url");
      if (!id) return;
      const r = el.getBoundingClientRect();
      const overlap = !(x2 < r.left || x1 > r.right || y2 < r.top || y1 > r.bottom);
      if (overlap) band.dragTouched.add(id);
    });
    // Apply: initial ± dragTouched theo mode.
    const next = new Set(band.initial);
    band.dragTouched.forEach((id) => {
      if (band.mode === "add") next.delete(id);
      else next.add(id);
    });
    setExcluded(next);
  };

  // RAF tick: đẩy scrollTop liên tục cho mượt.
  useEffect(() => {
    const tick = () => {
      const st = scrollRafRef.current;
      if (st.el && st.speed !== 0) {
        st.el.scrollTop += st.speed;
      }
      if (bandRef.current.active && bandRef.current.visible) {
        recompute();
      }
      st.raf = requestAnimationFrame(tick);
    };
    scrollRafRef.current.raf = requestAnimationFrame(tick);
    return () => {
      if (scrollRafRef.current.raf != null) cancelAnimationFrame(scrollRafRef.current.raf);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      const band = bandRef.current;
      if (!band.active) return;
      band.curClientX = e.clientX;
      band.curClientY = e.clientY;
      const dx = Math.abs(e.pageX - band.anchorPageX);
      const dy = Math.abs(e.pageY - band.anchorPageY);
      if (!band.visible && (dx > RUBBER_THRESHOLD || dy > RUBBER_THRESHOLD)) {
        band.visible = true;
        band.suppressClick = true;
      }
      if (band.visible) {
        e.preventDefault();
        // Pick scroll target & speed dựa vị trí chuột so với mép list.
        const SCROLL_ZONE = 80;
        const MAX_SPEED = 60;
        let chosenEl: HTMLElement | null = null;
        let speed = 0;
        document.querySelectorAll<HTMLElement>("[data-scroll-area]").forEach((el) => {
          const r = el.getBoundingClientRect();
          if (e.clientX < r.left || e.clientX > r.right) return;
          if (e.clientY > r.top && e.clientY < r.top + SCROLL_ZONE) {
            const intensity = (SCROLL_ZONE - (e.clientY - r.top)) / SCROLL_ZONE;
            chosenEl = el;
            speed = -Math.max(15, intensity * MAX_SPEED);
          } else if (e.clientY < r.bottom && e.clientY > r.bottom - SCROLL_ZONE) {
            const intensity = (SCROLL_ZONE - (r.bottom - e.clientY)) / SCROLL_ZONE;
            chosenEl = el;
            speed = Math.max(15, intensity * MAX_SPEED);
          }
        });
        scrollRafRef.current.el = chosenEl;
        scrollRafRef.current.speed = speed;
        recompute();
        setBandTick((n) => n + 1);
      } else {
        scrollRafRef.current.speed = 0;
      }
    };
    const onUp = () => {
      const band = bandRef.current;
      if (!band.active) return;
      // Click ngoài (mousedown ngoài row, k drag) → bỏ chọn tab đang xem.
      if (band.clearOnUp && !band.visible) {
        const next = new Set<string>(excluded);
        for (const v of visible) next.add(v.url);
        setExcluded(next);
      }
      scrollRafRef.current.el = null;
      scrollRafRef.current.speed = 0;
      bandRef.current = { ...band, active: false, visible: false, clearOnUp: false };
      setBandTick((n) => n + 1);
      setTimeout(() => {
        bandRef.current.suppressClick = false;
      }, 0);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [visible, excluded]);

  const onListClickCapture = (e: React.MouseEvent) => {
    if (bandRef.current.suppressClick) {
      e.preventDefault();
      e.stopPropagation();
    }
  };

  const toggleOne = (u: string) => {
    toggleExcluded(u);
  };

  /** Click bất kỳ đâu ngoài list video → bỏ tick tất cả tab đang xem.
   *  Dùng global mousedown listener vì click ngoài `<div>` wrapper k
   *  trigger handler cục bộ. */
  useEffect(() => {
    const onGlobalDown = (e: MouseEvent) => {
      if (e.button !== 0) return;
      const target = e.target as HTMLElement;
      // Bỏ qua nếu click trong input/select/button/list-area.
      if (target.closest("input, button, a, select, textarea, label")) return;
      if (target.closest("[data-channel-list]")) return;
      // Click "thực sự ngoài" → bỏ tick tab đang xem.
      const next = new Set<string>(excluded);
      for (const v of visible) next.add(v.url);
      setExcluded(next);
    };
    document.addEventListener("mousedown", onGlobalDown);
    return () => document.removeEventListener("mousedown", onGlobalDown);
  }, [visible, excluded, setExcluded]);

  const handleSubmit = async () => {
    // Submit TẤT CẢ video đang được tick — đọc CHUNG `selectedForSubmit` với
    // nhãn nút để hai chỗ không bao giờ lệch nhau nữa.
    const urls = selectedForSubmit.map((v) => v.url);
    if (urls.length === 0) return;
    setSubmitting(true);
    try {
      const folder = channelSubfolder ? (folderName.trim() || undefined) : undefined;
      await Promise.resolve(onSubmit(urls, folder));
      const where = folder ? ` → thư mục "${folder}"` : "";
      setAddedMsg(`✓ Đã thêm ${urls.length} video vào hàng đợi${where}. Dán kênh khác để tải tiếp.`);
      setUrl("");
    } finally {
      setSubmitting(false);
    }
  };

  const band = bandRef.current;
  const showBand = band.active && band.visible;
  const bandStyle: React.CSSProperties | undefined = showBand
    ? (() => {
        const ax = band.anchorPageX - window.scrollX;
        const ay = band.anchorPageY - window.scrollY;
        const x = Math.min(ax, band.curClientX);
        const y = Math.min(ay, band.curClientY);
        const w = Math.abs(band.curClientX - ax);
        const h = Math.abs(band.curClientY - ay);
        return { left: x, top: y, width: w, height: h, position: "fixed" };
      })()
    : undefined;

  /** Render 1 cột (Video dài hoặc Shorts). */
  const renderColumn = (label: string, items: ChannelVideo[]) => (
    <div className="flex flex-col rounded-lg border border-border overflow-hidden">
      <div className="px-3 py-2 bg-surface-2 border-b border-border text-sm font-medium text-fg flex items-center justify-between">
        <span>{label}</span>
        <span className="text-xs text-muted">{items.length}</span>
      </div>
      <div className="space-y-1 max-h-[520px] overflow-y-auto" data-scroll-area>
        {items.length === 0 && (
          <p className="text-muted text-center py-8 text-xs">Không có video.</p>
        )}
        {items.map((v: ChannelVideo) => {
          const checked = !excluded.has(v.url);
          const date = parseDate(v.uploadDate);
          const dled = isDownloaded(v);
          return (
            <div
              key={v.url}
              data-vid-url={v.url}
              className={`flex items-center gap-3 px-3 py-2 cursor-pointer ${dled ? "bg-warning/10" : checked ? "bg-accent/5" : "bg-surface"}`}
            >
              <input
                type="checkbox"
                checked={checked}
                onChange={() => toggleOne(v.url)}
                onClick={(e) => e.stopPropagation()}
                className="h-4 w-4"
              />
              <div className="aspect-video w-24 shrink-0 rounded overflow-hidden">
                <Thumbnail src={v.thumbnail ?? null} extractor={info?.extractor ?? "youtube"} alt={v.title} />
              </div>
              <div className="flex-1 min-w-0">
                <div className="text-sm text-fg truncate flex items-center gap-1.5" title={v.title}>
                  {v.isPhoto && (
                    <span className="px-1.5 py-0.5 rounded bg-warning/20 text-warning text-[10px] font-medium shrink-0">
                      📷 Ảnh
                    </span>
                  )}
                  {dled && (
                    <span
                      className="px-1.5 py-0.5 rounded bg-warning/25 text-warning text-[10px] font-semibold shrink-0"
                      title="Video này đã tải trước đó (vẫn hiện để anh biết). Bấm Khôi phục để tải lại."
                    >
                      ✓ Đã tải
                    </span>
                  )}
                  <span className="truncate">{v.title || v.url}</span>
                </div>
                <div className="text-xs text-muted truncate flex items-center gap-2">
                  {v.durationSec != null && <span>{formatDuration(v.durationSec)}</span>}
                  {v.viewCount != null && <span>{formatComma(v.viewCount)} lượt xem</span>}
                  {/* Douyin không có view nhưng có tim — hiện số THẬT đang có
                      thay vì để dòng trống.
                      NHÃN PHẢI TỰ NÓI RA NÓ LÀ GÌ: chữ "tim" trơ trọi khiến anh
                      Hùng tưởng app hiện nhầm thứ (17/08/2026). Ghi "lượt tim"
                      kèm ❤ để không thể lẫn với lượt xem — và KHÔNG giấu lời
                      giải thích trong tooltip, đã có khung nói rõ ngay trên
                      danh sách. */}
                  {v.likeCount != null && (
                    <span className="whitespace-nowrap">
                      ❤ {formatComma(v.likeCount)} lượt tim
                    </span>
                  )}
                  {/* BÌNH LUẬN + CHIA SẺ — 0 lượt gọi mạng thêm, số đã nằm sẵn
                      trong CHÍNH gói app tải về để lấy lượt tim. Douyin không
                      cho lượt xem nên CHIA SẺ là thước "hot" tốt nhất còn lại:
                      bài nổi thường chia sẻ > bình luận. */}
                  {v.commentCount != null && (
                    <span className="whitespace-nowrap">
                      💬 {formatComma(v.commentCount)} bình luận
                    </span>
                  )}
                  {v.shareCount != null && (
                    <span className="whitespace-nowrap">
                      ↗ {formatComma(v.shareCount)} chia sẻ
                    </span>
                  )}
                  {date && <span>{date.toLocaleDateString("vi-VN")}</span>}
                  {dled && (
                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        void restoreOne(v.url);
                      }}
                      className="ml-1 px-1.5 py-0.5 rounded border border-warning/50 text-warning hover:bg-warning/15 text-[11px] shrink-0"
                      title="Coi như CHƯA tải: gỡ khỏi sổ đã tải để lần sau tải lại được."
                    >
                      ↩ Khôi phục
                    </button>
                  )}
                </div>
                {v.hashtags && v.hashtags.length > 0 && (
                  <div className="text-[11px] text-accent truncate" title={v.hashtags.join(" ")}>
                    {v.hashtags.slice(0, 6).join(" ")}
                  </div>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );

  return (
    <div className={`space-y-3 ${showBand ? "select-none" : ""}`}>
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
        {loading ? (
          <button
            onClick={() => void handleCancel()}
            className="px-3 py-2 rounded-md bg-danger/10 border border-danger text-danger text-sm hover:bg-danger/20"
          >
            {scrapeProgress > 0
              ? `Đã lấy ${scrapeProgress} video…`
              : `Huỷ (${elapsedSec}s)`}
          </button>
        ) : (
          <>
            <button
              onClick={() => void handleFetch(false)}
              disabled={!url.trim()}
              className="px-3 py-2 rounded-md bg-surface-2 border border-border text-fg text-sm disabled:opacity-50"
            >
              Lấy danh sách
            </button>
            {info && (
              <button
                onClick={() => void handleFetch(true)}
                disabled={!url.trim()}
                className="px-3 py-2 rounded-md border border-border text-fg text-sm disabled:opacity-50"
                title="Bỏ qua bộ nhớ đệm, lấy lại toàn bộ để có view mới nhất (tốn quota như lần đầu)"
              >
                🔄 Làm mới toàn bộ
              </button>
            )}
          </>
        )}
      </div>
      {/* Ô này CHỈ có tác dụng với YouTube: đường Douyin trả kết quả ngay,
          không đi qua bước "dò thêm" nên tích vào cũng không được gì. Hiện
          một ô vô tác dụng chỉ khiến anh Hùng tích rồi ngồi chờ vô ích. */}
      {douyinMode ? (
        <p className="text-[11px] text-muted">
          Kênh Douyin: <b>ngày đăng</b> và <b>lượt tim</b> đã lấy sẵn cùng danh sách,
          không cần bật gì thêm. Riêng <b>lượt xem</b> thì Douyin không cho lấy.
        </p>
      ) : (
        <>
          <label className="inline-flex items-center gap-2 text-xs text-muted">
            <input
              type="checkbox"
              checked={detailed}
              onChange={(e) => setDetailed(e.target.checked)}
              disabled={loading}
              className="h-3.5 w-3.5"
            />
            <span>Lấy thêm số view + ngày chính xác (chậm hơn nhiều)</span>
          </label>
          <p className="text-[11px] text-muted -mt-1">
            💡 Đã nhập <b>YouTube API key</b> trong Cài đặt? Thì khỏi cần tích ô trên —
            view/thời lượng/ngày/hashtag chuẩn tự lấy cho cả kênh trong vài giây.
          </p>
        </>
      )}
      {loading && elapsedSec > 60 && (
        <p className="text-xs text-warning">
          Lấy danh sách lâu hơn dự kiến. Có thể bấm "Huỷ" và thử lại.
        </p>
      )}
      {errorMsg && (
        <div className="px-3 py-2 rounded-md bg-danger/10 border border-danger text-danger text-sm">
          {errorMsg}
        </div>
      )}

      {addedMsg && (
        <div className="px-3 py-2 rounded-md bg-success/10 border border-success text-success text-sm">
          {addedMsg}
        </div>
      )}

      {douyinNote && (
        <div className="px-3 py-2 rounded-md bg-warning/10 border border-warning text-warning text-sm whitespace-pre-line">
          {douyinNote}
        </div>
      )}

      {info && (
        <div className="space-y-3 mt-2">
          <div className="flex items-center gap-3 p-3 rounded-xl bg-surface border border-border">
            {info.thumbnail && (
              // Qua `Thumbnail` chứ KHÔNG dùng <img src> thẳng: ảnh Douyin /
              // Instagram / Bilibili được backend tải hộ kèm Referer đúng, và
              // hỏng thì có ô nền thay vì icon ảnh vỡ. Trước đây riêng ô này
              // gọi thẳng CDN — lạc khỏi đường chung mà mọi dòng video đều đi.
              <div
                data-channel-avatar={info.thumbnail}
                className="w-12 h-12 rounded-full overflow-hidden shrink-0"
              >
                <Thumbnail
                  src={info.thumbnail}
                  extractor={info.extractor}
                  alt={`Ảnh đại diện kênh ${info.title}`}
                />
              </div>
            )}
            <div className="flex-1 min-w-0">
              <div className="text-sm font-medium text-fg truncate">{info.title || info.url}</div>
              <div className="text-xs text-muted">
                {info.videoCount ? `${formatComma(info.videoCount)} bài trên kênh · ` : ""}
                {isYoutube
                  ? `Đã lấy ${videos.length} (${longList.length} dài + ${shortList.length} shorts)`
                  : `Đã lấy ${videos.length} bài`}
                {(() => {
                  const n = videos.filter(isDownloaded).length;
                  return n > 0 ? ` · ${formatComma(n)} đã tải (tích vàng)` : "";
                })()}
              </div>
            </div>
          </div>

          {info.apiNote && (
            <div className="px-3 py-2 rounded-md bg-warning/10 border border-warning text-warning text-xs">
              {info.apiNote}
            </div>
          )}

          {channelSubfolder && (
            <label className="flex items-center gap-2 text-sm">
              <span className="text-muted whitespace-nowrap">📁 Lưu vào thư mục:</span>
              <input
                type="text"
                value={folderName}
                onChange={(e) => setFolderName(e.target.value)}
                placeholder="(tên thư mục cho kênh này)"
                className="flex-1 px-2 py-1.5 rounded-md bg-surface border border-border text-fg"
                title="Mặc định = tên kênh. Sửa nếu muốn tên khác. Để trống = lưu thẳng vào thư mục gốc."
              />
            </label>
          )}

          <div className="flex items-center gap-2 flex-wrap text-sm">
            <select value={sortKey} onChange={(e) => setSortKey(e.target.value as SortKey)} className="px-2 py-1.5 rounded-md bg-surface border border-border">
              <option value="newest">Mới nhất</option>
              <option value="oldest">Cũ nhất</option>
              <option value="popular">Nhiều {nhãnThướcĐo} nhất</option>
              <option value="longest">Dài nhất</option>
              <option value="shortest">Ngắn nhất</option>
            </select>
            {/* CHỌN THƯỚC ĐO — chỉ hiện khi thật sự có nhiều hơn một số để
                chọn. Ô này điều khiển CẢ "Nhiều … nhất" LẪN hai ô lọc từ–đến
                bên dưới, nên không bao giờ lệch nhau. Danh sách dựng từ dữ
                liệu THẬT: nền tảng không trả số nào thì số đó không xuất hiện,
                thay vì hiện ra rồi lọc trên số rỗng. */}
            {thướcĐoCóThật.length > 1 && (
              <select
                data-metric-picker
                value={thướcĐo}
                onChange={(e) => setThướcĐoChọn(e.target.value as Metric)}
                className="px-2 py-1.5 rounded-md bg-surface border border-border"
                title="Sắp xếp và lọc theo số nào"
              >
                {thướcĐoCóThật.map((m) => (
                  <option key={m} value={m}>
                    {`${KÝ_HIỆU[m]} Theo ${NHÃN_THƯỚC_ĐO[m]}`.trim()}
                  </option>
                ))}
              </select>
            )}
            <select value={dateFilter} onChange={(e) => setDateFilter(e.target.value as DateFilter)} className="px-2 py-1.5 rounded-md bg-surface border border-border">
              <option value="all">Mọi thời gian</option>
              <option value="7d">7 ngày qua</option>
              <option value="30d">30 ngày qua</option>
              <option value="90d">3 tháng qua</option>
              <option value="1y">1 năm qua</option>
              <option value="year">Theo năm…</option>
              <option value="month">Theo tháng…</option>
              <option value="custom">Khoảng ngày…</option>
            </select>
            {(dateFilter === "year" || dateFilter === "month") && (
              <select value={filterYear} onChange={(e) => setFilterYear(e.target.value)} className="px-2 py-1.5 rounded-md bg-surface border border-border" title="Chọn năm">
                <option value="">— Chọn năm —</option>
                {availableYears.map((y) => (
                  <option key={y} value={y}>{y}</option>
                ))}
              </select>
            )}
            {dateFilter === "month" && (
              <select value={filterMonth} onChange={(e) => setFilterMonth(e.target.value)} className="px-2 py-1.5 rounded-md bg-surface border border-border" title="Chọn tháng">
                <option value="">— Chọn tháng —</option>
                {Array.from({ length: 12 }, (_, i) => String(i + 1)).map((m) => (
                  <option key={m} value={m}>Tháng {m}</option>
                ))}
              </select>
            )}
            {dateFilter === "custom" && (
              <>
                <input type="date" value={customFromDate} onChange={(e) => setCustomFromDate(e.target.value)} className="px-2 py-1.5 rounded-md bg-surface border border-border" title="Từ ngày" />
                <input type="date" value={customToDate} onChange={(e) => setCustomToDate(e.target.value)} className="px-2 py-1.5 rounded-md bg-surface border border-border" title="Đến ngày" />
              </>
            )}
            <select value={lengthFilter} onChange={(e) => setLengthFilter(e.target.value as LengthFilter)} className="px-2 py-1.5 rounded-md bg-surface border border-border">
              <option value="all">Mọi độ dài</option>
              <option value="short">Ngắn (&lt; 1 phút)</option>
              <option value="medium">Vừa (1–10 phút)</option>
              <option value="long">Dài (&gt; 10 phút)</option>
            </select>
            <input
              type="text"
              inputMode="numeric"
              value={minViewsRaw}
              onChange={(e) => {
                const n = parseCommaNum(e.target.value);
                setMinViewsRaw(n != null ? formatComma(n) : "");
              }}
              placeholder={`${nhãnThướcĐo.charAt(0).toUpperCase()}${nhãnThướcĐo.slice(1)} từ`}
              className="px-2 py-1.5 rounded-md bg-surface border border-border w-28"
            />
            <span className="text-muted">–</span>
            <input
              type="text"
              inputMode="numeric"
              value={maxViewsRaw}
              onChange={(e) => {
                const n = parseCommaNum(e.target.value);
                setMaxViewsRaw(n != null ? formatComma(n) : "");
              }}
              placeholder={`${nhãnThướcĐo.charAt(0).toUpperCase()}${nhãnThướcĐo.slice(1)} đến`}
              className="px-2 py-1.5 rounded-md bg-surface border border-border w-28"
            />
            <button onClick={toggleAll} className="px-3 py-1.5 rounded-md border border-border hover:bg-surface-2">
              {visible.every((v) => excluded.has(v.url)) ? "Chọn tất cả" : "Bỏ chọn tất cả"}
            </button>
            <select
              onChange={(e) => {
                const v = e.target.value;
                if (!v) return;
                const [side, ns] = v.split(":");
                const n = parseInt(ns);
                if (!Number.isFinite(n) || n <= 0) return;
                const next = new Set<string>(excluded);
                for (const item of visible) next.add(item.url);
                const slice = side === "first" ? visible.slice(0, n) : visible.slice(-n);
                for (const item of slice) next.delete(item.url);
                setExcluded(next);
                e.target.value = "";
              }}
              className="px-2 py-1.5 rounded-md bg-surface border border-border"
              defaultValue=""
              title="Chỉ chọn N video đầu/cuối theo thứ tự đang sắp xếp"
            >
              <option value="" disabled>Chọn nhanh…</option>
              <optgroup label="N đầu">
                <option value="first:10">10 đầu</option>
                <option value="first:25">25 đầu</option>
                <option value="first:50">50 đầu</option>
                <option value="first:100">100 đầu</option>
              </optgroup>
              <optgroup label="N cuối">
                <option value="last:10">10 cuối</option>
                <option value="last:25">25 cuối</option>
                <option value="last:50">50 cuối</option>
                <option value="last:100">100 cuối</option>
              </optgroup>
            </select>
            <span className="text-muted text-xs ml-auto">
              Sẽ thêm {selectedCount} / {visible.length}{" "}
              {isYoutube ? (resultTab === "short" ? "shorts (tab này)" : "video dài (tab này)") : "bài"}
            </span>
          </div>

          {/* NÓI THẲNG vì sao không có cột lượt xem, thay vì im lặng để trống
              hoặc tệ hơn là hiện 0 như thể là số thật.
              17/08/2026 — anh Hùng: "sao tool tải nó hiện lượt tim là sao".
              Câu này TRƯỚC ĐÂY đã có, nhưng là chữ xám cỡ 11px lẫn vào nền nên
              anh ấy lướt qua. Nay là khung có viền, đặt ngay trên danh sách —
              đúng chỗ mắt dừng lại trước khi nhìn các con số. */}
          {!hasViews && thướcĐoCóThật.length > 0 && (
            <div
              data-metric-note
              className="px-3 py-2 rounded-md bg-accent/10 border border-accent/40 text-fg text-xs"
            >
              <b>❤ Con số ở mỗi dòng là LƯỢT TIM, không phải lượt xem.</b>{" "}
              Douyin <b>không cho lấy lượt xem</b> — API của họ luôn trả 0, kể cả khi
              đã nạp cookie đăng nhập (đã đo: 0/21 bài có lượt xem, 21/21 bài có lượt tim).
              Nên app hiện <b>lượt tim</b>, kèm <b>💬 bình luận</b> và <b>↗ chia sẻ</b> —
              cả ba đều là số THẬT, lấy từ chính gói dữ liệu app đã tải, không tốn
              thêm giây nào. Bài nổi trên Douyin thường có <b>chia sẻ cao hơn bình
              luận</b>, nên chọn <b>Theo lượt chia sẻ</b> ở ô bên trên là cách tìm
              bài hot tốt nhất. Ô lọc từ–đến và mục &quot;Nhiều … nhất&quot; đều chạy
              theo đúng số đang chọn.
            </div>
          )}

          {/* YouTube: 2 tab (Video dài / Shorts). Khác: 1 list. */}
          <div data-channel-list onMouseDown={onListMouseDown} onClickCapture={onListClickCapture}>
            {isYoutube ? (
              <>
                <div className="flex items-center gap-1 mb-2">
                  <button
                    onClick={() => setResultTab("long")}
                    className={`flex-1 px-3 py-2 rounded-md text-sm transition-colors ${
                      resultTab === "long"
                        ? "bg-accent text-accent-fg font-medium"
                        : "bg-surface border border-border text-fg hover:bg-surface-2"
                    }`}
                  >
                    Video dài <span className="text-xs opacity-70">({longList.length})</span>
                  </button>
                  <button
                    onClick={() => setResultTab("short")}
                    className={`flex-1 px-3 py-2 rounded-md text-sm transition-colors ${
                      resultTab === "short"
                        ? "bg-accent text-accent-fg font-medium"
                        : "bg-surface border border-border text-fg hover:bg-surface-2"
                    }`}
                  >
                    Shorts <span className="text-xs opacity-70">({shortList.length})</span>
                  </button>
                </div>
                {resultTab === "long"
                  ? renderColumn("Video dài", longList)
                  : renderColumn("Shorts", shortList)}
              </>
            ) : (
              renderColumn("Tất cả bài đăng", allList)
            )}
          </div>

          <button
            onClick={() => void handleSubmit()}
            disabled={submitting}
            className="w-full py-2.5 rounded-md bg-accent text-accent-fg font-medium disabled:opacity-50"
          >
            {(() => {
              // PHẢI dùng ĐÚNG nguồn mà handleSubmit dùng. YouTube = 2 tab
              // (longList+shortList); nền tảng khác (Douyin/TikTok) chỉ có
              // allList — longList/shortList luôn RỖNG. Bản trước cộng cứng
              // longList+shortList nên với Douyin luôn ra 0 → nút báo oan
              // "Chưa chọn video nào" dù 250 bài đang tích.
              const totalSelected = selectedForSubmit.length;
              if (submitting) return "Đang thêm…";
              if (totalSelected === 0) return "Chưa chọn video nào";
              return `Tải ${totalSelected} video vào hàng đợi`;
            })()}
          </button>
        </div>
      )}

      {showBand && bandStyle && (
        <div
          aria-hidden
          className="pointer-events-none border-2 border-dashed border-accent bg-accent/10 z-50"
          style={bandStyle}
        />
      )}
    </div>
  );
}
