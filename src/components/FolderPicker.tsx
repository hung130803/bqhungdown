import { useTranslation } from "react-i18next";
import { useUrlStore } from "@/stores/useUrlStore";
import { useSettingsStore } from "@/stores/useSettingsStore";
import * as cmd from "@/ipc/commands";

export function FolderPicker() {
  const { t } = useTranslation();
  const settings = useSettingsStore(s => s.settings);
  const updateSettings = useSettingsStore(s => s.update);
  const saveFolder = useUrlStore(s => s.saveFolder);
  const setSaveFolder = useUrlStore(s => s.setSaveFolder);
  const current = saveFolder || settings?.defaultFolder || "";

  const choose = async () => {
    const f = await cmd.pickFolder();
    if (f) {
      setSaveFolder(f);
      void updateSettings({ defaultFolder: f });
    }
  };

  return (
    <div className="space-y-2">
      <label className="text-sm font-medium text-fg">{t("home.saveFolderLabel")}</label>
      <div className="flex gap-2">
        <input
          readOnly
          value={current}
          className="flex-1 px-3 py-2 rounded-md bg-surface border border-border text-fg"
        />
        <button onClick={choose} className="px-3 py-2 rounded-md bg-surface-2 border border-border text-fg hover:bg-surface">
          {t("home.chooseFolder")}
        </button>
      </div>
    </div>
  );
}
