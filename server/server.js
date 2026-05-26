/**
 * ProDowwn API Server
 *
 * Endpoints:
 *   GET  /health                    — Health check
 *   POST /api/info                 — Get video metadata (yt-dlp or Invidious for YouTube)
 *   POST /api/download             — Download video (streams to browser)
 *
 * Architecture:
 * - YouTube: uses Invidious API (no bot detection)
 * - Other platforms: uses yt-dlp directly
 * - Videos are never saved to disk (ephemeral filesystem safe)
 * - Temporary files go to /tmp and are cleaned up automatically
 */

import express from "express";
import cors from "cors";
import { spawn } from "child_process";
import { existsSync } from "fs";
import path from "path";
import { fileURLToPath } from "url";
import https from "https";
import http from "http";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const app = express();
const PORT = process.env.PORT || 3001;

// ── Invidious instances (public, free, rotates if one fails) ──────────────────
const INVIDIOUS_INSTANCES = [
  "https://invidious.nerdvpn.de",
  "https://iv.nboeck.de",
  "https://invidious.fdn.fr",
  "https://iv.melonium.dev",
];
let invidiousIndex = 0;
function getInvidiousBase() {
  return INVIDIOUS_INSTANCES[invidiousIndex % INVIDIOUS_INSTANCES.length];
}
function rotateInvidious() {
  invidiousIndex++;
}

// ── Middleware ──────────────────────────────────────────────────────────────
app.use(cors({
  origin: process.env.ALLOWED_ORIGIN || "*",
  methods: ["GET", "POST", "OPTIONS"],
  allowedHeaders: ["Content-Type", "Authorization"],
}));
app.use(express.json({ limit: "10mb" }));

// ── Utilities ───────────────────────────────────────────────────────────────

/** Simple fetch that works with both http and https */
function fetchUrl(targetUrl) {
  return new Promise((resolve, reject) => {
    const urlObj = new URL(targetUrl);
    const lib = urlObj.protocol === "https:" ? https : http;
    const req = lib.get(targetUrl, { headers: { "User-Agent": "Mozilla/5.0" } }, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        return fetchUrl(res.headers.location).then(resolve).catch(reject);
      }
      if (res.statusCode !== 200) {
        return reject(new Error(`HTTP ${res.statusCode}`));
      }
      const chunks = [];
      res.on("data", (d) => chunks.push(d));
      res.on("end", () => resolve(Buffer.concat(chunks).toString("utf8")));
    });
    req.on("error", reject);
    req.setTimeout(15000, () => { req.destroy(); reject(new Error("Timeout")); });
  });
}

/** Run yt-dlp with args, capture stdout+stderr. */
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

/** Run yt-dlp and pipe stdout directly to a writable stream. */
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

/** Run yt-dlp and pipe stdout to an http(s) response. */
function pipeYtDlpToUrl(args, targetUrl) {
  return new Promise((resolve) => {
    const urlObj = new URL(targetUrl);
    const lib = urlObj.protocol === "https:" ? https : http;
    const req = lib.request(targetUrl, { method: "GET", headers: { "User-Agent": "Mozilla/5.0" } }, (proxyRes) => {
      const { code, stderr } = { code: proxyRes.statusCode, stderr: "" };
      if (proxyRes.statusCode >= 400) {
        return resolve({ code: proxyRes.statusCode, stderr: `Proxy error: ${proxyRes.statusCode}` });
      }
      proxyRes.on("data", (d) => { try { process.stdout.write(d); } catch {} });
      proxyRes.on("end", () => resolve({ code: 0, stderr: "" }));
    });
    req.on("error", (e) => resolve({ code: -1, stderr: e.message }));
    const proc = spawn("yt-dlp", args, { stdio: ["ignore", "pipe", "pipe"] });
    let stderr = "";
    proc.stderr?.on("data", (d) => { stderr += d.toString(); });
    proc.on("error", (e) => { stderr += e.message; });
    proc.stdout?.pipe(req);
    req.on("error", () => {});
    proc.on("close", (code) => resolve({ code: code ?? 0, stderr }));
    proc.on("error", (e) => resolve({ code: -1, stderr: e.message }));
  });
}

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
    return "Video không tồn tại or đã bị xoá.";
  if (s.includes("too many requests") || s.includes("rate limit"))
    return "Bị giới hạn tạm thời. Thử lại sau vài phút.";
  if (s.includes("sign in to confirm")) return "YouTube yêu cầu đăng nhập. Dùng app desktop để tải YouTube.";
  const lines = stderr.split("\n").filter(Boolean);
  return lines[lines.length - 1]?.slice(0, 200) || fallback;
}

function isYouTubeUrl(url) {
  const u = url.toLowerCase();
  return u.includes("youtube.com") || u.includes("youtu.be");
}

function extractVideoId(url) {
  const u = url.toLowerCase();
  if (u.includes("youtu.be/")) {
    const m = u.match(/youtu\.be\/([^?&/]+)/);
    if (m) return m[1];
  }
  if (u.includes("youtube.com/")) {
    const m = u.match(/[?&]v=([^&]+)/);
    if (m) return m[1];
    const m2 = u.match(/\/shorts\/([^?&]+)/);
    if (m2) return m2[1];
  }
  return null;
}

async function getInvidiousVideoInfo(videoId) {
  const base = getInvidiousBase();
  const apiUrl = `${base}/api/v1/videos/${videoId}?fields=videoId,title,thumbnailUrl,description,duration,viewCount,likeCount,uploadDate,channelId,channelName,adaptiveFormats,formatStreams`;

  for (let attempt = 0; attempt < INVIDIOUS_INSTANCES.length; attempt++) {
    try {
      const text = await fetchUrl(apiUrl);
      const data = JSON.parse(text);

      if (data.error) {
        rotateInvidious();
        continue;
      }

      // Normalize formats
      const formats = [];

      // formatStreams (non-adaptive, has both video+audio)
      if (data.formatStreams) {
        for (const f of data.formatStreams) {
          formats.push({
            format_id: f.itag,
            ext: f.type.split(";")[0].split("/")[1] || f.qualityLabel || "mp4",
            resolution: f.qualityLabel || f.quality,
            filesize: parseInt(f.contentLength) || null,
            url: f.url,
            type: "stream",
            vcodec: f.type.includes("video") ? "h264" : "none",
            acodec: f.type.includes("audio") ? "aac" : "none",
          });
        }
      }

      // adaptiveFormats (separate audio/video streams)
      if (data.adaptiveFormats) {
        for (const f of data.adaptiveFormats) {
          formats.push({
            format_id: f.itag,
            ext: f.type.split(";")[0].split("/")[1] || "mp4",
            resolution: f.qualityLabel || f.quality,
            filesize: parseInt(f.contentLength) || null,
            url: f.url,
            type: "adaptive",
            vcodec: f.type.includes("video") ? "h264" : "none",
            acodec: f.type.includes("audio") ? "aac" : "none",
          });
        }
      }

      return {
        title: data.title || "Không có tiêu đề",
        thumbnail: data.thumbnailUrl || null,
        duration: data.duration || null,
        channel: data.channelName || null,
        extractor: "youtube",
        viewCount: data.viewCount || null,
        uploadDate: data.uploadDate || null,
        description: data.description || null,
        isLive: false,
        formats: formats.slice(0, 30),
      };
    } catch (e) {
      rotateInvidious();
      console.error(`Invidious attempt ${attempt + 1} failed:`, e.message);
    }
  }

  throw new Error("Tất cả Invidious servers đều không hoạt động. Thử lại sau.");
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
      // YouTube: use Invidious
      const videoId = extractVideoId(url);
      if (!videoId) {
        return res.status(400).json({ error: "Không nhận diện được video YouTube." });
      }
      const metadata = await getInvidiousVideoInfo(videoId);
      return res.json(metadata);
    } else {
      // Other platforms: use yt-dlp
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

/** POST /api/download — stream video */
app.post("/api/download", async (req, res) => {
  const { url, formatId, isAudioOnly } = req.body;
  if (!url || typeof url !== "string") {
    return res.status(400).json({ error: "Thiếu URL" });
  }

  try {
    if (isYouTubeUrl(url)) {
      // YouTube: get streaming URL from Invidious and pipe to browser
      const videoId = extractVideoId(url);
      if (!videoId) {
        return res.status(400).json({ error: "Không nhận diện được video YouTube." });
      }

      const metadata = await getInvidiousVideoInfo(videoId);

      // Find best matching format
      let targetFormat = null;
      if (isAudioOnly) {
        // Find best audio
        targetFormat = metadata.formats.find(f =>
          f.acodec !== "none" && f.vcodec === "none"
        ) || metadata.formats.find(f => f.acodec !== "none");
      } else if (formatId) {
        targetFormat = metadata.formats.find(f => f.format_id === formatId);
      } else {
        // Best video+audio combined
        targetFormat = metadata.formats.find(f =>
          f.vcodec !== "none" && f.acodec !== "none"
        ) || metadata.formats.find(f => f.type === "stream" && f.vcodec !== "none");
      }

      if (!targetFormat || !targetFormat.url) {
        return res.status(404).json({ error: "Không tìm được link tải video." });
      }

      // Stream directly from Invidious URL
      const filename = `${(metadata.title || "video").replace(/[^a-zA-Z0-9\u00C0-\u024F\u1EA0-\u1EF9 -]/g, "").slice(0, 50)}_${videoId}.mp4`;
      res.setHeader("Content-Type", isAudioOnly ? "audio/mpeg" : "video/mp4");
      res.setHeader("Content-Disposition", `attachment; filename="${filename}"`);
      res.setHeader("Transfer-Encoding", "chunked");
      res.setHeader("Cache-Control", "no-store");

      const streamUrl = targetFormat.url;
      const urlObj = new URL(streamUrl);
      const lib = urlObj.protocol === "https:" ? https : http;

      const proxyReq = lib.get(streamUrl, { headers: { "User-Agent": "Mozilla/5.0" } }, (proxyRes) => {
        if (proxyRes.statusCode >= 400) {
          if (!res.headersSent) {
            return res.status(502).json({ error: "Không thể tải video từ Invidious proxy." });
          }
          return proxyRes.destroy();
        }
        res.setHeader("Content-Length", proxyRes.headers["content-length"]);
        proxyRes.pipe(res);
        proxyRes.on("error", () => res.destroy());
      });

      proxyReq.on("error", (e) => {
        if (!res.headersSent) {
          res.status(502).json({ error: "Lỗi kết nối: " + e.message });
        } else {
          res.destroy();
        }
      });

      proxyReq.setTimeout(10000, () => {
        proxyReq.destroy();
        if (!res.headersSent) {
          res.status(504).json({ error: "Timeout khi tải video." });
        }
      });

    } else {
      // Other platforms: use yt-dlp
      const args = [
        "-o", "-",
        "--no-playlist",
        "--user-agent",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36",
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

      const filename = `video_${Date.now()}.mp4`;
      res.setHeader("Content-Type", isAudioOnly ? "audio/mpeg" : "video/mp4");
      res.setHeader("Content-Disposition", `attachment; filename="${filename}"`);
      res.setHeader("Transfer-Encoding", "chunked");
      res.setHeader("Cache-Control", "no-store");

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

/** GET /api/formats — get supported formats */
app.post("/api/formats", async (req, res) => {
  const { url } = req.body;
  if (!url) return res.status(400).json({ error: "Thiếu URL" });

  try {
    if (isYouTubeUrl(url)) {
      const videoId = extractVideoId(url);
      if (!videoId) return res.status(400).json({ error: "Không nhận diện được video." });
      const metadata = await getInvidiousVideoInfo(videoId);
      return res.json({ formats: metadata.formats });
    } else {
      const { code, stdout, stderr } = await runYtDlp([
        "-F",
        "--no-playlist",
        url,
      ]);
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
  console.log(`YouTube: Invidious (${INVIDIOUS_INSTANCES.length} instances)`);
  console.log(`Other: yt-dlp direct`);
});
