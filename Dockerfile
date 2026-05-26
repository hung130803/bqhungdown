# ── Stage 1: Build frontend ────────────────────────────────────────────────────
FROM node:20-alpine AS frontend-builder

WORKDIR /app

COPY package.json package-lock.json ./
RUN npm install

COPY . .
RUN npx vite build --config vite.web.config.ts

# ── Stage 2: Full app (backend + frontend) ───────────────────────────────────
FROM node:20-alpine

# Install yt-dlp and ffmpeg (required for video processing)
RUN apk add --no-cache \
    python3 \
    py3-pip \
    ffmpeg \
    && pip3 install --break-system-packages yt-dlp \
    && ln -s /usr/local/bin/yt-dlp /usr/local/bin/yt-dlp \
    && rm -rf /var/cache/apk/*

# Install Playwright chromium for TikTok/Douyin
RUN npm install -g playwright \
    && playwright install chromium --with-deps \
    || true

WORKDIR /app

COPY --from=frontend-builder /app/dist ./dist
COPY server/ ./server/
COPY package.json package-lock.json ./

# Don't install dev dependencies for production
RUN npm install --omit=dev

EXPOSE 3001

ENV NODE_ENV=production
ENV PORT=3001

# yt-dlp needs a writable /tmp
ENV TMPDIR=/tmp

CMD ["node", "server/server.js"]
