#!/usr/bin/env node
/**
 * verify-sidecars.mjs
 * Kiểm tra binaries sidecar (yt-dlp + ffmpeg) đã được đặt đúng tên theo target triple
 * tại `src-tauri/binaries/<name>-<triple>[.exe]`.
 *
 * Exit code:
 *   0 — đầy đủ và executable.
 *   1 — thiếu hoặc chỉ có archive chưa giải nén.
 *
 * Script này KHÔNG throw — luôn kết thúc bằng `process.exit(...)` với thông điệp tiếng Việt.
 */

import { stat, access } from 'node:fs/promises';
import { constants as FS } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import process from 'node:process';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const ROOT = resolve(__dirname, '..');
const BIN_DIR = join(ROOT, 'src-tauri', 'binaries');

// ---------- CLI args ----------
const argv = process.argv.slice(2);
const TRIPLE_OVERRIDE = (() => {
  const a = argv.find((x) => x.startsWith('--triple='));
  return a ? a.split('=')[1] : null;
})();

// ---------- Triple detection (mirror fetch-sidecars.mjs) ----------
function detectTriple() {
  if (TRIPLE_OVERRIDE) return TRIPLE_OVERRIDE;
  const { platform, arch } = process;
  if (platform === 'win32' && arch === 'x64') return 'x86_64-pc-windows-msvc';
  if (platform === 'darwin' && arch === 'x64') return 'x86_64-apple-darwin';
  if (platform === 'darwin' && arch === 'arm64') return 'aarch64-apple-darwin';
  if (platform === 'linux' && arch === 'x64') return 'x86_64-unknown-linux-gnu';
  if (platform === 'linux' && arch === 'arm64') return 'aarch64-unknown-linux-gnu';
  console.error(
    `[verify-sidecars] Nền tảng không được hỗ trợ: ${platform}/${arch}. ` +
      `Dùng --triple=<target> để override.`,
  );
  process.exit(1);
}

const TRIPLE = detectTriple();
const IS_WINDOWS = TRIPLE.includes('windows');
const EXE = IS_WINDOWS ? '.exe' : '';

// ---------- Archive hints (đồng bộ với fetch-sidecars.mjs) ----------
const FFMPEG_ARCHIVE_NAMES = {
  'x86_64-pc-windows-msvc': 'ffmpeg-essentials.zip',
  'x86_64-apple-darwin': 'ffmpeg-macos-x64.zip',
  'aarch64-apple-darwin': 'ffmpeg-macos-arm64.zip',
  'x86_64-unknown-linux-gnu': 'ffmpeg-linux64-gpl.tar.xz',
  'aarch64-unknown-linux-gnu': 'ffmpeg-linuxarm64-gpl.tar.xz',
};

// ---------- Helpers ----------
async function pathExists(p) {
  try {
    await stat(p);
    return true;
  } catch {
    return false;
  }
}

async function isExecutable(p) {
  if (IS_WINDOWS) return true; // Windows không có bit X
  try {
    await access(p, FS.X_OK);
    return true;
  } catch {
    return false;
  }
}

/**
 * Kiểm tra một sidecar binary.
 * @param {string} toolName - 'yt-dlp' | 'ffmpeg'
 * @param {string|null} archiveHint - tên archive nếu tool có archive (ffmpeg)
 * @returns {Promise<{ok: boolean, message: string}>}
 */
async function checkTool(toolName, archiveHint) {
  const binaryName = `${toolName}-${TRIPLE}${EXE}`;
  const binaryPath = join(BIN_DIR, binaryName);

  if (await pathExists(binaryPath)) {
    if (!(await isExecutable(binaryPath))) {
      return {
        ok: false,
        message:
          `[verify-sidecars] ✗ ${binaryName} tồn tại nhưng không có quyền thực thi.\n` +
          `    Chạy: chmod +x "${binaryPath}"`,
      };
    }
    return { ok: true, message: `[verify-sidecars]   ✓ ${binaryName}` };
  }

  // Binary thiếu — kiểm tra xem có archive đang nằm chờ giải nén không.
  if (archiveHint) {
    const archivePath = join(BIN_DIR, archiveHint);
    if (await pathExists(archivePath)) {
      return {
        ok: false,
        message:
          `[verify-sidecars] ✗ Thiếu binary ${binaryName}.\n` +
          `    Đã thấy archive: ${archivePath}\n` +
          `    → Hãy giải nén thủ công và đặt binary tại: ${binaryPath}\n` +
          `    (Script không tự giải nén để tránh phụ thuộc unzip/tar.)`,
      };
    }
  }

  return {
    ok: false,
    message:
      `[verify-sidecars] ✗ Thiếu binary ${binaryName} tại ${BIN_DIR}.\n` +
      `    Chạy: npm run setup:sidecars`,
  };
}

// ---------- Main ----------
async function main() {
  console.log(`[verify-sidecars] Triple: ${TRIPLE}`);
  console.log(`[verify-sidecars] Bin dir: ${BIN_DIR}`);

  if (!(await pathExists(BIN_DIR))) {
    console.error(
      `[verify-sidecars] ✗ Thư mục ${BIN_DIR} chưa tồn tại.\n` +
        `    Chạy: npm run setup:sidecars`,
    );
    process.exit(1);
  }

  const checks = await Promise.all([
    checkTool('yt-dlp', null),
    checkTool('ffmpeg', FFMPEG_ARCHIVE_NAMES[TRIPLE] ?? null),
  ]);

  const failed = checks.filter((c) => !c.ok);
  for (const c of checks) {
    if (c.ok) console.log(c.message);
    else console.error(c.message);
  }

  if (failed.length > 0) {
    console.error(
      `[verify-sidecars] Có ${failed.length} sidecar chưa sẵn sàng. ` +
        `Vui lòng khắc phục trước khi build.`,
    );
    process.exit(1);
  }

  console.log('[verify-sidecars] ✓ yt-dlp + ffmpeg sẵn sàng');
  process.exit(0);
}

main().catch((err) => {
  // Bắt mọi lỗi không lường trước để tuân thủ "không throw".
  console.error(`[verify-sidecars] Lỗi không mong đợi: ${err?.message || err}`);
  process.exit(1);
});
