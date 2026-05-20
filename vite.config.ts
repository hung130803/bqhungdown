import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

// @ts-expect-error process is provided by Node at config time
const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  // Vite options tailored for Tauri development.
  // 1. prevent vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // tell vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
  // 3. expose env vars prefixed with TAURI_ to the source
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    // Tauri supports modern browsers
    target: "es2021",
    // don't minify for debug builds
    minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
    // produce sourcemaps for debug builds
    sourcemap: !!process.env.TAURI_DEBUG,
    // Split big vendor chunks so the initial JS payload stays small.
    // React + i18n đi 1 file, Tauri API + plugins 1 file, app code 1 file.
    rollupOptions: {
      output: {
        manualChunks: {
          react: ["react", "react-dom", "react-router-dom"],
          i18n: ["i18next", "react-i18next"],
          tauri: ["@tauri-apps/api"],
        },
      },
    },
    // Inline tiny assets directly into JS, skip extra HTTP roundtrips.
    assetsInlineLimit: 4096,
    // Drop console.* / debugger from production for smaller bundles.
    // (esbuild handles this transparently when minify === "esbuild".)
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
}));
