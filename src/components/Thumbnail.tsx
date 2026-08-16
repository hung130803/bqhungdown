import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { platformInfo } from "@/lib/platforms";

interface Props {
  src: string | null | undefined;
  extractor: string;
  /** ARIA label / tooltip (thường là tiêu đề video). */
  alt?: string;
}

/** Cache toàn cục: URL → data URL. Tránh fetch lại khi component re-mount
 *  hoặc nhiều row dùng cùng URL (rare but cheap). */
const dataUrlCache = new Map<string, string>();
/** URLs đang được fetch — share promise để tránh fetch trùng. */
const inflight = new Map<string, Promise<string>>();

/** Một số CDN luôn fail từ `<img src>` direct vì họ check Referer/CORS.
 *  Khi src thuộc các CDN này, ta proxy qua backend (set Referer phù hợp)
 *  rồi nhận về data URL. */
function needsProxy(src: string): boolean {
  const lower = src.toLowerCase();
  return (
    lower.includes("cdninstagram.com") ||
    lower.includes("fbcdn.net") ||
    lower.includes("scontent") || // Instagram subdomain
    lower.includes("aweme") ||
    lower.includes("tiktokcdn-eu") || // EU CDN có sometimes Referer check
    lower.includes("douyinpic.com") ||
    lower.includes("hdslb.com") || // Bilibili.com CDN — chặn hotlink (cần Referer)
    lower.includes("bstarstatic.com") // Bilibili.tv CDN — cũng cần Referer
  );
}

async function getProxiedSrc(src: string): Promise<string> {
  const cached = dataUrlCache.get(src);
  if (cached) return cached;
  let pending = inflight.get(src);
  if (!pending) {
    pending = invoke<string>("fetch_thumbnail_data_url", { url: src })
      .then((data) => {
        dataUrlCache.set(src, data);
        return data;
      })
      .finally(() => inflight.delete(src));
    inflight.set(src, pending);
  }
  return pending;
}

/**
 * 16:9 thumbnail with graceful fallback.
 *
 * - Khi `src` thuộc CDN có Referer check (Instagram/Facebook/Douyin) → proxy
 *   qua backend để nhận về data URL embed thẳng vào `<img>`.
 * - Site khác: dùng URL trực tiếp.
 * - Lỗi → gradient placeholder + glyph platform.
 */
export function Thumbnail({ src, extractor, alt }: Props) {
  const [errored, setErrored] = useState(false);
  const [resolvedSrc, setResolvedSrc] = useState<string | null>(src ?? null);
  const boxRef = useRef<HTMLDivElement>(null);
  /** Đã lọt vào (gần) tầm nhìn chưa — CHỈ khi rồi mới đi tải ảnh qua backend. */
  const [inView, setInView] = useState(false);

  // ── CHỈ TẢI ẢNH CỦA DÒNG ĐANG NHÌN THẤY ───────────────────────────────
  // Ảnh Douyin/Instagram/Bilibili bị CDN chặn hotlink nên phải nhờ backend
  // tải hộ (1 lượt IPC + 1 request cho MỖI dòng). Không chặn theo tầm nhìn
  // thì mở kênh 250 bài là bắn 250 request một lúc — đo được đúng 250, xem
  // `ChannelInput.perf.test.tsx`. Kênh to 3600 bài thì 3600.
  //
  // Hai cái hỏng vì chuyện này: (1) giật + tốn RAM, vì mỗi ảnh về dạng data
  // URL base64 phình ~33%; (2) Douyin CHẶN THEO TẦN SUẤT — nã vài trăm
  // request cùng lúc vào CDN của họ là tự chuốc 403.
  //
  // `loading="lazy"` của <img> KHÔNG cứu được: lúc đó ảnh đã tải xong rồi.
  useEffect(() => {
    // WebView cũ / môi trường không có API này -> giữ nguyên nếp cũ (tải ngay)
    // để không bao giờ có chuyện ảnh không bao giờ hiện.
    if (typeof IntersectionObserver === "undefined") {
      setInView(true);
      return;
    }
    const el = boxRef.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setInView(true);
          io.disconnect(); // tải một lần là đủ, khỏi theo dõi tiếp
        }
      },
      // Tải sớm hơn tầm nhìn một quãng để cuộn tới là ảnh đã sẵn.
      { rootMargin: "400px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, []);

  useEffect(() => {
    setErrored(false);
    if (!src) {
      setResolvedSrc(null);
      return;
    }
    if (!needsProxy(src)) {
      setResolvedSrc(src);
      return;
    }
    // Ảnh phải nhờ backend tải: đợi tới lượt nhìn thấy đã.
    if (!inView) {
      setResolvedSrc(dataUrlCache.get(src) ?? null);
      return;
    }
    // Cache hit → set sync.
    const cached = dataUrlCache.get(src);
    if (cached) {
      setResolvedSrc(cached);
      return;
    }
    // Hiện placeholder ngay, fetch async.
    setResolvedSrc(null);
    let cancelled = false;
    void getProxiedSrc(src)
      .then((data) => {
        if (!cancelled) setResolvedSrc(data);
      })
      .catch(() => {
        if (!cancelled) setErrored(true);
      });
    return () => {
      cancelled = true;
    };
  }, [src, inView]);

  const showImg = !!resolvedSrc && !errored;
  const p = platformInfo(extractor);

  return (
    <div ref={boxRef} className="relative w-full h-full rounded-md overflow-hidden bg-surface-2 shrink-0">
      {showImg && (
        <img
          src={resolvedSrc!}
          alt={alt ?? ""}
          loading="lazy"
          className="absolute inset-0 w-full h-full object-cover"
          onError={() => setErrored(true)}
        />
      )}
      {!showImg && (
        <div className={`absolute inset-0 bg-gradient-to-br ${p.tint} flex items-center justify-center`}>
          <span className="text-3xl text-white/80 font-bold drop-shadow">{p.glyph}</span>
        </div>
      )}
    </div>
  );
}
