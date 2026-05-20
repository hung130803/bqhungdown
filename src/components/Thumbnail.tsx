import { useState } from "react";
import { platformInfo } from "@/lib/platforms";

interface Props {
  src: string | null | undefined;
  extractor: string;
  /** ARIA label / tooltip (thường là tiêu đề video). */
  alt?: string;
}

/**
 * 16:9 thumbnail with graceful fallback.
 *
 * - Khi có `src`: render `<img>` với object-cover; nếu request thất bại
 *   (CORS, link expired, 404) tự fallback sang gradient placeholder.
 * - Khi không có `src`: hiện gradient mang màu nền tảng + glyph platform.
 *
 * Width/height được quyết định bởi parent (lớp `aspect-video w-44` v.v.).
 */
export function Thumbnail({ src, extractor, alt }: Props) {
  const [errored, setErrored] = useState(false);
  const showImg = !!src && !errored;
  const p = platformInfo(extractor);

  return (
    <div className="relative w-full h-full rounded-md overflow-hidden bg-surface-2 shrink-0">
      {showImg && (
        <img
          src={src!}
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
