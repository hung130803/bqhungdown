import { useTranslation } from "react-i18next";
import { useUrlStore } from "@/stores/useUrlStore";

export function SubtitlePicker() {
  const { t } = useTranslation();
  const md = useUrlStore(s => s.metadata);
  const subLangs = useUrlStore(s => s.subLangs);
  const setSubLangs = useUrlStore(s => s.setSubLangs);
  const autoTranslateTo = useUrlStore(s => s.autoTranslateTo);
  const setAutoTranslateTo = useUrlStore(s => s.setAutoTranslateTo);
  if (!md || md.subtitles.length === 0) return null;

  const toggle = (code: string) => {
    if (subLangs.includes(code)) setSubLangs(subLangs.filter(c => c !== code));
    else setSubLangs([...subLangs, code]);
  };

  return (
    <div className="space-y-2">
      <label className="text-sm font-medium text-fg">{t("home.subtitlesLabel")}</label>
      <div className="flex flex-wrap gap-2">
        {md.subtitles.map(s => (
          <button
            key={s.langCode}
            onClick={() => toggle(s.langCode)}
            className={`px-2 py-1 rounded-md text-xs border ${subLangs.includes(s.langCode) ? "bg-accent text-accent-fg border-accent" : "bg-surface text-fg border-border"}`}
          >
            {s.langCode}{s.isAuto ? " (auto)" : ""}
          </button>
        ))}
      </div>
      <div className="flex items-center gap-2">
        <span className="text-sm text-muted">{t("home.autoTranslateLabel")}</span>
        <input
          type="text"
          placeholder="vi, en…"
          value={autoTranslateTo ?? ""}
          onChange={e => setAutoTranslateTo(e.target.value || null)}
          className="px-2 py-1 rounded-md bg-surface border border-border text-fg text-sm w-24"
        />
      </div>
    </div>
  );
}
