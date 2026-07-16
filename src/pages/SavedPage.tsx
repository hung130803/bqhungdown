import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import * as cmd from "@/ipc/commands";
import { useChannelStore } from "@/stores/useChannelStore";
import type { Bookmark } from "@/types/models";
import { EmptyState } from "@/components/EmptyState";

/**
 * "Đã lưu" — a simple bookmark list. Save channels/links to come back to later
 * (download or watch). Each item has an editable note + actions.
 */
export function SavedPage() {
  const navigate = useNavigate();
  const setChannelUrl = useChannelStore((s) => s.setUrl);
  const setSubTab = useChannelStore((s) => s.setSubTab);

  const [items, setItems] = useState<Bookmark[]>([]);
  const [url, setUrl] = useState("");
  const [note, setNote] = useState("");
  const [error, setError] = useState<string | null>(null);

  const reload = async () => {
    try {
      setItems(await cmd.listBookmarks());
    } catch (e) {
      setError(formatErr(e));
    }
  };

  useEffect(() => { void reload(); }, []);

  const add = async () => {
    const u = url.trim();
    if (!u) return;
    setError(null);
    try {
      await cmd.addBookmark(u, note.trim());
      setUrl("");
      setNote("");
      await reload();
    } catch (e) {
      setError(formatErr(e));
    }
  };

  const remove = async (id: string) => {
    try { await cmd.removeBookmark(id); await reload(); }
    catch (e) { setError(formatErr(e)); }
  };

  const saveNote = async (id: string, value: string) => {
    try { await cmd.updateBookmarkNote(id, value); }
    catch (e) { setError(formatErr(e)); }
  };

  /** Load a saved link into the channel tab so the user can fetch + download. */
  const loadForDownload = (u: string) => {
    setChannelUrl(u);
    setSubTab("channel");
    navigate("/");
  };

  return (
    <div className="max-w-2xl mx-auto space-y-5">
      <div>
        <h2 className="text-xl font-medium text-fg">Đã lưu</h2>
        <p className="text-sm text-muted mt-1">
          Lưu các kênh/link để dành — sau muốn tải hay xem thì lấy ra. Thêm ghi chú cho dễ nhớ.
        </p>
      </div>

      {/* Add */}
      <div className="space-y-2 p-3 rounded-lg border border-border bg-surface">
        <input
          type="url"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="Dán link kênh / video cần lưu"
          className="w-full px-3 py-2 rounded-md bg-surface-2 border border-border text-fg placeholder:text-muted text-sm"
        />
        <div className="flex gap-2">
          <input
            type="text"
            value={note}
            onChange={(e) => setNote(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") void add(); }}
            placeholder="Ghi chú (vd: kênh hài, để tải sau...)"
            className="flex-1 px-3 py-2 rounded-md bg-surface-2 border border-border text-fg placeholder:text-muted text-sm"
          />
          <button
            onClick={() => void add()}
            disabled={!url.trim()}
            className="px-4 py-2 rounded-md bg-accent text-accent-fg font-medium text-sm disabled:opacity-50"
          >
            Lưu
          </button>
        </div>
      </div>

      {error && (
        <div className="px-3 py-2 rounded-md bg-danger/10 border border-danger text-danger text-sm">{error}</div>
      )}

      {/* List */}
      <div className="space-y-2">
        {items.length === 0 && (
          <EmptyState
            icon="🔖"
            title="Chưa lưu mục nào"
            hint="Lưu video/kênh yêu thích để tải lại nhanh sau này — chúng sẽ hiện ở đây."
          />
        )}
        {items.map((b) => (
          <div key={b.id} className="p-3 rounded-lg border border-border bg-surface space-y-2">
            <div className="flex items-start gap-2">
              <div className="flex-1 min-w-0">
                <input
                  type="text"
                  defaultValue={b.note}
                  onBlur={(e) => void saveNote(b.id, e.target.value)}
                  placeholder="(ghi chú)"
                  className="w-full bg-transparent text-sm font-medium text-fg outline-none border-b border-transparent focus:border-border"
                />
                <a
                  href={b.url}
                  onClick={(e) => { e.preventDefault(); void cmd.openUrl(b.url); }}
                  className="text-xs text-muted truncate block hover:text-accent"
                  title={b.url}
                >
                  {b.url}
                </a>
              </div>
              <button
                onClick={() => void remove(b.id)}
                className="px-2 py-1 rounded-md border border-border text-fg shrink-0 hover:bg-surface-2"
                title="Xóa khỏi danh sách lưu"
              >
                ✕
              </button>
            </div>
            <div className="flex gap-2">
              <button
                onClick={() => loadForDownload(b.url)}
                className="px-3 py-1.5 rounded-md bg-accent text-accent-fg text-xs font-medium"
              >
                Tải kênh
              </button>
              <button
                onClick={() => void cmd.openUrl(b.url)}
                className="px-3 py-1.5 rounded-md bg-surface-2 border border-border text-fg text-xs"
              >
                Mở web
              </button>
              <button
                onClick={() => { void navigator.clipboard?.writeText(b.url); }}
                className="px-3 py-1.5 rounded-md bg-surface-2 border border-border text-fg text-xs"
              >
                Sao chép
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function formatErr(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object") {
    const o = e as Record<string, unknown>;
    if (typeof o.message === "string") return o.message;
    if (typeof o.data === "string") return o.data;
    if (typeof o.kind === "string") return o.kind;
  }
  return String(e);
}
