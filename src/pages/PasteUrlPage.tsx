import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useSearchParams } from "react-router-dom";
import { UrlInput } from "@/components/UrlInput";
import { ClipboardBanner } from "@/components/ClipboardBanner";
import { MetadataCard } from "@/components/MetadataCard";
import { ModeToggle } from "@/components/ModeToggle";
import { QualityPicker } from "@/components/QualityPicker";
import { FolderPicker } from "@/components/FolderPicker";
import { BatchInput } from "@/components/BatchInput";
import { ChannelInput } from "@/components/ChannelInput";
import { PlaylistEntryList } from "@/components/PlaylistEntryList";
import { useUrlStore } from "@/stores/useUrlStore";
import { useSettingsStore } from "@/stores/useSettingsStore";
import { useChannelStore } from "@/stores/useChannelStore";
import * as cmd from "@/ipc/commands";

export function PasteUrlPage() {
  const { t } = useTranslation();
  const [params, setParams] = useSearchParams();
  const navigate = useNavigate();
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
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const startSingle = async () => {
    if (submitting || !url || !valid || !folder) return;
    setSubmitting(true);
    setErrorMsg(null);
    try {
      const item = await cmd.enqueueDownload({
        url,
        options: { mode, formatId, saveFolder: folder, subLangs, autoTranslateTo, onConflict, playlistAll: null },
        title: metadata?.title,
        thumbnail: metadata?.thumbnail ?? null,
        extractor: metadata?.extractor,
        channel: metadata?.channel ?? null,
      });
      reset();
      // Jump to the queue page so the user sees the new download immediately,
      // matching the batch-add flow.
      navigate(`/queue?focus=${encodeURIComponent(item.shortId)}`);
    } catch (e) {
      // Surface the backend error so the user knows why nothing happened.
      // Common causes: folder not writable, format conflict, duplicate URL.
      const msg = (e as { message?: string })?.message ?? String(e);
      console.error("[enqueueDownload] failed:", e);
      setErrorMsg(msg || "Không thêm được vào hàng đợi. Kiểm tra thư mục lưu hoặc URL.");
    } finally {
      setSubmitting(false);
    }
  };

  const startBatch = async (urls: string[]) => {
    if (!folder) return;
    // Big pasted batches get "polite mode" too (random 2-5s sleep between
    // requests) so tải số lượng lớn không bị YouTube/TikTok chặn IP (429).
    const polite = urls.length > 15;
    await cmd.enqueueBatch({
      urls,
      options: { mode, formatId: null, saveFolder: folder, subLangs: [], autoTranslateTo: null, onConflict, playlistAll: null, polite },
    });
  };

  /** Channel batch — polite mode on, and (by default) each channel's videos go
   *  into their own subfolder `<folder>/<channel name>` so downloading many
   *  channels stays organised. */
  const startChannelBatch = async (urls: string[], channelName?: string) => {
    if (!folder) return;
    const useSub = settings?.channelSubfolder !== false && channelName;
    const target = useSub
      ? `${folder.replace(/[\\/]+$/, "")}/${sanitizeFolderName(channelName!)}`
      : folder;
    await cmd.enqueueBatch({
      urls,
      options: {
        mode,
        formatId: null,
        saveFolder: target,
        subLangs: [],
        autoTranslateTo: null,
        onConflict,
        playlistAll: null,
        polite: true,
      },
    });
    // Jump to the queue page so the user can watch the batch start working.
    navigate("/queue");
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
          {errorMsg && (
            <div className="px-3 py-2 rounded-md bg-danger/10 border border-danger text-danger text-sm">
              {errorMsg}
            </div>
          )}
        </>
      )}

      {!metadata && (
        <BatchOrChannel
          onBatch={startBatch}
          onChannel={startChannelBatch}
        />
      )}
    </div>
  );
}

/** Two-tab block shown when there's no single-URL metadata to display:
 *   - "Hàng loạt": paste many URLs
 *   - "Kênh": paste a channel URL, fetch videos, filter, queue */
/** Make a channel title safe to use as a Windows folder name. */
function sanitizeFolderName(name: string): string {
  return name
    .replace(/[\\/:*?"<>|]/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 80) || "channel";
}

function BatchOrChannel({
  onBatch,
  onChannel,
}: {
  onBatch: (urls: string[]) => Promise<void> | void;
  onChannel: (urls: string[], channelName?: string) => Promise<void> | void;
}) {
  // Sub-tab được lưu trong store nên user chuyển sang trang khác xong quay
  // lại sẽ vẫn ở đúng tab "Kênh" nếu họ đang dở việc lấy danh sách.
  const tab = useChannelStore((s) => s.subTab);
  const setTab = useChannelStore((s) => s.setSubTab);
  return (
    <div className="space-y-3 pt-2 border-t border-border">
      {/* Folder picker right here so batch/channel downloads don't force a
          trip to Settings to change where files are saved. */}
      <FolderPicker />
      <div className="inline-flex rounded-md border border-border overflow-hidden">
        <button
          onClick={() => setTab("batch")}
          className={`px-3 py-1.5 text-sm ${tab === "batch" ? "bg-accent text-accent-fg" : "hover:bg-surface-2"}`}
        >
          Hàng loạt
        </button>
        <button
          onClick={() => setTab("channel")}
          className={`px-3 py-1.5 text-sm ${tab === "channel" ? "bg-accent text-accent-fg" : "hover:bg-surface-2"}`}
        >
          Kênh
        </button>
      </div>
      {tab === "batch" ? (
        <BatchInput onSubmit={onBatch} />
      ) : (
        <ChannelInput onSubmit={onChannel} />
      )}
    </div>
  );
}
