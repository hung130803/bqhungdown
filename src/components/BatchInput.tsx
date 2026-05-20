import { useState } from "react";
import { useTranslation } from "react-i18next";
import { parseBatch } from "@/lib/parse-batch";

export function BatchInput({ onSubmit }: { onSubmit: (urls: string[]) => Promise<void> | void }) {
  const { t } = useTranslation();
  const [text, setText] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const valid = parseBatch(text);

  const handleClick = async () => {
    if (submitting || valid.length === 0) return;
    setSubmitting(true);
    try {
      await Promise.resolve(onSubmit(valid));
      setText("");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="space-y-2">
      <label className="text-sm font-medium text-fg">Tải hàng loạt</label>
      <textarea
        rows={4}
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder={"https://...\nhttps://..."}
        disabled={submitting}
        className="w-full px-3 py-2 rounded-md bg-surface border border-border text-fg placeholder:text-muted text-sm font-mono disabled:opacity-50"
      />
      <div className="flex items-center justify-between">
        <span className="text-xs text-muted">
          {t("home.batchHint")} ({valid.length} hợp lệ)
        </span>
        <button
          disabled={valid.length === 0 || submitting}
          onClick={handleClick}
          className="px-3 py-1.5 rounded-md bg-accent text-accent-fg text-sm disabled:opacity-50"
        >
          {submitting ? "Đang thêm…" : "Thêm hàng loạt"}
        </button>
      </div>
    </div>
  );
}
