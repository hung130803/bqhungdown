import { useState } from "react";
import { useUrlStore } from "@/stores/useUrlStore";

export function PlaylistEntryList({ onConfirm }: { onConfirm: (urls: string[], all: boolean) => void }) {
  const md = useUrlStore(s => s.metadata);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  if (!md?.playlistEntries || md.playlistEntries.length === 0) return null;

  const toggle = (url: string) => {
    setSelected(prev => {
      const next = new Set(prev);
      if (next.has(url)) next.delete(url); else next.add(url);
      return next;
    });
  };
  const toggleAll = () => {
    if (selected.size === md.playlistEntries!.length) setSelected(new Set());
    else setSelected(new Set(md.playlistEntries!.map(e => e.url)));
  };

  return (
    <div className="space-y-2 p-3 rounded-md bg-surface border border-border">
      <div className="flex items-center justify-between">
        <span className="text-sm font-medium text-fg">Playlist ({md.playlistEntries.length} mục)</span>
        <button onClick={toggleAll} className="text-xs text-accent">Chọn tất cả</button>
      </div>
      <div className="max-h-48 overflow-y-auto space-y-1">
        {md.playlistEntries.map(e => (
          <label key={e.url} className="flex items-center gap-2 text-sm">
            <input type="checkbox" checked={selected.has(e.url)} onChange={() => toggle(e.url)} />
            <span className="truncate">{e.title}</span>
          </label>
        ))}
      </div>
      <div className="flex justify-end gap-2">
        <button onClick={() => onConfirm(md.playlistEntries!.map(e => e.url), true)} className="px-3 py-1 rounded-md bg-surface-2 border border-border text-sm">Tải tất cả</button>
        <button disabled={selected.size === 0} onClick={() => onConfirm(Array.from(selected), false)} className="px-3 py-1 rounded-md bg-accent text-accent-fg text-sm disabled:opacity-50">Tải đã chọn</button>
      </div>
    </div>
  );
}
