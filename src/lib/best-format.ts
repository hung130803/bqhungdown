import type { QualityFormat } from "@/types/models";

export function selectBest(formats: QualityFormat[]): QualityFormat | null {
  const candidates = formats.filter(f => !f.isAudioOnly);
  if (candidates.length === 0) return null;
  return [...candidates].sort((a, b) => {
    const ha = a.height ?? 0, hb = b.height ?? 0;
    if (hb !== ha) return hb - ha;
    const fa = a.fps ?? 0, fb = b.fps ?? 0;
    if (fb !== fa) return fb - fa;
    const ta = a.tbr ?? 0, tb = b.tbr ?? 0;
    return tb - ta;
  })[0];
}

export function selectBestAudio(formats: QualityFormat[]): QualityFormat | null {
  const candidates = formats.filter(f => f.isAudioOnly);
  if (candidates.length === 0) return null;
  return [...candidates].sort(
    (a, b) => (b.abr ?? 0) - (a.abr ?? 0) || a.formatId.localeCompare(b.formatId)
  )[0];
}
