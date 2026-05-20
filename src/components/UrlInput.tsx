import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useUrlStore } from "@/stores/useUrlStore";
import { isValidUrl, quickPlatformGuess } from "@/lib/url";
import { PlatformBadge } from "./PlatformBadge";

export function UrlInput() {
  const { t } = useTranslation();
  const url = useUrlStore(s => s.url);
  const setUrl = useUrlStore(s => s.setUrl);
  const valid = useUrlStore(s => s.valid);
  const validate = useUrlStore(s => s.validate);
  const extractor = useUrlStore(s => s.extractor);
  const [touched, setTouched] = useState(false);

  useEffect(() => {
    setTouched(url.length > 0);
    if (!url) return;
    const id = setTimeout(() => { void validate(); }, 100);
    return () => clearTimeout(id);
  }, [url, validate]);

  const guess = quickPlatformGuess(url);
  const localValid = url.length === 0 ? null : isValidUrl(url);
  const showInvalid = touched && (localValid === false || valid === false);

  return (
    <div className="space-y-2">
      <label htmlFor="url" className="text-sm font-medium text-fg">
        {t("home.pasteLabel")}
      </label>
      <div className="flex gap-2">
        <input
          id="url"
          type="url"
          spellCheck={false}
          autoComplete="off"
          value={url}
          onChange={e => setUrl(e.target.value)}
          placeholder={t("home.placeholder")}
          className={`flex-1 px-3 py-2 rounded-md bg-surface border ${showInvalid ? "border-danger" : "border-border"} text-fg placeholder:text-muted`}
        />
        {(extractor ?? guess) && <PlatformBadge extractor={extractor ?? guess!} />}
      </div>
      {showInvalid && <p className="text-sm text-danger">{t("home.invalidUrl")}</p>}
    </div>
  );
}
