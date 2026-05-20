import { useState } from "react";

/**
 * Title hiển thị dạng text bình thường (chọn được, copy bằng tay được) cộng
 * với 1 nút copy nhỏ bên phải để copy nhanh toàn bộ chuỗi. Không phải link →
 * không bị accidental click.
 */
export function TitleWithCopy({ title }: { title: string }) {
  const [copied, setCopied] = useState(false);

  const onCopy = async () => {
    try {
      await navigator.clipboard.writeText(title);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* clipboard có thể fail trong dev mode — bỏ qua */
    }
  };

  return (
    <div className="flex items-start gap-2">
      <span
        className="font-semibold text-fg leading-snug line-clamp-2 flex-1 select-text"
        title={title}
      >
        {title}
      </span>
      <button
        onClick={() => void onCopy()}
        title="Sao chép tiêu đề"
        aria-label="Sao chép tiêu đề"
        className="shrink-0 mt-0.5 px-1.5 py-0.5 rounded text-[11px] text-muted hover:text-fg hover:bg-surface-2 transition-colors"
      >
        {copied ? "✓ Đã sao chép" : "📋 Sao chép"}
      </button>
    </div>
  );
}
