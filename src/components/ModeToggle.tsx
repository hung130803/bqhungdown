import { useTranslation } from "react-i18next";
import { useUrlStore } from "@/stores/useUrlStore";

export function ModeToggle() {
  const { t } = useTranslation();
  const mode = useUrlStore(s => s.mode);
  const setMode = useUrlStore(s => s.setMode);
  return (
    <div className="inline-flex rounded-md border border-border overflow-hidden">
      {(["video", "audio"] as const).map(m => (
        <button
          key={m}
          onClick={() => setMode(m)}
          className={`px-3 py-1.5 text-sm ${mode === m ? "bg-accent text-accent-fg" : "bg-surface text-fg hover:bg-surface-2"}`}
        >
          {m === "video" ? t("home.modeVideo") : t("home.modeAudio")}
        </button>
      ))}
    </div>
  );
}
