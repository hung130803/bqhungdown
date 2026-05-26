import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

export default defineConfig({
  plugins: [react()],
  define: {
    "import.meta.env.VITE_WEB_MODE": JSON.stringify("1"),
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
      "@tauri-apps/api/core": path.resolve(__dirname, "./src/tauri-stubs/api/core.ts"),
      "@tauri-apps/api/event": path.resolve(__dirname, "./src/tauri-stubs/api/event.ts"),
      "@tauri-apps/plugin-updater": path.resolve(__dirname, "./src/tauri-stubs/plugin-updater/index.ts"),
      "@tauri-apps/plugin-process": path.resolve(__dirname, "./src/tauri-stubs/plugin-process/index.ts"),
    },
  },
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: process.env.VITE_API_URL || "http://localhost:3001",
        changeOrigin: true,
      },
    },
  },
  build: {
    outDir: "dist",
    base: "./",
    target: "es2021",
    rollupOptions: {
      input: "web.html",
    },
  },
});
