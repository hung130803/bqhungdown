import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { onDownloadConflict, type ConflictEventPayload } from "@/ipc/events";
import * as cmd from "@/ipc/commands";

export function ConflictDialog() {
  const { t } = useTranslation();
  const [payload, setPayload] = useState<ConflictEventPayload | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    onDownloadConflict(p => setPayload(p)).then(fn => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, []);

  if (!payload) return null;
  const name = payload.conflictingPath.split(/[\\/]/).pop() ?? "";
  const suggested = payload.suggestedPath.split(/[\\/]/).pop() ?? "";

  const choose = async (choice: "overwrite" | "skip" | "autorename") => {
    await cmd.resolveConflict(payload.shortId, choice);
    setPayload(null);
  };

  return (
    <div className="fixed inset-0 bg-black/60 z-50 flex items-center justify-center p-4">
      <div className="bg-bg border border-border rounded-lg p-5 w-full max-w-md space-y-4">
        <h3 className="text-lg font-medium text-fg">{t("conflict.title")}</h3>
        <p className="text-sm text-muted">{t("conflict.message", { name })}</p>
        <div className="flex flex-col gap-2">
          <button onClick={() => choose("overwrite")} className="px-3 py-2 rounded-md bg-danger text-white text-sm">{t("conflict.overwrite")}</button>
          <button onClick={() => choose("skip")} className="px-3 py-2 rounded-md bg-surface-2 border border-border text-sm">{t("conflict.skip")}</button>
          <button onClick={() => choose("autorename")} className="px-3 py-2 rounded-md bg-accent text-accent-fg text-sm">{t("conflict.autoRename", { suggested })}</button>
        </div>
      </div>
    </div>
  );
}
