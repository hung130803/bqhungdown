# ── Stage 1: Build frontend ────────────────────────────────────────────────────
FROM node:20-alpine AS frontend-builder

WORKDIR /app

COPY package.json package-lock.json ./
RUN npm install

COPY . .
RUN npx vite build --config vite.web.config.ts

# ── Stage 2: Full app (backend + frontend) ───────────────────────────────────
FROM node:20-slim

WORKDIR /app

# Install yt-dlp and ffmpeg (required for video processing)
# Using node:20-slim (Debian) instead of Alpine for better package compatibility
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3 \
    python3-pip \
    ffmpeg \
    curl \
    && pip3 install --break-system-packages yt-dlp \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/* \
    && ln -sf /usr/local/bin/yt-dlp /usr/local/bin/yt-dlp

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
