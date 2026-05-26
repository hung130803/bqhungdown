# ── Stage 1: Build frontend ────────────────────────────────────────────────────
FROM node:18-bookworm AS frontend-builder

WORKDIR /app

COPY package.json package-lock.json ./
RUN npm install

COPY . .
RUN npx vite build --config vite.web.config.ts

# ── Stage 2: Full app (backend + frontend) ───────────────────────────────────
FROM node:18-slim

WORKDIR /app

# Install yt-dlp and ffmpeg
RUN apt-get update && apt-get install -y --no-install-recommends \
    ffmpeg curl python3 python3-pip ca-certificates \
    && curl -L https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp -o /usr/local/bin/yt-dlp \
    && chmod a+rx /usr/local/bin/yt-dlp \
    && pip3 install --break-system-packages yt-dlp --upgrade \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

COPY --from=frontend-builder /app/dist ./dist
COPY server/ ./server/
COPY package.json package-lock.json ./

RUN npm install --omit=dev

EXPOSE 3001

ENV NODE_ENV=production
ENV PORT=3001
ENV TMPDIR=/tmp

CMD ["node", "server/server.js"]
