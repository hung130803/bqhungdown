import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useHistoryStore } from "@/stores/useHistoryStore";
import { HistoryRow } from "@/components/HistoryRow";
import { ConfirmDialog } from "@/components/ConfirmDialog";

export function HistoryPage() {
  const { t } = useTranslation();
  const entries = useHistoryStore((s) => s.entries);
  const query = useHistoryStore((s) => s.query);
  const setQuery = useHistoryStore((s) => s.setQuery);
  const refresh = useHistoryStore((s) => s.refresh);
  const clearAll = useHistoryStore((s) => s.clearAll);

  const [confirmOpen, setConfirmOpen] = useState(false);
  const [alsoDeleteFiles, setAlsoDeleteFiles] = useState(false);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const onClearAllClick = () => {
    if (entries.length === 0) return;
    setAlsoDeleteFiles(false);
    setConfirmOpen(true);
  };

  const onConfirmClear = async () => {
    await clearAll(alsoDeleteFiles);
    setConfirmOpen(false);
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3">
        <input
          type="search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("history.searchPlaceholder")}
          className="flex-1 px-3 py-2 rounded-md bg-surface border border-border text-fg placeholder:text-muted"
        />
        {entries.length > 0 && (
          <button
            onClick={onClearAllClick}
            className="px-3 py-2 rounded-md border border-danger text-danger text-sm hover:bg-danger/10 shrink-0"
          >
            Xoá tất cả ({entries.length})
          </button>
        )}
      </div>
      {entries.length === 0 ? (
        <p className="text-muted text-center py-12">{t("history.empty")}</p>
      ) : (
        <div className="space-y-2">
          {entries.map((e) => (
            <HistoryRow key={e.shortId} entry={e} />
          ))}
        </div>
      )}

      <ConfirmDialog
        open={confirmOpen}
        title={`Xoá tất cả ${entries.length} mục lịch sử?`}
        message={
          <span>
            Hành động này sẽ xoá toàn bộ danh sách lịch sử trong app.
            <br />
            Tích vào ô bên dưới nếu bạn muốn xoá luôn các <strong>file video trên đĩa</strong>.
          </span>
        }
        confirmText="Xoá"
        cancelText="Huỷ"
        variant="danger"
        extraToggle={{
          label: "Xoá luôn các file video trên đĩa",
          value: alsoDeleteFiles,
          onChange: setAlsoDeleteFiles,
        }}
        onConfirm={() => void onConfirmClear()}
        onCancel={() => setConfirmOpen(false)}
      />
    </div>
  );
}
