import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useClipboardStore } from "@/stores/useClipboardStore";
import { useUrlStore } from "@/stores/useUrlStore";
import { onClipboardDetected } from "@/ipc/events";
import { PlatformBadge } from "./PlatformBadge";

export function ClipboardBanner() {
  const { t } = useTranslation();
  const detectedUrl = useClipboardStore(s => s.detectedUrl);
  const detectedExtractor = useClipboardStore(s => s.detectedExtractor);
  const setDetected = useClipboardStore(s => s.setDetected);
  const dismiss = useClipboardStore(s => s.dismiss);
  const isDismissed = useClipboardStore(s => s.isDismissed);
  const setUrl = useUrlStore(s => s.setUrl);
  const fetchMetadata = useUrlStore(s => s.fetchMetadata);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    onClipboardDetected(p => {
      if (!isDismissed(p.url)) setDetected(p.url, p.extractor);
    }).then(fn => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, [setDetected, isDismissed]);

  if (!detectedUrl) return null;

  return (
    <div className="flex items-center gap-3 p-3 rounded-md bg-surface border border-border">
      <span className="text-sm text-fg flex-1 truncate">
        <span className="text-muted mr-2">{t("clipboard.bannerTitle")}:</span>
        <span className="truncate">{detectedUrl}</span>
      </span>
      {detectedExtractor && <PlatformBadge extractor={detectedExtractor} />}
      <button
        onClick={() => {
          setUrl(detectedUrl);
          void fetchMetadata();
          setDetected(null, null);
        }}
        className="px-3 py-1.5 rounded-md bg-accent text-accent-fg text-sm font-medium"
      >
        {t("clipboard.bannerAction")}
      </button>
      <button
        onClick={() => dismiss(detectedUrl)}
        className="px-2 py-1 text-sm text-muted hover:text-fg"
      >
        {t("clipboard.dismiss")}
      </button>
    </div>
  );
}
