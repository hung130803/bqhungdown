/**
 * ProDowwn API Server
 *
 * Endpoints:
 *   GET  /health                    — Health check
 *   POST /api/info                 — Get video metadata
 *   POST /api/download             — Download video (streams to browser)
 *
 * YouTube: uses ytdl-core (library-level bypass, no external server needed)
 * Other:  uses yt-dlp directly
 */

import express from "express";
import cors from "cors";
import { spawn } from "child_process";
import { existsSync } from "fs";
import path from "path";
import { fileURLToPath } from "url";
import ytdl from "ytdl-core";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const app = express();
const PORT = process.env.PORT || 3001;

// ── Middleware ──────────────────────────────────────────────────────────────
app.use(cors({
  origin: process.env.ALLOWED_ORIGIN || "*",
  methods: ["GET", "POST", "OPTIONS"],
  allowedHeaders: ["Content-Type", "Authorization"],
}));
app.use(express.json({ limit: "10mb" }));

// ── Helpers ─────────────────────────────────────────────────────────────────

function isYouTubeUrl(url) {
  const u = url.toLowerCase();
  return u.includes("youtube.com") || u.includes("youtu.be");
}

function slugify(title, id) {
  const clean = (title || "video")
    .replace(/[^a-zA-Z0-9\u00C0-\u024F\u1EA0-\u1EF9 \-+_]/g, "")
    .trim()
    .slice(0, 60);
  return clean ? `${clean}_${id}.mp4` : `video_${id}.mp4`;
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

  try {
    if (isYouTubeUrl(url)) {
      // ── YouTube: ytdl-core ──────────────────────────────────────────────
      const info = await ytdl.getInfo(url);

      const formats = info.formats
        .filter(f => f.hasAudio && f.hasVideo || f.hasAudio || f.hasVideo)
        .map(f => ({
          format_id: f.formatId,
          ext: f.container || "mp4",
          resolution: f.resolution || (f.audioBitrate ? "audio" : "unknown"),
          filesize: f.contentLength ? parseInt(f.contentLength) : null,
          bitrate: f.bitrate || null,
          hasAudio: f.hasAudio,
          hasVideo: f.hasVideo,
          audioBitrate: f.audioBitrate || null,
          videoBitrate: f.videoBitrate || null,
        }));

      const metadata = {
        title: info.videoDetails.title || "Không có tiêu đề",
        thumbnail: info.videoDetails.thumbnails?.[0]?.url || null,
        duration: parseInt(info.videoDetails.lengthSeconds) || null,
        channel: info.videoDetails.ownerChannelName || info.videoDetails.author?.name || null,
        extractor: "youtube",
        viewCount: parseInt(info.videoDetails.viewCount) || null,
        uploadDate: info.player_response?.microformat?.playerMicroformatRenderer?.uploadDate || null,
        description: info.videoDetails.description || null,
        isLive: info.videoDetails.isLiveContent || false,
        formats,
      };

      return res.json(metadata);

    } else {
      // ── Other platforms: yt-dlp ─────────────────────────────────────────
      const { code, stdout, stderr } = await runYtDlp([
        "--dump-json", "--no-playlist", "--flat-playlist",
        "--user-agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        url,
      ]);

      if (code !== 0) {
        return res.status(400).json({ error: friendlyError(stderr) });
      }

      const info = JSON.parse(stdout);
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
      return res.json(metadata);
    }
  } catch (e) {
    console.error("api/info error:", e.message);
    return res.status(500).json({ error: e.message || "Lỗi không xác định" });
  }
});

/** POST /api/download — stream video to browser */
app.post("/api/download", async (req, res) => {
  const { url, formatId, isAudioOnly } = req.body;
  if (!url || typeof url !== "string") {
    return res.status(400).json({ error: "Thiếu URL" });
  }

  try {
    if (isYouTubeUrl(url)) {
      // ── YouTube: ytdl-core streaming ─────────────────────────────────────
      const info = await ytdl.getInfo(url);
      const videoId = info.videoDetails.videoId;
      const title = slugify(info.videoDetails.title, videoId);

      const filter = isAudioOnly
        ? (f) => f.hasAudio && !f.hasVideo
        : formatId
          ? (f) => f.formatId === formatId
          : (f) => f.hasAudio && f.hasVideo;

      const stream = ytdl(url, { filter, requestOptions: { headers: { "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36" } } });

      res.setHeader("Content-Type", isAudioOnly ? "audio/mpeg" : "video/mp4");
      res.setHeader("Content-Disposition", `attachment; filename="${title}"`);
      res.setHeader("Transfer-Encoding", "chunked");
      res.setHeader("Cache-Control", "no-store");

      stream.on("error", (e) => {
        console.error("ytdl error:", e.message);
        if (!res.headersSent) {
          res.status(500).json({ error: "Lỗi khi tải video: " + e.message });
        } else {
          res.destroy();
        }
      });

      stream.pipe(res);

    } else {
      // ── Other platforms: yt-dlp streaming ─────────────────────────────────
      const args = [
        "-o", "-",
        "--no-playlist",
        "--user-agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36",
        "--compress-progress-bar",
      ];

      if (isAudioOnly) {
        args.push("-x", "--audio-format", "mp3", "--audio-quality", "0");
      } else if (formatId) {
        args.push("-f", formatId);
      } else {
        args.push("-f", "bestvideo[ext=mp4]+bestaudio[ext=m4a]/bestvideo+bestaudio/best");
      }

      args.push(url);

      const { code, stderr } = await pipeYtDlp(args, res);

      if (code !== 0 && !res.headersSent) {
        return res.status(400).json({ error: friendlyError(stderr) });
      }
      if (code !== 0) {
        console.error("yt-dlp error:", stderr);
      }
    }
  } catch (e) {
    console.error("api/download error:", e.message);
    if (!res.headersSent) {
      res.status(500).json({ error: e.message || "Lỗi không xác định" });
    }
  }
});

/** POST /api/formats */
app.post("/api/formats", async (req, res) => {
  const { url } = req.body;
  if (!url) return res.status(400).json({ error: "Thiếu URL" });

  try {
    if (isYouTubeUrl(url)) {
      const info = await ytdl.getInfo(url);
      const formats = info.formats.map(f => ({
        format_id: f.formatId,
        ext: f.container || "mp4",
        resolution: f.resolution || (f.audioBitrate ? "audio" : "unknown"),
        filesize: f.contentLength ? parseInt(f.contentLength) : null,
        hasAudio: f.hasAudio,
        hasVideo: f.hasVideo,
        bitrate: f.bitrate || null,
      }));
      return res.json({ formats });
    } else {
      const { code, stdout, stderr } = await runYtDlp(["-F", "--no-playlist", url]);
      if (code !== 0) return res.status(400).json({ error: friendlyError(stderr) });

      const lines = stdout.split("\n");
      const formats = [];
      for (const line of lines) {
        const m = line.match(/^(\S+)\s+(\w+)\s+(.+?)(?:\s+(\d+(?:\.\d+)?[GMmk]?)?)?\s*$/);
        if (m) {
          const [, id, ext, note, size] = m;
          formats.push({ id, ext, note: note.trim(), size: size || null });
        }
      }
      return res.json({ formats: formats.slice(0, 50) });
    }
  } catch (e) {
    return res.status(500).json({ error: e.message });
  }
});

// ── yt-dlp helpers ──────────────────────────────────────────────────────────

function runYtDlp(args, opts = {}) {
  return new Promise((resolve) => {
    const proc = spawn("yt-dlp", args, { stdio: ["ignore", "pipe", "pipe"], ...opts });
    let stdout = "", stderr = "";
    proc.stdout?.on("data", (d) => { stdout += d.toString(); });
    proc.stderr?.on("data", (d) => { stderr += d.toString(); });
    proc.on("close", (code) => resolve({ code, stdout, stderr }));
    proc.on("error", (err) => resolve({ code: -1, stdout: "", stderr: err.message }));
  });
}

function pipeYtDlp(args, outputStream, opts = {}) {
  return new Promise((resolve) => {
    const proc = spawn("yt-dlp", args, { stdio: ["ignore", "pipe", "pipe"], ...opts });
    let stderr = "";
    proc.stderr?.on("data", (d) => { stderr += d.toString(); });
    proc.on("error", (err) => { stderr += err.message; });
    proc.stdout?.pipe(outputStream);
    outputStream.on("finish", () => { proc.kill(); resolve({ code: proc.exitCode ?? 0, stderr }); });
    outputStream.on("error", (e) => { stderr += " | " + e.message; proc.kill(); resolve({ code: -1, stderr }); });
    proc.on("close", (code) => resolve({ code: code ?? 0, stderr }));
  });
}

function friendlyError(stderr, fallback = "Lỗi không xác định") {
  if (!stderr) return fallback;
  const s = stderr.toLowerCase();
  if (s.includes("unable to extract")) return "Không truy cập được video này.";
  if (s.includes("is not a valid url") || s.includes("not a supported")) return "URL không hỗ trợ.";
  if (s.includes("private") || s.includes("login")) return "Video riêng tư hoặc yêu cầu đăng nhập.";
  if (s.includes("geographic restriction") || s.includes("blocked")) return "Video bị chặn theo khu vực.";
  if (s.includes("not found") || s.includes("is unavailable")) return "Video không tồn tại hoặc đã bị xoá.";
  if (s.includes("too many requests") || s.includes("rate limit")) return "Bị giới hạn tạm thời. Thử lại sau vài phút.";
  if (s.includes("sign in to confirm")) return "YouTube yêu cầu đăng nhập. Dùng app desktop để tải YouTube.";
  const lines = stderr.split("\n").filter(Boolean);
  return lines[lines.length - 1]?.slice(0, 200) || fallback;
}

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
  console.log(`YouTube: ytdl-core | Other: yt-dlp`);
});
