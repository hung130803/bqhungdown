import { platformInfo } from "@/lib/platforms";

export function PlatformBadge({ extractor }: { extractor: string }) {
  const p = platformInfo(extractor);
  return (
    <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-surface-2 border border-border text-sm">
      <span aria-hidden className="font-bold">{p.glyph}</span>
      <span>{p.label}</span>
    </span>
  );
}
