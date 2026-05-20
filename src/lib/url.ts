export function isValidUrl(s: string): boolean {
  try {
    const u = new URL(s.trim());
    return (u.protocol === "http:" || u.protocol === "https:") && !!u.host;
  } catch { return false; }
}

export function extractHost(s: string): string | null {
  try { return new URL(s.trim()).host.toLowerCase(); } catch { return null; }
}

const FEATURED_REGEX: Array<{ name: string; re: RegExp }> = [
  { name: "youtube",   re: /^(?:www\.|m\.|music\.)?youtube\.com$|^youtu\.be$/ },
  { name: "tiktok",    re: /^(?:www\.|m\.|vm\.|vt\.)?tiktok\.com$/ },
  { name: "facebook",  re: /^(?:www\.|m\.|web\.|business\.|fb\.)?facebook\.com$|^fb\.watch$/ },
  { name: "instagram", re: /^(?:www\.)?instagram\.com$/ },
  { name: "twitter",   re: /^(?:www\.|mobile\.)?(?:twitter|x)\.com$|^t\.co$/ },
  { name: "twitch",    re: /^(?:www\.|m\.|clips\.)?twitch\.tv$/ },
];

export function quickPlatformGuess(url: string): string | null {
  const host = extractHost(url); if (!host) return null;
  for (const { name, re } of FEATURED_REGEX) if (re.test(host)) return name;
  return null;
}
