import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";
import { UrlInput } from "@/components/UrlInput";
import { ClipboardBanner } from "@/components/ClipboardBanner";
import { MetadataCard } from "@/components/MetadataCard";
import { ModeToggle } from "@/components/ModeToggle";
import { QualityPicker } from "@/components/QualityPicker";
import { FolderPicker } from "@/components/FolderPicker";
import { BatchInput } from "@/components/BatchInput";
import { PlaylistEntryList } from "@/components/PlaylistEntryList";
import { useUrlStore } from "@/stores/useUrlStore";
import { useSettingsStore } from "@/stores/useSettingsStore";
import * as cmd from "@/ipc/commands";

export function PasteUrlPage() {
  const { t } = useTranslation();
  const [params, setParams] = useSearchParams();
  const url = useUrlStore(s => s.url);
  const setUrl = useUrlStore(s => s.setUrl);
  const valid = useUrlStore(s => s.valid);
  const metadata = useUrlStore(s => s.metadata);
  const fetchMetadata = useUrlStore(s => s.fetchMetadata);
  const fetching = useUrlStore(s => s.fetching);
  const mode = useUrlStore(s => s.mode);
  const formatId = useUrlStore(s => s.formatId);
  const subLangs = useUrlStore(s => s.subLangs);
  const autoTranslateTo = useUrlStore(s => s.autoTranslateTo);
  const onConflict = useUrlStore(s => s.onConflict);
  const saveFolder = useUrlStore(s => s.saveFolder);
  const reset = useUrlStore(s => s.reset);
  const settings = useSettingsStore(s => s.settings);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    const incoming = params.get("url");
    if (incoming && incoming !== url) {
      setUrl(incoming);
      void fetchMetadata();
      const next = new URLSearchParams(params); next.delete("url");
      setParams(next, { replace: true });
    }
  }, [params, url, setUrl, fetchMetadata, setParams]);

  const folder = saveFolder || settings?.defaultFolder || "";

  const startSingle = async () => {
    if (submitting || !url || !valid || !folder) return;
    setSubmitting(true);
    try {
      await cmd.enqueueDownload({
        url,
        options: { mode, formatId, saveFolder: folder, subLangs, autoTranslateTo, onConflict, playlistAll: null },
        title: metadata?.title,
        thumbnail: metadata?.thumbnail ?? null,
        extractor: metadata?.extractor,
        channel: metadata?.channel ?? null,
      });
      reset();
    } finally {
      setSubmitting(false);
    }
  };

  const startBatch = async (urls: string[]) => {
    if (!folder) return;
    await cmd.enqueueBatch({
      urls,
      options: { mode, formatId: null, saveFolder: folder, subLangs: [], autoTranslateTo: null, onConflict, playlistAll: null },
    });
  };

  const startPlaylist = async (urls: string[], all: boolean) => {
    if (submitting || !metadata || !folder) return;
    setSubmitting(true);
    try {
      await cmd.enqueuePlaylist({
        playlistUrl: metadata.url,
        selected: urls,
        options: { mode, formatId, saveFolder: folder, subLangs, autoTranslateTo, onConflict, playlistAll: all ? true : null },
        allWithYesPlaylist: all,
      });
      reset();
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="space-y-4 max-w-3xl mx-auto">
      <ClipboardBanner />
      <UrlInput />
      <div className="flex gap-2">
        <button
          disabled={!valid || fetching}
          onClick={() => void fetchMetadata()}
          className="px-3 py-2 rounded-md bg-surface-2 border border-border text-fg disabled:opacity-50"
        >
          {fetching ? t("home.fetching") : t("home.fetchMetadata")}
        </button>
      </div>

      <MetadataCard />

      {metadata && (
        <>
          <PlaylistEntryList onConfirm={startPlaylist} />
          <ModeToggle />
          <QualityPicker />
          <FolderPicker />
          <button
            onClick={() => void startSingle()}
            disabled={!folder || submitting}
            className="w-full py-2.5 rounded-md bg-accent text-accent-fg font-medium disabled:opacity-50"
          >
            {submitting ? "Đang thêm…" : t("home.downloadButton")}
          </button>
        </>
      )}

      {!metadata && <BatchInput onSubmit={startBatch} />}
    </div>
  );
}
