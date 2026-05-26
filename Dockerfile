# ── Stage 1: Build frontend ────────────────────────────────────────────────────
FROM node:20-alpine AS frontend-builder

WORKDIR /app

COPY package.json package-lock.json ./
RUN npm install

COPY . .
RUN npx vite build --config vite.web.config.ts

# ── Stage 2: Full app (backend + frontend) ───────────────────────────────────
FROM node:20-alpine

WORKDIR /app

# Install ffmpeg and curl
RUN apk add --no-cache ffmpeg curl ca-certificates python3

# Install yt-dlp via binary (avoids pip issues on Alpine)
RUN curl -L https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp -o /usr/local/bin/yt-dlp \
    && chmod a+rx /usr/local/bin/yt-dlp

# Set python SSL certs
RUN update-ca-certificates 2>/dev/null || true

COPY --from=frontend-builder /app/dist ./dist
COPY server/ ./server/
COPY package.json package-lock.json ./

RUN npm install --omit=dev

EXPOSE 3001

ENV NODE_ENV=production
ENV PORT=3001
ENV TMPDIR=/tmp

CMD ["node", "server/server.js"]
