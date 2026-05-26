/**
 * ProDowwn API Server
 *
 * Endpoints:
 *   GET  /health                    — Health check
 *   POST /api/info                 — Get video metadata (yt-dlp --dump-json)
 *   GET  /api/download/:id         — Download video (streams to browser)
 *   POST /api/download             — Start a download, returns download URL
 *
 * Architecture:
 * - yt-dlp streams stdout to response directly (no disk storage)
 * - Videos are never saved to disk (ephemeral filesystem safe)
 * - Temporary files go to /tmp and are cleaned up automatically
 */

import express from "express";
import cors from "cors";
import { spawn } from "child_process";
import { existsSync } from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const app = express();
const PORT = process.env.PORT || 3001;

// ── Middleware ──────────────────────────────────────────────────────────────
app.use(cors({
  origin: process.env.ALLOWED_ORIGIN || "*",
  methods: ["GET", "POST", "OPTIONS"],
  allowedHeaders: ["Content-Type", "Authorization"],
}));
app.use(express.json());

// ── Utilities ───────────────────────────────────────────────────────────────

/** Run yt-dlp with args, capture stdout+stderr. Returns { code, stdout, stderr }. */
function runYtDlp(args, opts = {}) {
  return new Promise((resolve) => {
    const proc = spawn("yt-dlp", args, {
      stdio: ["ignore", "pipe", "pipe"],
      ...opts,
    });

    let stdout = "";
    let stderr = "";

    proc.stdout?.on("data", (d) => { stdout += d.toString(); });
    proc.stderr?.on("data", (d) => { stderr += d.toString(); });
    proc.on("close", (code) => resolve({ code, stdout, stderr }));
    proc.on("error", (err) => resolve({ code: -1, stdout: "", stderr: err.message }));
  });
}

/** Run yt-dlp and pipe stdout directly to a writable stream (e.g. HTTP response).
 *  Returns a promise that resolves to { code, stderr } when the pipe ends. */
function pipeYtDlp(args, outputStream, opts = {}) {
  return new Promise((resolve) => {
    const proc = spawn("yt-dlp", args, {
      stdio: ["ignore", "pipe", "pipe"],
      ...opts,
    });

    let stderr = "";
    proc.stderr?.on("data", (d) => { stderr += d.toString(); });
    proc.on("error", (err) => { stderr += err.message; });

    proc.stdout?.pipe(outputStream);
    outputStream.on("finish", () => {
      proc.kill();
      resolve({ code: proc.exitCode ?? 0, stderr });
    });
    outputStream.on("error", (e) => {
      stderr += " | Output error: " + e.message;
      proc.kill();
      resolve({ code: -1, stderr });
    });
    proc.on("close", (code) => resolve({ code: code ?? 0, stderr }));
  });
}

/** Convert error messages to user-friendly Vietnamese. */
function friendlyError(stderr, fallback = "Lỗi không xác định") {
  if (!stderr) return fallback;
  const s = stderr.toLowerCase();
  if (s.includes("unable to extract")) return "Không truy cập được video này.";
  if (s.includes("is not a valid url") || s.includes("not a supported"))
    return "URL không hỗ trợ.";
  if (s.includes("private") || s.includes("login")) return "Video riêng tư hoặc yêu cầu đăng nhập.";
  if (s.includes("geographic restriction") || s.includes("blocked"))
    return "Video bị chặn theo khu vực.";
  if (s.includes("not found") || s.includes("is unavailable"))
    return "Video không tồn tại hoặc đã bị xoá.";
  if (s.includes("too many requests") || s.includes("rate limit"))
    return "Bị giới hạn tạm thời. Thử lại sau vài phút.";
  // Truncate long errors
  const lines = stderr.split("\n").filter(Boolean);
  return lines[lines.length - 1]?.slice(0, 200) || fallback;
}

// ── Routes ──────────────────────────────────────────────────────────────────

/** GET /health */
app.get("/health", (_req, res) => {
  res.json({ status: "ok", timestamp: new Date().toISOString() });
});

/** POST /api/info — get video metadata */
app.post("/api/info", async (req, res) => {
  const { url } = req.body;
  if (!url || typeof url !== "string") {
    return res.status(400).json({ error: "Thiếu URL" });
  }

  // Check if yt-dlp is installed
  const versionCheck = await runYtDlp(["--version"]);
  if (versionCheck.code !== 0) {
    return res.status(503).json({
      error: "yt-dlp chưa được cài đặt trên server. Vui lòng báo admin.",
    });
  }

  const { code, stdout, stderr } = await runYtDlp([
    "--dump-json",
    "--no-playlist",
    "--flat-playlist",
    "--user-agent",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
    url,
  ]);

  if (code !== 0) {
    return res.status(400).json({ error: friendlyError(stderr) });
  }

  try {
    const info = JSON.parse(stdout);
    // Normalize to a simple shape
    const metadata = {
      title: info.title || "Không có tiêu đề",
      thumbnail: info.thumbnail || null,
      duration: info.duration || null,
      channel: info.channel || info.uploader || null,
      extractor: info.extractor || detectExtractor(url),
      viewCount: info.view_count || null,
      uploadDate: info.upload_date || null,
      description: info.description || null,
      isLive: info.is_live || false,
      formats: (info.formats || [])
        .filter(f => f.ext === "mp4" || f.ext === "webm")
        .map(f => ({
          format_id: f.format_id,
          ext: f.ext,
          resolution: f.resolution || (f.height ? `${f.height}p` : "audio"),
          filesize: f.filesize || f.filesize_approx || null,
          tbr: f.tbr || null,
          vcodec: f.vcodec || "none",
          acodec: f.acodec || "none",
        })),
    };
    res.json(metadata);
  } catch (e) {
    res.status(500).json({ error: "Dữ liệu video không đọc được: " + e.message });
  }
});

/** POST /api/download — start download, stream video back */
app.post("/api/download", async (req, res) => {
  const { url, formatId, isAudioOnly } = req.body;
  if (!url || typeof url !== "string") {
    return res.status(400).json({ error: "Thiếu URL" });
  }

  // Detect extractor from URL
  const extractor = detectExtractor(url);

  // Build yt-dlp args
  const args = [
    // Output: send to stdout
    "-o", "-",
    // Don't save to disk — stream immediately
    "--no-playlist",
    // User agent
    "--user-agent",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36",
    // Accept gzip compression to speed up transfer
    "--compress-progress-bar",
  ];

  // Format selection
  if (isAudioOnly) {
    args.push(
      "-x",                          // extract audio
      "--audio-format", "mp3",
      "--audio-quality", "0",
    );
  } else if (formatId) {
    args.push("-f", formatId);
  } else {
    // Default: best video + best audio merged
    args.push("-f", "bestvideo[ext=mp4]+bestaudio[ext=m4a]/bestvideo+bestaudio/best");
  }

  args.push(url);

  // Set response headers for download
  const filename = `video_${Date.now()}.mp4`;
  res.setHeader("Content-Type", isAudioOnly ? "audio/mpeg" : "video/mp4");
  res.setHeader("Content-Disposition", `attachment; filename="${filename}"`);
  res.setHeader("Transfer-Encoding", "chunked");
  res.setHeader("Cache-Control", "no-store");

  const { code, stderr } = await pipeYtDlp(args, res);

  if (code !== 0) {
    // If headers haven't been sent, send error JSON
    if (!res.headersSent) {
      return res.status(400).json({ error: friendlyError(stderr) });
    }
    // Otherwise the stream already started, just log
    console.error("yt-dlp error:", stderr);
  }
});

/** GET /api/formats — get supported formats for URL (for format picker UI) */
app.post("/api/formats", async (req, res) => {
  const { url } = req.body;
  if (!url) return res.status(400).json({ error: "Thiếu URL" });

  const { code, stdout, stderr } = await runYtDlp([
    "-F",
    "--no-playlist",
    url,
  ]);

  if (code !== 0) {
    return res.status(400).json({ error: friendlyError(stderr) });
  }

  // Parse the format list
  const lines = stdout.split("\n");
  const formats = [];
  for (const line of lines) {
    const m = line.match(/^(\S+)\s+(\w+)\s+(.+?)(?:\s+(\d+(?:\.\d+)?[GMmk]?)?)?\s*$/);
    if (m) {
      const [, id, ext, note, size] = m;
      formats.push({ id, ext, note: note.trim(), size: size || null });
    }
  }

  res.json({ formats: formats.slice(0, 50) });
});

/** Detect extractor from URL. */
function detectExtractor(url) {
  const u = url.toLowerCase();
  if (u.includes("youtube.com") || u.includes("youtu.be")) return "youtube";
  if (u.includes("tiktok.com")) return "tiktok";
  if (u.includes("douyin.com")) return "douyin";
  if (u.includes("instagram.com")) return "instagram";
  if (u.includes("facebook.com") || u.includes("fb.watch")) return "facebook";
  if (u.includes("twitter.com") || u.includes("x.com")) return "twitter";
  if (u.includes("reddit.com")) return "reddit";
  if (u.includes("pinterest.com")) return "pinterest";
  if (u.includes("threads.net")) return "threads";
  return "unknown";
}

// ── Static files (frontend) ───────────────────────────────────────────────────
const distPath = path.join(__dirname, "..", "dist");
if (existsSync(distPath)) {
  app.use(express.static(distPath));
  // SPA fallback: serve index.html for unknown routes
  app.get("*", (_req, res) => {
    res.sendFile(path.join(distPath, "web.html"));
  });
} else {
  app.get("/", (_req, res) => {
    res.json({
      status: "ok",
      message: "ProDowwn API Server — frontend chưa được build. Chạy: npm run web:build",
      endpoints: ["/health", "/api/info", "/api/download"],
    });
  });
}

// ── Start ───────────────────────────────────────────────────────────────────
app.listen(PORT, () => {
  console.log(`ProDowwn API running on port ${PORT}`);
});
