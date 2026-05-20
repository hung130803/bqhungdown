#!/usr/bin/env node
/**
 * fetch-sidecars.mjs
 * Downloads yt-dlp and ffmpeg sidecar binaries into src-tauri/binaries/<name>-<triple>[.exe]
 * Run automatically via npm postinstall.
 */

import { mkdir, stat, chmod } from 'node:fs/promises';
import { createWriteStream } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Readable, Transform } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import process from 'node:process';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const ROOT = resolve(__dirname, '..');
const BIN_DIR = join(ROOT, 'src-tauri', 'binaries');

// ---------- CLI args ----------
const argv = process.argv.slice(2);
const FORCE = argv.includes('--force');
const ONLY = (() => {
  const a = argv.find((x) => x.startsWith('--only='));
  return a ? a.split('=')[1] : null; // 'ytdlp' | 'ffmpeg' | null
})();
const TRIPLE_OVERRIDE = (() => {
  const a = argv.find((x) => x.startsWith('--triple='));
  return a ? a.split('=')[1] : null;
})();

// ---------- Triple detection ----------
function detectTriple() {
  if (TRIPLE_OVERRIDE) return TRIPLE_OVERRIDE;
  const { platform, arch } = process;
  if (platform === 'win32' && arch === 'x64') return 'x86_64-pc-windows-msvc';
  if (platform === 'darwin' && arch === 'x64') return 'x86_64-apple-darwin';
  if (platform === 'darwin' && arch === 'arm64') return 'aarch64-apple-darwin';
  if (platform === 'linux' && arch === 'x64') return 'x86_64-unknown-linux-gnu';
  if (platform === 'linux' && arch === 'arm64') return 'aarch64-unknown-linux-gnu';
  console.error(
    `[fetch-sidecars] Nền tảng không được hỗ trợ: ${platform}/${arch}. ` +
      `Hãy build sidecar thủ công hoặc dùng --triple=<target> để override.`,
  );
  process.exit(1);
}

const TRIPLE = detectTriple();
const IS_WINDOWS = TRIPLE.includes('windows');
const EXE = IS_WINDOWS ? '.exe' : '';

// ---------- Source URL maps ----------
/**
 * Each entry: { url, archive: boolean, archiveName?: string }
 * archive=true means the file needs manual extraction (zip/tar.xz).
 */
const YTDLP_SOURCES = {
  'x86_64-pc-windows-msvc': {
    url: 'https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe',
    archive: false,
  },
  'x86_64-apple-darwin': {
    url: 'https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos',
    archive: false,
  },
  'aarch64-apple-darwin': {
    url: 'https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos',
    archive: false,
  },
  'x86_64-unknown-linux-gnu': {
    url: 'https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux',
    archive: false,
  },
  'aarch64-unknown-linux-gnu': {
    url: 'https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux_aarch64',
    archive: false,
  },
};

const FFMPEG_SOURCES = {
  'x86_64-pc-windows-msvc': {
    // Gyan.dev "essentials" build — ~80MB instead of BtbN's ~200MB GPL build.
    // Vẫn đủ codec phổ biến cho yt-dlp: h264, hevc, vp9, av1, aac, mp3,
    // opus, vorbis, flac → mọi format YouTube/TikTok/Douyin merge được.
    url: 'https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip',
    archive: true,
    archiveName: 'ffmpeg-essentials.zip',
  },
  'x86_64-apple-darwin': {
    url: 'https://evermeet.cx/ffmpeg/getrelease/ffmpeg/zip',
    archive: true,
    archiveName: 'ffmpeg-macos-x64.zip',
  },
  'aarch64-apple-darwin': {
    url: 'https://www.osxexperts.net/ffmpeg7arm.zip',
    archive: true,
    archiveName: 'ffmpeg-macos-arm64.zip',
  },
  'x86_64-unknown-linux-gnu': {
    url: 'https://github.com/BtbN/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-linux64-gpl.tar.xz',
    archive: true,
    archiveName: 'ffmpeg-linux64-gpl.tar.xz',
  },
  'aarch64-unknown-linux-gnu': {
    url: 'https://github.com/BtbN/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-linuxarm64-gpl.tar.xz',
    archive: true,
    archiveName: 'ffmpeg-linuxarm64-gpl.tar.xz',
  },
};

// ---------- Helpers ----------
async function exists(p) {
  try {
    await stat(p);
    return true;
  } catch {
    return false;
  }
}

function formatBytes(n) {
  if (!Number.isFinite(n)) return '?';
  const mb = n / (1024 * 1024);
  return `${mb.toFixed(1)} MB`;
}

async function downloadStream(url, destPath, label) {
  const res = await fetch(url, { redirect: 'follow' });
  if (!res.ok || !res.body) {
    throw new Error(`HTTP ${res.status} ${res.statusText} cho ${url}`);
  }

  const totalHeader = res.headers.get('content-length');
  const total = totalHeader ? Number(totalHeader) : NaN;
  let received = 0;
  let lastPrint = 0;

  const reporter = new TransformReporter((chunkLen) => {
    received += chunkLen;
    const now = Date.now();
    if (now - lastPrint > 250 || (Number.isFinite(total) && received === total)) {
      lastPrint = now;
      const pct = Number.isFinite(total) ? Math.floor((received / total) * 100) : null;
      const line = `[fetch] ${label}: ${formatBytes(received)} / ${formatBytes(total)}${
        pct !== null ? ` (${pct}%)` : ''
      }`;
      process.stdout.write(`\r${line}   `);
    }
  });

  const out = createWriteStream(destPath);
  // Convert web ReadableStream -> Node Readable
  const nodeStream = Readable.fromWeb(res.body);
  await pipeline(nodeStream, reporter, out);
  process.stdout.write('\n');
}

/**
 * Lightweight Transform that just observes byte counts without altering data.
 */
class TransformReporter extends Transform {
  constructor(onChunk) {
    super();
    this.onChunk = onChunk;
  }
  _transform(chunk, _enc, cb) {
    this.onChunk(chunk.length);
    cb(null, chunk);
  }
}

// ---------- Per-tool fetcher ----------
async function fetchTool(toolName, sources) {
  const src = sources[TRIPLE];
  if (!src) {
    console.error(`[fetch-sidecars] Không có nguồn ${toolName} cho triple ${TRIPLE}`);
    process.exit(1);
  }

  const expectedBinaryName = `${toolName}-${TRIPLE}${EXE}`;
  const expectedBinaryPath = join(BIN_DIR, expectedBinaryName);

  if (!FORCE && (await exists(expectedBinaryPath))) {
    console.log(`[fetch-sidecars] Bỏ qua ${toolName}: đã tồn tại ${expectedBinaryPath} (dùng --force để tải lại).`);
    return;
  }

  if (src.archive) {
    // Download archive into BIN_DIR but don't extract automatically.
    const archivePath = join(BIN_DIR, src.archiveName);
    if (!FORCE && (await exists(archivePath))) {
      console.log(`[fetch-sidecars] Archive đã có: ${archivePath}`);
    } else {
      console.log(`[fetch-sidecars] Đang tải ${toolName} archive từ ${src.url}`);
      try {
        await downloadStream(src.url, archivePath, `${toolName} archive`);
      } catch (err) {
        console.error(`[fetch-sidecars] Lỗi khi tải ${toolName}: ${err.message}`);
        process.exit(1);
      }
    }

    console.warn(
      `[fetch-sidecars] ⚠  Đã tải ${archivePath}. Vui lòng giải nén thủ công và đặt binary tại:\n` +
        `    ${expectedBinaryPath}\n` +
        `  (Script không tự giải nén để tránh phụ thuộc unzip/tar.)`,
    );
    return;
  }

  // Direct binary download.
  console.log(`[fetch-sidecars] Đang tải ${toolName} từ ${src.url}`);
  try {
    await downloadStream(src.url, expectedBinaryPath, toolName);
  } catch (err) {
    console.error(`[fetch-sidecars] Lỗi khi tải ${toolName}: ${err.message}`);
    process.exit(1);
  }

  if (!IS_WINDOWS) {
    try {
      await chmod(expectedBinaryPath, 0o755);
    } catch (err) {
      console.warn(`[fetch-sidecars] Không thể chmod +x ${expectedBinaryPath}: ${err.message}`);
    }
  }

  console.log(`[fetch-sidecars] ✓ ${toolName} → ${expectedBinaryPath}`);
}

// ---------- Main ----------
async function main() {
  console.log(`[fetch-sidecars] Triple: ${TRIPLE}`);
  console.log(`[fetch-sidecars] Output dir: ${BIN_DIR}`);

  await mkdir(BIN_DIR, { recursive: true });

  if (!ONLY || ONLY === 'ytdlp') {
    await fetchTool('yt-dlp', YTDLP_SOURCES);
  }
  if (!ONLY || ONLY === 'ffmpeg') {
    await fetchTool('ffmpeg', FFMPEG_SOURCES);
  }

  console.log('[fetch-sidecars] Hoàn tất.');
}

main().catch((err) => {
  console.error(`[fetch-sidecars] Lỗi không mong đợi: ${err?.stack || err}`);
  process.exit(1);
});
