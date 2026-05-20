/**
 * Single source of truth for platform display metadata.
 *
 * Mọi component (PlatformBadge, Thumbnail, HistoryRow, QueueRow…) đều phải
 * dùng helper `platformInfo()` ở đây — KHÔNG inline label/icon/color rời rạc
 * trong từng file. Khi thêm nền tảng mới: thêm 1 entry vào PLATFORMS, đồng bộ
 * với `src-tauri/src/extractors.rs` (cùng `name`).
 */

export interface PlatformInfo {
  /** ID khớp với extractor name bên Rust. */
  name: string;
  /** Tên hiển thị thân thiện. */
  label: string;
  /** Glyph 1-2 ký tự dùng làm icon dạng text. */
  glyph: string;
  /**
   * Tailwind gradient classes cho fallback thumbnail (`from-X to-Y`).
   * Dùng khi không có ảnh thumbnail thật.
   */
  tint: string;
}

export const PLATFORMS: Record<string, PlatformInfo> = {
  // ── Featured ──
  youtube:     { name: "youtube",     label: "YouTube",     glyph: "▶",  tint: "from-red-600/40 to-red-800/40" },
  tiktok:      { name: "tiktok",      label: "TikTok",      glyph: "♪",  tint: "from-pink-500/40 to-cyan-500/40" },
  facebook:    { name: "facebook",    label: "Facebook",    glyph: "f",  tint: "from-blue-600/40 to-blue-800/40" },
  instagram:   { name: "instagram",   label: "Instagram",   glyph: "◉",  tint: "from-purple-500/40 to-pink-500/40" },
  twitter:     { name: "twitter",     label: "X / Twitter", glyph: "𝕏",  tint: "from-sky-500/40 to-slate-500/40" },
  twitch:      { name: "twitch",      label: "Twitch",      glyph: "◆",  tint: "from-purple-600/40 to-purple-800/40" },
  vimeo:       { name: "vimeo",       label: "Vimeo",       glyph: "V",  tint: "from-cyan-500/40 to-blue-700/40" },
  reddit:      { name: "reddit",      label: "Reddit",      glyph: "🅡", tint: "from-orange-500/40 to-red-600/40" },
  dailymotion: { name: "dailymotion", label: "Dailymotion", glyph: "ⓓ", tint: "from-blue-500/40 to-indigo-700/40" },
  bilibili:    { name: "bilibili",    label: "Bilibili",    glyph: "B",  tint: "from-cyan-400/40 to-pink-400/40" },

  // ── Streaming / Live ──
  kick:        { name: "kick",        label: "Kick",        glyph: "K",  tint: "from-green-400/40 to-green-700/40" },
  rumble:      { name: "rumble",      label: "Rumble",      glyph: "R",  tint: "from-emerald-500/40 to-emerald-800/40" },
  odysee:      { name: "odysee",      label: "Odysee",      glyph: "O",  tint: "from-pink-400/40 to-pink-700/40" },
  bitchute:    { name: "bitchute",    label: "BitChute",    glyph: "B",  tint: "from-red-500/40 to-red-800/40" },
  dtube:       { name: "dtube",       label: "DTube",       glyph: "D",  tint: "from-red-400/40 to-red-700/40" },
  streamable:  { name: "streamable",  label: "Streamable",  glyph: "S",  tint: "from-sky-400/40 to-sky-700/40" },

  // ── Social / Short-form ──
  threads:     { name: "threads",     label: "Threads",     glyph: "@",  tint: "from-zinc-600/40 to-zinc-900/40" },
  snapchat:    { name: "snapchat",    label: "Snapchat",    glyph: "👻", tint: "from-yellow-300/40 to-yellow-500/40" },
  tumblr:      { name: "tumblr",      label: "Tumblr",      glyph: "t",  tint: "from-indigo-600/40 to-indigo-900/40" },
  linkedin:    { name: "linkedin",    label: "LinkedIn",    glyph: "in", tint: "from-blue-600/40 to-blue-900/40" },
  pinterest:   { name: "pinterest",   label: "Pinterest",   glyph: "P",  tint: "from-rose-500/40 to-rose-800/40" },
  vk:          { name: "vk",          label: "VK",          glyph: "VK", tint: "from-blue-500/40 to-blue-800/40" },
  ok_ru:       { name: "ok_ru",       label: "OK.ru",       glyph: "OK", tint: "from-orange-500/40 to-orange-800/40" },
  weibo:       { name: "weibo",       label: "Weibo",       glyph: "W",  tint: "from-red-500/40 to-orange-600/40" },
  viralhog:    { name: "viralhog",    label: "ViralHog",    glyph: "🐗", tint: "from-amber-500/40 to-orange-700/40" },
  "9gag":      { name: "9gag",        label: "9GAG",        glyph: "9",  tint: "from-yellow-400/40 to-amber-600/40" },
  imgur:       { name: "imgur",       label: "Imgur",       glyph: "i",  tint: "from-emerald-400/40 to-green-700/40" },
  gfycat:      { name: "gfycat",      label: "Gfycat",      glyph: "G",  tint: "from-purple-400/40 to-purple-700/40" },
  redgifs:     { name: "redgifs",     label: "RedGifs",     glyph: "R",  tint: "from-red-400/40 to-red-700/40" },
  coub:        { name: "coub",        label: "Coub",        glyph: "C",  tint: "from-blue-400/40 to-purple-600/40" },

  // ── Asian / Chinese ──
  douyin:      { name: "douyin",      label: "Douyin",      glyph: "抖", tint: "from-pink-500/40 to-rose-700/40" },
  kuaishou:    { name: "kuaishou",    label: "Kuaishou",    glyph: "快", tint: "from-orange-500/40 to-red-600/40" },
  iqiyi:       { name: "iqiyi",       label: "iQIYI",       glyph: "爱", tint: "from-green-400/40 to-green-700/40" },
  youku:       { name: "youku",       label: "Youku",       glyph: "优", tint: "from-blue-400/40 to-blue-700/40" },
  niconico:    { name: "niconico",    label: "Niconico",    glyph: "ニ", tint: "from-zinc-300/40 to-zinc-600/40" },
  naver:       { name: "naver",       label: "Naver TV",    glyph: "N",  tint: "from-green-500/40 to-green-800/40" },
  vlive:       { name: "vlive",       label: "V LIVE",      glyph: "V",  tint: "from-fuchsia-500/40 to-pink-700/40" },

  // ── Music / Audio ──
  soundcloud:  { name: "soundcloud",  label: "SoundCloud",  glyph: "♫",  tint: "from-orange-400/40 to-orange-700/40" },
  mixcloud:    { name: "mixcloud",    label: "Mixcloud",    glyph: "M",  tint: "from-sky-500/40 to-indigo-700/40" },
  bandcamp:    { name: "bandcamp",    label: "Bandcamp",    glyph: "B",  tint: "from-cyan-500/40 to-cyan-800/40" },
  audiomack:   { name: "audiomack",   label: "Audiomack",   glyph: "A",  tint: "from-yellow-500/40 to-orange-700/40" },

  // ── Education ──
  ted:         { name: "ted",         label: "TED",         glyph: "T",  tint: "from-red-500/40 to-red-800/40" },
  coursera:    { name: "coursera",    label: "Coursera",    glyph: "C",  tint: "from-blue-500/40 to-blue-800/40" },
  udemy:       { name: "udemy",       label: "Udemy",       glyph: "U",  tint: "from-purple-500/40 to-purple-800/40" },
  khanacademy: { name: "khanacademy", label: "Khan Academy",glyph: "K",  tint: "from-emerald-500/40 to-emerald-800/40" },

  // ── Broadcasters / News ──
  bbc:         { name: "bbc",         label: "BBC",         glyph: "B",  tint: "from-zinc-700/40 to-zinc-900/40" },
  cnn:         { name: "cnn",         label: "CNN",         glyph: "C",  tint: "from-red-600/40 to-red-900/40" },
  arte:        { name: "arte",        label: "ARTE",        glyph: "A",  tint: "from-orange-500/40 to-pink-600/40" },
  zdf:         { name: "zdf",         label: "ZDF",         glyph: "Z",  tint: "from-orange-500/40 to-orange-800/40" },
  nhk:         { name: "nhk",         label: "NHK",         glyph: "N",  tint: "from-red-500/40 to-rose-800/40" },
  nbcnews:     { name: "nbcnews",     label: "NBC News",    glyph: "N",  tint: "from-blue-600/40 to-purple-700/40" },
  espn:        { name: "espn",        label: "ESPN",        glyph: "E",  tint: "from-red-500/40 to-red-800/40" },

  // ── Subscription / Patreon-like ──
  patreon:     { name: "patreon",     label: "Patreon",     glyph: "P",  tint: "from-orange-500/40 to-red-600/40" },
  newgrounds:  { name: "newgrounds",  label: "Newgrounds",  glyph: "N",  tint: "from-amber-500/40 to-amber-800/40" },

  // ── File hosting / Cloud ──
  gdrive:      { name: "gdrive",      label: "Google Drive",glyph: "🗂", tint: "from-blue-500/40 to-emerald-600/40" },
  dropbox:     { name: "dropbox",     label: "Dropbox",     glyph: "D",  tint: "from-blue-500/40 to-blue-800/40" },

  // ── Vietnamese / SEA ──
  zingmp3:     { name: "zingmp3",     label: "Zing MP3",    glyph: "Z",  tint: "from-blue-500/40 to-indigo-700/40" },
  nhaccuatui:  { name: "nhaccuatui",  label: "NhacCuaTui",  glyph: "N",  tint: "from-orange-400/40 to-rose-600/40" },
  vidio:       { name: "vidio",       label: "Vidio",       glyph: "V",  tint: "from-sky-500/40 to-blue-700/40" },
};

const FALLBACK: PlatformInfo = {
  name: "generic",
  label: "Khác",
  glyph: "•",
  tint: "from-slate-500/40 to-slate-700/40",
};

/**
 * Get display info for an extractor name. Always returns a value — falls back
 * to a neutral "Khác" card when the extractor isn't in the table.
 */
export function platformInfo(extractor: string | null | undefined): PlatformInfo {
  if (!extractor) return FALLBACK;
  return PLATFORMS[extractor] ?? {
    ...FALLBACK,
    // Use the raw extractor id as label so user still sees something useful.
    label: extractor,
  };
}
