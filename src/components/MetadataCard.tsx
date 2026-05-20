import { useTranslation } from "react-i18next";
import { useUrlStore } from "@/stores/useUrlStore";
import { formatDuration } from "@/lib/format";

export function MetadataCard() {
  const { t } = useTranslation();
  const md = useUrlStore(s => s.metadata);
  const fetching = useUrlStore(s => s.fetching);
  const error = useUrlStore(s => s.error);

  if (fetching) return <div className="p-4 rounded-md bg-surface border border-border text-muted">{t("home.fetching")}</div>;
  if (error) return <div className="p-4 rounded-md bg-surface border border-danger text-danger text-sm">{error}</div>;
  if (!md) return null;

  return (
    <div className="flex gap-4 p-4 rounded-md bg-surface border border-border">
      {md.thumbnail && <img src={md.thumbnail} alt="" className="w-40 h-24 object-cover rounded-md bg-surface-2" />}
      <div className="flex-1 min-w-0">
        <h3 className="font-medium text-fg truncate">{md.title}</h3>
        {md.channel && <p className="text-sm text-muted truncate">{md.channel}</p>}
        <p className="text-sm text-muted mt-1">
          {formatDuration(md.durationSec)}
          {md.playlistTotal != null && ` · ${md.playlistTotal} mục`}
        </p>
      </div>
    </div>
  );
}
