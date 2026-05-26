import { useEffect, useState } from "react";
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
    lower.includes("douyinpic.com")
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
  }, [src]);

  const showImg = !!resolvedSrc && !errored;
  const p = platformInfo(extractor);

  return (
    <div className="relative w-full h-full rounded-md overflow-hidden bg-surface-2 shrink-0">
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
